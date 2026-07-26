pub mod win32;

use std::path::PathBuf;

/// 获取可执行文件所在目录
pub fn get_exe_dir() -> Result<PathBuf, String> {
    std::env::current_exe()
        .map_err(|e| format!("获取可执行文件路径失败: {}", e))?
        .parent()
        .ok_or_else(|| "无法确定可执行文件目录".to_string())
        .map(|p| p.to_path_buf())
}
