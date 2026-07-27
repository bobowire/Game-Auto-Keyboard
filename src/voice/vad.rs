// 语音活动检测（VAD）+ 指令录音
//
// 唤醒后用于判断"用户说完了没"：持续喂音频，检测到连续静音超过阈值即认为说完。
// WebRTC VAD 要求帧长为 10/20/30ms，这里用 20ms（16kHz = 320 样本）。
//
// 抗底噪设计（单靠 WebRTC VAD 在有稳态底噪时会一直判"有声"，导致录音只能靠
// runtime 的 8 秒兜底超时结束，用户感知为"要等好几秒才有反应"）：
//   1. VAD 用 VeryAggressive 档（最严）
//   2. 叠一层自适应噪声底 + RMS 能量门：只有 VAD 判有声 **且** 能量显著高于
//      噪声底才算语音，噪声底在非语音帧上用 EMA 持续更新，自动适应环境
//   3. 语音起始要求连续 2 帧，避免键盘/鼠标单击这类瞬态误触发
//
// 同时记录首/末语音帧位置，结束时裁掉首尾静音，减少 ASR 上传体积和识别延迟。

use webrtc_vad::{SampleRate, Vad, VadMode};

use crate::voice::capture::TARGET_SAMPLE_RATE;

/// VAD 帧长：20ms @ 16kHz = 320 样本
const VAD_FRAME_SAMPLES: usize = (TARGET_SAMPLE_RATE as usize / 1000) * 20;
/// 判定"说完"的静音时长（毫秒）
const SILENCE_END_MS: u32 = 500;
/// 对应的静音帧数（每帧 20ms）
const SILENCE_END_FRAMES: u32 = SILENCE_END_MS / 20;
/// 判定"开始说话"需要的连续语音帧数（抗瞬态误触发）
const SPEECH_START_FRAMES: u32 = 2;
/// 噪声底 EMA 更新系数（越小越平滑）
const NOISE_EMA_ALPHA: f32 = 0.05;
/// 语音能量需高于噪声底的倍数
const SPEECH_SNR_FACTOR: f32 = 2.5;
/// 语音能量绝对下限（噪声底极低时防止把细微杂音当语音）
const MIN_SPEECH_RMS: f32 = 180.0;
/// 噪声底初始值（还没测到静音帧时的保守估计）
const INITIAL_NOISE_FLOOR: f32 = 120.0;
/// 裁剪时语音段前保留的余量（毫秒，避免削掉起始辅音）
const TRIM_LEAD_PAD_MS: usize = 150;
/// 裁剪时语音段后保留的余量（毫秒）
const TRIM_TAIL_PAD_MS: usize = 200;

/// 指令录音器：唤醒后持续喂音频，聚合成完整指令，检测到静音结束时输出
pub struct CommandRecorder {
    vad: Vad,
    frame_buf: Vec<i16>,
    /// 已录制的指令音频（含说话内容）
    command: Vec<i16>,
    /// 连续静音帧计数
    silence_frames: u32,
    /// 连续语音帧计数（用于语音起始的滞后判定）
    speech_frames: u32,
    /// 是否已经检测到过语音（避免开头静音就结束）
    speech_started: bool,
    /// 自适应噪声底（RMS）
    noise_floor: f32,
    /// `command` 中首个语音样本的偏移
    first_speech: Option<usize>,
    /// `command` 中末个语音帧的结束偏移
    last_speech_end: Option<usize>,
}

/// 喂入音频后的状态
#[derive(Debug, PartialEq)]
pub enum RecordState {
    /// 还在录音（等待更多音频或等待说话）
    Recording,
    /// 检测到说完，返回完整指令音频（已裁掉首尾静音）
    Done(Vec<i16>),
}

impl CommandRecorder {
    pub fn new() -> Self {
        // VeryAggressive：最严档。底噪环境下比 Aggressive 明显更少把噪声判成语音，
        // 配合下面的能量门足以让静音判定正常收敛。
        let vad = Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::VeryAggressive);
        Self {
            vad,
            frame_buf: Vec::with_capacity(VAD_FRAME_SAMPLES),
            command: Vec::new(),
            silence_frames: 0,
            speech_frames: 0,
            speech_started: false,
            noise_floor: INITIAL_NOISE_FLOOR,
            first_speech: None,
            last_speech_end: None,
        }
    }

    /// 用回溯的音频预填充（唤醒后从环形缓冲取出的指令起点）
    pub fn prefill(&mut self, samples: &[i16]) {
        self.push(samples);
    }

    /// 喂入音频，返回录音状态
    pub fn process(&mut self, samples: &[i16]) -> RecordState {
        self.push(samples);

        // 按 VAD 帧长处理
        while self.frame_buf.len() >= VAD_FRAME_SAMPLES {
            let frame: Vec<i16> = self.frame_buf.drain(..VAD_FRAME_SAMPLES).collect();
            let frame_start = self.command.len();

            // 帧数据始终累积到指令音频
            self.command.extend_from_slice(&frame);

            if self.classify_frame(&frame) {
                self.speech_frames += 1;
                self.silence_frames = 0;

                // 起始需连续若干帧确认，避免瞬态噪声误开
                if self.speech_frames >= SPEECH_START_FRAMES {
                    if self.first_speech.is_none() {
                        // 回退到这串连续语音帧的真正起点
                        let back = (SPEECH_START_FRAMES as usize - 1) * VAD_FRAME_SAMPLES;
                        self.first_speech = Some(frame_start.saturating_sub(back));
                    }
                    self.speech_started = true;
                    self.last_speech_end = Some(self.command.len());
                }
            } else {
                self.speech_frames = 0;
                if self.speech_started {
                    // 说过话之后的静音才计数
                    self.silence_frames += 1;
                    if self.silence_frames >= SILENCE_END_FRAMES {
                        return RecordState::Done(self.take_trimmed());
                    }
                }
            }
        }

        RecordState::Recording
    }

    /// 判断一帧是否为语音：WebRTC VAD 与能量门都通过才算
    ///
    /// 非语音帧用于更新噪声底，所以环境噪声变化时门限会自动跟随。
    fn classify_frame(&mut self, frame: &[i16]) -> bool {
        let vad_voice = self.vad.is_voice_segment(frame).unwrap_or(false);
        let energy = frame_rms(frame);
        let gate = (self.noise_floor * SPEECH_SNR_FACTOR).max(MIN_SPEECH_RMS);
        let is_voice = vad_voice && energy >= gate;

        if !is_voice {
            // 只在非语音帧更新噪声底，避免语音把底噪估计抬高
            self.noise_floor += NOISE_EMA_ALPHA * (energy - self.noise_floor);
        }

        is_voice
    }

    /// 取出指令音频并裁掉首尾静音（各留一点余量）
    fn take_trimmed(&mut self) -> Vec<i16> {
        let audio = std::mem::take(&mut self.command);
        let lead_pad = TARGET_SAMPLE_RATE as usize * TRIM_LEAD_PAD_MS / 1000;
        let tail_pad = TARGET_SAMPLE_RATE as usize * TRIM_TAIL_PAD_MS / 1000;

        match (self.first_speech, self.last_speech_end) {
            (Some(start), Some(end)) => {
                let s = start.saturating_sub(lead_pad);
                let e = (end + tail_pad).min(audio.len());
                if s < e {
                    audio[s..e].to_vec()
                } else {
                    audio
                }
            }
            // 没检测到语音边界，原样返回让上层决定
            _ => audio,
        }
    }

    /// 强制结束（超时时调用），返回已录制的音频（同样裁掉首尾静音）
    pub fn finish(&mut self) -> Vec<i16> {
        // 把剩余不足一帧的也带上
        let tail = std::mem::take(&mut self.frame_buf);
        self.command.extend_from_slice(&tail);
        self.take_trimmed()
    }

    /// 是否已检测到语音开始
    pub fn speech_started(&self) -> bool {
        self.speech_started
    }

    /// 当前估计的噪声底（调试用）
    pub fn noise_floor(&self) -> f32 {
        self.noise_floor
    }

    fn push(&mut self, samples: &[i16]) {
        self.frame_buf.extend_from_slice(samples);
    }
}

impl Default for CommandRecorder {
    fn default() -> Self {
        Self::new()
    }
}

/// 单帧 RMS 能量
fn frame_rms(frame: &[i16]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = frame.iter().map(|&s| (s as f64) * (s as f64)).sum();
    (sum_sq / frame.len() as f64).sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    /// 低幅值白噪声，模拟环境底噪
    fn noise(len: usize, amp: f32, seed: u32) -> Vec<i16> {
        // 简单 LCG，避免依赖 rand 且保证可重复
        let mut s = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
        (0..len)
            .map(|_| {
                s = s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let unit = ((s >> 16) as f32 / 32768.0) - 1.0;
                (unit * amp) as i16
            })
            .collect()
    }

    /// 类语音信号：带谐波的强正弦，能量远高于底噪
    fn speech_like(len: usize, amp: f32) -> Vec<i16> {
        (0..len)
            .map(|i| {
                let t = i as f32 / TARGET_SAMPLE_RATE as f32;
                let v = (2.0 * PI * 220.0 * t).sin() * 0.6
                    + (2.0 * PI * 700.0 * t).sin() * 0.3
                    + (2.0 * PI * 1500.0 * t).sin() * 0.1;
                (v * amp) as i16
            })
            .collect()
    }

    #[test]
    fn silence_alone_never_finishes() {
        // 纯静音（未说话）不应触发 Done，否则唤醒后立刻就结束了
        let mut rec = CommandRecorder::new();
        for i in 0..50 {
            let state = rec.process(&noise(VAD_FRAME_SAMPLES, 30.0, i));
            assert_eq!(state, RecordState::Recording);
        }
        assert!(!rec.speech_started());
    }

    #[test]
    fn noise_floor_adapts_to_ambient() {
        // 持续底噪应把噪声底抬到接近其 RMS，从而提高语音门限
        let mut rec = CommandRecorder::new();
        for i in 0..100 {
            rec.process(&noise(VAD_FRAME_SAMPLES, 500.0, i));
        }
        let floor = rec.noise_floor();
        assert!(
            floor > INITIAL_NOISE_FLOOR,
            "噪声底应随环境上升，实际 {:.1}",
            floor
        );
    }

    #[test]
    fn low_energy_frames_do_not_start_speech() {
        // 能量低于门限的帧即使 VAD 判有声也不算语音开始
        let mut rec = CommandRecorder::new();
        for _ in 0..30 {
            rec.process(&speech_like(VAD_FRAME_SAMPLES, 40.0));
        }
        assert!(
            !rec.speech_started(),
            "低能量信号不应触发语音开始（噪声底 {:.1}）",
            rec.noise_floor()
        );
    }

    #[test]
    fn single_transient_frame_does_not_start_speech() {
        // 单帧瞬态（键盘敲击）不应触发语音开始，需连续 SPEECH_START_FRAMES 帧
        let mut rec = CommandRecorder::new();
        rec.process(&speech_like(VAD_FRAME_SAMPLES, 8000.0));
        rec.process(&noise(VAD_FRAME_SAMPLES, 30.0, 7));
        assert!(!rec.speech_started());
    }

    #[test]
    fn finish_trims_leading_silence() {
        // 前面塞 1 秒静音再说话，finish 后应裁掉大部分前导静音
        let mut rec = CommandRecorder::new();
        let lead_silence = TARGET_SAMPLE_RATE as usize; // 1 秒
        rec.prefill(&noise(lead_silence, 30.0, 3));
        for _ in 0..25 {
            rec.process(&speech_like(VAD_FRAME_SAMPLES, 9000.0));
        }
        assert!(rec.speech_started(), "应已检测到语音");

        let audio = rec.finish();
        // 裁剪后不该还包含整秒的前导静音
        assert!(
            audio.len() < lead_silence,
            "应裁掉前导静音，实际长度 {} 样本",
            audio.len()
        );
    }

    #[test]
    fn trim_keeps_lead_padding() {
        // 裁剪要保留 padding，不能把起始辅音削掉
        let mut rec = CommandRecorder::new();
        rec.prefill(&noise(TARGET_SAMPLE_RATE as usize, 30.0, 5));
        let speech_frames = 25;
        for _ in 0..speech_frames {
            rec.process(&speech_like(VAD_FRAME_SAMPLES, 9000.0));
        }
        let audio = rec.finish();
        let speech_len = speech_frames * VAD_FRAME_SAMPLES;
        // 结果应比纯语音段长（含 padding），但远短于原始总长
        assert!(
            audio.len() > speech_len,
            "应保留 padding 余量，实际 {} vs 语音 {}",
            audio.len(),
            speech_len
        );
    }

    #[test]
    fn no_speech_returns_audio_unchanged() {
        // 从未检测到语音时原样返回，交给上层判断
        let mut rec = CommandRecorder::new();
        let n = VAD_FRAME_SAMPLES * 10;
        rec.prefill(&noise(n, 20.0, 11));
        let audio = rec.finish();
        assert_eq!(audio.len(), n);
    }
}
