// 语音控制模块
//
// Phase 1: 音频采集 + 环形缓冲（当前）
// 后续: 唤醒词检测、VAD、ASR、意图理解

#[macro_use]
pub mod vlog;
pub mod ring_buffer;
pub mod capture;
pub mod wakeword;
pub mod vad;
pub mod dsp;
pub mod audio_util;
pub mod baidu_asr;
pub mod intent;
pub mod runtime;

pub use ring_buffer::AudioRingBuffer;
pub use capture::{AudioCapture, TARGET_SAMPLE_RATE};
pub use wakeword::{WakewordDetector, train_wakeword};
pub use vad::{CommandRecorder, RecordState};
pub use audio_util::{trim_silence, rms};
pub use baidu_asr::BaiduAsr;
pub use intent::{match_script, parse_intent, VoiceIntent};
pub use runtime::{VoiceConfig, VoiceEvent, VoiceRuntime};
