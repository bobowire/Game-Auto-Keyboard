// 全局热键管理器
//
// 在独立线程注册 Ctrl+Shift+[0-9] 共 10 个热键，通过消息循环接收 WM_HOTKEY，
// 把原始按键事件投进主事件总线（EventSender 会顺带唤醒主窗口，所以窗口隐藏时
// 热键依然即时生效 —— 以前只靠 update 轮询 channel，隐藏后热键就哑了）。
// UI 线程从总线取到 HotkeyKey 后再交给状态机处理。

use crate::event_bus::{EventSender, MainEvent};
use crossbeam_channel::Sender;
use std::thread;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_CONTROL, MOD_SHIFT, MOD_NOREPEAT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, TranslateMessage, MSG, WM_HOTKEY,
};

/// 热键原始事件
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HotkeyKey {
    /// Ctrl+Shift+<digit>，digit 为 0-9
    Digit(u8),
    /// Ctrl+Shift+-（单次执行标识方案）
    Minus,
    /// Ctrl+Shift+Insert（进入发送模式）
    Insert,
    /// Ctrl+Shift+<字母>，A-Z
    Letter(char),
    /// Ctrl+Shift+<功能键>，F1-F12
    FKey(u8),
    /// Ctrl+Shift+<特殊键>
    Special(SpecialKey),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpecialKey {
    Space,
    Enter,
    Tab,
    Escape,
}

/// 热键 ID 基址；数字键 id = BASE + digit
const HOTKEY_ID_BASE: i32 = 0xA000;
/// 减号键的热键 ID
const HOTKEY_ID_MINUS: i32 = 0xA010;
/// Insert 键
const HOTKEY_ID_INSERT: i32 = 0xA020;
/// 字母键 A-Z
const HOTKEY_ID_LETTER_BASE: i32 = 0xA100;
/// 功能键 F1-F12
const HOTKEY_ID_FKEY_BASE: i32 = 0xA200;
/// 特殊键
const HOTKEY_ID_SPECIAL_BASE: i32 = 0xA300;

pub struct HotkeyManager {
    /// 消息循环线程句柄
    _thread: thread::JoinHandle<()>,
    /// 用于向消息线程投递退出请求的线程 id
    thread_id: u32,
}

impl HotkeyManager {
    /// 启动热键监听。热键事件通过 `events` 投进主事件总线。
    /// 失败（如热键被占用）会在事件流里体现为收不到对应按键，
    /// 这里仅在注册全部失败时返回 Err。
    pub fn start(events: EventSender) -> Result<Self, String> {
        let (id_tx, id_rx) = crossbeam_channel::bounded::<u32>(1);
        let (ok_tx, ok_rx) = crossbeam_channel::bounded::<Result<(), String>>(1);

        let thread = thread::spawn(move || {
            run_message_loop(events, id_tx, ok_tx);
        });

        // 等待线程报告注册结果
        let register_result = ok_rx
            .recv()
            .map_err(|_| "热键线程启动失败".to_string())?;
        register_result?;

        let thread_id = id_rx.recv().map_err(|_| "无法获取热键线程 id".to_string())?;

        Ok(Self {
            _thread: thread,
            thread_id,
        })
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        // 向消息线程发送 WM_QUIT，使 GetMessageW 返回 0，循环退出并注销热键
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

/// 消息线程主体：注册热键 -> 消息循环 -> 注销
fn run_message_loop(
    events: EventSender,
    id_tx: Sender<u32>,
    ok_tx: Sender<Result<(), String>>,
) {
    unsafe {
        use windows::Win32::System::Threading::GetCurrentThreadId;
        let thread_id = GetCurrentThreadId();

        // 注册 Ctrl+Shift+0 .. Ctrl+Shift+9
        let modifiers = MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT;
        let mut registered = 0;
        for digit in 0u8..=9 {
            let vk = b'0' as u32 + digit as u32; // '0'..'9' 的 VK 与 ASCII 相同
            let id = HOTKEY_ID_BASE + digit as i32;
            // hwnd 传 None（等价 NULL），热键绑定到当前线程
            if RegisterHotKey(None, id, modifiers, vk).is_ok() {
                registered += 1;
            }
        }

        // 注册 Ctrl+Shift+-（VK_OEM_MINUS = 0xBD）
        const VK_OEM_MINUS: u32 = 0xBD;
        if RegisterHotKey(None, HOTKEY_ID_MINUS, modifiers, VK_OEM_MINUS).is_ok() {
            registered += 1;
        }

        // 注册 Insert
        const VK_INSERT: u32 = 0x2D;
        if RegisterHotKey(None, HOTKEY_ID_INSERT, modifiers, VK_INSERT).is_ok() {
            registered += 1;
        }

        // 注册 A-Z
        for letter in 0u8..26 {
            let vk = 0x41 + letter as u32; // A=0x41, Z=0x5A
            let id = HOTKEY_ID_LETTER_BASE + letter as i32;
            if RegisterHotKey(None, id, modifiers, vk).is_ok() {
                registered += 1;
            }
        }

        // 注册 F1-F12
        for fkey in 1u8..=12 {
            let vk = 0x70 + (fkey - 1) as u32; // F1=0x70, F12=0x7B
            let id = HOTKEY_ID_FKEY_BASE + (fkey - 1) as i32;
            if RegisterHotKey(None, id, modifiers, vk).is_ok() {
                registered += 1;
            }
        }

        // 注册特殊键
        const SPECIALS: [(u32, i32); 4] = [
            (0x20, 0), // Space
            (0x0D, 1), // Enter
            (0x09, 2), // Tab
            (0x1B, 3), // Escape
        ];
        for (vk, idx) in SPECIALS {
            let id = HOTKEY_ID_SPECIAL_BASE + idx;
            if RegisterHotKey(None, id, modifiers, vk).is_ok() {
                registered += 1;
            }
        }

        if registered == 0 {
            let _ = ok_tx.send(Err(
                "热键注册全部失败（可能被其他程序占用 Ctrl+Shift+0~9）".to_string(),
            ));
            return;
        }
        let _ = ok_tx.send(Ok(()));
        let _ = id_tx.send(thread_id);

        // 消息循环
        let mut msg = MSG::default();
        // GetMessageW 返回 >0 继续，==0 收到 WM_QUIT，<0 出错
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if msg.message == WM_HOTKEY {
                let id = msg.wParam.0 as i32;

                let key = if id == HOTKEY_ID_MINUS {
                    Some(HotkeyKey::Minus)
                } else if id == HOTKEY_ID_INSERT {
                    Some(HotkeyKey::Insert)
                } else if id >= HOTKEY_ID_BASE && id < HOTKEY_ID_BASE + 10 {
                    Some(HotkeyKey::Digit((id - HOTKEY_ID_BASE) as u8))
                } else if id >= HOTKEY_ID_LETTER_BASE && id < HOTKEY_ID_LETTER_BASE + 26 {
                    let letter_idx = (id - HOTKEY_ID_LETTER_BASE) as u8;
                    Some(HotkeyKey::Letter((b'A' + letter_idx) as char))
                } else if id >= HOTKEY_ID_FKEY_BASE && id < HOTKEY_ID_FKEY_BASE + 12 {
                    Some(HotkeyKey::FKey(((id - HOTKEY_ID_FKEY_BASE) + 1) as u8))
                } else if id >= HOTKEY_ID_SPECIAL_BASE && id < HOTKEY_ID_SPECIAL_BASE + 4 {
                    let special = match id - HOTKEY_ID_SPECIAL_BASE {
                        0 => SpecialKey::Space,
                        1 => SpecialKey::Enter,
                        2 => SpecialKey::Tab,
                        3 => SpecialKey::Escape,
                        _ => unreachable!(),
                    };
                    Some(HotkeyKey::Special(special))
                } else {
                    None
                };

                // 投进总线：入队 + 唤醒主窗口（窗口隐藏时也能立刻处理）
                if let Some(key) = key {
                    events.send(MainEvent::Hotkey(key));
                }
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // 退出：注销所有热键
        for digit in 0u8..=9 {
            let id = HOTKEY_ID_BASE + digit as i32;
            let _ = UnregisterHotKey(None, id);
        }
        let _ = UnregisterHotKey(None, HOTKEY_ID_MINUS);
        let _ = UnregisterHotKey(None, HOTKEY_ID_INSERT);
        for letter in 0u8..26 {
            let id = HOTKEY_ID_LETTER_BASE + letter as i32;
            let _ = UnregisterHotKey(None, id);
        }
        for fkey in 0u8..12 {
            let id = HOTKEY_ID_FKEY_BASE + fkey as i32;
            let _ = UnregisterHotKey(None, id);
        }
        for idx in 0i32..4 {
            let id = HOTKEY_ID_SPECIAL_BASE + idx;
            let _ = UnregisterHotKey(None, id);
        }
    }
}
