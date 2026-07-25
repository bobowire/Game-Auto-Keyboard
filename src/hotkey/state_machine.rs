// 热键状态机
//
// 交互规则：
// - 按 Ctrl+Shift+[1-8]：将对应窗口加入"选择集"（前缀选择），并刷新超时计时
// - 按 Ctrl+Shift+9：
//     若选择集非空 -> 启动选择集中各窗口的标识方案，然后清空选择集
//     若选择集为空 -> 启动所有窗口的标识方案
// - 按 Ctrl+Shift+0：
//     若选择集非空 -> 停止选择集中各窗口，然后清空选择集
//     若选择集为空 -> 停止所有窗口
// - 选择后若超过 timeout 未按 9/0，则选择集自动清空

use std::time::{Duration, Instant};

use super::manager::{HotkeyKey, SpecialKey};

/// 状态机对外产生的动作
#[derive(Debug, Clone, PartialEq)]
pub enum HotkeyAction {
    /// 循环启动指定窗口（1-8）的标识方案
    StartWindows(Vec<u8>),
    /// 停止指定窗口（1-8）
    StopWindows(Vec<u8>),
    /// 循环启动所有已绑定窗口
    StartAll,
    /// 停止所有窗口
    StopAll,
    /// 单次执行指定窗口（1-8）的标识方案
    RunOnceWindows(Vec<u8>),
    /// 单次执行所有已绑定窗口的标识方案
    RunOnceAll,
    /// 向指定窗口发送单个按键
    SendKey { windows: Vec<u8>, key_name: String },
}

pub struct HotkeyStateMachine {
    /// 当前选择集（窗口编号 1-8）
    selected: Vec<u8>,
    /// 上次选择时间，用于超时清空
    last_select: Option<Instant>,
    /// 选择超时时长
    timeout: Duration,
    /// 发送模式：按下 Insert 后进入，2秒内收到按键就发送
    send_mode_since: Option<Instant>,
}

impl HotkeyStateMachine {
    pub fn new() -> Self {
        Self {
            selected: Vec::new(),
            last_select: None,
            timeout: Duration::from_secs(3),
            send_mode_since: None,
        }
    }

    #[cfg(test)]
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            selected: Vec::new(),
            last_select: None,
            timeout,
            send_mode_since: None,
        }
    }

    /// 当前选择集快照（用于 UI 高亮显示）
    pub fn selected(&self) -> &[u8] {
        &self.selected
    }

    /// 检查是否超时，超时则清空选择集
    fn expire_if_timeout(&mut self) {
        if let Some(t) = self.last_select {
            if t.elapsed() > self.timeout {
                self.selected.clear();
                self.last_select = None;
            }
        }
    }

    /// 按下窗口选择键 [1-8]
    /// 返回值：首次按下加入选择集返回 None，重复按下则停止该窗口并返回 StopWindows
    pub fn on_select(&mut self, window: u8) -> Option<HotkeyAction> {
        self.expire_if_timeout();
        if !(1..=8).contains(&window) {
            return None;
        }

        // 检查是否已在选择集中
        if let Some(pos) = self.selected.iter().position(|&w| w == window) {
            // 重复按下，停止该窗口并从选择集移除
            self.selected.remove(pos);
            self.last_select = if self.selected.is_empty() {
                None
            } else {
                Some(Instant::now())
            };
            return Some(HotkeyAction::StopWindows(vec![window]));
        }

        // 首次按下，加入选择集
        self.selected.push(window);
        self.last_select = Some(Instant::now());
        None
    }

    /// 按下启动键（9）
    pub fn on_start(&mut self) -> HotkeyAction {
        self.expire_if_timeout();
        if self.selected.is_empty() {
            HotkeyAction::StartAll
        } else {
            let mut windows = std::mem::take(&mut self.selected);
            windows.sort_unstable();
            self.last_select = None;
            HotkeyAction::StartWindows(windows)
        }
    }

    /// 按下停止键（0）
    pub fn on_stop(&mut self) -> HotkeyAction {
        self.expire_if_timeout();
        if self.selected.is_empty() {
            HotkeyAction::StopAll
        } else {
            let mut windows = std::mem::take(&mut self.selected);
            windows.sort_unstable();
            self.last_select = None;
            HotkeyAction::StopWindows(windows)
        }
    }

    /// 按下单次执行键（-）
    pub fn on_run_once(&mut self) -> HotkeyAction {
        self.expire_if_timeout();
        if self.selected.is_empty() {
            HotkeyAction::RunOnceAll
        } else {
            let mut windows = std::mem::take(&mut self.selected);
            windows.sort_unstable();
            self.last_select = None;
            HotkeyAction::RunOnceWindows(windows)
        }
    }

    /// 按下 Insert（进入发送模式）
    pub fn on_insert(&mut self) {
        self.expire_if_timeout();
        self.send_mode_since = Some(Instant::now());
    }

    /// 在发送模式中按下了某个键，返回 SendKey action
    pub fn on_send_key(&mut self, key: HotkeyKey) -> Option<HotkeyAction> {
        // 检查是否在发送模式且未超时（2 秒）
        let Some(since) = self.send_mode_since else {
            return None;
        };
        if since.elapsed() > Duration::from_secs(2) {
            self.send_mode_since = None;
            return None;
        }

        self.send_mode_since = None;
        let key_name = key_to_name(key)?;
        let mut windows = std::mem::take(&mut self.selected);
        windows.sort_unstable();
        self.last_select = None;

        Some(HotkeyAction::SendKey { windows, key_name })
    }

    /// 是否处于发送模式
    pub fn in_send_mode(&self) -> bool {
        self.send_mode_since
            .map_or(false, |t| t.elapsed() < Duration::from_secs(2))
    }
}

/// 将 HotkeyKey 转换为按键名（供 InputBackend 使用）
fn key_to_name(key: HotkeyKey) -> Option<String> {
    match key {
        HotkeyKey::Digit(d) => Some(d.to_string()),
        HotkeyKey::Letter(c) => Some(c.to_string()),
        HotkeyKey::FKey(f) => Some(format!("F{}", f)),
        HotkeyKey::Special(s) => Some(match s {
            SpecialKey::Space => "Space".to_string(),
            SpecialKey::Enter => "Enter".to_string(),
            SpecialKey::Tab => "Tab".to_string(),
            SpecialKey::Escape => "Escape".to_string(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_all_when_no_selection() {
        let mut sm = HotkeyStateMachine::new();
        assert_eq!(sm.on_start(), HotkeyAction::StartAll);
    }

    #[test]
    fn stop_all_when_no_selection() {
        let mut sm = HotkeyStateMachine::new();
        assert_eq!(sm.on_stop(), HotkeyAction::StopAll);
    }

    #[test]
    fn select_then_start() {
        let mut sm = HotkeyStateMachine::new();
        sm.on_select(1);
        sm.on_select(3);
        assert_eq!(sm.on_start(), HotkeyAction::StartWindows(vec![1, 3]));
        // 触发后选择集应清空
        assert!(sm.selected().is_empty());
    }

    #[test]
    fn select_then_stop() {
        let mut sm = HotkeyStateMachine::new();
        sm.on_select(5);
        sm.on_select(2);
        assert_eq!(sm.on_stop(), HotkeyAction::StopWindows(vec![2, 5]));
    }

    #[test]
    fn duplicate_select_ignored() {
        let mut sm = HotkeyStateMachine::new();
        sm.on_select(1);
        sm.on_select(1);
        assert_eq!(sm.on_start(), HotkeyAction::StartWindows(vec![1]));
    }

    #[test]
    fn selection_expires() {
        let mut sm = HotkeyStateMachine::with_timeout(Duration::from_millis(50));
        sm.on_select(1);
        std::thread::sleep(Duration::from_millis(80));
        // 超时后再按 start，选择集已清空 -> StartAll
        assert_eq!(sm.on_start(), HotkeyAction::StartAll);
    }

    #[test]
    fn out_of_range_ignored() {
        let mut sm = HotkeyStateMachine::new();
        sm.on_select(9);
        sm.on_select(0);
        assert_eq!(sm.on_start(), HotkeyAction::StartAll);
    }
}
