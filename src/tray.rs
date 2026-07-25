// 系统托盘 - 托盘图标 + 右键菜单
//
// tray-icon 通过全局 channel 投递事件，在 egui 的 update 循环里轮询。
// 窗口最小化/关闭时隐藏到托盘（而非退出），保证热键在后台仍生效。

use tray_icon::menu::{Menu, MenuItem, MenuEvent};
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
    show_id: tray_icon::menu::MenuId,
    quit_id: tray_icon::menu::MenuId,
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

        Ok(Self {
            _icon: tray,
            show_id,
            quit_id,
        })
    }

    /// 轮询托盘事件，返回用户意图（可能多个）
    pub fn poll(&self) -> Vec<TrayCommand> {
        let mut out = Vec::new();

        // 菜单点击
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.show_id {
                out.push(TrayCommand::Show);
            } else if event.id == self.quit_id {
                out.push(TrayCommand::Quit);
            }
        }

        // 托盘图标本身的点击（左键双击 -> 显示）
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::DoubleClick { .. } = event {
                out.push(TrayCommand::Show);
            }
        }

        out
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
