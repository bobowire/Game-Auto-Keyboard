// 音频采集 + 环形缓冲测试
//
// 验证：能否从麦克风持续采集音频（与游戏语音共享）
// 运行后对着麦克风说话，观察音量电平变化

use game_auto_keyboard::voice::{AudioCapture, AudioRingBuffer, TARGET_SAMPLE_RATE};
use std::thread;
use std::time::{Duration, Instant};

fn main() {
    println!("=== 音频采集 + 环形缓冲测试 ===");
    println!();

    let capture = match AudioCapture::start() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("启动采集失败: {}", e);
            return;
        }
    };

    println!("✓ 麦克风采集已启动");
    println!("  输入采样率: {} Hz", capture.input_sample_rate());
    println!("  目标采样率: {} Hz", TARGET_SAMPLE_RATE);
    println!();
    println!("请对着麦克风说话，观察音量电平（Ctrl+C 退出）");
    println!("提示：可同时开着游戏语音软件，验证是否共享麦克风");
    println!();

    // 3 秒环形缓冲
    let mut ring = AudioRingBuffer::new(TARGET_SAMPLE_RATE as usize, 3);

    let start = Instant::now();
    let mut last_print = Instant::now();

    loop {
        let frame = capture.poll();
        if !frame.is_empty() {
            ring.push(&frame);
        }

        // 每 200ms 打印一次音量电平
        if last_print.elapsed() >= Duration::from_millis(200) {
            last_print = Instant::now();

            // 计算最近 200ms 的 RMS 音量
            let recent = ring.take_recent(200);
            let level = rms_level(&recent);
            let bar = volume_bar(level);
            print!("\r音量 [{}] {:5.0}   缓冲 {:5} 样本   已采集 {:.1}s     ",
                bar, level, ring.len(), start.elapsed().as_secs_f32());
            use std::io::Write;
            std::io::stdout().flush().ok();
        }

        thread::sleep(Duration::from_millis(10));
    }
}

/// 计算 RMS 音量（0-32767）
fn rms_level(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples.iter().map(|&s| (s as f64).powi(2)).sum();
    (sum_sq / samples.len() as f64).sqrt() as f32
}

/// 生成音量条形图
fn volume_bar(level: f32) -> String {
    let max_bars = 30;
    // 音量映射到 0-30（对数感更自然，这里简单线性）
    let filled = ((level / 3000.0) * max_bars as f32).min(max_bars as f32) as usize;
    let mut bar = String::new();
    for i in 0..max_bars {
        if i < filled {
            bar.push('#');
        } else {
            bar.push('-');
        }
    }
    bar
}
