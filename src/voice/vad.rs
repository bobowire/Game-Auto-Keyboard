// 语音活动检测（VAD）+ 指令录音
//
// 唤醒后用于判断"用户说完了没"：持续喂音频，检测到连续静音超过阈值即认为说完。
// WebRTC VAD 要求帧长为 10/20/30ms，这里用 20ms（16kHz = 320 样本）。
//
// 结束判定：只有一条规则
// ----------------------
// **连续静音达到阈值就算说完**，短于阈值的静音一律容忍。两种说法自动统一：
//
//   场景1 连读 "小助手奶妈加血"
//     唤醒 → 实时区一直有声 → 说完后静音攒够 → 结束
//
//   场景2 停顿 "小助手" + 200ms + "奶妈加血"
//     唤醒 → 实时区先静音 10 帧（200ms，未达阈值，容忍）→ 有声（计数清零）
//     → 说完后静音攒够 → 结束
//
// 不需要区分"第一次静音/第二次静音"，静音计数器天然处理：够阈值就结束，
// 不够就被下一段语音清零。用户始终没开口时，实时区的静音自己攒够阈值结束，
// 此时实时区无有声帧，返回空音频让上层丢弃。
//
// 回溯区 vs 实时区
// ----------------
// 唤醒后 runtime 用环形缓冲回溯 2 秒预填充（prefill），补上连读时已经说出去
// 的指令开头。这段回溯必然包含唤醒词本身，所以：
//   - 静音计数只在实时区进行（回溯区的唤醒词不参与结束判定）
//   - 裁剪起点只从实时区找，但会沿连续有声帧向回溯区回退，把连读时落在回溯
//     区的指令开头捞回来
//
// 抗底噪设计（单靠 WebRTC VAD 在有稳态底噪时会一直判"有声"，静音永远攒不够，
// 录音只能靠兜底超时结束，用户感知为"要等好几秒才有反应"）：
//   1. VAD 用 VeryAggressive 档（最严）
//   2. 叠一层自适应噪声底 + RMS 能量门：只有 VAD 判有声 **且** 能量显著高于
//      噪声底才算语音，噪声底在非语音帧上用 EMA 持续更新，自动适应环境

use webrtc_vad::{SampleRate, Vad, VadMode};

use crate::voice::capture::TARGET_SAMPLE_RATE;

/// VAD 帧长：20ms @ 16kHz = 320 样本
const VAD_FRAME_SAMPLES: usize = (TARGET_SAMPLE_RATE as usize / 1000) * 20;
/// 判定"说完"的连续静音时长（毫秒）
///
/// 同时也是唤醒词与指令之间允许的最大空档：短于这个值的停顿会被容忍，
/// 说 "小助手" 停 200ms 再说 "奶妈加血" 不会被判成说完。
pub const SILENCE_END_MS: u32 = 1200;
/// 对应的静音帧数（每帧 20ms）
const SILENCE_END_FRAMES: u32 = SILENCE_END_MS / 20;
/// 噪声底 EMA 更新系数（越小越平滑）
const NOISE_EMA_ALPHA: f32 = 0.05;
/// 语音能量需高于噪声底的倍数
const SPEECH_SNR_FACTOR: f32 = 1.5;
/// 语音能量绝对下限（噪声底极低时防止把细微杂音当语音）
const MIN_SPEECH_RMS: f32 = 50.0;
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
    /// 已录制的音频（回溯区 + 实时区）
    command: Vec<i16>,
    /// 每个已处理帧是否为有声（下标 = 帧序号），用于结束时裁剪
    frame_voiced: Vec<bool>,
    /// 回溯区占用的帧数，实时区从这一帧开始
    backfill_frames: usize,
    /// 是否已结束 prefill（进入实时区）
    live: bool,
    /// 连续静音帧计数（只在实时区累积）
    silence_frames: u32,
    /// 实时区是否出现过语音（用于上层判断"用户到底说没说"）
    speech_started: bool,
    /// 自适应噪声底（RMS）
    noise_floor: f32,
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
            frame_voiced: Vec::new(),
            backfill_frames: 0,
            live: false,
            silence_frames: 0,
            speech_started: false,
            noise_floor: INITIAL_NOISE_FLOOR,
        }
    }

    /// 用回溯的音频预填充（唤醒后从环形缓冲取出的指令起点）
    ///
    /// 这段音频含唤醒词，只做 VAD 标记和噪声底估计，不参与静音结束判定——
    /// 否则唤醒词后面的停顿会立刻被判成"说完了"。音频本身会保留，连读时
    /// 裁剪会回退进来把指令开头捞出。可多次调用，之后第一次 `process`
    /// 即进入实时区。
    pub fn prefill(&mut self, samples: &[i16]) {
        self.push(samples);
        // 回溯区按整帧处理，剩余不足一帧的留给实时区
        self.drain_frames(false);
        self.backfill_frames = self.frame_voiced.len();
    }

    /// 喂入音频，返回录音状态
    pub fn process(&mut self, samples: &[i16]) -> RecordState {
        self.live = true;
        self.push(samples);
        self.drain_frames(true)
    }

    /// 按 VAD 帧长消费缓冲。`live` 为假时只标记不判定（回溯区）
    fn drain_frames(&mut self, live: bool) -> RecordState {
        while self.frame_buf.len() >= VAD_FRAME_SAMPLES {
            let frame: Vec<i16> = self.frame_buf.drain(..VAD_FRAME_SAMPLES).collect();

            // 帧数据始终累积，回溯区也要留着（连读时那里就是指令开头）
            self.command.extend_from_slice(&frame);
            let voiced = self.classify_frame(&frame);
            self.frame_voiced.push(voiced);

            if !live {
                continue;
            }

            if voiced {
                // 有声就清零静音计数：短停顿被容忍，天然支持"小助手"+停顿+指令
                self.silence_frames = 0;
                self.speech_started = true;
            } else {
                // 静音无条件计数（不要求先检测到语音）：用户始终没开口时也能
                // 靠这里收敛，不必等 runtime 的兜底超时
                self.silence_frames += 1;
                if self.silence_frames >= SILENCE_END_FRAMES {
                    return RecordState::Done(self.take_trimmed());
                }
            }
        }

        RecordState::Recording
    }

    /// 取出音频并裁剪首尾静音
    ///
    /// 从前后两端向中间扫描，找到第一个"真正有声音"的帧（有声且非底噪），
    /// 裁剪中间区域作为有效音频。这样无论连读还是停顿，都能准确裁掉首尾
    /// 的纯静音/底噪部分，保留中间所有有效内容。
    fn take_trimmed(&mut self) -> Vec<i16> {
        let audio = std::mem::take(&mut self.command);
        let voiced = std::mem::take(&mut self.frame_voiced);

        if voiced.is_empty() {
            return audio;
        }

        // 从前往后找第一个有声帧
        let first_voiced = voiced.iter().position(|&v| v);
        // 从后往前找最后一个有声帧
        let last_voiced = voiced.iter().rposition(|&v| v);

        let (Some(first), Some(last)) = (first_voiced, last_voiced) else {
            // 完全没有检测到有声帧，返回完整音频交给 ASR 判断
            return audio;
        };

        // 加上前后 padding，避免削掉起始辅音和尾音
        let lead_pad = TARGET_SAMPLE_RATE as usize * TRIM_LEAD_PAD_MS / 1000;
        let tail_pad = TARGET_SAMPLE_RATE as usize * TRIM_TAIL_PAD_MS / 1000;
        let s = (first * VAD_FRAME_SAMPLES).saturating_sub(lead_pad);
        let e = (((last + 1) * VAD_FRAME_SAMPLES) + tail_pad).min(audio.len());

        if s < e {
            audio[s..e].to_vec()
        } else {
            audio
        }
    }

    /// 判断一帧是否为语音：WebRTC VAD 与能量门都通过才算
    ///
    /// 非语音帧用于更新噪声底，所以环境噪声变化时门限会自动跟随。
    /// 回溯区也参与噪声底估计，等于唤醒前就已经在测环境噪声。
    fn classify_frame(&mut self, frame: &[i16]) -> bool {
        let vad_voice = self.vad.is_voice_segment(frame).unwrap_or(false);
        let energy = frame_rms(frame);
        let is_voice = vad_voice && energy >= self.speech_gate();

        if !is_voice {
            // 只在非语音帧更新噪声底，避免语音把底噪估计抬高
            self.noise_floor += NOISE_EMA_ALPHA * (energy - self.noise_floor);
        }

        is_voice
    }

    /// 强制结束（超时时调用），返回已录制的音频（同样裁掉首尾静音）
    pub fn finish(&mut self) -> Vec<i16> {
        // 把剩余不足一帧的也带上，并按有声/无声给它一个标记位
        let tail = std::mem::take(&mut self.frame_buf);
        if !tail.is_empty() {
            let voiced = self.live && frame_rms(&tail) >= self.speech_gate();
            self.command.extend_from_slice(&tail);
            self.frame_voiced.push(voiced);
        }
        self.take_trimmed()
    }

    /// 当前的语音能量门限
    fn speech_gate(&self) -> f32 {
        (self.noise_floor * SPEECH_SNR_FACTOR).max(MIN_SPEECH_RMS)
    }

    /// 指令语音是否已开始（只看实时区，唤醒词不算）
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

    /// 喂音频直到 Done，返回音频；超过 limit 帧仍未结束则 panic
    fn drive_to_done(rec: &mut CommandRecorder, limit: usize, seed: u32) -> Vec<i16> {
        for i in 0..limit {
            if let RecordState::Done(a) = rec.process(&noise(VAD_FRAME_SAMPLES, 30.0, seed + i as u32)) {
                return a;
            }
        }
        panic!("{} 帧内未触发 Done", limit);
    }

    #[test]
    fn scenario_continuous_speech() {
        // 场景1：连读 "小助手奶妈加血"
        // 回溯含唤醒词，实时区继续说，连读时回退应能跨回溯区边界把唤醒词捞回来
        let mut rec = CommandRecorder::new();
        rec.prefill(&speech_like(VAD_FRAME_SAMPLES * 25, 9000.0)); // 唤醒词 500ms
        // 实时区：完整指令 30 帧（600ms），一直有声不应结束
        for _ in 0..30 {
            assert_eq!(
                rec.process(&speech_like(VAD_FRAME_SAMPLES, 9000.0)),
                RecordState::Recording,
                "说话期间不应结束"
            );
        }
        assert!(rec.speech_started());

        let audio = drive_to_done(&mut rec, 40, 100);
        assert!(!audio.is_empty(), "连读场景应返回指令音频");
        // 至少包含实时区那 30 帧
        assert!(
            audio.len() >= 30 * VAD_FRAME_SAMPLES,
            "不应削掉指令，实际 {} 帧",
            audio.len() / VAD_FRAME_SAMPLES
        );
    }

    #[test]
    fn scenario_pause_then_command() {
        // 场景2："小助手" + 停顿 200ms + "奶妈加血"
        // 200ms 静音（10 帧）未达 25 帧阈值，应被容忍
        let mut rec = CommandRecorder::new();
        rec.prefill(&speech_like(VAD_FRAME_SAMPLES * 25, 9000.0)); // 唤醒词

        // 停顿 200ms = 10 帧，不应结束
        for i in 0..10 {
            assert_eq!(
                rec.process(&noise(VAD_FRAME_SAMPLES, 30.0, i + 20)),
                RecordState::Recording,
                "200ms 停顿应被容忍（第 {} 帧）",
                i
            );
        }
        assert!(!rec.speech_started(), "停顿期间实时区还没有语音");

        // 指令 30 帧
        for _ in 0..30 {
            assert_eq!(
                rec.process(&speech_like(VAD_FRAME_SAMPLES, 9000.0)),
                RecordState::Recording
            );
        }
        assert!(rec.speech_started(), "指令应被检测到");

        let audio = drive_to_done(&mut rec, 40, 200);
        assert!(!audio.is_empty(), "停顿场景应返回指令音频");
        assert!(
            audio.len() >= 30 * VAD_FRAME_SAMPLES,
            "不应削掉指令，实际 {} 帧",
            audio.len() / VAD_FRAME_SAMPLES
        );
    }

    #[test]
    fn pause_just_under_threshold_is_tolerated() {
        // 边界：停顿 24 帧（480ms）刚好未达 25 帧阈值，应被容忍
        let mut rec = CommandRecorder::new();
        rec.prefill(&speech_like(VAD_FRAME_SAMPLES * 10, 9000.0));
        for i in 0..(SILENCE_END_FRAMES - 1) {
            assert_eq!(
                rec.process(&noise(VAD_FRAME_SAMPLES, 30.0, i + 30)),
                RecordState::Recording,
                "第 {} 帧静音（阈值 {}）不应结束",
                i + 1,
                SILENCE_END_FRAMES
            );
        }
        // 再来一帧静音就该结束
        assert!(
            matches!(
                rec.process(&noise(VAD_FRAME_SAMPLES, 30.0, 99)),
                RecordState::Done(_)
            ),
            "第 {} 帧静音应触发 Done",
            SILENCE_END_FRAMES
        );
    }

    #[test]
    fn no_command_returns_empty() {
        // 唤醒后一直没开口：静音攒够后结束，返回空（不能把唤醒词送去 ASR）
        let mut rec = CommandRecorder::new();
        rec.prefill(&speech_like(VAD_FRAME_SAMPLES * 25, 9000.0)); // 唤醒词
        let audio = drive_to_done(&mut rec, 40, 300);
        assert!(audio.is_empty(), "未开口应返回空音频，实际 {} 样本", audio.len());
    }

    #[test]
    fn low_energy_speech_ignored_as_noise() {
        // 能量低于门限的信号即使 VAD 判有声也不算语音，静音照常攒够结束
        let mut rec = CommandRecorder::new();
        rec.prefill(&noise(VAD_FRAME_SAMPLES * 10, 30.0, 2));
        let mut done = false;
        for _ in 0..40 {
            if let RecordState::Done(_) = rec.process(&speech_like(VAD_FRAME_SAMPLES, 40.0)) {
                done = true;
                break;
            }
        }
        assert!(done, "低能量信号应被当作静音并结束录音");
        assert!(
            !rec.speech_started(),
            "低能量信号不应算语音（噪声底 {:.1}）",
            rec.noise_floor()
        );
    }

    #[test]
    fn trailing_silence_is_trimmed() {
        // 尾部静音应被裁掉，只留 padding
        let mut rec = CommandRecorder::new();
        rec.prefill(&noise(VAD_FRAME_SAMPLES * 10, 30.0, 4));
        let speech_frames = 30;
        for _ in 0..speech_frames {
            rec.process(&speech_like(VAD_FRAME_SAMPLES, 9000.0));
        }
        let audio = drive_to_done(&mut rec, 40, 400);

        // 原始总长 = 回溯 10 + 语音 30 + 静音 25 帧
        let total = (10 + speech_frames + SILENCE_END_FRAMES as usize) * VAD_FRAME_SAMPLES;
        assert!(
            audio.len() < total,
            "应裁掉首尾静音，实际 {} 原始 {}",
            audio.len(),
            total
        );
        // 但要保住语音本体
        assert!(
            audio.len() >= speech_frames * VAD_FRAME_SAMPLES,
            "不应削掉语音本体，实际 {} 语音 {}",
            audio.len(),
            speech_frames * VAD_FRAME_SAMPLES
        );
    }

    #[test]
    fn trim_keeps_padding() {
        // 裁剪保留 padding，不能把起始辅音削掉
        let mut rec = CommandRecorder::new();
        rec.prefill(&noise(TARGET_SAMPLE_RATE as usize, 30.0, 5));
        let speech_frames = 25;
        for _ in 0..speech_frames {
            rec.process(&speech_like(VAD_FRAME_SAMPLES, 9000.0));
        }
        let audio = rec.finish();
        let speech_len = speech_frames * VAD_FRAME_SAMPLES;
        assert!(
            audio.len() > speech_len,
            "应保留 padding 余量，实际 {} vs 语音 {}",
            audio.len(),
            speech_len
        );
    }

    #[test]
    fn finish_without_command_returns_empty() {
        // 兜底超时触发 finish 时，实时区无语音同样返回空
        let mut rec = CommandRecorder::new();
        rec.prefill(&speech_like(VAD_FRAME_SAMPLES * 10, 9000.0));
        rec.process(&noise(VAD_FRAME_SAMPLES * 5, 20.0, 12));
        assert!(rec.finish().is_empty(), "无指令时 finish 应返回空");
    }

    #[test]
    fn multiple_short_pauses_tolerated() {
        // 词间多次短停顿（"奶妈…加血"）不应提前结束
        let mut rec = CommandRecorder::new();
        rec.prefill(&speech_like(VAD_FRAME_SAMPLES * 10, 9000.0));
        for round in 0..3 {
            for _ in 0..10 {
                assert_eq!(
                    rec.process(&speech_like(VAD_FRAME_SAMPLES, 9000.0)),
                    RecordState::Recording
                );
            }
            // 每段之间停 10 帧（200ms）
            for i in 0..10 {
                assert_eq!(
                    rec.process(&noise(VAD_FRAME_SAMPLES, 30.0, round * 100 + i + 50)),
                    RecordState::Recording,
                    "第 {} 轮词间停顿不应结束",
                    round
                );
            }
        }
        // 最后真正说完
        let audio = drive_to_done(&mut rec, 40, 900);
        assert!(!audio.is_empty());
    }

    #[test]
    fn noise_floor_adapts_to_ambient() {
        let mut rec = CommandRecorder::new();
        for i in 0..100 {
            rec.prefill(&noise(VAD_FRAME_SAMPLES, 500.0, i));
        }
        let floor = rec.noise_floor();
        assert!(
            floor > INITIAL_NOISE_FLOOR,
            "噪声底应随环境上升，实际 {:.1}",
            floor
        );
    }
}
