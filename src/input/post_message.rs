use windows::Win32::Foundation::{HWND, WPARAM, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    PostMessageW, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE,
    WM_ACTIVATE, WM_SETFOCUS,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, MapVirtualKeyW, MAPVK_VK_TO_VSC, VK_CONTROL, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
    VK_SHIFT,
};
use crate::input::backend::InputBackend;
use crate::input::keymap::{parse_key, MouseButton};

// MK_*（鼠标按键状态位，对应 Win32 MK_LBUTTON 等）。用字面量避免仅为这几个常量
// 引入 Win32_System_SystemServices feature。
const MK_LBUTTON: usize = 0x0001;
const MK_RBUTTON: usize = 0x0002;
const MK_SHIFT: usize = 0x0004;
const MK_CONTROL: usize = 0x0008;
const MK_MBUTTON: usize = 0x0010;

/// 读取当前物理按键/修饰键状态，合成 wParam 的 MK_* 位。
///
/// 注意：GetKeyState 反映调用线程消息队列里的按键状态；后台注入线程通常无消息循环，
/// 物理按键位多返回 0。因此 send_mouse_down/up 会对"正在模拟的那个按键"额外做
/// 强制置位/清位，保证目标窗口看到正确的按下/弹起语义。
fn current_mk_state() -> usize {
    unsafe {
        let mut mk = 0usize;
        if GetKeyState(VK_LBUTTON.0 as i32) < 0 {
            mk |= MK_LBUTTON;
        }
        if GetKeyState(VK_RBUTTON.0 as i32) < 0 {
            mk |= MK_RBUTTON;
        }
        if GetKeyState(VK_MBUTTON.0 as i32) < 0 {
            mk |= MK_MBUTTON;
        }
        if GetKeyState(VK_SHIFT.0 as i32) < 0 {
            mk |= MK_SHIFT;
        }
        if GetKeyState(VK_CONTROL.0 as i32) < 0 {
            mk |= MK_CONTROL;
        }
        mk
    }
}

/// 鼠标按钮 → MK_* 位
fn button_mk(button: MouseButton) -> usize {
    match button {
        MouseButton::Left => MK_LBUTTON,
        MouseButton::Right => MK_RBUTTON,
        MouseButton::Middle => MK_MBUTTON,
    }
}

pub struct PostMessageBackend;

impl PostMessageBackend {
    pub fn new() -> Self {
        Self
    }
}

impl InputBackend for PostMessageBackend {
    fn name(&self) -> &str {
        "PostMessage (后台)"
    }

    fn supports_background(&self) -> bool {
        true
    }

    fn send_key_down(&self, hwnd: HWND, key: &str) -> Result<(), String> {
        let vk = parse_key(key)?;
        let scan_code = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) };

        // LPARAM = (scan_code << 16) | repeat_count(1)
        let lparam = LPARAM(((scan_code as isize) << 16) | 1);

        unsafe {
            PostMessageW(hwnd, WM_KEYDOWN, WPARAM(vk as usize), lparam)
                .map_err(|e| format!("PostMessage 失败: {:?}", e))?;
        }

        Ok(())
    }

    fn send_key_up(&self, hwnd: HWND, key: &str) -> Result<(), String> {
        let vk = parse_key(key)?;
        let scan_code = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) };

        // LPARAM = (scan_code << 16) | 0xC0000001
        let lparam = LPARAM(((scan_code as isize) << 16) | 0xC0000001u32 as isize);

        unsafe {
            PostMessageW(hwnd, WM_KEYUP, WPARAM(vk as usize), lparam)
                .map_err(|e| format!("PostMessage 失败: {:?}", e))?;
        }

        Ok(())
    }

    fn send_mouse_move(&self, hwnd: HWND, x: i32, y: i32) -> Result<(), String> {
        // LPARAM = MAKELPARAM(x, y)，WPARAM = 当前 MK_* 按键状态
        let lparam = LPARAM(((y as isize) << 16) | (x as isize & 0xFFFF));

        unsafe {
            PostMessageW(hwnd, WM_MOUSEMOVE, WPARAM(current_mk_state()), lparam)
                .map_err(|e| format!("发送鼠标移动消息失败: {:?}", e))?;
        }

        Ok(())
    }

    fn send_mouse_down(
        &self,
        hwnd: HWND,
        button: MouseButton,
        x: i32,
        y: i32,
    ) -> Result<(), String> {
        let msg = match button {
            MouseButton::Left => WM_LBUTTONDOWN,
            MouseButton::Right => WM_RBUTTONDOWN,
            MouseButton::Middle => WM_MBUTTONDOWN,
        };

        // LPARAM = MAKELPARAM(x, y)；WPARAM 强制置上本次按下的按键位（GetKeyState 在后台线程
        // 多返回 0，不能反映被模拟的按键）
        let lparam = LPARAM(((y as isize) << 16) | (x as isize & 0xFFFF));
        let wparam = current_mk_state() | button_mk(button);

        unsafe {
            PostMessageW(hwnd, msg, WPARAM(wparam), lparam)
                .map_err(|e| format!("发送鼠标消息失败: {:?}", e))?;
        }

        Ok(())
    }

    fn send_mouse_up(&self, hwnd: HWND, button: MouseButton, x: i32, y: i32) -> Result<(), String> {
        let msg = match button {
            MouseButton::Left => WM_LBUTTONUP,
            MouseButton::Right => WM_RBUTTONUP,
            MouseButton::Middle => WM_MBUTTONUP,
        };

        let lparam = LPARAM(((y as isize) << 16) | (x as isize & 0xFFFF));
        // 弹起时清掉本次释放的按键位
        let wparam = current_mk_state() & !button_mk(button);

        unsafe {
            PostMessageW(hwnd, msg, WPARAM(wparam), lparam)
                .map_err(|e| format!("发送鼠标消息失败: {:?}", e))?;
        }

        Ok(())
    }

    fn send_window_active(&self, hwnd: HWND) -> Result<(), String> {
        unsafe {
            // WM_ACTIVATE: WPARAM 高字 = 激活类型 (WA_ACTIVE=1)，低字 = 最小化状态 (0=未最小化)
            // LPARAM = 上一个激活的窗口句柄（我们传 0）
            PostMessageW(hwnd, WM_ACTIVATE, WPARAM(1), LPARAM(0))
                .map_err(|e| format!("发送 WM_ACTIVATE 失败: {:?}", e))?;

            // WM_SETFOCUS: 通知窗口获得键盘焦点
            PostMessageW(hwnd, WM_SETFOCUS, WPARAM(0), LPARAM(0))
                .map_err(|e| format!("发送 WM_SETFOCUS 失败: {:?}", e))?;
        }

        Ok(())
    }
}
