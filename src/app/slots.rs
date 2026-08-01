// 槽位 / 窗口执行：脚本 Runner 的启停与批量调度。
//
// slots 是被多方依赖的叶子层：voice_ctrl / events / ui 都调用这里的启停方法。
// 它不反向依赖任何上层业务模块（check_window_validity 调 overlay.stop_overlay 除外，
// 那是 app 内 pub(super) 调用）。

use super::{App, SLOT_COUNT};
use std::time::Instant;

use crate::runner::Runner;
use crate::utils::win32;

impl App {
    /// 循环启动某槽位标识方案
    pub(super) fn start_slot(&mut self, idx: usize) -> bool {
        self.run_slot(idx, false, 0)
    }

    /// 单次执行某槽位标识方案（预留给 UI 单次执行按钮）
    #[allow(dead_code)]
    pub(super) fn run_slot_once(&mut self, idx: usize) -> bool {
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

    pub(super) fn stop_slot(&mut self, idx: usize) {
        self.slots[idx].stop();
    }

    pub(super) fn start_windows(&mut self, windows: &[u8]) {
        let mut started = 0;
        for &w in windows {
            let idx = (w - 1) as usize;
            if idx < SLOT_COUNT && self.start_slot_hotkey(idx) {
                started += 1;
            }
        }
        self.status = format!("热键：启动了 {} 个窗口（1秒后开始执行）", started);
    }

    pub(super) fn stop_windows(&mut self, windows: &[u8]) {
        for &w in windows {
            let idx = (w - 1) as usize;
            if idx < SLOT_COUNT {
                self.stop_slot(idx);
            }
        }
        self.status = format!("热键：停止了指定窗口");
    }

    pub(super) fn start_all(&mut self) {
        let mut started = 0;
        for idx in 0..SLOT_COUNT {
            if self.slots[idx].is_bound() && self.start_slot_hotkey(idx) {
                started += 1;
            }
        }
        self.status = format!("热键：启动全部，共 {} 个窗口（1秒后开始执行）", started);
    }

    pub(super) fn stop_all(&mut self) {
        for idx in 0..SLOT_COUNT {
            self.stop_slot(idx);
        }
        self.status = "热键：停止全部".to_string();
    }

    pub(super) fn run_once_windows(&mut self, windows: &[u8]) {
        let mut n = 0;
        for &w in windows {
            let idx = (w - 1) as usize;
            if idx < SLOT_COUNT && self.run_slot_once_hotkey(idx) {
                n += 1;
            }
        }
        self.status = format!("热键：单次执行了 {} 个窗口（1秒后开始）", n);
    }

    pub(super) fn run_once_all(&mut self) {
        let mut n = 0;
        for idx in 0..SLOT_COUNT {
            if self.slots[idx].is_bound() && self.run_slot_once_hotkey(idx) {
                n += 1;
            }
        }
        self.status = format!("热键：单次执行全部，共 {} 个窗口（1秒后开始）", n);
    }

    /// 定期检查已绑定窗口是否仍有效，失效则停止运行、清除绑定并提示
    pub(super) fn check_window_validity(&mut self) {
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
