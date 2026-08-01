// 唤醒词训练：录音采集 → 裁剪静音 → 训练 rustpotter 模型。
//
// 本模块是 app 子模块中最独立的一块：不调用 slots / voice_ctrl / overlay 任何方法，
// 只读自身 wakeword_training 状态与 events.sender()。write_wav 随训练逻辑一同放置于此。

use super::{App, WakewordTrainingState, WAKEWORD_MODEL_PATH, WAKEWORD_THRESHOLD};
use std::time::Instant;

use crate::event_bus::WakeTicker;
use crate::voice::{AudioCapture, TARGET_SAMPLE_RATE, train_wakeword, trim_silence};

impl App {
    /// 处理唤醒词训练的录音逻辑
    pub(super) fn process_wakeword_training(&mut self) {
        let Some(training) = &mut self.wakeword_training else { return };

        if training.is_recording {
            // 持续采集音频
            if let Some(capture) = &training.capture {
                let frame = capture.poll();
                training.recording_buffer.extend_from_slice(&frame);
            }

            // 检查录音是否完成
            if let Some(start) = training.record_start {
                let elapsed = start.elapsed().as_secs_f32();
                if elapsed >= training.record_duration {
                    // 录音完成，处理音频数据
                    training.is_recording = false;
                    training.record_start = None;

                    // 裁剪首尾静音
                    let sr = TARGET_SAMPLE_RATE as usize;
                    let trimmed = trim_silence(&training.recording_buffer, sr, 20, 300.0, 80);

                    // 保存样本
                    training.samples.push(trimmed);

                    if training.samples.len() >= training.total_rounds {
                        // 所有样本录制完成，开始训练
                        training.status_msg = "录制完成！正在训练模型...".to_string();
                        self.train_wakeword_model();
                    } else {
                        // 进入下一轮
                        training.current_round += 1;
                        training.status_msg = format!("✓ 第 {} 遍完成", training.samples.len());
                    }
                }
            }
        }
    }

    /// 训练唤醒词模型
    fn train_wakeword_model(&mut self) {
        let Some(training) = &self.wakeword_training else { return };

        // 1. 保存样本到临时文件（如果配置开启）
        let mut sample_paths = Vec::new();

        if self.save_wakeword_samples {
            std::fs::create_dir_all("wakeword_samples").ok();

            for (i, samples) in training.samples.iter().enumerate() {
                let path = format!("wakeword_samples/sample_{}.wav", i + 1);
                if let Err(e) = write_wav(&path, samples) {
                    self.status = format!("保存样本失败: {}", e);
                    self.show_wakeword_guide = false;
                    self.wakeword_training = None;
                    return;
                }
                sample_paths.push(path);
            }
        } else {
            // 不保存文件，使用临时文件
            for (i, samples) in training.samples.iter().enumerate() {
                let path = format!("wakeword_sample_temp_{}.wav", i + 1);
                if let Err(e) = write_wav(&path, samples) {
                    self.status = format!("保存临时样本失败: {}", e);
                    self.show_wakeword_guide = false;
                    self.wakeword_training = None;
                    return;
                }
                sample_paths.push(path);
            }
        }

        // 2. 训练模型
        let result = train_wakeword("小助手", sample_paths.clone(), WAKEWORD_MODEL_PATH, Some(WAKEWORD_THRESHOLD));

        // 3. 清理临时文件（如果不保存样本）
        if !self.save_wakeword_samples {
            for path in &sample_paths {
                std::fs::remove_file(path).ok();
            }
        }

        // 4. 处理训练结果
        match result {
            Ok(_) => {
                self.status = format!("✓ 唤醒词模型训练完成！已保存到 {}", WAKEWORD_MODEL_PATH);
                self.show_wakeword_guide = false;
                self.wakeword_training = None;
            }
            Err(e) => {
                self.status = format!("训练失败: {}", e);
                self.show_wakeword_guide = false;
                self.wakeword_training = None;
            }
        }
    }

    pub(super) fn start_wakeword_training(&mut self) {
        // 尝试启动音频采集
        let capture = match AudioCapture::start() {
            Ok(c) => {
                self.status = "麦克风已就绪，准备录制".to_string();
                Some(c)
            }
            Err(e) => {
                self.status = format!("启动麦克风失败: {}", e);
                self.show_wakeword_guide = false;
                return;
            }
        };

        self.wakeword_training = Some(WakewordTrainingState {
            current_round: 1,
            total_rounds: 4,
            is_recording: false,
            record_start: None,
            record_duration: 1.5,
            samples: Vec::new(),
            status_msg: "准备录制第 1 遍".to_string(),
            capture,
            recording_buffer: Vec::new(),
            // 20ms 一次，保证录音期间 update 稳定被调用
            _ticker: WakeTicker::start(self.events.sender(), 20),
        });
    }

    /// 开始录制一遍
    pub(super) fn start_wakeword_recording(&mut self) {
        if let Some(training) = &mut self.wakeword_training {
            // 清空之前的缓冲
            if let Some(capture) = &training.capture {
                capture.poll();
            }
            training.recording_buffer.clear();
            training.is_recording = true;
            training.record_start = Some(Instant::now());
            training.status_msg = "正在录制...".to_string();
        }
    }
}

/// 写 16bit 单声道 PCM wav（采样率 = TARGET_SAMPLE_RATE）
fn write_wav(path: &str, samples: &[i16]) -> std::io::Result<()> {
    use std::fs::File;
    use std::io::{BufWriter, Write};

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
