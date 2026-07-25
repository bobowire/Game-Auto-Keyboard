use windows::Win32::Foundation::{HWND, WPARAM, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    PostMessageW, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE,
    WM_ACTIVATE, WM_SETFOCUS,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, MAPVK_VK_TO_VSC};
use crate::input::backend::InputBackend;
use crate::input::keymap::{parse_key, MouseButton};

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
        // LPARAM = MAKELPARAM(x, y)，WPARAM 为按键状态（移动时无按键则为 0）
        let lparam = LPARAM(((y as isize) << 16) | (x as isize & 0xFFFF));

        unsafe {
            PostMessageW(hwnd, WM_MOUSEMOVE, WPARAM(0), lparam)
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

        // LPARAM = MAKELPARAM(x, y)
        let lparam = LPARAM(((y as isize) << 16) | (x as isize & 0xFFFF));

        unsafe {
            PostMessageW(hwnd, msg, WPARAM(0), lparam)
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

        unsafe {
            PostMessageW(hwnd, msg, WPARAM(0), lparam)
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
