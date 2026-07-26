// 脚本文件加载器 - 扫描目录、读取并解析 .ag 文件

use crate::script::ast::Command;
use crate::script::parser::Parser;
use std::fs;
use std::path::{Path, PathBuf};

/// 一个已加载的脚本方案
#[derive(Debug, Clone)]
pub struct ScriptFile {
    /// 文件名（含扩展名），如 "farming.ag"
    pub name: String,
    /// 完整路径
    pub path: PathBuf,
    /// 分类路径（相对于脚本根目录的子目录路径，如 "蜀门/百花"），根目录文件为 "通用"
    pub category: String,
    /// 原始文本内容（供 UI 浏览）
    pub source: String,
    /// 解析后的命令；解析失败时为 None，错误存在 parse_error
    pub commands: Option<Vec<Command>>,
    /// 解析错误信息
    pub parse_error: Option<String>,
}

impl ScriptFile {
    /// 从单个文件加载并解析
    pub fn load(path: &Path, root_dir: &Path) -> Result<Self, String> {
        let source = fs::read_to_string(path)
            .map_err(|e| format!("读取文件失败 {:?}: {}", path, e))?;

        let name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("<unknown>")
            .to_string();

        // 计算分类：相对路径的父目录部分
        let category = if let Ok(rel_path) = path.strip_prefix(root_dir) {
            if let Some(parent) = rel_path.parent() {
                let parent_str = parent.to_string_lossy().to_string();
                if parent_str.is_empty() {
                    "通用".to_string()
                } else {
                    parent_str.replace('\\', "/")
                }
            } else {
                "通用".to_string()
            }
        } else {
            "通用".to_string()
        };

        let (commands, parse_error) = match Parser::new(&source) {
            Ok(mut parser) => match parser.parse() {
                Ok(cmds) => (Some(cmds), None),
                Err(e) => (None, Some(e)),
            },
            Err(e) => (None, Some(e)),
        };

        Ok(ScriptFile {
            name,
            path: path.to_path_buf(),
            category,
            source,
            commands,
            parse_error,
        })
    }

    /// 是否解析成功
    pub fn is_valid(&self) -> bool {
        self.commands.is_some()
    }
}

/// 递归扫描目录下所有 .ag 文件并加载
pub fn load_dir(dir: &Path) -> Result<Vec<ScriptFile>, String> {
    if !dir.exists() {
        return Err(format!("脚本目录不存在: {:?}", dir));
    }

    let mut scripts = Vec::new();
    load_dir_recursive(dir, dir, &mut scripts)?;

    // 按分类和文件名排序
    scripts.sort_by(|a, b| {
        a.category.cmp(&b.category)
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(scripts)
}

/// 递归扫描目录
fn load_dir_recursive(root_dir: &Path, current_dir: &Path, scripts: &mut Vec<ScriptFile>) -> Result<(), String> {
    let entries = fs::read_dir(current_dir)
        .map_err(|e| format!("读取目录失败 {:?}: {}", current_dir, e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("遍历目录项失败: {}", e))?;
        let path = entry.path();

        if path.is_dir() {
            // 递归子目录
            load_dir_recursive(root_dir, &path, scripts)?;
        } else if path.is_file()
            && path.extension().and_then(|e| e.to_str()) == Some("ag")
        {
            match ScriptFile::load(&path, root_dir) {
                Ok(sf) => scripts.push(sf),
                Err(e) => eprintln!("跳过文件 {:?}: {}", path, e),
            }
        }
    }

    Ok(())
}
