// 鼠标事件转发覆盖窗
//
// 一个独立的 Win32 layered 窗口，精确覆盖主窗口（锚点）的客户区：
// - 整体 50% 半透明 + 居中提示文字"鼠标事件转发模式"
// - 50ms 定时器跟随主窗口移动/缩放；最小化/隐藏时隐藏，恢复后自动回来
// - 鼠标消息广播给所有已绑定的目标窗口（多开同步操作）；主窗口失效时自毁
//   并经事件总线回报 OverlayEvent::TargetLost
//
// 焦点模型（刻意不设 WS_EX_NOACTIVATE）：
// - 转发模式下覆盖窗理应持有焦点——点击覆盖窗即获焦，焦点跟着用户的点击走
// - 滚轮消息（WM_MOUSEWHEEL）按 Windows 规则只送给焦点窗口：覆盖窗获焦后
//   滚轮直接进入本窗口消息循环，原样转发即可，无需安装任何全局钩子
// - 键盘消息同理送达本窗口，现阶段忽略，后续可扩展为转发给目标窗口
//
// 线程模型（照抄 hotkey/manager.rs 先例）：
// - 覆盖窗活在独立线程的原生消息循环里（窗口必须在创建它的线程销毁）
// - start() 用 bounded channel 同步等待创建结果 + Win32 线程 id
// - stop() = PostThreadMessageW(WM_QUIT) + join；幂等，Drop 兜底

use crossbeam_channel::bounded;
use std::thread::{self, JoinHandle};

use windows::core::w;
use windows::Win32::Foundation::{
    GetLastError, COLORREF, ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, ClientToScreen, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint,
    FillRect, SelectObject, SetBkMode, SetTextColor, DT_CENTER, DT_SINGLELINE, DT_VCENTER,
    HGDIOBJ, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetCapture, GetKeyState, ReleaseCapture, SetCapture, VK_CONTROL, VK_Q,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetWindowLongPtrW, IsIconic, IsWindow, IsWindowVisible, LoadCursorW, PeekMessageW,
    PostMessageW, PostQuitMessage, PostThreadMessageW, RegisterClassExW,
    HCURSOR, SetCursor, SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HWND_TOPMOST, IDC_ARROW,
    LWA_ALPHA, MSG, PM_NOREMOVE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WM_ACTIVATE, WM_CLOSE,
    WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_PAINT, WM_QUIT, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_TIMER,
    WM_XBUTTONDBLCLK, WM_XBUTTONDOWN, WM_XBUTTONUP, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

use crate::event_bus::{EventSender, MainEvent};
use crate::input::{InputBackend, PostMessageBackend};

/// 覆盖窗窗口类名（进程级，重复注册按成功处理）
const OVERLAY_CLASS: windows::core::PCWSTR = w!("GAK_MouseOverlay");
/// 跟随定时器 id
const FOLLOW_TIMER_ID: usize = 1;
/// 跟随间隔（毫秒）
const FOLLOW_INTERVAL_MS: u32 = 50;
/// 右键拖视角时，落在按下点（游戏 SetCursorPos 回拉目标）此范围内的移动视为回拉，不转发
const RBUTTON_SKIP_TOLERANCE: i32 = 3;

/// 覆盖窗回报给 UI 的事件
#[derive(Debug, Clone)]
pub enum OverlayEvent {
    /// 目标窗口已失效（关闭等）：覆盖窗已自毁，线程正在退出。UI 据此复位开关
    TargetLost,
    /// 用户在覆盖窗上按了 Ctrl+Q：请求 UI 关闭转发（覆盖窗仍活着，等 UI 侧 stop）
    CloseRequested,
}

/// 覆盖窗句柄（App 持有，控制启停）
pub struct OverlayWindow {
    /// 覆盖窗线程的 Win32 线程 id（PostThreadMessageW 发 WM_QUIT 用）
    thread_id: u32,
    handle: Option<JoinHandle<()>>,
}

impl OverlayWindow {
    /// 启动覆盖窗线程并同步等待窗口创建完成。
    ///
    /// `anchor_raw`：主窗口句柄（isize），覆盖窗跟随它定位/显隐
    /// `targets_raw`：所有需接收鼠标消息的目标窗口句柄（isize，含主窗口）
    /// `events`：事件总线发送端（用于回报 TargetLost）
    pub fn start(
        anchor_raw: isize,
        targets_raw: Vec<isize>,
        events: EventSender,
    ) -> Result<Self, String> {
        let (ok_tx, ok_rx) = bounded::<Result<(), String>>(1);
        let (id_tx, id_rx) = bounded::<u32>(1);
        let handle = thread::Builder::new()
            .name("mouse-overlay".to_string())
            .spawn(move || run_loop(anchor_raw, targets_raw, events, ok_tx, id_tx))
            .map_err(|e| format!("启动覆盖窗线程失败: {}", e))?;

        // 线程保证在成功路径或失败路径都先报结果；id 在结果之前发送（两端都有缓冲）
        match ok_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let _ = handle.join();
                return Err(e);
            }
            Err(_) => {
                let _ = handle.join();
                return Err("覆盖窗线程异常退出".to_string());
            }
        }
        let thread_id = id_rx.recv().unwrap_or(0);
        Ok(Self {
            thread_id,
            handle: Some(handle),
        })
    }

    /// 停止：PostThreadMessageW(WM_QUIT) 唤醒消息循环 + join。幂等，可重复调用
    pub fn stop(&mut self) {
        if let Some(h) = self.handle.take() {
            unsafe {
                // 线程已退出时该调用返回错误，静默忽略
                let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
            let _ = h.join();
        }
    }
}

impl Drop for OverlayWindow {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 覆盖窗线程主体
fn run_loop(
    anchor_raw: isize,
    targets_raw: Vec<isize>,
    events: EventSender,
    ok_tx: crossbeam_channel::Sender<Result<(), String>>,
    id_tx: crossbeam_channel::Sender<u32>,
) {
    unsafe {
        // 1. 注册窗口类（已存在按成功处理，不做 Unregister——类是进程级共享的）
        let hinstance = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(e) => {
                let _ = ok_tx.send(Err(format!("GetModuleHandle 失败: {:?}", e)));
                return;
            }
        };
        let cursor = match LoadCursorW(None, IDC_ARROW) {
            Ok(c) => c,
            Err(e) => {
                let _ = ok_tx.send(Err(format!("LoadCursor 失败: {:?}", e)));
                return;
            }
        };
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS | CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(overlay_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance.into(),
            hIcon: Default::default(),
            hCursor: cursor,
            hbrBackground: Default::default(),
            lpszMenuName: windows::core::PCWSTR::null(),
            lpszClassName: OVERLAY_CLASS,
            hIconSm: Default::default(),
        };
        // 返回 0 表示失败；类已存在（重启开关场景）按成功处理，不做 Unregister
        if RegisterClassExW(&wc) == 0 {
            let err = GetLastError();
            if err != ERROR_CLASS_ALREADY_EXISTS {
                let _ = ok_tx.send(Err(format!("注册覆盖窗类失败: {:?}", err)));
                return;
            }
        }

        // 2. 创建窗口（隐藏创建，位置由首个 tick 确定，避免在 (0,0) 闪现）
        //    刻意不带 WS_EX_NOACTIVATE：覆盖窗需要接受激活/焦点，滚轮才能进本窗口消息循环
        let hwnd = match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            OVERLAY_CLASS,
            w!("鼠标转发覆盖窗"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            hinstance,
            None,
        ) {
            Ok(h) => h,
            Err(e) => {
                let _ = ok_tx.send(Err(format!("创建覆盖窗失败: {:?}", e)));
                return;
            }
        };

        // 失败路径统一走这里清理
        macro_rules! fail_cleanup {
            ($msg:expr) => {{
                let _ = DestroyWindow(hwnd);
                let _ = ok_tx.send(Err($msg));
                return;
            }};
        }

        // 3. 整体 50% 半透明
        if let Err(e) = SetLayeredWindowAttributes(hwnd, COLORREF(0), 128, LWA_ALPHA) {
            fail_cleanup!(format!("SetLayeredWindowAttributes 失败: {:?}", e));
        }

        // 4. 窗口状态存入 GWLP_USERDATA（此刻消息循环未跑，不会有派发竞争）
        let state = Box::new(WndState {
            target: HWND(anchor_raw as *mut _),
            targets: targets_raw.iter().map(|&h| HWND(h as *mut _)).collect(),
            events: events.clone(),
            last_rect: RECT::default(),
            shown: false,
            rbutton_anchor: None,
            arrow_cursor: cursor,
        });
        let state_ptr = Box::into_raw(state);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);

        // 5. 跟随定时器（返回 0 表示失败）
        if SetTimer(hwnd, FOLLOW_TIMER_ID, FOLLOW_INTERVAL_MS, None) == 0 {
            let err = GetLastError();
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            let _ = Box::from_raw(state_ptr);
            fail_cleanup!(format!("SetTimer 失败: {:?}", err));
        }

        // 6. 强制创建消息队列（PeekMessage PM_NOREMOVE），之后 PostThreadMessageW 才有效
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);

        // 7. 报告就绪（先发线程 id 再发成功，start 端先收成功再收 id）
        let _ = id_tx.send(GetCurrentThreadId());
        let _ = ok_tx.send(Ok(()));

        // 8. 消息循环（无键盘消息，免 TranslateMessage）
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = DispatchMessageW(&msg);
        }

        // 9. 收尾（唯一释放点；窗口可能已在 tick 里自毁，用 IsWindow 判断幂等）
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        if IsWindow(hwnd).as_bool() {
            let _ = DestroyWindow(hwnd);
        }
        let _ = Box::from_raw(state_ptr);
    }
}

/// 窗口过程内部状态（堆上，经 GWLP_USERDATA 存取，全程不出覆盖窗线程）
struct WndState {
    /// 主窗口（锚点）：覆盖窗跟随它定位/显隐、失效时自毁
    target: HWND,
    /// 所有转发目标（含主窗口）：鼠标消息广播给这些窗口
    targets: Vec<HWND>,
    events: EventSender,
    last_rect: RECT,
    shown: bool,
    /// 右键拖视角期间的按下点（游戏 SetCursorPos 回拉目标）；非拖动期为 None
    rbutton_anchor: Option<POINT>,
    /// 默认箭头光标（拖动结束恢复用）
    arrow_cursor: HCURSOR,
}

unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WndState;
    // USERDATA 设置前到达的消息（极少）交给默认过程
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    match msg {
        WM_TIMER => {
            follow_tick(hwnd, &mut *ptr);
            LRESULT(0)
        }
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        // 背景由 WM_PAINT 整块填充，吞掉防闪烁
        WM_ERASEBKGND => LRESULT(1),
        WM_DESTROY => {
            // 让 GetMessageW 循环退出（事件回报在 tick 自毁处完成）
            PostQuitMessage(0);
            LRESULT(0)
        }
        // ===== 鼠标消息转发 =====
        // wParam/lParam 原样转发：OS 派发时 wParam 已带 MK_* 位；
        // WS_POPUP 无边框，客户区 == 窗口区，lParam 即目标客户区坐标
        WM_MOUSEMOVE => {
            // 右键拖视角期间：落在按下点（游戏 SetCursorPos 回拉目标）附近的移动
            // 视为游戏的回拉而非用户操作，不转发，避免反馈环
            let skip = match (&*ptr).rbutton_anchor {
                Some(a) => {
                    let p = point_from_lparam(lparam);
                    (p.x - a.x).abs() <= RBUTTON_SKIP_TOLERANCE
                        && (p.y - a.y).abs() <= RBUTTON_SKIP_TOLERANCE
                }
                None => false,
            };
            if !skip {
                forward(&*ptr, msg, wparam, lparam);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN => {
            // 捕获鼠标：拖拽时光标移出覆盖窗边界也不断流
            let _ = SetCapture(hwnd);
            if msg == WM_RBUTTONDOWN {
                // 进入右键拖视角：记下按下点（游戏回拉目标），隐藏光标
                let st = &mut *ptr;
                st.rbutton_anchor = Some(point_from_lparam(lparam));
                SetCursor(HCURSOR(std::ptr::null_mut()));
            }
            forward(&*ptr, msg, wparam, lparam);
            // XBUTTON 消息要求返回 TRUE，其余返回 0
            LRESULT(if msg == WM_XBUTTONDOWN { 1 } else { 0 })
        }
        WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP | WM_XBUTTONUP => {
            if GetCapture() == hwnd {
                let _ = ReleaseCapture();
            }
            if msg == WM_RBUTTONUP {
                // 退出右键拖视角：恢复光标
                let st = &mut *ptr;
                st.rbutton_anchor = None;
                SetCursor(st.arrow_cursor);
            }
            forward(&*ptr, msg, wparam, lparam);
            LRESULT(if msg == WM_XBUTTONUP { 1 } else { 0 })
        }
        WM_LBUTTONDBLCLK | WM_RBUTTONDBLCLK | WM_MBUTTONDBLCLK | WM_XBUTTONDBLCLK => {
            forward(&*ptr, msg, wparam, lparam);
            LRESULT(0)
        }
        // 滚轮：焦点在覆盖窗时由 OS 直接送进本消息循环，原样转发
        // （wParam 的 MK_* 位与 delta、lParam 屏幕坐标都由 OS 组好）
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            forward(&*ptr, msg, wparam, lparam);
            LRESULT(0)
        }
        // 吞掉 WM_CLOSE：覆盖窗持有焦点时 Alt+F4 不应销毁它（只能由开关/主窗口关闭来停）
        WM_CLOSE => LRESULT(0),
        // 拖视角期间持续隐藏光标（否则 DefWindowProc 会用类光标恢复箭头）
        WM_SETCURSOR => {
            if (&*ptr).rbutton_anchor.is_some() {
                SetCursor(HCURSOR(std::ptr::null_mut()));
                LRESULT(1) // 已处理，抑制默认
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        // 覆盖窗被激活（用户点击）→ 给所有目标窗口补发激活消息：很多游戏只在
        // 内部"激活态"为真时才接受输入，复用脚本执行器的同款方法
        WM_ACTIVATE => {
            // wParam 低字：0 = WA_INACTIVE（失活），非 0 = 激活（WA_ACTIVE/WA_CLICKACTIVE）
            if wparam.0 & 0xFFFF != 0 {
                let backend = PostMessageBackend::new();
                for &t in &(&*ptr).targets {
                    let _ = backend.send_window_active(t);
                }
            } else {
                // 失活时清理右键拖动状态，避免光标卡在隐藏
                let st = &mut *ptr;
                if st.rbutton_anchor.take().is_some() {
                    SetCursor(st.arrow_cursor);
                }
            }
            LRESULT(0)
        }
        // Ctrl+Q 关闭转发（Ctrl 是修饰键，用 GetKeyState 取当前按下态）；bit30 排除长按重键
        WM_KEYDOWN
            if wparam.0 as u32 == VK_Q.0 as u32
                && GetKeyState(VK_CONTROL.0 as i32) < 0
                && lparam.0 & (1 << 30) == 0 =>
        {
            (&*ptr)
                .events
                .send(MainEvent::Overlay(OverlayEvent::CloseRequested));
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// 跟随定时器：同步覆盖窗到目标窗口客户区屏幕矩形
unsafe fn follow_tick(hwnd: HWND, st: &mut WndState) {
    // 目标失效 → 上报 + 自毁（销毁发生在创建线程，合法）
    if !IsWindow(st.target).as_bool() {
        st.events
            .send(MainEvent::Overlay(OverlayEvent::TargetLost));
        let _ = DestroyWindow(hwnd);
        return;
    }

    // 最小化/隐藏 → 隐藏覆盖窗（恢复后自动回来）
    if IsIconic(st.target).as_bool() || !IsWindowVisible(st.target).as_bool() {
        if st.shown {
            let _ = ShowWindow(hwnd, SW_HIDE);
            st.shown = false;
        }
        return;
    }

    // 计算目标客户区的屏幕矩形（天然排除边框/标题栏/菜单栏）
    let mut client = RECT::default();
    if GetClientRect(st.target, &mut client).is_err() {
        return;
    }
    let mut tl = POINT {
        x: client.left,
        y: client.top,
    };
    let mut br = POINT {
        x: client.right,
        y: client.bottom,
    };
    if !ClientToScreen(st.target, &mut tl).as_bool()
        || !ClientToScreen(st.target, &mut br).as_bool()
    {
        return;
    }
    let target_rect = RECT {
        left: tl.x,
        top: tl.y,
        right: br.x,
        bottom: br.y,
    };
    if target_rect.right <= target_rect.left || target_rect.bottom <= target_rect.top {
        return;
    }

    if !st.shown || !rect_eq(&st.last_rect, &target_rect) {
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            target_rect.left,
            target_rect.top,
            target_rect.right - target_rect.left,
            target_rect.bottom - target_rect.top,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        st.last_rect = target_rect;
        st.shown = true;
    }
}

/// 鼠标消息转发：原样广播给所有目标窗口
unsafe fn forward(st: &WndState, msg: u32, wparam: WPARAM, lparam: LPARAM) {
    for &t in &st.targets {
        let _ = PostMessageW(t, msg, wparam, lparam);
    }
}

/// 从 LPARAM 取客户区坐标（GET_X/Y_LPARAM 语义，有符号）
fn point_from_lparam(lparam: LPARAM) -> POINT {
    POINT {
        x: ((lparam.0 as u32) & 0xFFFF) as i16 as i32,
        y: (((lparam.0 as u32) >> 16) & 0xFFFF) as i16 as i32,
    }
}

/// 绘制半透明底色 + 居中提示文字（整体经 LWA_ALPHA 呈 50% 透明）
unsafe fn paint(hwnd: HWND) {
    let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    if hdc.0.is_null() {
        return;
    }

    let mut rect = RECT::default();
    if GetClientRect(hwnd, &mut rect).is_ok() {
        // 深蓝灰底：50% 透明叠在亮色游戏画面上仍可辨
        let brush = CreateSolidBrush(COLORREF(0x0050_3c1e)); // RGB(30, 60, 80)，COLORREF 为 BGR
        let _ = FillRect(hdc, &rect, brush);
        let _ = DeleteObject(brush);

        // 粗体微软雅黑白字居中
        let font = CreateFontW(
            -28, 0, 0, 0,
            700, // FW_BOLD
            0, 0, 0,
            1, // DEFAULT_CHARSET
            0, 0, 0, 0,
            w!("微软雅黑"),
        );
        let old = SelectObject(hdc, HGDIOBJ(font.0));
        SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, COLORREF(0x00FF_FFFF));
        let mut text: Vec<u16> = "鼠标事件转发模式".encode_utf16().collect();
        let _ = DrawTextW(
            hdc,
            &mut text,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        let _ = SelectObject(hdc, old);
        let _ = DeleteObject(font);
    }

    let _ = EndPaint(hwnd, &ps);
}

fn rect_eq(a: &RECT, b: &RECT) -> bool {
    a.left == b.left && a.top == b.top && a.right == b.right && a.bottom == b.bottom
}
