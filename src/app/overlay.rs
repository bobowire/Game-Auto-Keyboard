// 鼠标事件转发覆盖窗：开启 / 关闭 / 事件处理。
//
// overlay 被 events / slots / grab / ui 四组共同操作（start_overlay / stop_overlay
// 被调用 18 处），故独立成模块；若并入 voice_ctrl 等组，会让 slots、grab 反向依赖它，
// 形成循环依赖风险。

use super::App;
use crate::config::ForwardConfig;
use crate::overlay::{OverlayEvent, OverlayWindow};
use crate::utils::win32;

impl App {
    /// 开启鼠标转发：前置校验通过后启动覆盖窗线程
    pub(super) fn start_overlay(&mut self) {
        if self.overlay.is_some() {
            return;
        }
        let Some(idx) = self.main_slot_index() else {
            self.status = "⚑ 请先标记主窗口（点槽位序号左侧的旗帜）".to_string();
            return;
        };
        let Some(anchor_raw) = self.slots[idx].hwnd else {
            self.status = "⚠ 主窗口尚未绑定，请先抓取窗口".to_string();
            return;
        };
        if !win32::is_valid(windows::Win32::Foundation::HWND(anchor_raw as *mut _)) {
            self.status = "⚠ 主窗口句柄已失效，请重新抓取窗口".to_string();
            return;
        }
        // 收集所有已绑定且有效的目标窗口（含主窗口），鼠标消息广播给它们
        let targets: Vec<isize> = self
            .slots
            .iter()
            .filter_map(|s| s.hwnd)
            .filter(|&h| win32::is_valid(windows::Win32::Foundation::HWND(h as *mut _)))
            .collect();
        let n = targets.len();
        let cfg = ForwardConfig {
            rbutton_broadcast_move: self.forward_rbutton_move,
            keyboard_broadcast: self.forward_keyboard,
            keyboard_marked_only: self.forward_marked_only,
        };
        match OverlayWindow::start(anchor_raw, targets, cfg, self.events.sender()) {
            Ok(o) => {
                self.overlay = Some(o);
                self.status = format!(
                    "🖱 鼠标转发已开启：广播给 {} 个绑定窗口（覆盖窗跟随主窗口）；Ctrl+Q 关闭",
                    n
                );
            }
            Err(e) => self.status = format!("🖱 鼠标转发启动失败: {}", e),
        }
    }

    /// 关闭鼠标转发
    pub(super) fn stop_overlay(&mut self) {
        if let Some(mut o) = self.overlay.take() {
            o.stop();
            self.status = "🖱 鼠标转发已关闭".to_string();
        }
    }

    /// 处理覆盖窗回报事件
    pub(super) fn handle_overlay_event(&mut self, ev: OverlayEvent) {
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
}
