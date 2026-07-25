// egui 主应用 - 多窗口 + 方案标识 + 热键

use crate::color_picker::ColorPicker;
use crate::config::AppConfig;
use crate::hotkey::{HotkeyAction, HotkeyKey, HotkeyManager, HotkeyStateMachine};
use crate::runner::Runner;
use crate::script::{load_dir, ScriptFile};
use crate::tray::{Tray, TrayCommand};
use crate::utils::win32;
use crate::window_slot::{Scheme, WindowSlot};
use eframe::egui;
use std::path::PathBuf;
use std::time::Instant;

const SCRIPTS_DIR: &str = "scripts";
const GRAB_COUNTDOWN_SECS: u64 = 3;
const SLOT_COUNT: usize = 8;

pub struct App {
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
    hotkey_mgr: Option<HotkeyManager>,
    hotkey_sm: HotkeyStateMachine,

    // 系统托盘
    tray: Option<Tray>,
    // 是否正在真正退出（托盘“退出”触发），用于放行关闭请求
    quitting: bool,

    // 状态提示
    status: String,
}

impl App {
    pub fn new() -> Self {
        let scripts_dir = PathBuf::from(SCRIPTS_DIR);
        let scripts = load_dir(&scripts_dir).unwrap_or_default();

        // 从配置恢复方案绑定（按文件名从脚本池重建命令）
        let config = AppConfig::load();
        let mut slots = Vec::with_capacity(SLOT_COUNT);
        for i in 0..SLOT_COUNT {
            let mut slot = WindowSlot::default();
            if let Some(sc) = config.slots.get(i) {
                for name in &sc.scheme_names {
                    if let Some(sf) = scripts.iter().find(|s| &s.name == name) {
                        if let Some(cmds) = &sf.commands {
                            slot.schemes.push(Scheme {
                                script_name: sf.name.clone(),
                                commands: cmds.clone(),
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
            }
            slots.push(slot);
        }

        // 尝试启动热键
        let (hotkey_mgr, status) = match HotkeyManager::start() {
            Ok(mgr) => (
                Some(mgr),
                format!("已加载 {} 个脚本；热键已就绪 (Ctrl+Shift+0~9)", scripts.len()),
            ),
            Err(e) => (None, format!("已加载 {} 个脚本；热键注册失败: {}", scripts.len(), e)),
        };

        // 创建托盘（失败不致命，仅记录）
        let tray = match Tray::new() {
            Ok(t) => Some(t),
            Err(e) => {
                eprintln!("托盘创建失败: {}", e);
                None
            }
        };

        Self {
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
            hotkey_mgr,
            hotkey_sm: HotkeyStateMachine::new(),
            tray,
            quitting: false,
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
            cfg.slots[i].scheme_names =
                slot.schemes.iter().map(|s| s.script_name.clone()).collect();
            cfg.slots[i].marked = slot.marked;
        }
        if let Err(e) = cfg.save() {
            eprintln!("保存配置失败: {}", e);
        }
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

    /// 处理热键事件
    fn process_hotkeys(&mut self) {
        let Some(mgr) = &self.hotkey_mgr else { return };
        let keys = mgr.poll();
        for key in keys {
            // 优先检查是否处于发送模式
            if self.hotkey_sm.in_send_mode() {
                if let Some(action) = self.hotkey_sm.on_send_key(key) {
                    self.apply_action(action);
                    continue;
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
                    self.hotkey_sm.on_insert();
                    self.status = "🎯 发送模式已激活（2秒内按任意键发送）".to_string();
                }
                _ => {
                    // 其他键在非发送模式下忽略
                }
            }
        }
    }

    /// 处理托盘事件
    fn process_tray(&mut self, ctx: &egui::Context) {
        let Some(tray) = &self.tray else { return };
        for cmd in tray.poll() {
            match cmd {
                TrayCommand::Show => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    self.status = "已从托盘恢复".to_string();
                }
                TrayCommand::Quit => {
                    // 停止所有运行，标记真正退出，然后关闭
                    self.stop_all();
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
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
            self.slots[*idx].stop();
            self.slots[*idx].hwnd = None;
            self.slots[*idx].title.clear();
            self.status = format!("⚠ 窗口 {} 已关闭/失效（{}），已解除绑定", idx + 1, title);
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_hotkeys();
        self.process_tray(ctx);
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

        // 无条件定时重绘，保证热键轮询有稳定节奏（否则空闲时 egui 不重绘，
        // 热键事件会滞留在 channel 里直到下次交互才被处理，表现为“启动很慢”）。
        ctx.request_repaint_after(std::time::Duration::from_millis(30));

        self.ui_bottom_status(ctx);
        self.ui_source_panel(ctx);
        self.ui_add_scheme_window(ctx);
        self.ui_hotkey_help_window(ctx);
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
                        egui::Color32::from_rgb(255, 200, 0),
                        "🎯 发送模式激活中（2秒内按任意键）",
                    );
                    ui.separator();
                }

                // 热键选择集提示
                let sel = self.hotkey_sm.selected();
                if !sel.is_empty() {
                    let list: Vec<String> = sel.iter().map(|n| n.to_string()).collect();
                    ui.colored_label(
                        egui::Color32::YELLOW,
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
        let mut to_add: Option<usize> = None;

        egui::Window::new(format!("为窗口 {} 添加方案", slot_idx + 1))
            .collapsible(false)
            .resizable(true)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("点击脚本加入该窗口的方案集：");
                ui.separator();
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for (i, sf) in self.scripts.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let valid = sf.is_valid();
                            if valid {
                                ui.colored_label(egui::Color32::GREEN, "✓");
                            } else {
                                ui.colored_label(egui::Color32::RED, "✗");
                            }
                            ui.label(&sf.name);
                            // 仅解析成功的脚本可加入
                            if valid && ui.small_button("加入").clicked() {
                                to_add = Some(i);
                            }
                        });
                    }
                });
            });

        if let Some(script_idx) = to_add {
            let sf = &self.scripts[script_idx];
            if let Some(commands) = &sf.commands {
                let scheme = Scheme {
                    script_name: sf.name.clone(),
                    commands: commands.clone(),
                };
                if self.slots[slot_idx].add_scheme(scheme) {
                    self.status = format!("窗口 {} 已添加方案: {}", slot_idx + 1, sf.name);
                    self.save_config();
                } else {
                    self.status = format!("方案已存在: {}", sf.name);
                }
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
                if ui.button("❓ 热键说明").clicked() {
                    self.show_hotkey_help = true;
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

        frame.show(ui, |ui| {
            // 标题行
            ui.horizontal(|ui| {
                ui.strong(format!("窗口 {}", idx + 1));
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
    }
}

