// 系统托盘 - 托盘图标 + 右键菜单
//
// 关键设计（不要改回轮询）：
// tray-icon / muda 默认把事件投进全局 channel，需要有人主动收。但本程序隐藏到
// 托盘后，窗口不可见 → Windows 不再产生 WM_PAINT → winit 收不到 RedrawRequested
// → eframe 不再调用 App::update。轮询点在 update 里，于是菜单点了没反应。
//
// 因此改用 set_event_handler：回调在消息循环里同步触发（隐藏时消息循环照常跑），
// 把意图作为 MainEvent 投进事件总线。总线的 EventSender 内部会 PostMessage(WM_PAINT)
// 唤醒主窗口，强制产生一帧 update 来消费队列 —— 唤醒逻辑不再由本模块自己维护。

use crate::event_bus::{EventSender, MainEvent};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// 托盘产生的用户意图
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrayCommand {
    /// 显示/恢复主窗口
    Show,
    /// 退出程序
    Quit,
}

pub struct Tray {
    // 持有 TrayIcon 保证其生命周期（drop 后图标消失）
    _icon: TrayIcon,
}

impl Tray {
    /// 创建托盘。事件通过 `events` 投进主事件总线。
    pub fn new(events: EventSender) -> Result<Self, String> {
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

        install_menu_handler(events.clone(), show_id, quit_id);
        install_icon_handler(events);

        Ok(Self { _icon: tray })
    }
}

/// 菜单点击：映射成 TrayCommand 投进总线（总线负责唤醒主窗口）
fn install_menu_handler(events: EventSender, show_id: MenuId, quit_id: MenuId) {
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let cmd = if event.id == show_id {
            Some(TrayCommand::Show)
        } else if event.id == quit_id {
            Some(TrayCommand::Quit)
        } else {
            None
        };

        if let Some(cmd) = cmd {
            events.send(MainEvent::Tray(cmd));
        }
    }));
}

/// 图标本身的点击：左键双击 -> 显示
fn install_icon_handler(events: EventSender) {
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::DoubleClick { .. } = event {
            events.send(MainEvent::Tray(TrayCommand::Show));
        }
    }));
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
