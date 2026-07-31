// 覆盖窗冒烟测试：3 秒倒计时后给前台窗口盖上鼠标转发覆盖窗
//
// 用法：cargo run --example overlay_smoke
// 验证点：半透明底色 + "鼠标事件转发模式"文字、移动/缩放跟随、
//         最小化隐藏/恢复回来、关闭目标窗口后事件回报并退出。

use std::thread;
use std::time::Duration;

use game_auto_keyboard::event_bus::{MainEvent, MainEventBus};
use game_auto_keyboard::overlay::OverlayWindow;
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

fn main() {
    println!("=== 覆盖窗冒烟测试 ===");
    println!("3 秒后覆盖当前前台窗口，请立即切换到目标窗口（如记事本）...");
    thread::sleep(Duration::from_secs(3));

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        println!("✗ 未取到前台窗口");
        return;
    }
    println!("目标窗口: {:?}", hwnd.0);

    let bus = MainEventBus::new();
    let _overlay = match OverlayWindow::start(hwnd.0 as isize, vec![hwnd.0 as isize], bus.sender()) {
        Ok(o) => o,
        Err(e) => {
            println!("✗ 启动失败: {}", e);
            return;
        }
    };
    println!("✓ 覆盖窗已启动。移动/缩放/最小化目标窗口观察跟随。");
    println!("  关闭目标窗口 → 收到 TargetLost 后自动退出（_overlay 由 Drop 收尾）。");

    loop {
        for ev in bus.poll() {
            println!("事件: {:?}", ev);
            if matches!(ev, MainEvent::Overlay(_)) {
                println!("✓ 覆盖窗已自毁，测试结束");
                return;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
}
