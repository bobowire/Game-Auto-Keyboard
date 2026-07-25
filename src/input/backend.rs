use windows::Win32::Foundation::HWND;
use crate::input::keymap::MouseButton;

/// 输入后端 trait（策略模式）
///
/// 脚本引擎以字符串名称驱动按键（如 "A"、"space"、"left"），
/// 各后端负责把名称解析为具体的 VK 码/鼠标消息并发送。
pub trait InputBackend: Send + Sync {
    /// 后端名称（用于 UI 显示/切换）
    fn name(&self) -> &str;

    /// 是否支持后台发送（窗口非激活状态）
    fn supports_background(&self) -> bool;

    // ===== 键盘接口 =====

    /// 发送键盘按下事件（key 为按键名，如 "A"、"space"、"f1"）
    fn send_key_down(&self, hwnd: HWND, key: &str) -> Result<(), String>;

    /// 发送键盘弹起事件
    fn send_key_up(&self, hwnd: HWND, key: &str) -> Result<(), String>;

    // ===== 鼠标接口 =====

    /// 发送鼠标移动事件（客户区坐标）
    fn send_mouse_move(&self, hwnd: HWND, x: i32, y: i32) -> Result<(), String>;

    /// 发送鼠标按下事件（客户区坐标）
    fn send_mouse_down(
        &self,
        hwnd: HWND,
        button: MouseButton,
        x: i32,
        y: i32,
    ) -> Result<(), String>;

    /// 发送鼠标弹起事件
    fn send_mouse_up(&self, hwnd: HWND, button: MouseButton, x: i32, y: i32) -> Result<(), String>;

    // ===== 窗口消息 =====

    /// 发送窗口激活消息（欺骗窗口使其认为自己被激活）
    fn send_window_active(&self, hwnd: HWND) -> Result<(), String>;

    // ===== 组合便捷方法 =====

    /// 点击（按下+弹起）。button_or_key 既可能是鼠标按钮名也可能是键盘按键名。
    /// x/y 仅在鼠标点击时有意义。
    fn click(&self, hwnd: HWND, button_or_key: &str, x: i32, y: i32) -> Result<(), String> {
        use crate::input::keymap::parse_mouse_button;

        if let Some(btn) = parse_mouse_button(button_or_key) {
            self.send_mouse_down(hwnd, btn, x, y)?;
            self.send_mouse_up(hwnd, btn, x, y)?;
        } else {
            self.send_key_down(hwnd, button_or_key)?;
            self.send_key_up(hwnd, button_or_key)?;
        }
        Ok(())
    }
}
