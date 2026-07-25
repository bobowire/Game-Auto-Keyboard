// 唤醒词检测（rustpotter）+ 训练
//
// - 训练：用户录制若干 wav 样本，构建 WakewordRef 模板并保存为模型文件
// - 检测：加载模型，持续喂入音频帧，检测到唤醒词返回 true
//
// 注意：rustpotter 的 process_samples 要求每次传入恰好 samples_per_frame 个样本，
// 因此内部做帧缓冲，攒够一帧再喂。

use rustpotter::{
    Rustpotter, RustpotterConfig, WakewordRef, WakewordRefBuildFromFiles, WakewordSave, WakewordLoad,
};

use crate::voice::capture::TARGET_SAMPLE_RATE;

/// 唤醒词检测器
pub struct WakewordDetector {
    rustpotter: Rustpotter,
    /// 帧缓冲：攒够 samples_per_frame 才喂给 rustpotter
    frame_buf: Vec<i16>,
    samples_per_frame: usize,
    /// 检测阈值（score 超过才算命中）
    threshold: f32,
}

impl WakewordDetector {
    /// 从模型文件加载唤醒词检测器
    pub fn from_model_file(model_path: &str, threshold: f32) -> Result<Self, String> {
        let config = Self::default_config();
        let mut rustpotter =
            Rustpotter::new(&config).map_err(|e| format!("创建 Rustpotter 失败: {}", e))?;

        let wakeword = WakewordRef::load_from_file(model_path)
            .map_err(|e| format!("加载唤醒词模型失败: {}", e))?;
        rustpotter
            .add_wakeword_ref("wakeword", wakeword)
            .map_err(|e| format!("添加唤醒词失败: {}", e))?;

        let samples_per_frame = rustpotter.get_samples_per_frame();

        Ok(Self {
            rustpotter,
            frame_buf: Vec::with_capacity(samples_per_frame),
            samples_per_frame,
            threshold,
        })
    }

    /// 喂入音频样本（i16 单声道 16kHz），检测到唤醒词返回 Some(score)
    pub fn process(&mut self, samples: &[i16]) -> Option<f32> {
        self.frame_buf.extend_from_slice(samples);

        let mut detected: Option<f32> = None;
        // 按帧长切分喂给 rustpotter
        while self.frame_buf.len() >= self.samples_per_frame {
            let frame: Vec<i16> = self.frame_buf.drain(..self.samples_per_frame).collect();
            if let Some(det) = self.rustpotter.process_samples(frame) {
                if det.score >= self.threshold {
                    detected = Some(det.score);
                }
            }
        }
        detected
    }

    /// 重置内部状态（唤醒后调用，避免连续触发）
    pub fn reset(&mut self) {
        self.rustpotter.reset();
        self.frame_buf.clear();
    }

    /// 默认配置：16kHz 单声道 i16
    fn default_config() -> RustpotterConfig {
        let mut config = RustpotterConfig::default();
        config.fmt.sample_rate = TARGET_SAMPLE_RATE as usize;
        config.fmt.channels = 1;
        config
    }
}

/// 从 wav 样本文件训练唤醒词模型并保存
///
/// - `name`: 唤醒词名称
/// - `sample_paths`: 用户录制的 wav 文件路径列表（3-5 个）
/// - `output_path`: 模型保存路径（.rpw）
/// - `threshold`: 检测阈值（None 用默认）
pub fn train_wakeword(
    name: &str,
    sample_paths: Vec<String>,
    output_path: &str,
    threshold: Option<f32>,
) -> Result<(), String> {
    if sample_paths.is_empty() {
        return Err("至少需要一个录音样本".to_string());
    }

    // mfcc_size 用默认 16
    let wakeword = WakewordRef::new_from_sample_files(
        name.to_string(),
        threshold,
        None, // avg_threshold
        sample_paths,
        16, // mfcc_size
    )
    .map_err(|e| format!("训练唤醒词失败: {}", e))?;

    wakeword
        .save_to_file(output_path)
        .map_err(|e| format!("保存唤醒词模型失败: {}", e))?;

    Ok(())
}
