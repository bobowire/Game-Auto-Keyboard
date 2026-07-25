// 音频处理工具

/// 裁剪音频首尾的静音段，只保留有声部分
///
/// 用于唤醒词训练样本：去掉"小助手"前后的静音，让模型只学词本身的声学特征，
/// 避免因训练样本自带尾部静音导致连读时无法唤醒。
///
/// - `samples`: 输入音频（i16 单声道）
/// - `frame_ms`: 能量检测的帧长（毫秒）
/// - `sample_rate`: 采样率
/// - `threshold`: RMS 能量阈值，低于此值视为静音
/// - `padding_ms`: 有声段前后保留的余量（避免切太狠削掉辅音）
pub fn trim_silence(
    samples: &[i16],
    sample_rate: usize,
    frame_ms: usize,
    threshold: f32,
    padding_ms: usize,
) -> Vec<i16> {
    if samples.is_empty() {
        return Vec::new();
    }

    let frame_len = (sample_rate * frame_ms / 1000).max(1);
    let padding = sample_rate * padding_ms / 1000;

    // 逐帧计算 RMS，标记有声帧
    let mut first_voice: Option<usize> = None;
    let mut last_voice: Option<usize> = None;

    let mut i = 0;
    while i < samples.len() {
        let end = (i + frame_len).min(samples.len());
        let rms = rms(&samples[i..end]);
        if rms >= threshold {
            if first_voice.is_none() {
                first_voice = Some(i);
            }
            last_voice = Some(end);
        }
        i += frame_len;
    }

    match (first_voice, last_voice) {
        (Some(start), Some(end)) => {
            // 前后加 padding 余量并裁剪到边界内
            let s = start.saturating_sub(padding);
            let e = (end + padding).min(samples.len());
            samples[s..e].to_vec()
        }
        // 全是静音，原样返回（让上层决定是否重录）
        _ => samples.to_vec(),
    }
}

/// 计算一段音频的 RMS 能量
pub fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples.iter().map(|&s| (s as f64).powi(2)).sum();
    (sum_sq / samples.len() as f64).sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rms_silence() {
        assert_eq!(rms(&[0, 0, 0, 0]), 0.0);
    }

    #[test]
    fn test_rms_nonzero() {
        let r = rms(&[100, -100, 100, -100]);
        assert!((r - 100.0).abs() < 0.1);
    }

    #[test]
    fn test_trim_removes_leading_trailing_silence() {
        // 前后静音，中间有声
        let mut audio = vec![0i16; 1000]; // 前导静音
        audio.extend(vec![5000i16; 1000]); // 有声
        audio.extend(vec![0i16; 1000]); // 尾部静音

        let trimmed = trim_silence(&audio, 1000, 10, 1000.0, 0);
        // 应该显著短于原始（去掉了大部分静音）
        assert!(trimmed.len() < audio.len());
        assert!(trimmed.len() >= 1000); // 至少保留有声部分
    }

    #[test]
    fn test_trim_all_silence_returns_original() {
        let audio = vec![0i16; 500];
        let trimmed = trim_silence(&audio, 1000, 10, 1000.0, 0);
        assert_eq!(trimmed.len(), audio.len());
    }
}
