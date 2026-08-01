// 抓取窗口与取色：倒计时结束后截取前台窗口。
//
// 仅依赖 overlay（重抓主窗口时切换转发目标）和 win32，是最轻量的业务模块之一。

use super::{App, GRAB_COUNTDOWN_SECS};
use eframe::egui;

use crate::utils::win32;

impl App {
    /// 处理抓取窗口倒计时
    pub(super) fn handle_grabbing(&mut self, ctx: &egui::Context) {
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
                } else {
                    let title = win32::window_title(hwnd);
                    self.slots[slot_idx].hwnd = Some(hwnd.0 as isize);
                    self.slots[slot_idx].title = if title.is_empty() {
                        format!("<无标题> ({:?})", hwnd.0)
                    } else {
                        title
                    };
                    self.status =
                        format!("窗口 {} 已抓取: {}", slot_idx + 1, self.slots[slot_idx].title);
                    // 重抓的槽是主窗口且转发在跑：换目标（停旧起新）
                    if self.slots[slot_idx].is_main && self.overlay.is_some() {
                        self.stop_overlay();
                        self.start_overlay();
                    }
                }
            } else {
                self.status = "抓取失败：未找到前台窗口".to_string();
            }
            // 抓取完成：把我们自己的窗口激活、弹到最前面，给用户反馈
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    /// 处理取色倒计时：结束后截取前台窗口并打开取色器
    pub(super) fn handle_picking(&mut self) {
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
}
