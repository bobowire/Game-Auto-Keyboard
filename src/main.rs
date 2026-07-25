// Windows 下隐藏控制台窗口（release 构建时）
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use game_auto_keyboard::App;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info")
    ).init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([720.0, 520.0])
            .with_title("游戏自动按键工具"),
        ..Default::default()
    };

    eframe::run_native(
        "游戏自动按键工具",
        options,
        Box::new(|cc| {
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}

/// 加载系统中文字体，避免中文显示为方块
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 尝试加载 Windows 自带的中文字体
    let candidates = [
        "C:/Windows/Fonts/msyh.ttc",    // 微软雅黑
        "C:/Windows/Fonts/simhei.ttf",  // 黑体
        "C:/Windows/Fonts/simsun.ttc",  // 宋体
    ];

    for path in candidates {
        if let Ok(data) = std::fs::read(path) {
            fonts.font_data.insert(
                "cn_font".to_owned(),
                egui::FontData::from_owned(data).into(),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "cn_font".to_owned());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .insert(0, "cn_font".to_owned());
            break;
        }
    }

    ctx.set_fonts(fonts);
}
