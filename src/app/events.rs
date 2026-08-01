// 事件分发枢纽：从统一总线取出后台事件并翻译成业务调用。
//
// 依赖 slots / voice_ctrl / overlay 三个方向，是后台事件源与业务方法之间的中转层。
// dispatch / capture_main_hwnd / pump_pending_wake 由 update() 每帧调用，故 pub(super)；
// handle_hotkey / handle_tray / apply_action / request_wake 仅本模块内部调用。

use super::{App, SLOT_COUNT, play_sound};
use eframe::egui;

use crate::event_bus::MainEvent;
use crate::hotkey::{HotkeyAction, HotkeyKey};
use crate::tray::TrayCommand;

impl App {
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
            // 语音开关切换（Ctrl+Shift+F1）：开=成功音，关=失败音（开启失败也播失败音）
            HotkeyKey::FKey(1) => {
                if self.voice.is_some() {
                    self.stop_voice();
                    play_sound("beep_fail.wav");
                } else {
                    self.start_voice();
                    if self.voice.is_some() {
                        play_sound("beep_success.wav");
                    } else {
                        play_sound("beep_fail.wav");
                    }
                }
            }
            // 消息转发开关切换（Ctrl+Shift+F2）：同上
            HotkeyKey::FKey(2) => {
                if self.overlay.is_some() {
                    self.stop_overlay();
                    play_sound("beep_fail.wav");
                } else {
                    self.start_overlay();
                    if self.overlay.is_some() {
                        play_sound("beep_success.wav");
                    } else {
                        play_sound("beep_fail.wav");
                    }
                }
            }
            _ => {
                // 其他键在非发送模式下忽略
            }
        }
    }

    /// 把主窗口 HWND 交给事件总线（只需成功一次）。
    /// 各后台事件源靠它在窗口隐藏时唤醒 update。
    pub(super) fn capture_main_hwnd(&mut self, frame: &eframe::Frame) {
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
    pub(super) fn pump_pending_wake(&mut self) {
        if self.wake_pending == 0 {
            return;
        }
        self.wake_pending -= 1;
        self.events.wake();
    }

    /// 从总线取出全部后台事件并分发（每帧一次）
    pub(super) fn dispatch_events(&mut self, ctx: &egui::Context) {
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
}
