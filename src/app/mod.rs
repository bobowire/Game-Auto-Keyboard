// egui 主应用 - 多窗口 + 方案标识 + 热键
//
// App 承载全部应用状态；本文件保留结构体定义、构造、配置读写，以及
// eframe::App::update 主编排。各功能子系统的实现按职责拆分到同目录子模块：
//   events（事件分发）/ grab（抓窗口取色）/ overlay（鼠标转发）/ slots（槽位执行）/
//   voice_ctrl（语音编排）/ wakeword_train（唤醒词训练）/ ui（界面渲染）。
mod events;
mod grab;
mod overlay;
mod slots;
mod ui;
mod voice_ctrl;
mod wakeword_train;

use crate::color_picker::ColorPicker;
use crate::config::AppConfig;
use crate::event_bus::{MainEventBus, WakeTicker};
use crate::hotkey::{HotkeyManager, HotkeyStateMachine};
use crate::overlay::OverlayWindow;
use crate::script::{load_dir, ScriptFile};
use crate::tray::Tray;
use crate::voice::{AudioCapture, VoiceRuntime, vlog};
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

    // 鼠标转发配置（编辑用），从 config 加载
    forward_rbutton_move: bool,
    forward_keyboard: bool,
    forward_marked_only: bool,

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
    Forward,
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
            forward_rbutton_move: config.forward.rbutton_broadcast_move,
            forward_keyboard: config.forward.keyboard_broadcast,
            forward_marked_only: config.forward.keyboard_marked_only,
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
        cfg.forward.rbutton_broadcast_move = self.forward_rbutton_move;
        cfg.forward.keyboard_broadcast = self.forward_keyboard;
        cfg.forward.keyboard_marked_only = self.forward_marked_only;

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

}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.capture_main_hwnd(_frame);
        self.pump_pending_wake();
        // 所有后台事件（托盘/热键/语音）统一从总线取出分发
        self.dispatch_events(ctx);
        self.process_wakeword_training();
        self.handle_grabbing(ctx);
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
