// 语音运行时 - 后台线程跑完整音频流水线，把事件回传给 UI 线程
//
// 流水线（与 examples/wakeword_test.rs 的状态机一致）：
//   麦克风采集 → 环形缓冲 → 唤醒词检测 → 回溯 2 秒补指令开头
//   → VAD 录音 → 指令音频 → 百度 ASR → 识别文本
//
// UI 线程只负责：给出配置启动线程、poll 事件、拿到识别文本后做意图解析与执行。
// 意图解析放在 UI 线程是因为窗口名/脚本都由 App 持有，避免跨线程共享槽位。

use crate::voice::{
    AudioCapture, AudioRingBuffer, BaiduAsr, CommandRecorder, RecordState, WakewordDetector,
    TARGET_SAMPLE_RATE,
};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// 后台线程回传给 UI 的事件
pub enum VoiceEvent {
    /// 一般状态提示
    Status(String),
    /// 已被唤醒，开始听指令
    Woke,
    /// 识别出的指令文本（含可能的唤醒词前缀，由意图解析剥离）
    Recognized(String),
    /// 出错（如麦克风/模型加载/ASR 失败）
    Error(String),
    /// 线程已停止
    Stopped,
}

/// 启动语音运行时所需的配置
pub struct VoiceConfig {
    /// 唤醒词模型文件路径（.rpw）
    pub model_path: String,
    /// 唤醒阈值
    pub threshold: f32,
    /// 百度 API Key
    pub api_key: String,
    /// 百度 Secret Key
    pub secret_key: String,
}

/// UI 侧持有的运行时句柄
pub struct VoiceRuntime {
    stop_tx: Sender<()>,
    event_rx: Receiver<VoiceEvent>,
    handle: Option<JoinHandle<()>>,
}

impl VoiceRuntime {
    /// 启动后台线程。立即返回句柄；实际就绪/失败通过事件回传。
    pub fn start(config: VoiceConfig) -> Self {
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        let (event_tx, event_rx) = std::sync::mpsc::channel::<VoiceEvent>();

        let handle = thread::spawn(move || {
            run_loop(config, stop_rx, event_tx);
        });

        Self {
            stop_tx,
            event_rx,
            handle: Some(handle),
        }
    }

    /// 非阻塞取出所有待处理事件
    pub fn poll(&self) -> Vec<VoiceEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = self.event_rx.try_recv() {
            out.push(ev);
        }
        out
    }

    /// 请求停止并等待线程退出
    pub fn stop(&mut self) {
        let _ = self.stop_tx.send(());
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for VoiceRuntime {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 语音状态机
enum State {
    /// 待命：监听唤醒词
    Idle,
    /// 已唤醒：录制指令
    Listening {
        recorder: CommandRecorder,
        started: Instant,
    },
}

fn run_loop(config: VoiceConfig, stop_rx: Receiver<()>, tx: Sender<VoiceEvent>) {
    // 启动麦克风
    let capture = match AudioCapture::start() {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(VoiceEvent::Error(format!("麦克风启动失败: {}", e)));
            let _ = tx.send(VoiceEvent::Stopped);
            return;
        }
    };

    // 加载唤醒词模型
    let mut detector =
        match WakewordDetector::from_model_file(&config.model_path, config.threshold) {
            Ok(d) => d,
            Err(e) => {
                let _ = tx.send(VoiceEvent::Error(format!(
                    "唤醒词模型加载失败({}): {}",
                    config.model_path, e
                )));
                let _ = tx.send(VoiceEvent::Stopped);
                return;
            }
        };

    let mut asr = BaiduAsr::new(config.api_key, config.secret_key);

    // 3 秒环形缓冲，唤醒后回溯取指令起点
    let mut ring = AudioRingBuffer::new(TARGET_SAMPLE_RATE as usize, 3);
    let mut state = State::Idle;

    capture.poll(); // 清空启动时积压
    let _ = tx.send(VoiceEvent::Status("语音待命：说\"小助手\"唤醒".to_string()));

    loop {
        // 检查停止请求
        match stop_rx.try_recv() {
            Ok(_) | Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => {}
        }

        let frame = capture.poll();
        if frame.is_empty() {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        ring.push(&frame);

        match &mut state {
            State::Idle => {
                if let Some(score) = detector.process(&frame) {
                    detector.reset();
                    vlog!("[voice] 唤醒 score={:.3}，开始听指令", score);
                    let _ = tx.send(VoiceEvent::Woke);

                    let mut recorder = CommandRecorder::new();
                    let backfill = ring.take_recent(2000);
                    vlog!("[voice] 回溯补齐 {} 样本({:.2}秒)",
                        backfill.len(),
                        backfill.len() as f32 / TARGET_SAMPLE_RATE as f32);
                    recorder.prefill(&backfill);

                    state = State::Listening {
                        recorder,
                        started: Instant::now(),
                    };
                }
            }
            State::Listening { recorder, started } => {
                let elapsed = started.elapsed();
                let no_speech_timeout =
                    !recorder.speech_started() && elapsed > Duration::from_secs(3);
                let max_timeout = elapsed > Duration::from_secs(8);

                let done_audio = match recorder.process(&frame) {
                    RecordState::Done(audio) => {
                        vlog!("[voice] VAD 检测到静音，录音结束");
                        Some(audio)
                    }
                    RecordState::Recording => {
                        if no_speech_timeout {
                            vlog!("[voice] 超时：3秒内未检测到语音，放弃");
                            let _ = tx.send(VoiceEvent::Status(
                                "超时（3秒无语音），回到待命".to_string(),
                            ));
                            None
                        } else if max_timeout {
                            vlog!("[voice] 达到最长8秒，强制结束录音");
                            Some(recorder.finish())
                        } else {
                            continue;
                        }
                    }
                };

                match done_audio {
                    Some(audio) if !audio.is_empty() => {
                        let secs = audio.len() as f32 / TARGET_SAMPLE_RATE as f32;
                        vlog!("[voice] 指令音频 {} 样本({:.2}秒)，开始 ASR 识别...",
                            audio.len(), secs);
                        // ASR 识别（阻塞 HTTP，但在后台线程无妨）
                        match asr.recognize(&audio) {
                            Ok(text) if !text.trim().is_empty() => {
                                vlog!("[voice] ASR 识别结果: 「{}」", text);
                                let _ = tx.send(VoiceEvent::Recognized(text));
                            }
                            Ok(_) => {
                                vlog!("[voice] ASR 返回空文本");
                                let _ = tx.send(VoiceEvent::Status(
                                    "未识别到内容".to_string(),
                                ));
                            }
                            Err(e) => {
                                vlog!("[voice] ASR 识别失败: {}", e);
                                let _ = tx.send(VoiceEvent::Error(format!("识别失败: {}", e)));
                            }
                        }
                        state = State::Idle;
                        let _ = tx
                            .send(VoiceEvent::Status("语音待命：说\"小助手\"唤醒".to_string()));
                    }
                    Some(_) => {
                        vlog!("[voice] 录音结束但音频为空，放弃");
                        state = State::Idle;
                        let _ = tx
                            .send(VoiceEvent::Status("语音待命：说\"小助手\"唤醒".to_string()));
                    }
                    None => {
                        state = State::Idle;
                        let _ = tx
                            .send(VoiceEvent::Status("语音待命：说\"小助手\"唤醒".to_string()));
                    }
                }
            }
        }
    }

    let _ = tx.send(VoiceEvent::Stopped);
}
