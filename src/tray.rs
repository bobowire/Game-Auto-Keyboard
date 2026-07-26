// 系统托盘 - 托盘图标 + 右键菜单
//
// 关键设计（不要改回轮询）：
// tray-icon / muda 默认把事件投进全局 channel，需要有人主动收。但本程序隐藏到
// 托盘后，窗口不可见 → Windows 不再产生 WM_PAINT → winit 收不到 RedrawRequested
// → eframe 不再调用 App::update。轮询点在 update 里，于是菜单点了没反应。
//
// 因此改用 set_event_handler：回调在消息循环里同步触发（隐藏时消息循环照常跑），
// 把意图存进本结构体的队列，并 PostMessage(WM_PAINT) 给主窗口，强制唤醒一次
// update 来消费队列。实测隐藏状态下该唤醒有效。

use crossbeam_channel::{Receiver, Sender};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_PAINT};

/// 托盘产生的用户意图
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrayCommand {
    /// 显示/恢复主窗口
    Show,
    /// 退出程序
    Quit,
}

/// 主窗口 HWND 的共享槽位。0 表示还没拿到。
/// 事件回调靠它唤醒 update；App 在首帧把真实 HWND 填进来。
type HwndSlot = Arc<AtomicIsize>;

pub struct Tray {
    // 持有 TrayIcon 保证其生命周期（drop 后图标消失）
    _icon: TrayIcon,
    rx: Receiver<TrayCommand>,
    hwnd: HwndSlot,
}

impl Tray {
    pub fn new() -> Result<Self, String> {
        let menu = Menu::new();
        let show_item = MenuItem::new("显示主界面", true, None);
        let quit_item = MenuItem::new("退出", true, None);
        let show_id = show_item.id().clone();
        let quit_id = quit_item.id().clone();

        menu.append(&show_item)
            .map_err(|e| format!("添加菜单项失败: {}", e))?;
        menu.append(&quit_item)
            .map_err(|e| format!("添加菜单项失败: {}", e))?;

        let icon = build_icon();

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("游戏自动按键工具")
            .with_icon(icon)
            .build()
            .map_err(|e| format!("创建托盘图标失败: {}", e))?;

        let (tx, rx) = crossbeam_channel::unbounded::<TrayCommand>();
        let hwnd: HwndSlot = Arc::new(AtomicIsize::new(0));

        install_menu_handler(tx.clone(), hwnd.clone(), show_id, quit_id);
        install_icon_handler(tx, hwnd.clone());

        Ok(Self {
            _icon: tray,
            rx,
            hwnd,
        })
    }

    /// 记录主窗口 HWND（App 在首帧调用；隐藏时靠它唤醒 update）
    pub fn set_main_hwnd(&self, raw: isize) {
        self.hwnd.store(raw, Ordering::Relaxed);
    }

    /// 是否已拿到主窗口句柄
    pub fn has_main_hwnd(&self) -> bool {
        self.hwnd.load(Ordering::Relaxed) != 0
    }

    /// 取出已排队的用户意图
    pub fn poll(&self) -> Vec<TrayCommand> {
        self.rx.try_iter().collect()
    }

    /// 强制主窗口再产生一次 update（隐藏状态下 egui 不会自然重绘）
    pub fn wake_main_window(&self) {
        wake_main_window(&self.hwnd);
    }
}

/// 菜单点击：映射成 TrayCommand 入队，并唤醒主窗口
fn install_menu_handler(tx: Sender<TrayCommand>, hwnd: HwndSlot, show_id: MenuId, quit_id: MenuId) {
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let cmd = if event.id == show_id {
            Some(TrayCommand::Show)
        } else if event.id == quit_id {
            Some(TrayCommand::Quit)
        } else {
            None
        };

        if let Some(cmd) = cmd {
            let _ = tx.send(cmd);
            wake_main_window(&hwnd);
        }
    }));
}

/// 图标本身的点击：左键双击 -> 显示
fn install_icon_handler(tx: Sender<TrayCommand>, hwnd: HwndSlot) {
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::DoubleClick { .. } = event {
            let _ = tx.send(TrayCommand::Show);
            wake_main_window(&hwnd);
        }
    }));
}

/// 强制主窗口产生一次 update。
///
/// 窗口隐藏时 egui 的 request_repaint 不起作用（winit 用 RDW_INTERNALPAINT，
/// 不可见窗口不会收到 WM_PAINT），所以直接 post 一条 WM_PAINT 过去。
fn wake_main_window(hwnd: &HwndSlot) {
    let raw = hwnd.load(Ordering::Relaxed);
    if raw == 0 {
        // 还没拿到句柄：命令已入队，等下一次 update 自然消费
        return;
    }
    unsafe {
        let _ = PostMessageW(HWND(raw as *mut _), WM_PAINT, WPARAM(0), LPARAM(0));
    }
}

/// 生成一个简单的 32x32 图标（蓝底白色方块），避免依赖外部图片文件
fn build_icon() -> Icon {
    const SIZE: u32 = 32;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let border = x < 3 || x >= SIZE - 3 || y < 3 || y >= SIZE - 3;
            if border {
                // 深蓝边框
                rgba.extend_from_slice(&[30, 90, 200, 255]);
            } else {
                // 亮蓝主体
                rgba.extend_from_slice(&[70, 150, 240, 255]);
            }
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE).expect("生成托盘图标失败")
}
