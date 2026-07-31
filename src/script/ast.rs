// AST - 抽象语法树节点定义
// 对齐原始括号式语法（见 scripts/example.ag）

/// 脚本设置项
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Setting {
    /// 语音场景下仅执行一次（执行一轮后自动停止）
    AudioOnlyOnce,
}

/// 鼠标按钮（复用 input::keymap::MouseButton，避免重复定义与桥接转换）
pub use crate::input::keymap::MouseButton;

/// 坐标定位方式
#[derive(Debug, Clone, PartialEq)]
pub enum Coord {
    /// 窗口客户区绝对坐标
    Absolute { x: i32, y: i32 },
    /// 相对窗口中心点的偏移
    Center { dx: i32, dy: i32 },
    /// 窗口宽/高的百分比 (0-100)
    Percent { px: i32, py: i32 },
}

/// 顶层命令 / 语句
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    // 设置（通常在脚本开头）
    Setting(Setting),

    // 键盘
    Down(String),              // down(1)
    Up(String),                // up(1)
    Click(String),             // click(2)
    ClickMs(String, u32),      // click_ms(2,50)
    DelayMs(u32),              // delay_ms(500)

    // 窗口消息
    SendWindowActive,          // send_window_active() 发送激活消息

    // 鼠标
    MouseMove(Coord),                // mouse_move / _center / _percent
    MouseDown(MouseButton, Coord),   // mouse_down / _center / _percent
    MouseUp(MouseButton),            // mouse_up(left)
    MouseClick(MouseButton, Coord),  // mouse_click / _center / _percent

    // 条件分支
    If {
        condition: BoolExpr,
        then_block: Vec<Command>,
        else_if_blocks: Vec<(BoolExpr, Vec<Command>)>,
    },
}

/// 颜色查找区域的定位方式
#[derive(Debug, Clone, PartialEq)]
pub enum FindArea {
    /// find_color(x, y, w, h, color) —— 绝对坐标区域
    Absolute { x: i32, y: i32, w: i32, h: i32 },
    /// find_color_center(dx, dy, w, h, color) —— 中心偏移区域
    Center { dx: i32, dy: i32, w: i32, h: i32 },
    /// find_color_percent(px, py, w, h, color) —— 百分比定位区域
    Percent { px: i32, py: i32, w: i32, h: i32 },
}

/// 值表达式：目前支持颜色查找和布尔字面量
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// 颜色查找，area + 目标颜色(0xRRGGBB)
    FindColor { area: FindArea, color: u32 },
    /// 布尔字面量 true / false
    Bool(bool),
}

/// 比较运算符
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompareOp {
    Eq,   // ==
    Ne,   // !=
}

/// 布尔表达式：左值 op 右值
/// 例如 find_color(...) == true, find_color(...) != find_color(...)
#[derive(Debug, Clone, PartialEq)]
pub struct BoolExpr {
    pub left: Value,
    pub op: CompareOp,
    pub right: Value,
}
