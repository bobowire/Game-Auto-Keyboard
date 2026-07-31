// egui 主应用 - 多窗口 + 方案标识 + 热键

use crate::color_picker::ColorPicker;
use crate::config::AppConfig;
use crate::event_bus::{MainEvent, MainEventBus, WakeTicker};
use crate::hotkey::{HotkeyAction, HotkeyKey, HotkeyManager, HotkeyStateMachine};
use crate::overlay::{OverlayEvent, OverlayWindow};
use crate::runner::Runner;
use crate::script::{load_dir, ScriptFile};
use crate::tray::{Tray, TrayCommand};
use crate::utils::win32;
use crate::vlog;
use crate::voice::{
    match_script_ex, parse_intent, vlog, AudioCapture, MatchSource, VoiceConfig, VoiceEvent,
    VoiceIntent, VoiceRuntime, train_wakeword, trim_silence, TARGET_SAMPLE_RATE,
};
use crate::window_slot::{Scheme, WindowSlot};
use eframe::egui;
use std::path::PathBuf;
use std::time::Instant;
use windows::Win32::Media::Audio::PlaySoundW;
use windows::core::PCWSTR;

const SCRIPTS_DIR: &str = "scripts";
const GRAB_COUNTDOWN_SECS: u64 = 3;
const SLOT_COUNT: usize = 8;
/// 唤醒词模型文件（由 wakeword_test 训练生成，放在工作目录）
const WAKEWORD_MODEL_PATH: &str = "wakeword_model.rpw";
/// 唤醒阈值
const WAKEWORD_THRESHOLD: f32 = 0.5;

/// 播放音效文件（用于语音控制反馈）
fn play_sound(name: &str) {
    // 获取可执行文件所在目录
    let path = if let Ok(exe_dir) = crate::utils::get_exe_dir() {
        exe_dir.join("assets").join(name)
    } else {
        PathBuf::from("assets").join(name)
    };

    // 转换为宽字符串
    let path_str = path.to_string_lossy().into_owned();
    let wide: Vec<u16> = path_str.encode_utf16().chain(std::iter::once(0)).collect();

    // 调用 Windows PlaySound API（忽略返回值）
    unsafe {
        let _ = PlaySoundW(PCWSTR(wide.as_ptr()), None, Default::default());
    }
}

pub struct App {
    // 统一事件总线：所有后台事件源（托盘/热键/语音）都往这里投事件，
    // 投递时自动 PostMessage(WM_PAINT) 唤醒主窗口，因此隐藏到托盘后依然即时响应。
    events: MainEventBus,

    // 脚本列表（全局候选池）
    scripts: Vec<ScriptFile>,
    scripts_dir: PathBuf,

    // 8 个窗口槽位
    slots: Vec<WindowSlot>,

    // 抓取窗口：记录正在为哪个槽位抓取，以及倒计时起点
    grabbing_slot: Option<usize>,
    grabbing_since: Option<Instant>,

    // 上次窗口有效性检查时间（节流）
    last_validity_check: Instant,

    // 正在浏览源码的脚本索引
    viewing_script: Option<usize>,

    // 为哪个槽位添加方案的弹窗（None 表示不显示）
    adding_scheme_for: Option<usize>,

    // 是否显示热键说明窗口
    show_hotkey_help: bool,

    // 取色器
    color_picker: ColorPicker,
    // 取色倒计时起点（用于抓取前台窗口后打开取色器）
    picking_since: Option<Instant>,

    // 热键
    // 只为保持生命周期：drop 时注销全局热键。事件走总线，不从这里 poll。
    _hotkey_mgr: Option<HotkeyManager>,
    hotkey_sm: HotkeyStateMachine,

    // 系统托盘
    tray: Option<Tray>,
    // 是否正在真正退出（托盘“退出”触发），用于放行关闭请求
    quitting: bool,
    // 还需强制唤醒多少帧（窗口隐藏时 egui 不会自然重绘，见 handle_tray）
    wake_pending: u8,

    // 语音控制运行时（None 表示未开启）
    voice: Option<VoiceRuntime>,
    /// 鼠标转发覆盖窗句柄（None = 未开启）
    overlay: Option<OverlayWindow>,
    // 百度语音配置（编辑用），从 config 加载
    baidu_api_key: String,
    baidu_secret_key: String,
    // 最近一次语音识别文本（UI 展示）
    last_voice_text: String,

    // 热键配置（编辑用），从 config 加载
    hotkey_enabled: bool,
    hotkey_impromptu_enabled: bool,

    // 通用配置（编辑用），从 config 加载
    log_enabled: bool,
    save_wakeword_samples: bool,
    save_asr_audio: bool,
    pinyin_assist: bool,

    // 统一配置窗口
    show_settings: bool,
    settings_tab: SettingsTab,
    // 是否显示语音帮助文档窗口
    show_voice_help: bool,
    // 是否显示百度申请引导窗口
    show_baidu_guide: bool,
    // 是否显示唤醒词训练引导窗口
    show_wakeword_guide: bool,
    // 唤醒词训练状态
    wakeword_training: Option<WakewordTrainingState>,

    // 状态提示
    status: String,
}

/// 唤醒词训练状态
struct WakewordTrainingState {
    current_round: usize,      // 当前第几遍 (1-4)
    total_rounds: usize,       // 总共几遍 (4)
    is_recording: bool,        // 是否正在录制
    record_start: Option<Instant>, // 录制开始时间
    record_duration: f32,      // 录制时长(秒)
    samples: Vec<Vec<i16>>,    // 已录制的样本
    status_msg: String,        // 状态消息
    capture: Option<AudioCapture>, // 音频采集器
    recording_buffer: Vec<i16>, // 当前录制的缓冲区
    /// 训练期间的定时唤醒器：录音要靠 update() 连续抽帧，
    /// 不能指望窗口自然重绘（隐藏/失焦时就断了）。drop 即停。
    _ticker: WakeTicker,
}

/// 配置窗口标签页
#[derive(Debug, Clone, Copy, PartialEq)]
enum SettingsTab {
    General,
    Voice,
    Hotkey,
    About,
}

/// 单窗口语音动作匹配+执行的结局（供汇总状态/响铃）
enum ActionOutcome {
    /// 窗口未绑定句柄
    NotBound,
    /// 该窗口脚本列表里没有匹配的脚本
    NoMatch,
    /// 匹配并尝试启动了脚本（success 标记 start_slot/run_slot_once 是否成功）
    Ran { script: String, success: bool },
}

impl App {
    pub fn new() -> Self {
        // 获取可执行文件所在目录下的脚本目录
        let scripts_dir = if let Ok(exe_dir) = crate::utils::get_exe_dir() {
            exe_dir.join(SCRIPTS_DIR)
        } else {
            PathBuf::from(SCRIPTS_DIR)
        };
        let scripts = load_dir(&scripts_dir).unwrap_or_default();

        // 从配置恢复方案绑定（按文件名从脚本池重建命令）
        let config = AppConfig::load();
        let mut slots = Vec::with_capacity(SLOT_COUNT);
        for i in 0..SLOT_COUNT {
            let mut slot = WindowSlot::default();
            // 默认名"窗口N"，配置里有非空自定义名则覆盖
            slot.name = format!("窗口{}", i + 1);
            if let Some(sc) = config.slots.get(i) {
                if !sc.name.trim().is_empty() {
                    slot.name = sc.name.clone();
                }
                for name in &sc.scheme_names {
                    if let Some(sf) = scripts.iter().find(|s| &s.name == name) {
                        if let Some(cmds) = &sf.commands {
                            slot.schemes.push(Scheme {
                                script_name: sf.name.clone(),
                                commands: cmds.clone(),
                                settings: sf.settings.clone(),
                            });
                        }
                    }
                    // 脚本文件已不存在则静默跳过
                }
                // 恢复标识（若越界则修正为 0 / None）
                slot.marked = match sc.marked {
                    Some(m) if m < slot.schemes.len() => Some(m),
                    _ if !slot.schemes.is_empty() => Some(0),
                    _ => None,
                };
                slot.is_main = sc.is_main;
            }
            slots.push(slot);
        }

        // 事件总线先建好，各事件源共用它的 sender
        let events = MainEventBus::new();

        // 尝试启动热键
        let (hotkey_mgr, status) = match HotkeyManager::start(events.sender()) {
            Ok(mgr) => (
                Some(mgr),
                format!("已加载 {} 个脚本；热键已就绪 (Ctrl+Shift+0~9)", scripts.len()),
            ),
            Err(e) => (None, format!("已加载 {} 个脚本；热键注册失败: {}", scripts.len(), e)),
        };

        // 创建托盘（失败不致命，仅记录）
        let tray = match Tray::new(events.sender()) {
            Ok(t) => Some(t),
            Err(e) => {
                eprintln!("托盘创建失败: {}", e);
                None
            }
        };

        // 初始化日志开关
        vlog::set_enabled(config.general.log_enabled);

        Self {
            events,
            scripts,
            scripts_dir,
            slots,
            grabbing_slot: None,
            grabbing_since: None,
            last_validity_check: Instant::now(),
            viewing_script: None,
            adding_scheme_for: None,
            show_hotkey_help: false,
            color_picker: ColorPicker::default(),
            picking_since: None,
            _hotkey_mgr: hotkey_mgr,
            hotkey_sm: HotkeyStateMachine::new(),
            tray,
            quitting: false,
            wake_pending: 0,
            voice: None,
            overlay: None,
            baidu_api_key: config.baidu.api_key.clone(),
            baidu_secret_key: config.baidu.secret_key.clone(),
            last_voice_text: String::new(),
            hotkey_enabled: config.hotkey.enabled,
            hotkey_impromptu_enabled: config.hotkey.impromptu_enabled,
            log_enabled: config.general.log_enabled,
            save_wakeword_samples: config.general.save_wakeword_samples,
            save_asr_audio: config.general.save_asr_audio,
            pinyin_assist: config.general.pinyin_assist,
            show_settings: false,
            settings_tab: SettingsTab::General,
            show_voice_help: false,
            show_baidu_guide: false,
            show_wakeword_guide: false,
            wakeword_training: None,
            status,
        }
    }

    fn reload_scripts(&mut self) {
        match load_dir(&self.scripts_dir) {
            Ok(list) => {
                self.status = format!("已重新加载 {} 个脚本", list.len());
                self.scripts = list;
                self.viewing_script = None;
            }
            Err(e) => self.status = format!("加载失败: {}", e),
        }
    }

    /// 将当前各槽位的方案绑定持久化到配置文件
    fn save_config(&self) {
        let mut cfg = AppConfig::default();
        for (i, slot) in self.slots.iter().enumerate() {
            // 与默认"窗口N"相同则存空串，保持配置干净
            cfg.slots[i].name = if slot.name == format!("窗口{}", i + 1) {
                String::new()
            } else {
                slot.name.clone()
            };
            cfg.slots[i].scheme_names =
                slot.schemes.iter().map(|s| s.script_name.clone()).collect();
            cfg.slots[i].marked = slot.marked;
            cfg.slots[i].is_main = slot.is_main;
        }
        cfg.baidu.api_key = self.baidu_api_key.clone();
        cfg.baidu.secret_key = self.baidu_secret_key.clone();
        cfg.hotkey.enabled = self.hotkey_enabled;
        cfg.hotkey.impromptu_enabled = self.hotkey_impromptu_enabled;
        cfg.general.log_enabled = self.log_enabled;
        cfg.general.save_wakeword_samples = self.save_wakeword_samples;
        cfg.general.save_asr_audio = self.save_asr_audio;
        cfg.general.pinyin_assist = self.pinyin_assist;

        // 同步日志开关到 vlog 模块
        vlog::set_enabled(self.log_enabled);

        if let Err(e) = cfg.save() {
            eprintln!("保存配置失败: {}", e);
        }
    }

    /// 查找标记为主窗口（鼠标转发目标）的槽位索引
    fn main_slot_index(&self) -> Option<usize> {
        self.slots.iter().position(|s| s.is_main)
    }

    /// 循环启动某槽位标识方案
    fn start_slot(&mut self, idx: usize) -> bool {
        self.run_slot(idx, false, 0)
    }

    /// 单次执行某槽位标识方案（预留给 UI 单次执行按钮）
    #[allow(dead_code)]
    fn run_slot_once(&mut self, idx: usize) -> bool {
        self.run_slot(idx, true, 0)
    }

    /// 热键触发：循环启动（带延迟）
    fn start_slot_hotkey(&mut self, idx: usize) -> bool {
        self.run_slot(idx, false, 1000)
    }

    /// 热键触发：单次执行（带延迟）
    fn run_slot_once_hotkey(&mut self, idx: usize) -> bool {
        self.run_slot(idx, true, 1000)
    }

    /// 执行某槽位的标识方案。once=true 单次，delay_ms 启动前延迟（给用户时间松开热键）
    fn run_slot(&mut self, idx: usize, once: bool, delay_ms: u64) -> bool {
        let slot = &self.slots[idx];
        let Some(hwnd) = slot.hwnd else {
            self.status = format!("窗口 {} 未绑定", idx + 1);
            return false;
        };
        if !win32::is_valid(windows::Win32::Foundation::HWND(hwnd as *mut _)) {
            self.status = format!("窗口 {} 已失效", idx + 1);
            self.slots[idx].hwnd = None;
            return false;
        }
        let Some(scheme) = slot.marked_scheme() else {
            self.status = format!("窗口 {} 没有标识方案", idx + 1);
            return false;
        };
        let commands = scheme.commands.clone();

        // 先停旧的
        self.slots[idx].stop();
        self.slots[idx].runner = Some(if once {
            if delay_ms > 0 {
                Runner::start_once_delayed(hwnd, commands, delay_ms)
            } else {
                Runner::start_once(hwnd, commands)
            }
        } else {
            if delay_ms > 0 {
                Runner::start_delayed(hwnd, commands, delay_ms)
            } else {
                Runner::start(hwnd, commands)
            }
        });
        true
    }

    fn stop_slot(&mut self, idx: usize) {
        self.slots[idx].stop();
    }

    fn start_windows(&mut self, windows: &[u8]) {
        let mut started = 0;
        for &w in windows {
            let idx = (w - 1) as usize;
            if idx < SLOT_COUNT && self.start_slot_hotkey(idx) {
                started += 1;
            }
        }
        self.status = format!("热键：启动了 {} 个窗口（1秒后开始执行）", started);
    }

    fn stop_windows(&mut self, windows: &[u8]) {
        for &w in windows {
            let idx = (w - 1) as usize;
            if idx < SLOT_COUNT {
                self.stop_slot(idx);
            }
        }
        self.status = format!("热键：停止了指定窗口");
    }

    fn start_all(&mut self) {
        let mut started = 0;
        for idx in 0..SLOT_COUNT {
            if self.slots[idx].is_bound() && self.start_slot_hotkey(idx) {
                started += 1;
            }
        }
        self.status = format!("热键：启动全部，共 {} 个窗口（1秒后开始执行）", started);
    }

    fn stop_all(&mut self) {
        for idx in 0..SLOT_COUNT {
            self.stop_slot(idx);
        }
        self.status = "热键：停止全部".to_string();
    }

    /// 开启语音控制：校验前置条件后启动后台运行时
    fn start_voice(&mut self) {
        if self.voice.is_some() {
            return;
        }

        // 检查百度密钥
        if self.baidu_api_key.trim().is_empty() || self.baidu_secret_key.trim().is_empty() {
            self.status = "⚠ 请先配置百度语音识别密钥".to_string();
            self.show_settings = true;
            self.settings_tab = SettingsTab::Voice;
            return;
        }

        // 检查唤醒词模型
        if !std::path::Path::new(WAKEWORD_MODEL_PATH).exists() {
            self.status = "⚠ 请先训练唤醒词模型".to_string();
            self.show_wakeword_guide = true;
            return;
        }

        let cfg = VoiceConfig {
            model_path: WAKEWORD_MODEL_PATH.to_string(),
            threshold: WAKEWORD_THRESHOLD,
            api_key: self.baidu_api_key.clone(),
            secret_key: self.baidu_secret_key.clone(),
            save_asr_audio: self.save_asr_audio,
        };
        self.voice = Some(VoiceRuntime::start(cfg, self.events.sender()));
        self.status = "语音控制已开启，正在初始化...".to_string();
    }

    /// 关闭语音控制
    fn stop_voice(&mut self) {
        if let Some(mut v) = self.voice.take() {
            v.stop();
            self.status = "语音控制已关闭".to_string();
        }
    }

    /// 处理覆盖窗回报事件
    fn handle_overlay_event(&mut self, ev: OverlayEvent) {
        // 覆盖窗已被 UI 侧关闭时丢弃残留事件（同 handle_voice_event 的竞态保护）：
        // stop_overlay() 会 join 线程，但线程退出前发出的 TargetLost 可能还留在总线里
        if self.overlay.is_none() {
            return;
        }
        match ev {
            OverlayEvent::TargetLost => {
                self.overlay = None;
                self.status = "🖱 鼠标转发已停止：主窗口已关闭/失效".to_string();
            }
            OverlayEvent::CloseRequested => {
                self.stop_overlay();
                self.status = "🖱 鼠标转发已关闭（Ctrl+Q）".to_string();
            }
        }
    }

    /// 开启鼠标转发：前置校验通过后启动覆盖窗线程
    fn start_overlay(&mut self) {
        if self.overlay.is_some() {
            return;
        }
        let Some(idx) = self.main_slot_index() else {
            self.status = "⚑ 请先标记主窗口（点槽位序号左侧的旗帜）".to_string();
            return;
        };
        let Some(hwnd_raw) = self.slots[idx].hwnd else {
            self.status = "⚠ 主窗口尚未绑定，请先抓取窗口".to_string();
            return;
        };
        if !win32::is_valid(windows::Win32::Foundation::HWND(hwnd_raw as *mut _)) {
            self.status = "⚠ 主窗口句柄已失效，请重新抓取窗口".to_string();
            return;
        }
        match OverlayWindow::start(hwnd_raw, self.events.sender()) {
            Ok(o) => {
                self.overlay = Some(o);
                self.status = "🖱 鼠标转发已开启：点击覆盖窗获取焦点后，鼠标操作（含滚轮）转发给主窗口；Ctrl+Q 关闭".to_string();
            }
            Err(e) => self.status = format!("🖱 鼠标转发启动失败: {}", e),
        }
    }

    /// 关闭鼠标转发
    fn stop_overlay(&mut self) {
        if let Some(mut o) = self.overlay.take() {
            o.stop();
            self.status = "🖱 鼠标转发已关闭".to_string();
        }
    }

    /// 处理单个语音事件
    fn handle_voice_event(&mut self, ev: VoiceEvent) {
        // 语音已关闭时丢弃残留事件：stop_voice() 会 join 线程，线程退出前发出的
        // Stopped/Status 可能还留在总线里。若不丢弃，这些旧事件会覆盖状态栏，
        // 甚至在"关闭→立刻重开"时把刚建好的 self.voice 误清成 None。
        if self.voice.is_none() {
            return;
        }

        let mut stopped = false;
        match ev {
            VoiceEvent::Status(s) => self.status = format!("🎤 {}", s),
            VoiceEvent::Woke => self.status = "🎤 已唤醒，请说指令...".to_string(),
            VoiceEvent::Recognized(text) => {
                self.last_voice_text = text.clone();
                self.handle_voice_text(&text);
            }
            VoiceEvent::Error(e) => self.status = format!("🎤 语音错误: {}", e),
            VoiceEvent::Stopped => stopped = true,
        }
        if stopped {
            // 后台线程已退出（多为出错自停），清理句柄
            self.voice = None;
        }
    }

    /// 处理唤醒词训练的录音逻辑
    fn process_wakeword_training(&mut self) {
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

    /// 解析识别文本为意图并执行
    fn handle_voice_text(&mut self, text: &str) {
        // 收集非空窗口名（默认名"窗口N"也算有效指称）
        let windows: Vec<(usize, String)> = self
            .slots
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s.name.clone()))
            .collect();

        vlog!("[intent] 原始文本: 「{}」", text);
        vlog!(
            "[intent] 当前窗口名: {:?}",
            windows.iter().map(|(i, n)| format!("{}={}", i + 1, n)).collect::<Vec<_>>()
        );

        match parse_intent(text, &windows) {
            Some(VoiceIntent::StopAll) => {
                vlog!("[intent] 匹配: 停止全部");
                self.stop_all();
                self.status = format!("🎤「{}」→ 停止全部", text);
                play_sound("beep_success.wav");
            }
            Some(VoiceIntent::StopWindow(idx)) => {
                vlog!("[intent] 匹配: 停止窗口 {}", idx + 1);
                self.stop_slot(idx);
                self.status = format!("🎤「{}」→ 停止 {}", text, self.slots[idx].name);
                play_sound("beep_success.wav");
            }
            Some(VoiceIntent::RunAction { window, action }) => {
                vlog!("[intent] 匹配: 窗口 {} 执行动作「{}」", window + 1, action);
                self.run_voice_action(window, &action, text);
            }
            Some(VoiceIntent::RunActionAll { action }) => {
                vlog!("[intent] 匹配: 所有窗口执行动作「{}」", action);
                self.run_voice_action_all(&action, text);
            }
            None => {
                vlog!("[intent] 未匹配到任何窗口名或停止指令");
                self.status = format!("🎤「{}」→ 未匹配到指令", text);
                play_sound("beep_fail.wav");
            }
        }
    }

    /// 在单个窗口按动作关键词匹配脚本并执行（核心：不含状态/响铃副作用）。
    ///
    /// 返回 `ActionOutcome` 供单窗口与全部窗口两个外壳各自汇总展示。
    fn try_voice_action(&mut self, idx: usize, action: &str) -> ActionOutcome {
        let win_name = self.slots[idx].name.clone();
        if !self.slots[idx].is_bound() {
            vlog!("[intent] 窗口 {}({}) 未绑定窗口句柄，跳过", idx + 1, win_name);
            return ActionOutcome::NotBound;
        }
        // 在该窗口已添加的方案里按动作关键词匹配脚本
        let names: Vec<String> = self.slots[idx]
            .schemes
            .iter()
            .map(|s| s.script_name.clone())
            .collect();
        vlog!(
            "[intent] 窗口 {}({}) 已添加脚本: {:?}",
            idx + 1,
            win_name,
            names
        );
        let result = match_script_ex(action, names.iter().map(|s| s.as_str()), self.pinyin_assist);
        match &result.winner {
            Some(m) => {
                let scheme_idx = m.index;
                let script_name = self.slots[idx].schemes[scheme_idx].script_name.clone();
                let audio_only_once = self.slots[idx].schemes[scheme_idx].settings.audio_only_once;

                let source = match m.source {
                    MatchSource::Char => "字符",
                    MatchSource::Pinyin => "拼音",
                };
                vlog!(
                    "[intent] 动作「{}」匹配到脚本「{}」（{}命中 得分 {:.2}；字符轮 {:?} 拼音轮 {:?}），启动",
                    action, script_name, source, m.score, result.char_best, result.pinyin_best
                );
                self.slots[idx].set_marked(scheme_idx);

                // 根据脚本设置选择执行模式
                let success = if audio_only_once {
                    vlog!("[intent] 脚本设置 audio_only_once=true，单次执行");
                    self.run_slot_once(idx)
                } else {
                    self.start_slot(idx)
                };

                if success {
                    vlog!("[intent] 已启动窗口 {} 的脚本「{}」", idx + 1, script_name);
                } else {
                    vlog!(
                        "[intent] start_slot/run_slot_once 返回 false（窗口失效/无标识方案），未执行"
                    );
                }
                ActionOutcome::Ran {
                    script: script_name,
                    success,
                }
            }
            None => {
                vlog!(
                    "[intent] 动作「{}」在窗口 {} 的脚本中未找到匹配（字符轮 {:?} 拼音轮 {:?}）",
                    action,
                    idx + 1,
                    result.char_best,
                    result.pinyin_best
                );
                ActionOutcome::NoMatch
            }
        }
    }

    /// 在指定窗口按动作关键词匹配脚本并执行（带状态提示与响铃）
    fn run_voice_action(&mut self, idx: usize, action: &str, raw: &str) {
        let win_name = self.slots[idx].name.clone();
        match self.try_voice_action(idx, action) {
            ActionOutcome::NotBound => {
                self.status = format!("🎤「{}」→ {} 未绑定窗口", raw, win_name);
                play_sound("beep_fail.wav");
            }
            ActionOutcome::NoMatch => {
                self.status = format!(
                    "🎤「{}」→ {} 未找到匹配「{}」的脚本",
                    raw, win_name, action
                );
                play_sound("beep_fail.wav");
            }
            ActionOutcome::Ran { script, success } => {
                if success {
                    self.status = format!("🎤「{}」→ {} 执行 {}", raw, win_name, script);
                    play_sound("beep_success.wav");
                } else {
                    play_sound("beep_fail.wav");
                }
            }
        }
    }

    /// 在所有已绑定窗口各自按动作关键词匹配脚本并执行。
    ///
    /// 每个窗口用各自的脚本列表独立匹配（未必命中同一脚本名），
    /// 命中即启动；最后统一汇总状态、响一次铃。
    fn run_voice_action_all(&mut self, action: &str, raw: &str) {
        let mut ran = 0usize;
        let mut failed = 0usize;
        let mut skipped = 0usize;
        for idx in 0..SLOT_COUNT {
            match self.try_voice_action(idx, action) {
                ActionOutcome::NotBound => skipped += 1,
                ActionOutcome::NoMatch => failed += 1,
                ActionOutcome::Ran { success, .. } => {
                    if success {
                        ran += 1;
                    } else {
                        failed += 1;
                    }
                }
            }
        }
        if ran > 0 {
            self.status = format!(
                "🎤「{}」→ 全部窗口执行「{}」：{} 个启动，{} 个未命中",
                raw, action, ran, failed
            );
            play_sound("beep_success.wav");
        } else {
            self.status = format!(
                "🎤「{}」→ 没有窗口可执行「{}」（未命中 {}，未绑定 {}）",
                raw, action, failed, skipped
            );
            play_sound("beep_fail.wav");
        }
    }

    fn run_once_windows(&mut self, windows: &[u8]) {
        let mut n = 0;
        for &w in windows {
            let idx = (w - 1) as usize;
            if idx < SLOT_COUNT && self.run_slot_once_hotkey(idx) {
                n += 1;
            }
        }
        self.status = format!("热键：单次执行了 {} 个窗口（1秒后开始）", n);
    }

    fn run_once_all(&mut self) {
        let mut n = 0;
        for idx in 0..SLOT_COUNT {
            if self.slots[idx].is_bound() && self.run_slot_once_hotkey(idx) {
                n += 1;
            }
        }
        self.status = format!("热键：单次执行全部，共 {} 个窗口（1秒后开始）", n);
    }

    /// 处理单个热键事件
    fn handle_hotkey(&mut self, key: HotkeyKey) {
        // 热键总开关关闭时忽略所有热键
        if !self.hotkey_enabled {
            return;
        }

        // 优先检查是否处于发送模式
        if self.hotkey_sm.in_send_mode() {
            if let Some(action) = self.hotkey_sm.on_send_key(key) {
                self.apply_action(action);
                return;
            }
        }

        match key {
            HotkeyKey::Digit(d) => match d {
                1..=8 => {
                    if let Some(action) = self.hotkey_sm.on_select(d) {
                        self.apply_action(action);
                    }
                }
                9 => {
                    let action = self.hotkey_sm.on_start();
                    self.apply_action(action);
                }
                0 => {
                    let action = self.hotkey_sm.on_stop();
                    self.apply_action(action);
                }
                _ => {}
            },
            HotkeyKey::Minus => {
                let action = self.hotkey_sm.on_run_once();
                self.apply_action(action);
            }
            HotkeyKey::Insert => {
                if !self.hotkey_impromptu_enabled {
                    return;
                }
                self.hotkey_sm.on_insert();
                self.status = "🎯 发送模式已激活（2秒内按任意键发送）".to_string();
            }
            _ => {
                // 其他键在非发送模式下忽略
            }
        }
    }

    /// 把主窗口 HWND 交给事件总线（只需成功一次）。
    /// 各后台事件源靠它在窗口隐藏时唤醒 update。
    fn capture_main_hwnd(&mut self, frame: &eframe::Frame) {
        if self.events.has_main_hwnd() {
            return;
        }
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = frame.window_handle() {
            if let RawWindowHandle::Win32(h) = handle.as_raw() {
                self.events.set_main_hwnd(isize::from(h.hwnd));
            }
        }
    }

    /// 请求接下来 n 帧强制唤醒，并立即先唤醒一次。
    ///
    /// 必须立即唤醒：窗口隐藏时本帧结束后不会有下一帧，
    /// 只把计数存起来是没用的（pump 永远等不到执行机会）。
    fn request_wake(&mut self, frames: u8) {
        self.wake_pending = frames;
        self.events.wake();
    }

    /// 消耗待唤醒帧数：窗口隐藏时 egui 不会自然重绘，
    /// 靠 PostMessage 续帧，直到关闭真正生效。
    fn pump_pending_wake(&mut self) {
        if self.wake_pending == 0 {
            return;
        }
        self.wake_pending -= 1;
        self.events.wake();
    }

    /// 从总线取出全部后台事件并分发（每帧一次）
    fn dispatch_events(&mut self, ctx: &egui::Context) {
        for event in self.events.poll() {
            match event {
                MainEvent::Tray(cmd) => self.handle_tray(ctx, cmd),
                MainEvent::Hotkey(key) => self.handle_hotkey(key),
                MainEvent::Voice(ev) => self.handle_voice_event(ev),
                MainEvent::Overlay(ev) => self.handle_overlay_event(ev),
            }
        }
    }

    /// 处理单个托盘事件
    fn handle_tray(&mut self, ctx: &egui::Context, cmd: TrayCommand) {
        match cmd {
            TrayCommand::Show => {
                // 先让窗口重新可见，再恢复正常显示状态（隐藏期间可能仍是最小化）
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                self.status = "已从托盘恢复".to_string();
            }
            TrayCommand::Quit => {
                // 停止所有运行，落盘配置，标记真正退出，然后关闭
                self.stop_voice();
                self.stop_overlay();
                self.stop_all();
                self.save_config();
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                // 关闭要两帧才生效（本帧投递 Close 事件，下一帧才被判定为
                // close_requested 并退出）。窗口隐藏时不会自然重绘，
                // 必须显式续帧，否则进程会卡住不退。
                self.request_wake(3);
            }
        }
    }

    fn apply_action(&mut self, action: HotkeyAction) {
        match action {
            HotkeyAction::StartWindows(ws) => self.start_windows(&ws),
            HotkeyAction::StopWindows(ws) => self.stop_windows(&ws),
            HotkeyAction::StartAll => self.start_all(),
            HotkeyAction::StopAll => self.stop_all(),
            HotkeyAction::RunOnceWindows(ws) => self.run_once_windows(&ws),
            HotkeyAction::RunOnceAll => self.run_once_all(),
            HotkeyAction::SendKey { windows, key_name } => {
                // 确定目标槽位索引
                let target_idxs: Vec<usize> = if windows.is_empty() {
                    // 没选中则发给所有绑定窗口
                    (0..SLOT_COUNT).filter(|&i| self.slots[i].is_bound()).collect()
                } else {
                    windows
                        .iter()
                        .map(|w| (w - 1) as usize)
                        .filter(|&i| i < SLOT_COUNT && self.slots[i].is_bound())
                        .collect()
                };

                // 即兴发送前先停止这些窗口的脚本执行（循环/单次都停）
                for &i in &target_idxs {
                    self.slots[i].stop();
                }

                // 收集目标窗口句柄
                let targets: Vec<isize> = target_idxs
                    .iter()
                    .filter_map(|&i| self.slots[i].hwnd)
                    .collect();

                let count = targets.len();
                self.status = format!("🎯 已停止相关窗口，1秒后发送按键 '{}' 到 {} 个窗口", key_name, count);

                // 独立线程：延迟1秒后发送（与脚本热键统一方案）
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(1));

                    use crate::input::{InputBackend, PostMessageBackend};
                    let backend = PostMessageBackend::new();

                    for hwnd in targets {
                        let handle = windows::Win32::Foundation::HWND(hwnd as *mut _);
                        let _ = backend.send_key_down(handle, &key_name);
                        let _ = backend.send_key_up(handle, &key_name);
                    }
                });
            }
        }
    }

    /// 处理抓取窗口倒计时
    fn handle_grabbing(&mut self) {
        let (Some(slot_idx), Some(since)) = (self.grabbing_slot, self.grabbing_since) else {
            return;
        };
        if since.elapsed().as_secs() >= GRAB_COUNTDOWN_SECS {
            self.grabbing_slot = None;
            self.grabbing_since = None;
            if let Some(hwnd) = win32::foreground_window() {
                // 抓到自己的窗口：清空该槽位绑定，避免自我操作
                if win32::is_own_window(hwnd) {
                    self.slots[slot_idx].stop();
                    self.slots[slot_idx].hwnd = None;
                    self.slots[slot_idx].title.clear();
                    if self.slots[slot_idx].is_main {
                        self.stop_overlay();
                    }
                    self.status = format!("窗口 {}: 抓取到本程序自己，已清空绑定", slot_idx + 1);
                    return;
                }
                let title = win32::window_title(hwnd);
                self.slots[slot_idx].hwnd = Some(hwnd.0 as isize);
                self.slots[slot_idx].title = if title.is_empty() {
                    format!("<无标题> ({:?})", hwnd.0)
                } else {
                    title
                };
                self.status = format!("窗口 {} 已抓取: {}", slot_idx + 1, self.slots[slot_idx].title);
                // 重抓的槽是主窗口且转发在跑：换目标（停旧起新）
                if self.slots[slot_idx].is_main && self.overlay.is_some() {
                    self.stop_overlay();
                    self.start_overlay();
                }
            } else {
                self.status = "抓取失败：未找到前台窗口".to_string();
            }
        }
    }

    /// 处理取色倒计时：结束后截取前台窗口并打开取色器
    fn handle_picking(&mut self) {
        let Some(since) = self.picking_since else { return };
        let elapsed = since.elapsed().as_secs();

        if elapsed >= GRAB_COUNTDOWN_SECS {
            self.picking_since = None;
            if let Some(hwnd) = win32::foreground_window() {
                if win32::is_own_window(hwnd) {
                    self.status = "取色失败：请切换到目标窗口，不能取色本程序自己".to_string();
                    return;
                }
                match self.color_picker.capture_and_open(hwnd) {
                    Ok(_) => self.status = "取色器已打开".to_string(),
                    Err(e) => self.status = format!("取色失败: {}", e),
                }
            } else {
                self.status = "取色失败：未找到前台窗口".to_string();
            }
        } else {
            // 实时更新倒计时
            let remaining = GRAB_COUNTDOWN_SECS - elapsed;
            self.status = format!("🎨 取色：{} 秒后截图，请切换到目标窗口...", remaining);
        }
    }

    /// 定期检查已绑定窗口是否仍有效，失效则停止运行、清除绑定并提示
    fn check_window_validity(&mut self) {
        // 每 1 秒检查一次
        if self.last_validity_check.elapsed().as_millis() < 1000 {
            return;
        }
        self.last_validity_check = Instant::now();

        let mut invalidated: Vec<usize> = Vec::new();
        for idx in 0..SLOT_COUNT {
            if let Some(hwnd) = self.slots[idx].hwnd {
                let handle = windows::Win32::Foundation::HWND(hwnd as *mut _);
                if !win32::is_valid(handle) {
                    invalidated.push(idx);
                }
            }
        }

        for idx in &invalidated {
            let title = self.slots[*idx].title.clone();
            // 失效的是主窗口 → 立即停止鼠标转发（覆盖窗线程也会 50ms 内自检，双保险）
            if self.slots[*idx].is_main {
                self.stop_overlay();
            }
            self.slots[*idx].stop();
            self.slots[*idx].hwnd = None;
            self.slots[*idx].title.clear();
            self.status = format!("⚠ 窗口 {} 已关闭/失效（{}），已解除绑定", idx + 1, title);
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.capture_main_hwnd(_frame);
        self.pump_pending_wake();
        // 所有后台事件（托盘/热键/语音）统一从总线取出分发
        self.dispatch_events(ctx);
        self.process_wakeword_training();
        self.handle_grabbing();
        self.handle_picking();
        self.check_window_validity();

        // 拦截关闭：点 X 时隐藏到托盘而非退出
        // （托盘可用且非“退出”指令时才拦截；否则放行真正退出）
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.tray.is_some() && !self.quitting {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.status = "已最小化到托盘，热键仍在后台生效".to_string();
            }
        }

        // 定时重绘：后台事件已由总线主动唤醒，这里只负责"没有事件也要推进"的
        // 时间驱动逻辑（抓取/取色倒计时、窗口有效性检查、状态栏刷新）。
        ctx.request_repaint_after(std::time::Duration::from_millis(30));

        self.ui_bottom_status(ctx);
        self.ui_source_panel(ctx);
        self.ui_add_scheme_window(ctx);
        self.ui_hotkey_help_window(ctx);
        self.ui_settings_window(ctx);
        self.ui_voice_help_window(ctx);
        self.ui_baidu_guide_window(ctx);
        self.ui_wakeword_guide_window(ctx);
        self.color_picker.ui(ctx);
        self.ui_central(ctx);
    }
}

impl App {
    fn ui_bottom_status(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                // 取色倒计时提示（醒目）
                if let Some(since) = self.picking_since {
                    let elapsed = since.elapsed().as_secs();
                    let remaining = GRAB_COUNTDOWN_SECS.saturating_sub(elapsed);
                    ui.colored_label(
                        egui::Color32::from_rgb(0, 200, 255),
                        format!("🎨 取色倒计时: {} 秒（请切换到目标窗口）", remaining),
                    );
                    ui.separator();
                }

                // 发送模式提示
                if self.hotkey_sm.in_send_mode() {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 140, 0),
                        "🎯 发送模式激活中（2秒内按任意键）",
                    );
                    ui.separator();
                }

                // 热键选择集提示
                let sel = self.hotkey_sm.selected();
                if !sel.is_empty() {
                    let list: Vec<String> = sel.iter().map(|n| n.to_string()).collect();
                    ui.colored_label(
                        egui::Color32::from_rgb(200, 120, 0),
                        format!("已选窗口 [{}]，按 Ctrl+Shift+9 启动 / +0 停止", list.join(",")),
                    );
                    ui.separator();
                }
                ui.label(&self.status);
            });
            ui.add_space(2.0);
        });
    }

    fn ui_source_panel(&mut self, ctx: &egui::Context) {
        let Some(idx) = self.viewing_script else { return };
        if idx >= self.scripts.len() {
            self.viewing_script = None;
            return;
        }
        egui::SidePanel::right("source_panel")
            .default_width(360.0)
            .show(ctx, |ui| {
                let sf = &self.scripts[idx];
                ui.horizontal(|ui| {
                    ui.heading(&sf.name);
                    if ui.button("关闭").clicked() {
                        self.viewing_script = None;
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut sf.source.clone())
                            .code_editor()
                            .desired_width(f32::INFINITY)
                            .interactive(false),
                    );
                });
            });
    }

    /// 为某槽位添加方案的弹窗：列出所有可用脚本，点击加入
    fn ui_add_scheme_window(&mut self, ctx: &egui::Context) {
        let Some(slot_idx) = self.adding_scheme_for else { return };
        let mut open = true;

        // 先收集所有脚本信息，避免借用冲突
        let script_list: Vec<(usize, String, String, bool, Option<Vec<crate::script::Command>>, Option<String>, crate::script::ScriptSettings)> =
            self.scripts.iter().enumerate().map(|(i, sf)| {
                (i, sf.name.clone(), sf.category.clone(), sf.is_valid(), sf.commands.clone(), sf.parse_error.clone(), sf.settings.clone())
            }).collect();

        let mut to_add: Option<(usize, String, Vec<crate::script::Command>, crate::script::ScriptSettings)> = None;

        egui::Window::new(format!("为窗口 {} 添加方案", slot_idx + 1))
            .collapsible(false)
            .resizable(true)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("点击脚本加入该窗口的方案集：");
                ui.separator();

                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    // 按分类分组
                    let mut categories: std::collections::BTreeMap<String, Vec<(usize, String, bool, Option<Vec<crate::script::Command>>, Option<String>, crate::script::ScriptSettings)>> =
                        std::collections::BTreeMap::new();
                    for (i, name, category, valid, commands, parse_error, settings) in &script_list {
                        categories.entry(category.clone())
                            .or_insert_with(Vec::new)
                            .push((*i, name.clone(), *valid, commands.clone(), parse_error.clone(), settings.clone()));
                    }

                    // 显示每个分类
                    for (category, scripts) in categories {
                        egui::CollapsingHeader::new(&category)
                            .default_open(true)
                            .show(ui, |ui| {
                                for (i, name, valid, commands, parse_error, settings) in scripts {
                                    ui.horizontal(|ui| {
                                        // 状态文本标签
                                        if valid {
                                            ui.colored_label(egui::Color32::from_rgb(0, 180, 0), "有效")
                                                .on_hover_text("脚本解析成功");
                                        } else {
                                            let error_text = parse_error.unwrap_or_else(|| "未知错误".to_string());
                                            ui.colored_label(egui::Color32::from_rgb(220, 0, 0), "无效")
                                                .on_hover_text(format!("语法错误:\n{}", error_text));
                                        }
                                        ui.label(&name);
                                        // 仅解析成功的脚本可加入
                                        if valid {
                                            if ui.small_button("加入").clicked() {
                                                if let Some(cmds) = commands {
                                                    to_add = Some((i, name.clone(), cmds, settings));
                                                }
                                            }
                                        }
                                    });
                                }
                            });
                    }
                });
            });

        // 处理添加操作
        if let Some((script_idx, script_name, commands, settings)) = to_add {
            let scheme = Scheme {
                script_name,
                commands,
                settings,
            };
            if self.slots[slot_idx].add_scheme(scheme) {
                self.status = format!("窗口 {} 已添加方案: {}", slot_idx + 1, self.scripts[script_idx].name);
                self.save_config();
            } else {
                self.status = format!("方案已存在: {}", self.scripts[script_idx].name);
            }
        }

        if !open {
            self.adding_scheme_for = None;
        }
    }

    /// 热键说明窗口
    fn ui_hotkey_help_window(&mut self, ctx: &egui::Context) {
        if !self.show_hotkey_help {
            return;
        }

        egui::Window::new("🎮 热键使用说明")
            .collapsible(false)
            .resizable(true)
            .default_width(600.0)
            .open(&mut self.show_hotkey_help)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("所有热键都以 Ctrl+Shift 开头，支持前缀选择和批量操作")
                        .strong()
                        .color(egui::Color32::from_rgb(100, 150, 255)),
                );
                ui.add_space(8.0);

                egui::ScrollArea::vertical().max_height(500.0).show(ui, |ui| {
                    // 场景1：窗口选择
                    ui.collapsing("📌 场景一：选择窗口（前缀选择）", |ui| {
                        ui.label("用于指定后续操作作用于哪些窗口。");
                        ui.add_space(4.0);
                        ui.label("• 热键：Ctrl+Shift+[1~8]");
                        ui.label("• 首次按下：加入选择集");
                        ui.label("• 重复按下：立即停止该窗口（移出选择集）");
                        ui.label("• 可连续按多个数字选择多个窗口");
                        ui.label("• 选中后状态栏会显示黄色提示");
                        ui.label("• 3秒内未触发操作则自动清空选择");
                        ui.add_space(4.0);
                        ui.group(|ui| {
                            ui.label(egui::RichText::new("示例：").strong());
                            ui.monospace("  Ctrl+Shift+1");
                            ui.label("  → 选中窗口1，状态栏显示「已选窗口 [1]」");
                            ui.add_space(2.0);
                            ui.monospace("  Ctrl+Shift+1 (再按一次)");
                            ui.label("  → 停止窗口1的脚本执行");
                            ui.add_space(2.0);
                            ui.monospace("  Ctrl+Shift+1, Ctrl+Shift+3");
                            ui.label("  → 选中窗口1和3");
                        });
                    });

                    ui.add_space(8.0);

                    // 场景2：循环执行
                    ui.collapsing("🔁 场景二：循环执行脚本", |ui| {
                        ui.label("启动窗口的标识方案，脚本会循环执行直到手动停止。");
                        ui.add_space(4.0);
                        ui.label("• 热键：Ctrl+Shift+9");
                        ui.label("• 有前缀选择 → 启动选中的窗口");
                        ui.label("• 无前缀选择 → 启动所有已绑定窗口");
                        ui.label("• 延迟1秒后开始执行（给你时间松开按键）");
                        ui.add_space(4.0);
                        ui.group(|ui| {
                            ui.label(egui::RichText::new("示例：").strong());
                            ui.monospace("  Ctrl+Shift+9");
                            ui.label("  → 启动所有窗口的标识方案");
                            ui.add_space(2.0);
                            ui.monospace("  Ctrl+Shift+2, Ctrl+Shift+9");
                            ui.label("  → 只启动窗口2的标识方案");
                        });
                    });

                    ui.add_space(8.0);

                    // 场景3：停止执行
                    ui.collapsing("⏹ 场景三：停止执行", |ui| {
                        ui.label("停止正在运行的脚本。");
                        ui.add_space(4.0);
                        ui.label("• 热键：Ctrl+Shift+0");
                        ui.label("• 有前缀选择 → 停止选中的窗口");
                        ui.label("• 无前缀选择 → 停止所有窗口");
                        ui.add_space(4.0);
                        ui.group(|ui| {
                            ui.label(egui::RichText::new("示例：").strong());
                            ui.monospace("  Ctrl+Shift+0");
                            ui.label("  → 停止所有正在运行的窗口");
                            ui.add_space(2.0);
                            ui.monospace("  Ctrl+Shift+1, Ctrl+Shift+3, Ctrl+Shift+0");
                            ui.label("  → 只停止窗口1和3");
                        });
                    });

                    ui.add_space(8.0);

                    // 场景4：单次执行
                    ui.collapsing("▶ 场景四：单次执行脚本", |ui| {
                        ui.label("执行一次标识方案后自动停止（不循环）。");
                        ui.add_space(4.0);
                        ui.label("• 热键：Ctrl+Shift+-（减号键）");
                        ui.label("• 有前缀选择 → 单次执行选中的窗口");
                        ui.label("• 无前缀选择 → 单次执行所有已绑定窗口");
                        ui.label("• 延迟1秒后开始执行");
                        ui.add_space(4.0);
                        ui.group(|ui| {
                            ui.label(egui::RichText::new("示例：").strong());
                            ui.monospace("  Ctrl+Shift+-");
                            ui.label("  → 所有窗口的标识方案各执行一次");
                            ui.add_space(2.0);
                            ui.monospace("  Ctrl+Shift+5, Ctrl+Shift+-");
                            ui.label("  → 只有窗口5执行一次");
                        });
                    });

                    ui.add_space(8.0);

                    // 场景5：即兴发送
                    ui.collapsing("🎯 场景五：即兴发送任意按键", |ui| {
                        ui.label("快速向窗口发送单个按键，无需编写脚本。");
                        ui.add_space(4.0);
                        ui.label("• 步骤1：Ctrl+Shift+Insert（进入发送模式）");
                        ui.label("• 步骤2：2秒内按 Ctrl+Shift+<任意键>");
                        ui.label("• 支持：字母A-Z、数字0-9、F1-F12、空格/回车/Tab/ESC");
                        ui.label("• 有前缀选择 → 发给选中窗口");
                        ui.label("• 无前缀选择 → 发给所有已绑定窗口");
                        ui.label("• 发送前会先停止目标窗口正在运行的脚本");
                        ui.label("• 延迟1秒后发送");
                        ui.add_space(4.0);
                        ui.group(|ui| {
                            ui.label(egui::RichText::new("示例：").strong());
                            ui.monospace("  Ctrl+Shift+Insert → Ctrl+Shift+H");
                            ui.label("  → 所有窗口收到按键 'H'");
                            ui.add_space(2.0);
                            ui.monospace("  Ctrl+Shift+2 → Ctrl+Shift+Insert → Ctrl+Shift+F5");
                            ui.label("  → 窗口2收到 'F5'（若窗口2在跑脚本，会先停止）");
                        });
                    });

                    ui.add_space(12.0);

                    // 小贴士
                    ui.group(|ui| {
                        ui.label(egui::RichText::new("💡 小贴士").strong());
                        ui.add_space(2.0);
                        ui.label("• 所有热键触发操作都会延迟1秒执行，避免修饰键冲突");
                        ui.label("• 发送模式激活后，状态栏会显示黄色提示");
                        ui.label("• 前缀选择3秒自动清空，发送模式2秒自动退出");
                        ui.label("• 窗口需先绑定方案并设置标识（★）才能执行");
                        ui.label("• 抓取窗口时不能选择本程序自己的窗口");
                        ui.label("• 同一窗口数字键按两次即可快速停止其脚本");
                    });
                });
            });
    }

    /// 语音设置窗口：百度密钥 + 使用说明

    /// 统一配置窗口
    fn ui_settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        let mut act_save = false;

        egui::Window::new("⚙ 设置")
            .collapsible(false)
            .resizable(true)
            .default_width(500.0)
            .open(&mut open)
            .show(ctx, |ui| {
                // 标签页选择
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.settings_tab, SettingsTab::General, "🔧 通用");
                    ui.selectable_value(&mut self.settings_tab, SettingsTab::Voice, "🎤 语音控制");
                    ui.selectable_value(&mut self.settings_tab, SettingsTab::Hotkey, "⌨️ 热键配置");
                    ui.selectable_value(&mut self.settings_tab, SettingsTab::About, "ℹ️ 关于");
                });
                ui.separator();

                match self.settings_tab {
                    SettingsTab::General => self.ui_settings_general(ui, &mut act_save),
                    SettingsTab::Voice => self.ui_settings_voice(ui, &mut act_save),
                    SettingsTab::Hotkey => self.ui_settings_hotkey(ui, &mut act_save),
                    SettingsTab::About => self.ui_settings_about(ui),
                }

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("💾 保存").clicked() {
                        act_save = true;
                    }
                });
            });

        if act_save {
            self.save_config();
            self.status = "配置已保存".to_string();
        }
        if !open {
            self.show_settings = false;
        }
    }

    /// 通用配置标签页
    fn ui_settings_general(&mut self, ui: &mut egui::Ui, _act_save: &mut bool) {
        ui.label(egui::RichText::new("日志设置").strong());
        ui.add_space(4.0);

        ui.checkbox(&mut self.log_enabled, "启用日志文件");
        ui.add_space(2.0);
        ui.label("禁用后将不会写入 voice_debug.log 日志文件");

        if !self.log_enabled {
            ui.add_space(4.0);
            ui.colored_label(
                egui::Color32::from_rgb(200, 120, 0),
                "⚠ 日志已禁用，问题排查将受限"
            );
        }

        ui.add_space(12.0);
        ui.label(egui::RichText::new("唤醒词训练").strong());
        ui.add_space(4.0);

        ui.checkbox(&mut self.save_wakeword_samples, "保存训练样本");
        ui.add_space(2.0);
        ui.label("开启后，训练唤醒词时会保存录音样本到 wakeword_samples/ 目录");
        ui.label("关闭后，训练时使用临时文件，训练完成后自动删除");

        ui.add_space(12.0);
        ui.label(egui::RichText::new("语音识别调试").strong());
        ui.add_space(4.0);

        ui.checkbox(&mut self.save_asr_audio, "保存 ASR 音频");
        ui.add_space(2.0);
        ui.label("开启后，将发送给百度 ASR 的音频保存到 sendvoice/ 目录");
        ui.label("关闭后，不保存音频文件（默认）");
    }

    /// 语音配置标签页
    fn ui_settings_voice(&mut self, ui: &mut egui::Ui, _act_save: &mut bool) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("百度语音识别密钥")
                    .strong(),
            );
            ui.label("（");
            if ui.link("在百度智能云控制台申请").clicked() {
                self.show_baidu_guide = true;
            }
            ui.label("）");
        });
        ui.add_space(4.0);
        egui::Grid::new("baidu_keys").num_columns(2).show(ui, |ui| {
            ui.label("API Key:");
            ui.add(
                egui::TextEdit::singleline(&mut self.baidu_api_key)
                    .desired_width(320.0)
                    .password(true),
            );
            ui.end_row();
            ui.label("Secret Key:");
            ui.add(
                egui::TextEdit::singleline(&mut self.baidu_secret_key)
                    .desired_width(320.0)
                    .password(true),
            );
            ui.end_row();
        });

        ui.add_space(8.0);
        let model_ok = std::path::Path::new(WAKEWORD_MODEL_PATH).exists();
        ui.horizontal(|ui| {
            ui.label("唤醒词模型:");
            if model_ok {
                if ui.link(format!("✓ {}", WAKEWORD_MODEL_PATH))
                    .on_hover_text("点击查看训练指南")
                    .clicked() {
                    self.show_wakeword_guide = true;
                }
            } else {
                if ui.link(format!("✗ 缺失 {}", WAKEWORD_MODEL_PATH))
                    .on_hover_text("点击查看如何训练唤醒词模型")
                    .clicked() {
                    self.show_wakeword_guide = true;
                }
            }
        });

        ui.add_space(12.0);
        ui.label(egui::RichText::new("指令匹配").strong());
        ui.add_space(4.0);

        ui.checkbox(&mut self.pinyin_assist, "拼音辅助匹配");
        ui.add_space(2.0);
        ui.label("开启后，字符匹配之外再做一轮拼音匹配（忽略声调、多音字全读音），取更优结果");
        ui.label("可救回同音误识别（如“加血”被听成“加雪”）；打平时以字符匹配为准");

        if !self.last_voice_text.is_empty() {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("最近识别:");
                ui.colored_label(
                    egui::Color32::LIGHT_BLUE,
                    &self.last_voice_text,
                );
            });
        }

        ui.add_space(8.0);
        if ui.button("📖 查看语音指令帮助").clicked() {
            self.show_voice_help = true;
        }
    }

    /// 热键配置标签页
    fn ui_settings_hotkey(&mut self, ui: &mut egui::Ui, _act_save: &mut bool) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.hotkey_enabled, "启用热键");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("❓ 热键说明").clicked() {
                    self.show_hotkey_help = true;
                }
            });
        });
        ui.add_space(4.0);

        if !self.hotkey_enabled {
            ui.colored_label(egui::Color32::from_rgb(200, 120, 0), "⚠ 热键已全局禁用");
            ui.add_space(4.0);
        }

        ui.label(egui::RichText::new("已注册热键列表").strong());
        ui.add_space(4.0);

        egui::Grid::new("hotkey_list")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                ui.label("功能");
                ui.label("热键");
                ui.end_row();

                ui.label("窗口选择");
                ui.monospace("Ctrl+Shift+1~8");
                ui.end_row();

                ui.label("循环启动");
                ui.monospace("Ctrl+Shift+9");
                ui.end_row();

                ui.label("全部停止");
                ui.monospace("Ctrl+Shift+0");
                ui.end_row();

                ui.label("单次执行");
                ui.monospace("Ctrl+Shift+- (减号)");
                ui.end_row();

                ui.label("即兴发送");
                ui.horizontal(|ui| {
                    ui.monospace("Ctrl+Shift+Insert + [A-Z/0-9/F1-F12/Space]");
                    ui.checkbox(&mut self.hotkey_impromptu_enabled, "");
                });
                ui.end_row();
            });

        ui.add_space(8.0);
        ui.label("💡 提示：");
        ui.label("• 即兴发送：按 Ctrl+Shift+Insert 进入发送模式，2秒内按任意支持的键");
        ui.label("• 热键仅在程序运行时生效，关闭后自动注销");
        ui.label("• 点击右上角「❓ 热键说明」查看详细使用指南");
    }

    /// 关于页面
    fn ui_settings_about(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(16.0);
            ui.heading("Game Auto Keyboard");
            ui.add_space(4.0);
            ui.label(egui::RichText::new(format!("版本 {}", env!("BUILD_DATE"))).weak());
            ui.add_space(16.0);
        });

        ui.add_space(8.0);

        ui.label(egui::RichText::new("作者").strong());
        ui.add_space(2.0);
        ui.label("wireboy");
        ui.add_space(12.0);

        ui.label(egui::RichText::new("源码仓库").strong());
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label("GitHub:");
            ui.hyperlink_to(
                "bobowire/Game-Auto-Keyboard",
                "https://github.com/bobowire/Game-Auto-Keyboard",
            );
        });
        ui.horizontal(|ui| {
            ui.label("Gitee:");
            ui.hyperlink_to(
                "wireboy/game-multi-utils",
                "https://gitee.com/wireboy/game-multi-utils",
            );
        });
        ui.add_space(12.0);

        ui.label(egui::RichText::new("功能简介").strong());
        ui.add_space(2.0);
        ui.label("• 多窗口脚本自动化执行");
        ui.label("• 语音控制（唤醒词 + ASR 识别）");
        ui.label("• 热键触发、颜色检测、自动循环等");
        ui.label("• 支持自定义脚本（.ag 文件）");
    }

    /// 语音帮助文档窗口
    fn ui_voice_help_window(&mut self, ctx: &egui::Context) {
        if !self.show_voice_help {
            return;
        }
        let mut open = true;

        egui::Window::new("📖 语音指令帮助")
            .collapsible(false)
            .resizable(true)
            .default_width(480.0)
            .open(&mut open)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("支持的语音指令");
                    ui.add_space(8.0);

                    ui.label(egui::RichText::new("1. 执行动作").strong());
                    ui.monospace("  小助手，窗口1跟随我");
                    ui.label("  → 窗口1执行名字含「跟随」的脚本");
                    ui.monospace("  小助手，窗口1加血");
                    ui.label("  → 窗口1执行名字含「加血」的脚本");
                    ui.add_space(4.0);

                    ui.label(egui::RichText::new("2. 停止指令").strong());
                    ui.monospace("  小助手，所有人停止");
                    ui.label("  → 停止全部窗口");
                    ui.monospace("  小助手，窗口1停止");
                    ui.label("  → 停止窗口1");
                    ui.add_space(8.0);

                    ui.label(egui::RichText::new("使用说明").strong());
                    ui.label("• 脚本需先在对应窗口「+ 添加方案」");
                    ui.label("• 窗口名可在各槽位标题处编辑（如改成「主号」）");
                    ui.label("• 动作按脚本名（去扩展名）包含匹配，优先匹配度高的");
                    ui.label("• 支持中文数字：「窗口一」自动识别为「窗口1」");
                    ui.add_space(8.0);

                    ui.label(egui::RichText::new("故障排查").strong());
                    ui.label("• 查看日志文件 voice_debug.log 了解识别过程");
                    ui.label("• 详细排查指南见 VOICE_DEBUG.md");
                });
            });

        if !open {
            self.show_voice_help = false;
        }
    }

    /// 百度申请引导窗口
    fn ui_baidu_guide_window(&mut self, ctx: &egui::Context) {
        if !self.show_baidu_guide {
            return;
        }

        let viewport_id = egui::ViewportId::from_hash_of("baidu_guide_viewport");
        let builder = egui::ViewportBuilder::default()
            .with_title("📝 如何申请百度语音识别密钥")
            .with_inner_size([520.0, 600.0]);

        let mut should_close = false;
        ctx.show_viewport_immediate(viewport_id, builder, |ctx, _class| {
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("百度智能云控制台");
                    ui.add_space(4.0);

                    ui.label("点击下方链接打开百度智能云控制台：");
                    ui.hyperlink_to(
                        "🔗 https://console.bce.baidu.com/ai/#/ai/speech/overview/index",
                        "https://console.bce.baidu.com/ai/#/ai/speech/overview/index"
                    );

                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.label(egui::RichText::new("申请步骤：").strong());
                    ui.add_space(4.0);

                    ui.label("1️⃣ 登录百度账号（没有则先注册）");
                    ui.add_space(2.0);

                    ui.label("2️⃣ 完成实名认证（必须，否则无免费额度）");
                    ui.indent("auth_tip", |ui| {
                        ui.colored_label(
                            egui::Color32::from_rgb(200, 80, 0),
                            "⚠ 未实名认证的账号无法使用免费额度"
                        );
                    });
                    ui.add_space(2.0);

                    ui.label("3️⃣ 进入「语音技术」→「短语音识别」");
                    ui.add_space(2.0);

                    ui.label("4️⃣ 点击「创建应用」");
                    ui.add_space(2.0);

                    ui.label("5️⃣ 填写应用信息：");
                    ui.indent("app_info", |ui| {
                        ui.label("• 应用名称：随意填写（如「游戏助手」）");
                        ui.label("• 接口选择：勾选「短语音识别」");
                        ui.label("• 应用归属：个人");
                    });
                    ui.add_space(2.0);

                    ui.label("6️⃣ 创建成功后，在应用列表查看：");
                    ui.indent("keys", |ui| {
                        ui.label("• API Key（复制到设置窗口）");
                        ui.label("• Secret Key（复制到设置窗口）");
                    });

                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.label("💡 提示：");
                    ui.label("• 实名认证后每个账号有免费额度（每天50,000次调用）");
                    ui.label("• 超出免费额度后按次计费");
                    ui.label("• 详细定价见官网文档");

                    ui.add_space(12.0);
                    if ui.button("✅ 我已了解").clicked() {
                        should_close = true;
                    }
                });
            });

            if ctx.input(|i| i.viewport().close_requested()) {
                should_close = true;
            }
        });

        if should_close {
            self.show_baidu_guide = false;
        }
    }

    /// 唤醒词训练引导窗口
    fn ui_wakeword_guide_window(&mut self, ctx: &egui::Context) {
        if !self.show_wakeword_guide {
            return;
        }

        let viewport_id = egui::ViewportId::from_hash_of("wakeword_guide_viewport");
        let builder = egui::ViewportBuilder::default()
            .with_title("🎤 唤醒词训练")
            .with_inner_size([450.0, 350.0]);

        let mut should_close = false;
        let mut start_training = false;
        let mut start_recording = false;

        ctx.show_viewport_immediate(viewport_id, builder, |ctx, _class| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_space(8.0);

                // 如果没有训练状态，显示开始界面
                if self.wakeword_training.is_none() {
                    ui.heading("训练唤醒词「小助手」");
                    ui.add_space(12.0);

                    ui.label("需要录制 4 遍唤醒词来训练识别模型");
                    ui.add_space(8.0);

                    ui.label("📝 录制要求：");
                    ui.indent("requirements", |ui| {
                        ui.label("• 每次录制 1.5 秒");
                        ui.label("• 环境安静，发音清晰");
                        ui.label("• 4 遍读音保持一致");
                        ui.label("• 点击按钮后立即说话");
                    });

                    ui.add_space(20.0);

                    ui.horizontal(|ui| {
                        if ui.button("✅ 开始训练").clicked() {
                            start_training = true;
                        }
                        ui.add_space(8.0);
                        if ui.button("❌ 取消").clicked() {
                            should_close = true;
                        }
                    });
                } else {
                    // 训练进行中
                    if let Some(training) = &self.wakeword_training {
                        ui.heading(format!("第 {}/{} 遍", training.current_round, training.total_rounds));
                        ui.add_space(12.0);

                        ui.label(
                            egui::RichText::new("请清晰地说：小助手")
                                .size(20.0)
                                .color(egui::Color32::from_rgb(100, 150, 255))
                        );
                        ui.add_space(16.0);

                        // 显示状态
                        if training.is_recording {
                            if let Some(start) = training.record_start {
                                let elapsed = start.elapsed().as_secs_f32();
                                let progress = (elapsed / training.record_duration).min(1.0);

                                ui.add(egui::ProgressBar::new(progress)
                                    .text(format!("🔴 录音中... {:.1}/{:.1}秒", elapsed, training.record_duration))
                                    .desired_width(350.0));
                            }
                        } else {
                            ui.colored_label(egui::Color32::GREEN, &training.status_msg);
                            ui.add_space(12.0);

                            if training.current_round <= training.total_rounds {
                                if ui.button("🎙 点击开始录制").clicked() {
                                    start_recording = true;
                                }
                            }
                        }

                        ui.add_space(16.0);
                        ui.separator();
                        ui.add_space(8.0);

                        // 显示已完成的样本
                        ui.label(format!("已完成: {}/{}", training.samples.len(), training.total_rounds));
                    }
                }
            });

            if ctx.input(|i| i.viewport().close_requested()) {
                should_close = true;
            }
        });

        // 处理动作
        if start_training {
            self.start_wakeword_training();
        }

        if start_recording {
            self.start_wakeword_recording();
        }

        if should_close {
            self.show_wakeword_guide = false;
            self.wakeword_training = None;
        }
    }

    /// 开始唤醒词训练
    fn start_wakeword_training(&mut self) {
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
    fn start_wakeword_recording(&mut self) {
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

    fn ui_central(&mut self, ctx: &egui::Context) {
        // 收集待执行的动作，避免在借用 slots 时修改 self
        let mut act_start: Option<usize> = None;
        let mut act_stop: Option<usize> = None;
        let mut act_grab: Option<usize> = None;
        let mut act_add: Option<usize> = None;
        let mut act_view: Option<usize> = None;
        let mut act_reload = false;
        let mut act_start_all = false;
        let mut act_stop_all = false;
        let mut act_pick = false;
        let mut act_toggle_voice = false;
        let mut act_toggle_overlay = false;

        egui::CentralPanel::default().show(ctx, |ui| {
            // 顶部工具行
            ui.horizontal(|ui| {
                ui.heading("窗口方案管理");
                if ui.button("🔄 重载脚本").clicked() {
                    act_reload = true;
                }
                if ui.button("🎨 取色").clicked() {
                    act_pick = true;
                }
                // 语音控制开关
                let voice_on = self.voice.is_some();
                let voice_label = if voice_on { "🎤 语音: 开" } else { "🎤 语音: 关" };
                if ui
                    .button(egui::RichText::new(voice_label).color(if voice_on {
                        egui::Color32::GREEN
                    } else {
                        egui::Color32::GRAY
                    }))
                    .on_hover_text("开启/关闭语音控制")
                    .clicked()
                {
                    act_toggle_voice = true;
                }
                // 鼠标转发开关
                let overlay_on = self.overlay.is_some();
                let overlay_label = if overlay_on { "🖱 转发: 开" } else { "🖱 转发: 关" };
                if ui
                    .button(egui::RichText::new(overlay_label).color(if overlay_on {
                        egui::Color32::GREEN
                    } else {
                        egui::Color32::GRAY
                    }))
                    .on_hover_text("用半透明覆盖窗盖住主窗口客户区，把鼠标操作转发给主窗口")
                    .clicked()
                {
                    act_toggle_overlay = true;
                }
                if ui.button("⚙ 设置").clicked() {
                    self.show_settings = true;
                }
                ui.separator();
                if ui.button("▶ 全部启动").clicked() {
                    act_start_all = true;
                }
                if ui.button("⏹ 全部停止").clicked() {
                    act_stop_all = true;
                }
            });
            ui.label(
                egui::RichText::new(
                    "热键: Ctrl+Shift+[1-8] 选窗口 → +9 循环启动 / +0 停止 / +- 单次执行 / +Insert 发送任意键；不选则作用于全部",
                )
                .small()
                .color(egui::Color32::GRAY),
            );
            ui.separator();

            egui::ScrollArea::vertical().show(ui, |ui| {
                for idx in 0..SLOT_COUNT {
                    self.ui_slot(
                        ui,
                        idx,
                        &mut act_start,
                        &mut act_stop,
                        &mut act_grab,
                        &mut act_add,
                        &mut act_view,
                    );
                }
            });
        });

        // 统一处理动作
        if act_reload {
            self.reload_scripts();
        }
        if act_pick {
            self.picking_since = Some(Instant::now());
            self.status = "取色：3 秒内切换到目标窗口...".to_string();
        }
        if act_start_all {
            self.start_all();
        }
        if act_stop_all {
            self.stop_all();
        }
        if let Some(i) = act_grab {
            self.grabbing_slot = Some(i);
            self.grabbing_since = Some(Instant::now());
            self.status = format!("窗口 {}: 倒计时中，请切换到目标窗口", i + 1);
        }
        if let Some(i) = act_add {
            self.adding_scheme_for = Some(i);
        }
        if let Some(i) = act_view {
            self.viewing_script = Some(i);
        }
        if let Some(i) = act_start {
            self.start_slot(i);
        }
        if let Some(i) = act_stop {
            self.stop_slot(i);
        }
        if act_toggle_voice {
            if self.voice.is_some() {
                self.stop_voice();
            } else {
                self.start_voice();
            }
        }
        if act_toggle_overlay {
            if self.overlay.is_some() {
                self.stop_overlay();
            } else {
                self.start_overlay();
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn ui_slot(
        &mut self,
        ui: &mut egui::Ui,
        idx: usize,
        act_start: &mut Option<usize>,
        act_stop: &mut Option<usize>,
        act_grab: &mut Option<usize>,
        act_add: &mut Option<usize>,
        act_view: &mut Option<usize>,
    ) {
        let selected = self.hotkey_sm.selected().contains(&((idx + 1) as u8));
        let running = self.slots[idx].is_running();

        let frame = egui::Frame::group(ui.style()).fill(if selected {
            egui::Color32::from_rgb(60, 55, 20)
        } else {
            ui.style().visuals.faint_bg_color
        });

        let mut name_changed = false;
        let mut main_toggled: Option<bool> = None;
        frame.show(ui, |ui| {
            // 标题行
            ui.horizontal(|ui| {
                // 主窗口旗标（鼠标转发目标，全局互斥）
                let is_main = self.slots[idx].is_main;
                if ui
                    .small_button(
                        egui::RichText::new("⚑").color(if is_main {
                            egui::Color32::GOLD
                        } else {
                            egui::Color32::DARK_GRAY
                        }),
                    )
                    .on_hover_text(if is_main {
                        "取消主窗口标记"
                    } else {
                        "设为主窗口（鼠标转发目标）"
                    })
                    .clicked()
                {
                    main_toggled = Some(!is_main);
                }
                ui.strong(format!("{}.", idx + 1));
                // 可编辑的窗口名（语音指称用）
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.slots[idx].name)
                        .desired_width(90.0)
                        .hint_text(format!("窗口{}", idx + 1)),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    name_changed = true;
                }
                if resp.lost_focus() {
                    name_changed = true;
                }
                if running {
                    ui.colored_label(egui::Color32::GREEN, "● 运行中");
                } else {
                    ui.colored_label(egui::Color32::GRAY, "○ 空闲");
                }

                // 抓取/清除窗口
                let grabbing_this =
                    self.grabbing_slot == Some(idx) && self.grabbing_since.is_some();
                if grabbing_this {
                    let remain = GRAB_COUNTDOWN_SECS.saturating_sub(
                        self.grabbing_since.unwrap().elapsed().as_secs(),
                    );
                    ui.label(format!("⏳ {}s", remain));
                } else if ui.small_button("抓取窗口").clicked() {
                    *act_grab = Some(idx);
                }
            });

            // 窗口标题显示
            if self.slots[idx].is_bound() {
                ui.horizontal(|ui| {
                    ui.label("目标:");
                    ui.monospace(
                        egui::RichText::new(&self.slots[idx].title)
                            .color(egui::Color32::LIGHT_BLUE),
                    );
                });
            } else {
                ui.colored_label(egui::Color32::DARK_GRAY, "未绑定窗口");
            }

            // 方案集
            ui.horizontal(|ui| {
                ui.label("方案:");
                if ui.small_button("+ 添加方案").clicked() {
                    *act_add = Some(idx);
                }
            });

            let scheme_count = self.slots[idx].schemes.len();
            if scheme_count == 0 {
                ui.colored_label(egui::Color32::DARK_GRAY, "  (无方案)");
            } else {
                let marked = self.slots[idx].marked;
                let mut set_marked: Option<usize> = None;
                let mut remove: Option<usize> = None;

                for s in 0..scheme_count {
                    ui.horizontal(|ui| {
                        // 标识单选（★）
                        let is_marked = marked == Some(s);
                        let star = if is_marked { "★" } else { "☆" };
                        if ui
                            .small_button(star)
                            .on_hover_text("设为标识方案")
                            .clicked()
                        {
                            set_marked = Some(s);
                        }

                        let name = &self.slots[idx].schemes[s].script_name;
                        if is_marked {
                            ui.colored_label(egui::Color32::GOLD, name);
                        } else {
                            ui.label(name);
                        }

                        // 查看源码（在脚本池中找同名）
                        if ui.small_button("查看").clicked() {
                            if let Some(p) =
                                self.scripts.iter().position(|sf| &sf.name == name)
                            {
                                *act_view = Some(p);
                            }
                        }
                        if ui.small_button("移除").clicked() {
                            remove = Some(s);
                        }
                    });
                }

                if let Some(s) = set_marked {
                    self.slots[idx].set_marked(s);
                    self.save_config();
                }
                if let Some(s) = remove {
                    self.slots[idx].remove_scheme(s);
                    self.save_config();
                }
            }

            // 启停按钮
            ui.horizontal(|ui| {
                let can_start = self.slots[idx].is_bound()
                    && self.slots[idx].marked_scheme().is_some();
                if ui
                    .add_enabled(!running && can_start, egui::Button::new("▶ 启动标识方案"))
                    .clicked()
                {
                    *act_start = Some(idx);
                }
                if ui
                    .add_enabled(running, egui::Button::new("⏹ 停止"))
                    .clicked()
                {
                    *act_stop = Some(idx);
                }
            });
        });
        ui.add_space(4.0);

        // 窗口名编辑完成后持久化（空则回退默认名）
        if name_changed {
            if self.slots[idx].name.trim().is_empty() {
                self.slots[idx].name = format!("窗口{}", idx + 1);
            }
            self.save_config();
        }

        // 主窗口标记切换（互斥：先清全部再按需设置）
        if let Some(make_main) = main_toggled {
            for s in &mut self.slots {
                s.is_main = false;
            }
            if make_main {
                self.slots[idx].is_main = true;
            }
            self.save_config();
            // 转发在跑时切换主窗口 = 换跟踪目标
            if self.overlay.is_some() {
                self.stop_overlay();
                if make_main {
                    self.start_overlay(); // 新主窗口未绑定时内部会给出提示
                } else {
                    self.status = "🖱 已取消主窗口标记，鼠标转发停止".to_string();
                }
            }
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

