// 脚本运行器 - 在后台线程执行脚本，支持启动/停止
// 执行是循环的：脚本跑完一轮后自动重新开始，直到收到停止信号

use crate::input::PostMessageBackend;
use crate::script::{Command, ScriptExecutor};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub struct Runner {
    stop_flag: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Runner {
    /// 循环执行（跑完一轮自动重来，直到 stop）
    pub fn start(hwnd_raw: isize, commands: Vec<Command>) -> Self {
        Self::spawn(hwnd_raw, commands, false, 0)
    }

    /// 单次执行（跑完一轮即结束）
    pub fn start_once(hwnd_raw: isize, commands: Vec<Command>) -> Self {
        Self::spawn(hwnd_raw, commands, true, 0)
    }

    /// 循环执行，但延迟指定毫秒后再开始（用于热键触发，给用户时间松开修饰键）
    pub fn start_delayed(hwnd_raw: isize, commands: Vec<Command>, delay_ms: u64) -> Self {
        Self::spawn(hwnd_raw, commands, false, delay_ms)
    }

    /// 单次执行，延迟后开始
    pub fn start_once_delayed(hwnd_raw: isize, commands: Vec<Command>, delay_ms: u64) -> Self {
        Self::spawn(hwnd_raw, commands, true, delay_ms)
    }

    /// hwnd 以 isize 传入，避免 HWND 的 Send 问题
    fn spawn(hwnd_raw: isize, commands: Vec<Command>, once: bool, initial_delay_ms: u64) -> Self {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();

        let handle = thread::spawn(move || {
            // 初始延迟（热键触发时给用户时间松开修饰键）
            if initial_delay_ms > 0 {
                thread::sleep(Duration::from_millis(initial_delay_ms));
            }

            let backend = PostMessageBackend::new();
            let hwnd = windows::Win32::Foundation::HWND(hwnd_raw as *mut _);

            loop {
                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }
                let executor = ScriptExecutor::new(&backend, hwnd);
                if let Err(e) = executor.execute_interruptible(&commands, &stop_clone) {
                    eprintln!("脚本执行出错: {}", e);
                    break;
                }
                // 单次模式或空脚本：执行完一轮即退出
                if once || commands.is_empty() {
                    break;
                }
            }
        });

        Self {
            stop_flag,
            handle: Some(handle),
        }
    }

    /// 是否仍在运行
    pub fn is_running(&self) -> bool {
        !self.stop_flag.load(Ordering::Relaxed)
            && self.handle.as_ref().map_or(false, |h| !h.is_finished())
    }

    /// 请求停止（非阻塞）
    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }

    /// 停止并等待线程结束
    pub fn stop_and_join(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}
