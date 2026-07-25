// 截图找色测试工具
//
// 用法：
//   1. 运行后 5 秒内点击目标窗口
//   2. 程序会截图并保存为 capture_output.ppm（可用图片查看器打开验证）
//   3. 打印窗口中心点及四角的颜色值
//   4. 统计截图是否为全黑（判断 PrintWindow 是否有效）

use game_auto_keyboard::capture::{CaptureBackend, PrintWindowCapture};
use game_auto_keyboard::utils::win32::window_title;
use std::fs::File;
use std::io::Write;
use std::thread;
use std::time::Duration;
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

fn main() {
    println!("=== 后台截图找色测试 ===");
    println!();

    // 第一步：抓取目标窗口句柄
    println!("【步骤1】请在 5 秒内点击/切换到目标窗口...");
    for i in (1..=5).rev() {
        println!("  {} 秒...", i);
        thread::sleep(Duration::from_secs(1));
    }

    let hwnd = unsafe { GetForegroundWindow() };
    let title = window_title(hwnd);
    println!();
    println!("✓ 已抓取目标窗口: {:?}", hwnd);
    println!("  标题: {}", title);
    println!();

    // 第二步：等待用户切走，验证后台截图
    println!("【步骤2】现在请切换到【其他窗口】，让目标窗口进入后台！");
    println!("         （这样才能验证后台截图能力）");
    for i in (1..=5).rev() {
        println!("  {} 秒后截图...", i);
        thread::sleep(Duration::from_secs(1));
    }
    println!();

    // 确认当前前台窗口不是目标窗口
    let current_fg = unsafe { GetForegroundWindow() };
    if current_fg == hwnd {
        println!("⚠ 注意：目标窗口仍在前台，未能验证后台截图效果");
    } else {
        println!("✓ 目标窗口已在后台，开始截图...");
    }
    println!();

    let capture = PrintWindowCapture::new();
    println!("使用截图后端: {}", capture.name());

    let bitmap = match capture.capture(hwnd) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("截图失败: {}", e);
            return;
        }
    };

    println!("截图成功: {}x{}", bitmap.width, bitmap.height);
    println!();

    // 采样几个关键点的颜色
    let w = bitmap.width;
    let h = bitmap.height;
    let sample_points = [
        ("左上", 0, 0),
        ("右上", w - 1, 0),
        ("中心", w / 2, h / 2),
        ("左下", 0, h - 1),
        ("右下", w - 1, h - 1),
    ];

    println!("采样点颜色：");
    for (name, x, y) in sample_points {
        if let Some(rgb) = bitmap.get_rgb(x, y) {
            println!("  {} ({},{}) = #{:06X}", name, x, y, rgb);
        }
    }
    println!();

    // 统计黑色像素比例，判断 PrintWindow 是否有效
    let total = (w * h) as usize;
    let mut black = 0usize;
    for y in 0..h {
        for x in 0..w {
            if bitmap.get_rgb(x, y) == Some(0x000000) {
                black += 1;
            }
        }
    }
    let black_ratio = black as f64 / total as f64 * 100.0;
    println!("黑色像素占比: {:.1}%", black_ratio);
    if black_ratio > 95.0 {
        println!("⚠ 截图几乎全黑，PrintWindow 可能对此窗口无效");
        println!("  （常见于 DirectX 独占渲染的游戏，需改用 Windows.Graphics.Capture）");
    } else {
        println!("✓ 截图内容正常，可用于颜色查找");
    }
    println!();

    // 保存为 PPM 图片（简单格式，无需依赖）
    if let Err(e) = save_ppm(&bitmap, "capture_output.ppm") {
        eprintln!("保存图片失败: {}", e);
    } else {
        println!("已保存截图到 capture_output.ppm（可用支持 PPM 的看图工具打开）");
    }
}

/// 将位图保存为 PPM 格式（P6 二进制）
fn save_ppm(bitmap: &game_auto_keyboard::capture::Bitmap, path: &str) -> std::io::Result<()> {
    let mut file = File::create(path)?;
    write!(file, "P6\n{} {}\n255\n", bitmap.width, bitmap.height)?;

    let mut rgb_data = Vec::with_capacity((bitmap.width * bitmap.height * 3) as usize);
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            if let Some(rgb) = bitmap.get_rgb(x, y) {
                rgb_data.push(((rgb >> 16) & 0xFF) as u8); // R
                rgb_data.push(((rgb >> 8) & 0xFF) as u8);  // G
                rgb_data.push((rgb & 0xFF) as u8);         // B
            } else {
                rgb_data.extend_from_slice(&[0, 0, 0]);
            }
        }
    }
    file.write_all(&rgb_data)?;
    Ok(())
}
