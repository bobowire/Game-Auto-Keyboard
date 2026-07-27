// 统一事件总线 - 解决窗口隐藏时后台事件无法处理的架构问题
//
// 背景：本程序隐藏到托盘后窗口不可见 → Windows 不再产生 WM_PAINT →
// winit 收不到 RedrawRequested → eframe 不再调用 App::update。
// 所有"在 update 里 poll channel"的事件源都会因此停摆（热键按了没反应、
// 语音识别完了不执行）。egui 的 request_repaint 在不可见窗口上无效
// （winit 用 RDW_INTERNALPAINT，不可见窗口不会收到 WM_PAINT）。
//
// 方案 A：所有后台事件源统一走 EventSender::send()，内部在入队后立刻
// PostMessage(WM_PAINT) 唤醒主窗口，强制产生一帧 update 来消费队列。
// 新增事件源只要用 EventSender 发事件，就自动获得唤醒能力，不用each自己记得写。
//
// 主线程侧：App 持有 MainEventBus，首帧调用 set_main_hwnd() 登记窗口句柄，
// 每帧调用 poll() 取出全部事件统一分发。

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_PAINT};

// 重导出事件类型，方便使用
pub use crate::hotkey::HotkeyKey;
pub use crate::tray::TrayCommand;
pub use crate::voice::VoiceEvent;

/// 统一的主事件类型（所有后台事件源的合集）
#[derive(Debug, Clone)]
pub enum MainEvent {
    /// 托盘事件：菜单点击、图标双击
    Tray(TrayCommand),
    /// 热键事件：Ctrl+Shift+按键
    Hotkey(HotkeyKey),
    /// 语音事件：唤醒、识别结果等
    Voice(VoiceEvent),
    // 未来可扩展：Timer / Network / ...
}

/// 主窗口 HWND 的共享槽位。0 表示还没拿到（App 首帧填入）。
type HwndSlot = Arc<AtomicIsize>;

/// 事件总线：后台线程 send 事件（自动唤醒），主线程 poll 事件
///
/// # Example
///
/// ```ignore
/// let bus = MainEventBus::new();
/// let sender = bus.sender();            // 交给后台线程
/// sender.send(MainEvent::Hotkey(HotkeyKey::Digit(1)));
/// for event in bus.poll() { /* 主线程分发 */ }
/// ```
pub struct MainEventBus {
    /// 事件接收端（主线程 poll）
    rx: Receiver<MainEvent>,
    /// 事件发送端模板（克隆给各后台事件源）
    tx: Sender<MainEvent>,
    /// 主窗口 HWND（用于唤醒）
    hwnd: HwndSlot,
}

impl MainEventBus {
    /// 创建新的事件总线
    pub fn new() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded::<MainEvent>();
        Self {
            rx,
            tx,
            hwnd: Arc::new(AtomicIsize::new(0)),
        }
    }

    /// 获取事件发送端（后台线程使用，可自由克隆）
    pub fn sender(&self) -> EventSender {
        EventSender {
            tx: self.tx.clone(),
            hwnd: self.hwnd.clone(),
        }
    }

    /// 轮询所有待处理事件（主线程在 update() 中调用）
    pub fn poll(&self) -> Vec<MainEvent> {
        self.rx.try_iter().collect()
    }

    /// 记录主窗口 HWND（App 在首帧调用）
    pub fn set_main_hwnd(&self, raw: isize) {
        self.hwnd.store(raw, Ordering::Relaxed);
    }

    /// 是否已拿到主窗口句柄
    pub fn has_main_hwnd(&self) -> bool {
        self.hwnd.load(Ordering::Relaxed) != 0
    }

    /// 主线程侧主动唤醒（例如投递 Close 后需要续帧）
    pub fn wake(&self) {
        wake_main_window(&self.hwnd);
    }
}

impl Default for MainEventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// 事件发送端（后台线程持有）
///
/// 内部自动处理窗口唤醒，调用方只管 `send()`。
#[derive(Clone)]
pub struct EventSender {
    tx: Sender<MainEvent>,
    hwnd: HwndSlot,
}

impl EventSender {
    /// 发送事件并唤醒主窗口。线程安全，可在任意线程调用。
    pub fn send(&self, event: MainEvent) {
        if self.tx.send(event).is_err() {
            // 总线已销毁（程序正在退出），静默丢弃
            return;
        }
        wake_main_window(&self.hwnd);
    }

    /// 只唤醒主窗口，不发事件（用于"需要一帧 update"的场景）
    pub fn wake(&self) {
        wake_main_window(&self.hwnd);
    }
}

/// 定时唤醒器：活着期间按固定间隔唤醒主窗口，drop 即停止。
///
/// 给"必须持续跑 update 才能推进"的流程用（如唤醒词训练要连续读麦克风帧）。
/// 窗口隐藏时 egui 不会自然重绘，靠它续帧。
pub struct WakeTicker {
    /// 停止信号：drop 时断开，让线程从 recv_timeout 立刻返回（不必等满一个间隔）
    stop_tx: Sender<()>,
    handle: Option<JoinHandle<()>>,
}

impl WakeTicker {
    /// 启动定时唤醒，interval_ms 为唤醒间隔
    pub fn start(sender: EventSender, interval_ms: u64) -> Self {
        let (stop_tx, stop_rx) = crossbeam_channel::bounded::<()>(1);
        let interval = Duration::from_millis(interval_ms);
        let handle = thread::spawn(move || loop {
            match stop_rx.recv_timeout(interval) {
                // 到点：唤醒一次主窗口
                Err(RecvTimeoutError::Timeout) => sender.wake(),
                // 收到停止信号，或 WakeTicker 已被 drop
                Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
            }
        });
        Self {
            stop_tx,
            handle: Some(handle),
        }
    }
}

impl Drop for WakeTicker {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// 强制主窗口产生一次 update
fn wake_main_window(hwnd: &HwndSlot) {
    let raw = hwnd.load(Ordering::Relaxed);
    if raw == 0 {
        // 还没拿到句柄：事件已入队，等下一次 update 自然消费
        return;
    }
    unsafe {
        let _ = PostMessageW(HWND(raw as *mut _), WM_PAINT, WPARAM(0), LPARAM(0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_drains_queue() {
        let bus = MainEventBus::new();
        let sender = bus.sender();

        sender.send(MainEvent::Hotkey(HotkeyKey::Digit(1)));
        sender.send(MainEvent::Tray(TrayCommand::Show));
        assert_eq!(bus.poll().len(), 2);
        // 第二次 poll 应该为空
        assert_eq!(bus.poll().len(), 0);
    }

    #[test]
    fn sender_is_cloneable() {
        let bus = MainEventBus::new();
        let s1 = bus.sender();
        let s2 = s1.clone();

        s1.send(MainEvent::Hotkey(HotkeyKey::Digit(1)));
        s2.send(MainEvent::Hotkey(HotkeyKey::Digit(2)));
        assert_eq!(bus.poll().len(), 2);
    }

    #[test]
    fn hwnd_slot_starts_empty() {
        let bus = MainEventBus::new();
        assert!(!bus.has_main_hwnd());
        bus.set_main_hwnd(12345);
        assert!(bus.has_main_hwnd());
    }

    #[test]
    fn send_without_hwnd_still_queues() {
        // 句柄未登记时 send 不能 panic，事件仍应入队
        let bus = MainEventBus::new();
        bus.sender().send(MainEvent::Tray(TrayCommand::Quit));
        assert_eq!(bus.poll().len(), 1);
    }

    #[test]
    fn wake_ticker_drop_returns_promptly() {
        // drop 不应等满一个唤醒间隔（这里间隔 5 秒，drop 必须立刻返回）
        let bus = MainEventBus::new();
        let ticker = WakeTicker::start(bus.sender(), 5_000);
        let t0 = std::time::Instant::now();
        drop(ticker);
        assert!(t0.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn sender_outliving_bus_does_not_panic() {
        // 总线先销毁（程序退出中），后台线程仍可能 send —— 必须静默丢弃
        let sender = {
            let bus = MainEventBus::new();
            bus.sender()
        };
        sender.send(MainEvent::Tray(TrayCommand::Show));
        sender.wake();
    }
}
