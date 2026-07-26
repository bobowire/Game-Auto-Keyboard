// 语音调试日志 - 同时输出到 stderr 和工作目录下的 voice_debug.log
//
// GUI 程序(release)没有控制台，用文件日志方便用户抓取排查。

use std::fs::OpenOptions;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

const LOG_FILE: &str = "voice_debug.log";

/// 写一行日志（带毫秒时间戳），同时打到 stderr
pub fn log(msg: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let line = format!("[{}] {}", ts, msg);
    eprintln!("{}", line);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(LOG_FILE) {
        let _ = writeln!(f, "{}", line);
    }
}

/// 宏：像 format! 一样用
#[macro_export]
macro_rules! vlog {
    ($($arg:tt)*) => {
        $crate::voice::vlog::log(&format!($($arg)*))
    };
}
