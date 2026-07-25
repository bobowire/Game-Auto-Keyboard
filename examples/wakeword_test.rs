// 唤醒词 录制 → 训练 → 检测 完整测试
//
// 流程：
//   1. 引导录制 N 遍"小助手"（每遍按回车开始，自动录 1.5 秒）
//   2. 训练出 .rpw 模型
//   3. 进入检测模式，喊"小助手"看能否触发，显示 score 和延迟

use game_auto_keyboard::voice::{
    trim_silence, train_wakeword, AudioCapture, AudioRingBuffer, CommandRecorder, RecordState,
    WakewordDetector, TARGET_SAMPLE_RATE,
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

        // 裁剪首尾静音，只保留"小助手"词本身（避免连读时无法唤醒）
        let sr = TARGET_SAMPLE_RATE as usize;
        let trimmed = trim_silence(&samples, sr, 20, 300.0, 80);

        let path = format!("wakeword_samples/sample_{}.wav", i);
        write_wav(&path, &trimmed).expect("写入 wav 失败");
        sample_paths.push(path);
        println!(
            "  ✓ 已保存（原始 {} → 裁剪后 {} 样本）",
            samples.len(),
            trimmed.len()
        );
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

    // === 阶段3：完整状态机（待命→唤醒→VAD录音→输出指令音频） ===
    println!();
    println!("【阶段3】完整流程测试（Ctrl+C 退出）");
    println!("  说\"小助手\"唤醒，然后说一句指令，静音后自动结束");
    println!("  指令音频会存成 command_N.wav，可播放验证是否完整");
    println!();

    let mut detector = match WakewordDetector::from_model_file(MODEL_PATH, DETECT_THRESHOLD) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("加载模型失败: {}", e);
            return;
        }
    };

    // 3秒环形缓冲，用于唤醒后回溯取指令起点
    let mut ring = AudioRingBuffer::new(TARGET_SAMPLE_RATE as usize, 3);
    let mut state = VoiceState::Idle;
    let mut cmd_count = 0;

    capture.poll(); // 清空

    loop {
        let frame = capture.poll();
        if frame.is_empty() {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        ring.push(&frame);

        match &mut state {
            VoiceState::Idle => {
                if let Some(score) = detector.process(&frame) {
                    println!("  🎯 唤醒！score={:.3}，开始听指令...", score);
                    detector.reset();

                    // 从环形缓冲回溯 2 秒，补回唤醒时已说出的指令开头
                    // （连读时"小助手"之后的指令部分已在缓冲里，需要取回；
                    //   含唤醒词本身也没关系，ASR 阶段再 strip 掉前缀）
                    let mut recorder = CommandRecorder::new();
                    let backfill = ring.take_recent(2000);
                    recorder.prefill(&backfill);

                    state = VoiceState::Listening {
                        recorder,
                        started: Instant::now(),
                    };
                }
            }
            VoiceState::Listening { recorder, started } => {
                let elapsed = started.elapsed();

                // 超时控制：3秒没说话 / 最长8秒
                let no_speech_timeout =
                    !recorder.speech_started() && elapsed > Duration::from_secs(3);
                let max_timeout = elapsed > Duration::from_secs(8);

                let done_audio = match recorder.process(&frame) {
                    RecordState::Done(audio) => Some(audio),
                    RecordState::Recording => {
                        if no_speech_timeout {
                            println!("  ⏱ 超时（3秒无语音），回到待命");
                            None // 下面统一处理，用空音频表示放弃
                        } else if max_timeout {
                            println!("  ⏱ 达到最长8秒，强制结束");
                            Some(recorder.finish())
                        } else {
                            continue;
                        }
                    }
                };

                match done_audio {
                    Some(audio) if !audio.is_empty() => {
                        cmd_count += 1;
                        let secs = audio.len() as f32 / TARGET_SAMPLE_RATE as f32;
                        let path = format!("command_{}.wav", cmd_count);
                        write_wav(&path, &audio).ok();
                        println!(
                            "  ✓ 指令录制完成: {:.2}秒，已存 {}（可播放验证）",
                            secs, path
                        );
                        state = VoiceState::Idle;
                    }
                    _ => {
                        // 超时无语音，放弃
                        state = VoiceState::Idle;
                    }
                }
            }
        }
    }
}

/// 语音状态机
enum VoiceState {
    /// 待命：监听唤醒词
    Idle,
    /// 已唤醒：录制指令
    Listening {
        recorder: CommandRecorder,
        started: Instant,
    },
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
