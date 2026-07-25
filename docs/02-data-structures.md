# 核心数据结构

## 1. 窗口槽位 (WindowSlot)

**位置**: `src/window/slot.rs`

```rust
use windows::Win32::Foundation::HWND;

/// 单个窗口槽位（最多8个）
pub struct WindowSlot {
    /// 槽位编号 (1-8)
    pub index: u8,
    
    /// 窗口句柄（None 表示未绑定）
    pub hwnd: Option<HWND>,
    
    /// 窗口标题（用于显示）
    pub title: String,
    
    /// 已加载的方案列表（从 scripts 目录读取）
    pub schemes: Vec<SchemeRef>,
    
    /// 当前选中的方案索引
    pub selected_scheme: usize,
    
    /// 标识方案索引（热键9触发的默认方案）
    pub marked_scheme: usize,
    
    /// 执行状态
    pub state: ExecutionState,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ExecutionState {
    Idle,       // 未运行
    Running,    // 正在执行
    Paused,     // 暂停（暂不实现）
}

/// 方案引用（轻量级，指向 SchemeManager 中的实际数据）
#[derive(Clone, Debug)]
pub struct SchemeRef {
    pub id: String,          // 脚本文件名（不含路径）
    pub display_name: String, // 显示名称
}
```

**说明**:
- `hwnd` 可能在窗口关闭后失效，需要定期验证
- `schemes` 列表从 SchemeManager 同步
- `marked_scheme` 是用户设置的"默认方案"索引

---

## 2. 脚本 AST (Script & Statement)

**位置**: `src/script/ast.rs`

### 2.1 根节点

```rust
/// 脚本抽象语法树的根节点
#[derive(Clone, Debug)]
pub struct Script {
    pub statements: Vec<Statement>,
}
```

### 2.2 语句类型

```rust
/// 语句（命令或控制流）
#[derive(Clone, Debug)]
pub enum Statement {
    Command(Command),       // 单个命令
    If(IfBlock),            // 条件块
    Comment(String),        // 注释
}
```

### 2.3 命令枚举

```rust
/// 命令（键盘/鼠标/延迟）
#[derive(Clone, Debug)]
pub enum Command {
    // ===== 键盘命令 =====
    Down(Key),                          // 按下键
    Up(Key),                            // 弹起键
    Click(Key),                         // 点击键（按下+立即弹起）
    ClickMs(Key, u32),                  // 点击键（按下+延迟+弹起）
    
    // ===== 鼠标命令 =====
    /// 鼠标按下（窗口绝对坐标）
    MouseDown { button: MouseButton, x: i32, y: i32 },
    
    /// 鼠标弹起
    MouseUp { button: MouseButton },
    
    /// 鼠标点击（窗口绝对坐标）
    MouseClick { button: MouseButton, x: i32, y: i32 },
    
    /// 鼠标按下（窗口中心偏移）
    MouseDownCenter { button: MouseButton, offset_x: i32, offset_y: i32 },
    
    /// 鼠标点击（窗口中心偏移）
    MouseClickCenter { button: MouseButton, offset_x: i32, offset_y: i32 },
    
    /// 鼠标按下（窗口百分比坐标）
    MouseDownPercent { button: MouseButton, percent_x: u8, percent_y: u8 },
    
    /// 鼠标点击（窗口百分比坐标）
    MouseClickPercent { button: MouseButton, percent_x: u8, percent_y: u8 },
    
    // ===== 延迟命令 =====
    DelayMs(u32),                       // 延迟指定毫秒
}
```

### 2.4 条件块

```rust
/// 条件块（if_start / else_if / if_end）
#[derive(Clone, Debug)]
pub struct IfBlock {
    pub branches: Vec<Branch>,  // 多个分支
}

#[derive(Clone, Debug)]
pub struct Branch {
    pub condition: Option<Expression>,  // None 表示 else
    pub body: Vec<Statement>,
}
```

### 2.5 表达式

```rust
/// 表达式（用于条件判断）
#[derive(Clone, Debug)]
pub enum Expression {
    // ===== 颜色查找 =====
    FindColor {
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        color: u32,  // RGB: 0xRRGGBB
    },
    FindColorCenter {
        offset_x: i32,
        offset_y: i32,
        w: u32,
        h: u32,
        color: u32,
    },
    FindColorPercent {
        percent_x: u8,
        percent_y: u8,
        w: u32,
        h: u32,
        color: u32,
    },
    
    // ===== 逻辑运算 =====
    Equals(Box<Expression>, Box<Expression>),
    NotEquals(Box<Expression>, Box<Expression>),
    Bool(bool),
}
```

### 2.6 辅助类型

```rust
/// 键盘按键（VK 码或字符）
#[derive(Clone, Copy, Debug)]
pub enum Key {
    VirtualKey(u8),     // VK_RETURN, VK_ESCAPE 等
    Char(char),         // '1', 'a', 'A' 等
}

/// 鼠标按钮
#[derive(Clone, Copy, Debug)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}
```

---

## 3. 方案 (Scheme)

**位置**: `src/scheme/scheme.rs`

```rust
use std::path::PathBuf;

/// 单个方案（对应一个 .ag 文件）
#[derive(Clone, Debug)]
pub struct Scheme {
    /// 唯一标识符（文件名，不含路径）
    pub id: String,
    
    /// 显示名称（从文件名提取或文件内定义）
    pub display_name: String,
    
    /// 文件路径
    pub file_path: PathBuf,
    
    /// 解析后的脚本（懒加载）
    pub script: Option<Script>,
}
```

---

## 4. 热键事件

**位置**: `src/hotkey/mod.rs`

```rust
/// 热键触发事件
#[derive(Clone, Debug)]
pub enum HotkeyEvent {
    /// 选择窗口（Ctrl+Shift+[1-8]）
    SelectWindow(u8),
    
    /// 启动方案（Ctrl+Shift+9）
    Start(Vec<u8>),  // 窗口索引列表
    
    /// 停止方案（Ctrl+Shift+0）
    Stop(Vec<u8>),   // 窗口索引列表
    
    /// 添加窗口（自定义快捷键，如 Ctrl+Alt+A）
    AddWindow,
}
```

---

## 5. 执行器命令

**位置**: `src/executor/runner.rs`

```rust
use std::sync::Arc;

/// 发送给执行线程的命令
#[derive(Clone)]
pub enum RunnerCommand {
    /// 启动执行指定脚本（循环执行）
    Start(Arc<Script>),
    
    /// 停止当前脚本
    Stop,
}
```

---

## 6. 主应用状态

**位置**: `src/app.rs`

```rust
use crate::window::WindowSlot;
use crate::scheme::SchemeManager;
use crate::hotkey::HotkeyManager;
use crate::executor::ExecutorManager;
use crate::input::InputManager;

/// 主应用状态（egui 的 App trait 实现）
pub struct AutoKeyboardApp {
    /// 8 个窗口槽位
    pub windows: [WindowSlot; 8],
    
    /// 方案管理器（加载所有 .ag 脚本）
    pub scheme_manager: SchemeManager,
    
    /// 热键管理器（注册全局热键）
    pub hotkey_manager: HotkeyManager,
    
    /// 执行引擎管理器（每个窗口一个 Runner）
    pub executor_manager: ExecutorManager,
    
    /// 输入后端管理器（可切换）
    pub input_manager: InputManager,
    
    /// UI 状态
    pub show_script_viewer: bool,
    pub selected_script_id: Option<String>,
    pub is_selecting_window: bool,  // 是否在窗口选择模式
}
```

---

## 数据流示意

### 启动方案流程

```
用户按热键
    ↓
HotkeyManager 产生 HotkeyEvent::Start([2])
    ↓
App.process_hotkeys() 处理事件
    ↓
获取 WindowSlot[1].marked_scheme
    ↓
从 SchemeManager 获取 Script
    ↓
ExecutorManager.start(2, hwnd, script)
    ↓
SchemeRunner[1] 接收 RunnerCommand::Start
    ↓
执行线程循环执行 Script 中的 Statement
```

### 脚本执行流程

```
ScriptExecutor.execute(script)
    ↓
遍历 script.statements
    ↓
匹配 Statement 类型：
    Command → InputBackend.send_xxx()
    If → 评估 Expression → 递归执行分支
    Comment → 跳过
    ↓
检查 stop_flag
    ↓
循环回到开头（直到停止）
```
