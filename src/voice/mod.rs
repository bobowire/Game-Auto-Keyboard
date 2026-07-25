// 语音控制模块
//
// Phase 1: 音频采集 + 环形缓冲（当前）
// 后续: 唤醒词检测、VAD、ASR、意图理解

pub mod ring_buffer;
pub mod capture;

pub use ring_buffer::AudioRingBuffer;
pub use capture::{AudioCapture, TARGET_SAMPLE_RATE};
