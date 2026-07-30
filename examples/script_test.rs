// 脚本文件加载 + 执行测试
use game_auto_keyboard::script::{ScriptFile, ScriptExecutor};
use game_auto_keyboard::input::PostMessageBackend;
use std::path::Path;
use std::time::Duration;
use std::thread;
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

fn main() {
    println!("=== 脚本文件加载 + 执行测试 ===");
    println!();

    // 加载测试脚本文件
    let script_path = Path::new("scripts/hello.ag");
    println!("加载脚本: {:?}", script_path);

    let sf = match ScriptFile::load(script_path, Path::new("scripts")) {
        Ok(sf) => sf,
        Err(e) => {
            eprintln!("加载失败: {}", e);
            return;
        }
    };

    let commands = match &sf.commands {
        Some(cmds) => {
            println!("✓ 解析成功，共 {} 条命令", cmds.len());
            cmds
        }
        None => {
            eprintln!("✗ 解析失败: {}", sf.parse_error.as_deref().unwrap_or("未知"));
            return;
        }
    };

    println!();
    println!("请在 5 秒内点击目标窗口（如记事本）...");
    thread::sleep(Duration::from_secs(5));

    let hwnd = unsafe { GetForegroundWindow() };
    println!("目标窗口 HWND: {:?}", hwnd);
    println!();
    println!("执行脚本...");
    println!("----------------------------------------");

    let backend = PostMessageBackend::new();
    let executor = ScriptExecutor::new(&backend, hwnd);

    match executor.execute(commands) {
        Ok(_) => {
            println!("----------------------------------------");
            println!("✓ 脚本执行完成！");
        }
        Err(e) => {
            eprintln!("----------------------------------------");
            eprintln!("✗ 执行失败: {}", e);
        }
    }
}
