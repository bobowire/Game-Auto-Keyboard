// 按键名称 → 虚拟键码(VK) 映射
// 脚本中用字符串表示按键，如 "A"、"1"、"space"、"f1"、"ctrl"

use windows::Win32::UI::Input::KeyboardAndMouse::*;

/// 鼠标按钮标识
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// 尝试把按键名解析为鼠标按钮（用于 click/down/up 的鼠标场景）
pub fn parse_mouse_button(name: &str) -> Option<MouseButton> {
    match name.to_ascii_lowercase().as_str() {
        "left" | "lbutton" | "mouse_left" => Some(MouseButton::Left),
        "right" | "rbutton" | "mouse_right" => Some(MouseButton::Right),
        "middle" | "mbutton" | "mouse_middle" => Some(MouseButton::Middle),
        _ => None,
    }
}

/// 把按键名解析为虚拟键码 (VK code)
/// 支持：单字符(a-z,0-9)、功能键(f1-f12)、常用特殊键
pub fn parse_key(name: &str) -> Result<u16, String> {
    let lower = name.to_ascii_lowercase();

    // 单个字符：字母或数字
    if name.chars().count() == 1 {
        let c = name.chars().next().unwrap();
        if c.is_ascii_alphabetic() {
            // 'A'..'Z' 的 VK 就是大写字母的 ASCII 码
            return Ok(c.to_ascii_uppercase() as u16);
        }
        if c.is_ascii_digit() {
            // '0'..'9' 的 VK 就是数字字符的 ASCII 码
            return Ok(c as u16);
        }
    }

    // 功能键 f1-f24
    if let Some(num_str) = lower.strip_prefix('f') {
        if let Ok(n) = num_str.parse::<u16>() {
            if (1..=24).contains(&n) {
                return Ok(VK_F1.0 + (n - 1));
            }
        }
    }

    // 特殊键
    let vk = match lower.as_str() {
        "space" | "spacebar" => VK_SPACE,
        "enter" | "return" => VK_RETURN,
        "tab" => VK_TAB,
        "esc" | "escape" => VK_ESCAPE,
        "backspace" | "back" => VK_BACK,
        "shift" => VK_SHIFT,
        "ctrl" | "control" => VK_CONTROL,
        "alt" | "menu" => VK_MENU,
        "capslock" | "caps" => VK_CAPITAL,
        "up" => VK_UP,
        "down" => VK_DOWN,
        "left" => VK_LEFT,
        "right" => VK_RIGHT,
        "home" => VK_HOME,
        "end" => VK_END,
        "pageup" | "pgup" => VK_PRIOR,
        "pagedown" | "pgdn" => VK_NEXT,
        "insert" | "ins" => VK_INSERT,
        "delete" | "del" => VK_DELETE,
        // 小键盘数字
        "num0" => VK_NUMPAD0,
        "num1" => VK_NUMPAD1,
        "num2" => VK_NUMPAD2,
        "num3" => VK_NUMPAD3,
        "num4" => VK_NUMPAD4,
        "num5" => VK_NUMPAD5,
        "num6" => VK_NUMPAD6,
        "num7" => VK_NUMPAD7,
        "num8" => VK_NUMPAD8,
        "num9" => VK_NUMPAD9,
        _ => return Err(format!("无法识别的按键名: {}", name)),
    };

    Ok(vk.0)
}
