// 截图后端抽象（策略模式，便于未来切换 BitBlt / PrintWindow / WGC）

use crate::capture::color::Bitmap;
use windows::Win32::Foundation::HWND;

pub trait CaptureBackend: Send + Sync {
    /// 后端名称
    fn name(&self) -> &str;

    /// 截取窗口客户区，返回位图（BGRA）
    fn capture(&self, hwnd: HWND) -> Result<Bitmap, String>;
}
