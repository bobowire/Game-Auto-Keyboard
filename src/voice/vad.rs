// 语音活动检测（VAD）+ 指令录音
//
// 唤醒后用于判断"用户说完了没"：持续喂音频，检测到连续静音超过阈值即认为说完。
// WebRTC VAD 要求帧长为 10/20/30ms，这里用 20ms（16kHz = 320 样本）。

use webrtc_vad::{SampleRate, Vad, VadMode};

use crate::voice::capture::TARGET_SAMPLE_RATE;

/// VAD 帧长：20ms @ 16kHz = 320 样本
const VAD_FRAME_SAMPLES: usize = (TARGET_SAMPLE_RATE as usize / 1000) * 20;
/// 判定"说完"的静音时长（毫秒）
const SILENCE_END_MS: u32 = 600;
/// 对应的静音帧数（每帧 20ms）
const SILENCE_END_FRAMES: u32 = SILENCE_END_MS / 20;

/// 指令录音器：唤醒后持续喂音频，聚合成完整指令，检测到静音结束时输出
pub struct CommandRecorder {
    vad: Vad,
    frame_buf: Vec<i16>,
    /// 已录制的指令音频（含说话内容）
    command: Vec<i16>,
    /// 连续静音帧计数
    silence_frames: u32,
    /// 是否已经检测到过语音（避免开头静音就结束）
    speech_started: bool,
}

/// 喂入音频后的状态
#[derive(Debug, PartialEq)]
pub enum RecordState {
    /// 还在录音（等待更多音频或等待说话）
    Recording,
    /// 检测到说完，返回完整指令音频
    Done(Vec<i16>),
}

impl CommandRecorder {
    pub fn new() -> Self {
        let vad = Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::Aggressive);
        Self {
            vad,
            frame_buf: Vec::with_capacity(VAD_FRAME_SAMPLES),
            command: Vec::new(),
            silence_frames: 0,
            speech_started: false,
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
            let is_voice = self.vad.is_voice_segment(&frame).unwrap_or(false);

            // 帧数据始终累积到指令音频
            self.command.extend_from_slice(&frame);

            if is_voice {
                self.speech_started = true;
                self.silence_frames = 0;
            } else if self.speech_started {
                // 说过话之后的静音才计数
                self.silence_frames += 1;
                if self.silence_frames >= SILENCE_END_FRAMES {
                    let result = std::mem::take(&mut self.command);
                    return RecordState::Done(result);
                }
            }
        }

        RecordState::Recording
    }

    /// 强制结束（超时时调用），返回已录制的音频
    pub fn finish(&mut self) -> Vec<i16> {
        // 把剩余不足一帧的也带上
        self.command.extend_from_slice(&self.frame_buf);
        self.frame_buf.clear();
        std::mem::take(&mut self.command)
    }

    /// 是否已检测到语音开始
    pub fn speech_started(&self) -> bool {
        self.speech_started
    }

    fn push(&mut self, samples: &[i16]) {
        self.frame_buf.extend_from_slice(samples);
    }
}
