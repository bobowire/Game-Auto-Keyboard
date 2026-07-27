// 音频前处理 DSP：抗混叠低通 + 去低频轰鸣高通
//
// 采集侧原来直接线性插值降采样，48kHz→16kHz 时超过 8kHz 的成分会折叠（混叠）
// 回语音频段变成宽带噪声，既伤唤醒词准确率，也让 VAD 更容易把噪声当语音。
// 这里在降采样前加 4 阶 Butterworth 低通（截止 7.2kHz），降采样后再加 2 阶
// 高通（80Hz）滤掉风扇/电流的低频轰鸣。
//
// 滤波器状态必须跨 cpal 回调保持，否则每个帧边界都会产生不连续的爆音，
// 所以做成结构体由采集流持有。

use std::f32::consts::PI;

/// 4 阶 Butterworth 级联所需的两级 Q 值
const BUTTERWORTH_Q4: [f32; 2] = [0.541_196, 1.306_563];
/// 抗混叠低通截止频率（留出过渡带，低于 16k 的奈奎斯特 8k）
const ANTI_ALIAS_CUTOFF_HZ: f32 = 7200.0;
/// 高通截止频率，滤掉低频轰鸣
const HIGHPASS_CUTOFF_HZ: f32 = 80.0;
/// 二阶 Butterworth（最平坦）的 Q
const Q_BUTTERWORTH_2: f32 = 0.707_106_8;

/// 双二阶（biquad）IIR 滤波器，Direct Form I
///
/// 系数用 RBJ Audio EQ Cookbook 的标准公式算，运行时按采样率构造，
/// 这样不同麦克风的输入采样率都能用同一套代码。
#[derive(Clone, Copy)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    // 历史状态（跨调用保持）
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    /// 构造低通。`q` 用于级联成高阶 Butterworth
    fn lowpass(sample_rate: f32, cutoff_hz: f32, q: f32) -> Self {
        let w0 = 2.0 * PI * cutoff_hz / sample_rate;
        let (sin_w0, cos_w0) = (w0.sin(), w0.cos());
        let alpha = sin_w0 / (2.0 * q);

        let b0 = (1.0 - cos_w0) / 2.0;
        let b1 = 1.0 - cos_w0;
        let b2 = (1.0 - cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        Self::normalized(b0, b1, b2, a0, a1, a2)
    }

    /// 构造高通
    fn highpass(sample_rate: f32, cutoff_hz: f32, q: f32) -> Self {
        let w0 = 2.0 * PI * cutoff_hz / sample_rate;
        let (sin_w0, cos_w0) = (w0.sin(), w0.cos());
        let alpha = sin_w0 / (2.0 * q);

        let b0 = (1.0 + cos_w0) / 2.0;
        let b1 = -(1.0 + cos_w0);
        let b2 = (1.0 + cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        Self::normalized(b0, b1, b2, a0, a1, a2)
    }

    /// 把系数按 a0 归一化后存下
    fn normalized(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// 处理单个样本
    #[inline]
    fn process(&mut self, x0: f32) -> f32 {
        let y0 = self.b0 * x0 + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x0;
        self.y2 = self.y1;
        self.y1 = y0;
        y0
    }
}

/// 采集前处理链：降采样前抗混叠 + 降采样后去轰鸣
///
/// 由采集流独占持有，状态跨 cpal 回调连续。
pub struct PreFilter {
    /// 输入采样率下的抗混叠低通（4 阶 = 2 级级联）
    anti_alias: [Biquad; 2],
    /// 目标采样率下的高通
    rumble: Biquad,
    /// 输入采样率等于目标采样率时无需抗混叠
    needs_anti_alias: bool,
}

impl PreFilter {
    pub fn new(input_rate: u32, target_rate: u32) -> Self {
        let in_sr = input_rate as f32;
        // 截止频率不能超过输入侧的奈奎斯特，低采样率设备上要收紧
        let cutoff = ANTI_ALIAS_CUTOFF_HZ.min(in_sr * 0.45);

        Self {
            anti_alias: [
                Biquad::lowpass(in_sr, cutoff, BUTTERWORTH_Q4[0]),
                Biquad::lowpass(in_sr, cutoff, BUTTERWORTH_Q4[1]),
            ],
            rumble: Biquad::highpass(target_rate as f32, HIGHPASS_CUTOFF_HZ, Q_BUTTERWORTH_2),
            needs_anti_alias: input_rate > target_rate,
        }
    }

    /// 降采样前：抗混叠低通（原地处理）
    pub fn apply_anti_alias(&mut self, samples: &mut [i16]) {
        if !self.needs_anti_alias {
            return;
        }
        for s in samples.iter_mut() {
            let y = self.anti_alias[0].process(*s as f32);
            let y = self.anti_alias[1].process(y);
            *s = clamp_to_i16(y);
        }
    }

    /// 降采样后：高通去低频轰鸣（原地处理）
    pub fn apply_rumble_filter(&mut self, samples: &mut [i16]) {
        for s in samples.iter_mut() {
            *s = clamp_to_i16(self.rumble.process(*s as f32));
        }
    }
}

/// 滤波可能轻微过冲，饱和截断而不是回绕
#[inline]
fn clamp_to_i16(v: f32) -> i16 {
    v.clamp(i16::MIN as f32, i16::MAX as f32) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 生成指定频率的正弦波
    fn sine(freq: f32, sample_rate: f32, len: usize) -> Vec<i16> {
        (0..len)
            .map(|i| {
                let t = i as f32 / sample_rate;
                ((2.0 * PI * freq * t).sin() * 10000.0) as i16
            })
            .collect()
    }

    fn rms(samples: &[i16]) -> f32 {
        let sum: f64 = samples.iter().map(|&s| (s as f64).powi(2)).sum();
        (sum / samples.len() as f64).sqrt() as f32
    }

    #[test]
    fn anti_alias_attenuates_above_cutoff() {
        // 12kHz @ 48k 远高于 7.2k 截止，应被大幅衰减
        let mut high = sine(12000.0, 48000.0, 4800);
        let before = rms(&high);
        PreFilter::new(48000, 16000).apply_anti_alias(&mut high);
        let after = rms(&high);
        assert!(
            after < before * 0.15,
            "12kHz 应衰减到 15% 以下，实际 {:.3}",
            after / before
        );
    }

    #[test]
    fn anti_alias_preserves_speech_band() {
        // 1kHz 在语音频段内，应基本保留（跳过瞬态收敛段）
        let mut speech = sine(1000.0, 48000.0, 4800);
        let before = rms(&speech[960..]);
        PreFilter::new(48000, 16000).apply_anti_alias(&mut speech);
        let after = rms(&speech[960..]);
        assert!(
            after > before * 0.85,
            "1kHz 应保留 85% 以上，实际 {:.3}",
            after / before
        );
    }

    #[test]
    fn rumble_filter_attenuates_low_freq() {
        // 30Hz 低频轰鸣应被 80Hz 高通压掉
        let mut rumble = sine(30.0, 16000.0, 8000);
        let before = rms(&rumble);
        PreFilter::new(48000, 16000).apply_rumble_filter(&mut rumble);
        let after = rms(&rumble);
        assert!(
            after < before * 0.35,
            "30Hz 应衰减到 35% 以下，实际 {:.3}",
            after / before
        );
    }

    #[test]
    fn no_anti_alias_when_rates_match() {
        // 采样率相同时不做抗混叠，信号原样通过
        let original = sine(6000.0, 16000.0, 1600);
        let mut same = original.clone();
        PreFilter::new(16000, 16000).apply_anti_alias(&mut same);
        assert_eq!(same, original);
    }

    #[test]
    fn filter_state_is_continuous_across_calls() {
        // 分块处理与整块处理结果应一致（验证状态跨调用保持）
        let signal = sine(1000.0, 48000.0, 2400);

        let mut whole = signal.clone();
        PreFilter::new(48000, 16000).apply_anti_alias(&mut whole);

        let mut chunked = signal.clone();
        let mut f = PreFilter::new(48000, 16000);
        for chunk in chunked.chunks_mut(480) {
            f.apply_anti_alias(chunk);
        }

        assert_eq!(whole, chunked);
    }
}
