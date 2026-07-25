// 最简单的 PostMessage 测试
use std::time::Duration;
use std::thread;
use windows::Win32::Foundation::{HWND, WPARAM, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, PostMessageW, WM_KEYDOWN, WM_KEYUP};
use windows::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, VkKeyScanW, MAPVK_VK_TO_VSC};

fn main() {
    println!("=== 最简 PostMessage 测试 ===");
    println!();
    println!("请在 5 秒内点击目标窗口（如记事本）...");

    thread::sleep(Duration::from_secs(5));

    let hwnd = unsafe { GetForegroundWindow() };
    println!("目标窗口 HWND: {:?}", hwnd);
    println!();

    println!("发送字符 'H'...");
    send_char(hwnd, 'H');
    thread::sleep(Duration::from_millis(100));

    println!("发送字符 'i'...");
    send_char(hwnd, 'i');
    thread::sleep(Duration::from_millis(100));

    println!();
    println!("测试完成！检查目标窗口是否显示 'Hi'");
}

fn send_char(hwnd: HWND, c: char) {
    unsafe {
        // 获取虚拟键码
        let vk_result = VkKeyScanW(c as u16);
        if vk_result == -1 {
            println!("  错误: 无法转换字符 '{}'", c);
            return;
        }
        let vk = (vk_result & 0xFF) as u8;

        // 获取扫描码
        let scan_code = MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC);

        // 发送 WM_KEYDOWN
        let lparam_down = LPARAM(((scan_code as isize) << 16) | 1);
        if let Err(e) = PostMessageW(hwnd, WM_KEYDOWN, WPARAM(vk as usize), lparam_down) {
            println!("  WM_KEYDOWN 失败: {:?}", e);
            return;
        }

        // 发送 WM_KEYUP
        let lparam_up = LPARAM(((scan_code as isize) << 16) | 0xC0000001);
        if let Err(e) = PostMessageW(hwnd, WM_KEYUP, WPARAM(vk as usize), lparam_up) {
            println!("  WM_KEYUP 失败: {:?}", e);
            return;
        }

        println!("  成功发送: VK={}, ScanCode={}", vk, scan_code);
    }
}
