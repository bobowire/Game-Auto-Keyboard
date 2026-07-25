// 取色器 - 显示窗口截图，鼠标悬停显示坐标/颜色，Ctrl 记录到列表

use crate::capture::{Bitmap, CaptureBackend, PrintWindowCapture};
use eframe::egui;
use windows::Win32::Foundation::HWND;

/// 记录的取色点
#[derive(Clone)]
pub struct PickedColor {
    pub x: i32,
    pub y: i32,
    pub rgb: u32,
}

impl PickedColor {
    /// 颜色的十六进制字符串 #RRGGBB
    pub fn hex(&self) -> String {
        format!("#{:06X}", self.rgb)
    }
}

pub struct ColorPicker {
    /// 是否显示取色窗口
    pub open: bool,
    /// 截图位图（原始像素数据，用于取色）
    bitmap: Option<Bitmap>,
    /// egui 纹理句柄（用于显示）
    texture: Option<egui::TextureHandle>,
    /// 截图尺寸
    size: (i32, i32),
    /// 已记录的取色点
    picked: Vec<PickedColor>,
    /// 右侧列表是否展开
    drawer_open: bool,
    /// 上一帧 Ctrl 是否按下（用于边缘检测，避免连续记录）
    ctrl_was_down: bool,
    /// 当前鼠标悬停位置的坐标和颜色
    hover_info: Option<(i32, i32, u32)>,
}

impl Default for ColorPicker {
    fn default() -> Self {
        Self {
            open: false,
            bitmap: None,
            texture: None,
            size: (0, 0),
            picked: Vec::new(),
            drawer_open: true,
            ctrl_was_down: false,
            hover_info: None,
        }
    }
}

impl ColorPicker {
    /// 截取目标窗口并打开取色窗口
    pub fn capture_and_open(&mut self, hwnd: HWND) -> Result<(), String> {
        let capture = PrintWindowCapture::new();
        let bitmap = capture.capture(hwnd)?;

        self.size = (bitmap.width, bitmap.height);
        self.bitmap = Some(bitmap);
        self.texture = None; // 下次渲染时重新生成纹理
        self.open = true;
        self.hover_info = None;

        Ok(())
    }

    /// 确保纹理已生成（首次显示时从 bitmap 创建）
    fn ensure_texture(&mut self, ctx: &egui::Context) {
        if self.texture.is_some() {
            return;
        }
        let Some(bitmap) = &self.bitmap else { return };

        // Bitmap 是 BGRA，转成 egui 需要的 RGBA
        let (w, h) = (bitmap.width as usize, bitmap.height as usize);
        let mut rgba = Vec::with_capacity(w * h * 4);
        for i in 0..(w * h) {
            let idx = i * 4;
            let b = bitmap.pixels[idx];
            let g = bitmap.pixels[idx + 1];
            let r = bitmap.pixels[idx + 2];
            rgba.extend_from_slice(&[r, g, b, 255]);
        }

        let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
        let texture = ctx.load_texture("color_picker_capture", image, egui::TextureOptions::NEAREST);
        self.texture = Some(texture);
    }

    /// 渲染取色窗口（独立操作系统窗口，在 App::update 中调用）
    pub fn ui(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        self.ensure_texture(ctx);

        let viewport_id = egui::ViewportId::from_hash_of("color_picker_viewport");
        let builder = egui::ViewportBuilder::default()
            .with_title("🎨 取色器")
            .with_inner_size([960.0, 640.0]);

        // 用 immediate viewport 创建独立窗口
        let mut should_close = false;
        ctx.show_viewport_immediate(viewport_id, builder, |ctx, _class| {
            // 顶部工具栏
            egui::TopBottomPanel::top("picker_toolbar").show(ctx, |ui| {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(format!("截图尺寸: {}x{}", self.size.0, self.size.1));
                    ui.separator();
                    ui.label("移动鼠标查看颜色，按 Ctrl 记录当前点");
                    ui.separator();
                    if ui.button(if self.drawer_open { "隐藏列表 ▶" } else { "显示列表 ◀" }).clicked() {
                        self.drawer_open = !self.drawer_open;
                    }
                });

                // 实时悬停信息
                ui.horizontal(|ui| {
                    if let Some((x, y, rgb)) = self.hover_info {
                        ui.label(format!("坐标: ({}, {})", x, y));
                        ui.separator();
                        ui.label(format!("颜色: #{:06X}", rgb));
                        let color = egui::Color32::from_rgb(
                            ((rgb >> 16) & 0xFF) as u8,
                            ((rgb >> 8) & 0xFF) as u8,
                            (rgb & 0xFF) as u8,
                        );
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(24.0, 18.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(rect, 2.0, color);
                    } else {
                        ui.label("坐标: -    颜色: -");
                    }
                });
                ui.add_space(2.0);
            });

            // 右侧抽屉列表
            egui::SidePanel::right("picker_drawer")
                .resizable(true)
                .default_width(220.0)
                .show_animated(ctx, self.drawer_open, |ui| {
                    self.ui_drawer(ui);
                });

            // 中间图像
            egui::CentralPanel::default().show(ctx, |ui| {
                self.ui_image(ui);
            });

            // 检测窗口关闭请求
            if ctx.input(|i| i.viewport().close_requested()) {
                should_close = true;
            }
        });

        if should_close {
            self.open = false;
        }
    }

    /// 显示截图并处理鼠标悬停/取色
    fn ui_image(&mut self, ui: &mut egui::Ui) {
        let Some(texture) = &self.texture else { return };
        let (img_w, img_h) = (self.size.0 as f32, self.size.1 as f32);

        egui::ScrollArea::both().show(ui, |ui| {
            // 1:1 显示图像
            let response = ui.add(
                egui::Image::new(texture)
                    .fit_to_exact_size(egui::vec2(img_w, img_h))
                    .sense(egui::Sense::hover()),
            );

            let img_rect = response.rect;

            // 计算鼠标在图像内的像素坐标
            if let Some(pointer) = response.hover_pos() {
                let rel_x = (pointer.x - img_rect.left()) as i32;
                let rel_y = (pointer.y - img_rect.top()) as i32;

                if let Some(bitmap) = &self.bitmap {
                    if let Some(rgb) = bitmap.get_rgb(rel_x, rel_y) {
                        self.hover_info = Some((rel_x, rel_y, rgb));

                        // 检测 Ctrl 按下（边缘触发，避免连续记录）
                        let ctrl_down = ui.input(|i| i.modifiers.ctrl);
                        if ctrl_down && !self.ctrl_was_down {
                            self.picked.push(PickedColor { x: rel_x, y: rel_y, rgb });
                        }
                        self.ctrl_was_down = ctrl_down;
                    }
                }
            }
        });
    }

    /// 右侧抽屉：已记录的取色点列表
    fn ui_drawer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("取色列表");
            if ui.small_button("清空").clicked() {
                self.picked.clear();
            }
        });
        ui.label(format!("共 {} 个点", self.picked.len()));
        ui.separator();

        let mut to_remove: Option<usize> = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, p) in self.picked.iter().enumerate() {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        // 颜色预览
                        let color = egui::Color32::from_rgb(
                            ((p.rgb >> 16) & 0xFF) as u8,
                            ((p.rgb >> 8) & 0xFF) as u8,
                            (p.rgb & 0xFF) as u8,
                        );
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(20.0, 20.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(rect, 2.0, color);

                        ui.vertical(|ui| {
                            ui.monospace(format!("({}, {})", p.x, p.y));
                            ui.monospace(p.hex());
                        });
                    });

                    ui.horizontal(|ui| {
                        // 复制坐标
                        if ui.small_button("复制坐标").clicked() {
                            ui.output_mut(|o| o.copied_text = format!("{},{}", p.x, p.y));
                        }
                        // 复制颜色
                        if ui.small_button("复制颜色").clicked() {
                            ui.output_mut(|o| o.copied_text = p.hex());
                        }
                        if ui.small_button("删除").clicked() {
                            to_remove = Some(i);
                        }
                    });
                });
                ui.add_space(2.0);
            }
        });

        if let Some(i) = to_remove {
            self.picked.remove(i);
        }
    }
}
