// PrintWindow 截图后端
//
// 使用 PrintWindow + PW_RENDERFULLCONTENT 捕获窗口客户区，
// 对大多数后台窗口有效（包括部分 DirectX/GPU 渲染的程序）。

use crate::capture::backend::CaptureBackend;
use crate::capture::color::Bitmap;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
    GetDC, GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP, HGDIOBJ, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};

pub struct PrintWindowCapture;

impl PrintWindowCapture {
    pub fn new() -> Self {
        Self
    }
}

impl CaptureBackend for PrintWindowCapture {
    fn name(&self) -> &str {
        "PrintWindow"
    }

    fn capture(&self, hwnd: HWND) -> Result<Bitmap, String> {
        capture_impl(hwnd)
    }
}

/// PW_CLIENTONLY = 0x00000001，只捕获客户区（不含标题栏/边框），与 GetClientRect 尺寸对齐
const PW_CLIENTONLY: u32 = 0x00000001;
/// PW_RENDERFULLCONTENT = 0x00000002，可捕获 DirectComposition/部分 GPU 渲染内容
const PW_RENDERFULLCONTENT: u32 = 0x00000002;

fn capture_impl(hwnd: HWND) -> Result<Bitmap, String> {
    unsafe {
        // 1. 获取客户区尺寸
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect)
            .map_err(|e| format!("获取客户区失败: {:?}", e))?;
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width <= 0 || height <= 0 {
            return Err("窗口尺寸无效".to_string());
        }

        // 2. 准备 DC 和位图
        let window_dc = GetDC(hwnd);
        if window_dc.is_invalid() {
            return Err("GetDC 失败".to_string());
        }
        // 用 RAII 守卫保证资源释放
        let guard = DcGuard { hwnd, window_dc };

        let mem_dc = CreateCompatibleDC(window_dc);
        if mem_dc.is_invalid() {
            return Err("CreateCompatibleDC 失败".to_string());
        }
        let hbitmap = CreateCompatibleBitmap(window_dc, width, height);
        if hbitmap.is_invalid() {
            let _ = DeleteDC(mem_dc);
            return Err("CreateCompatibleBitmap 失败".to_string());
        }
        let old_obj = SelectObject(mem_dc, HGDIOBJ(hbitmap.0));

        // 3. 截图：优先 PrintWindow，失败则回退 BitBlt
        // PW_CLIENTONLY 只渲染客户区，与 GetClientRect 尺寸一致（避免标题栏偏移）
        let flags = PW_CLIENTONLY | PW_RENDERFULLCONTENT;
        let pw_ok = PrintWindow(hwnd, mem_dc, PRINT_WINDOW_FLAGS(flags)).as_bool();
        if !pw_ok {
            // 回退：直接从窗口 DC 拷贝（仅前台可见时有效）
            let _ = BitBlt(mem_dc, 0, 0, width, height, window_dc, 0, 0, SRCCOPY);
        }

        // 4. 读取像素数据（GetDIBits）
        let result = read_pixels(mem_dc, hbitmap, width, height);

        // 5. 清理 GDI 资源
        SelectObject(mem_dc, old_obj);
        let _ = DeleteObject(HGDIOBJ(hbitmap.0));
        let _ = DeleteDC(mem_dc);
        drop(guard);

        result
    }
}

/// 从内存 DC 读取位图像素（BGRA，自上而下）
unsafe fn read_pixels(
    mem_dc: windows::Win32::Graphics::Gdi::HDC,
    hbitmap: HBITMAP,
    width: i32,
    height: i32,
) -> Result<Bitmap, String> {
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // 负高度 = 自上而下（top-down），像素行顺序与坐标一致
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let scan_lines = GetDIBits(
        mem_dc,
        hbitmap,
        0,
        height as u32,
        Some(pixels.as_mut_ptr() as *mut _),
        &mut info,
        DIB_RGB_COLORS,
    );

    if scan_lines == 0 {
        return Err("GetDIBits 读取像素失败".to_string());
    }

    Ok(Bitmap::new(width, height, pixels))
}

/// GetDC 的 RAII 守卫，析构时 ReleaseDC
struct DcGuard {
    hwnd: HWND,
    window_dc: windows::Win32::Graphics::Gdi::HDC,
}

impl Drop for DcGuard {
    fn drop(&mut self) {
        unsafe {
            ReleaseDC(self.hwnd, self.window_dc);
        }
    }
}
