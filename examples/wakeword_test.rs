// 唤醒词 录制 → 训练 → 检测 完整测试
//
// 流程：
//   1. 引导录制 N 遍"小助手"（每遍按回车开始，自动录 1.5 秒）
//   2. 训练出 .rpw 模型
//   3. 进入检测模式，喊"小助手"看能否触发，显示 score 和延迟

use game_auto_keyboard::voice::{
    train_wakeword, AudioCapture, WakewordDetector, TARGET_SAMPLE_RATE,
};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::thread;
use std::time::{Duration, Instant};

const SAMPLE_COUNT: usize = 4; // 录制遍数
const RECORD_SECS: f32 = 1.5; // 每遍录音时长
const MODEL_PATH: &str = "wakeword_model.rpw";
const DETECT_THRESHOLD: f32 = 0.5;

fn main() {
    println!("=== 唤醒词 录制→训练→检测 测试 ===");
    println!();

    let capture = match AudioCapture::start() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("启动麦克风失败: {}", e);
            return;
        }
    };
    println!("✓ 麦克风已就绪（{}Hz）", capture.input_sample_rate());
    println!();

    // === 阶段1：录制样本 ===
    fs::create_dir_all("wakeword_samples").ok();
    let mut sample_paths = Vec::new();

    println!("【阶段1】录制唤醒词样本，共 {} 遍", SAMPLE_COUNT);
    println!("每次按回车后，清晰地说\"小助手\"");
    println!();

    for i in 1..=SAMPLE_COUNT {
        print!("第 {}/{} 遍 - 按回车开始录音...", i, SAMPLE_COUNT);
        wait_enter();

        // 清空之前缓冲的音频
        capture.poll();

        println!("  🔴 录音中（{:.1}秒）...", RECORD_SECS);
        let samples = record(&capture, RECORD_SECS);
        let path = format!("wakeword_samples/sample_{}.wav", i);
        write_wav(&path, &samples).expect("写入 wav 失败");
        sample_paths.push(path);
        println!("  ✓ 已保存（{} 样本）", samples.len());
    }

    // === 阶段2：训练 ===
    println!();
    println!("【阶段2】训练唤醒词模型...");
    match train_wakeword("小助手", sample_paths, MODEL_PATH, Some(DETECT_THRESHOLD)) {
        Ok(_) => println!("  ✓ 模型已保存: {}", MODEL_PATH),
        Err(e) => {
            eprintln!("  训练失败: {}", e);
            return;
        }
    }

    // === 阶段3：检测 ===
    println!();
    println!("【阶段3】检测模式 - 喊\"小助手\"试试（Ctrl+C 退出）");
    println!();

    let mut detector = match WakewordDetector::from_model_file(MODEL_PATH, DETECT_THRESHOLD) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("加载模型失败: {}", e);
            return;
        }
    };

    capture.poll(); // 清空
    let mut detect_count = 0;

    loop {
        let frame = capture.poll();
        if !frame.is_empty() {
            if let Some(score) = detector.process(&frame) {
                detect_count += 1;
                println!(
                    "  🎯 检测到唤醒词！score={:.3}  (第 {} 次)",
                    score, detect_count
                );
                detector.reset();
                // 短暂冷却，避免连续触发
                thread::sleep(Duration::from_millis(500));
                capture.poll();
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// 录制指定秒数的音频
fn record(capture: &AudioCapture, secs: f32) -> Vec<i16> {
    let mut samples = Vec::new();
    let start = Instant::now();
    while start.elapsed().as_secs_f32() < secs {
        samples.extend(capture.poll());
        thread::sleep(Duration::from_millis(10));
    }
    samples
}

/// 等待用户按回车
fn wait_enter() {
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).ok();
}

/// 写 16bit 单声道 PCM wav（采样率 = TARGET_SAMPLE_RATE）
fn write_wav(path: &str, samples: &[i16]) -> std::io::Result<()> {
    let mut w = BufWriter::new(File::create(path)?);
    let sr = TARGET_SAMPLE_RATE;
    let data_bytes = (samples.len() * 2) as u32;
    let byte_rate = sr * 2;

    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_bytes).to_le_bytes())?;
    w.write_all(b"WAVE")?;
    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&1u16.to_le_bytes())?; // PCM
    w.write_all(&1u16.to_le_bytes())?; // 单声道
    w.write_all(&sr.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&2u16.to_le_bytes())?;
    w.write_all(&16u16.to_le_bytes())?;
    w.write_all(b"data")?;
    w.write_all(&data_bytes.to_le_bytes())?;
    for s in samples {
        w.write_all(&s.to_le_bytes())?;
    }
    Ok(())
}
