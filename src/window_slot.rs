// 窗口槽位 - 每个槽位对应一个目标窗口及其方案集
// 最多 8 个槽位（对应热键 1-8）

use crate::runner::Runner;
use crate::script::Command;

/// 一个绑定到窗口的方案（引用某个脚本文件）
#[derive(Clone)]
pub struct Scheme {
    /// 脚本文件名（作为唯一标识，与 ScriptFile.name 对应）
    pub script_name: String,
    /// 解析好的命令（从 ScriptFile 拷贝，执行时用）
    pub commands: Vec<Command>,
}

/// 单个窗口槽位
pub struct WindowSlot {
    /// 自定义窗口名（语音指称用，如"窗口1"、"主号"）
    pub name: String,
    /// 目标窗口句柄（isize 形式，未绑定为 None）
    pub hwnd: Option<isize>,
    /// 窗口标题
    pub title: String,
    /// 该窗口的方案集
    pub schemes: Vec<Scheme>,
    /// 标识方案的索引（默认执行方案），指向 schemes
    pub marked: Option<usize>,
    /// 当前后台运行器
    pub runner: Option<Runner>,
}

impl Default for WindowSlot {
    fn default() -> Self {
        Self {
            name: String::new(),
            hwnd: None,
            title: String::new(),
            schemes: Vec::new(),
            marked: None,
            runner: None,
        }
    }
}

impl WindowSlot {
    /// 是否已绑定窗口
    pub fn is_bound(&self) -> bool {
        self.hwnd.is_some()
    }

    /// 是否正在运行
    pub fn is_running(&self) -> bool {
        self.runner.as_ref().map_or(false, |r| r.is_running())
    }

    /// 添加一个方案（若已存在同名则跳过）。返回是否新增。
    pub fn add_scheme(&mut self, scheme: Scheme) -> bool {
        if self.schemes.iter().any(|s| s.script_name == scheme.script_name) {
            return false;
        }
        self.schemes.push(scheme);
        // 第一个加入的方案自动成为标识
        if self.marked.is_none() {
            self.marked = Some(self.schemes.len() - 1);
        }
        true
    }

    /// 移除指定索引的方案，并修正 marked
    pub fn remove_scheme(&mut self, idx: usize) {
        if idx >= self.schemes.len() {
            return;
        }
        self.schemes.remove(idx);

        // 修正标识索引
        match self.marked {
            Some(m) if m == idx => {
                // 被删的正是标识：若还有方案，退回到 0，否则清空
                self.marked = if self.schemes.is_empty() { None } else { Some(0) };
            }
            Some(m) if m > idx => {
                self.marked = Some(m - 1);
            }
            _ => {}
        }
    }

    /// 设置标识方案
    pub fn set_marked(&mut self, idx: usize) {
        if idx < self.schemes.len() {
            self.marked = Some(idx);
        }
    }

    /// 获取标识方案
    pub fn marked_scheme(&self) -> Option<&Scheme> {
        self.marked.and_then(|i| self.schemes.get(i))
    }

    /// 停止运行
    pub fn stop(&mut self) {
        if let Some(mut r) = self.runner.take() {
            r.stop_and_join();
        }
    }
}
