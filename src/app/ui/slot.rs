// 单个窗口槽位的 UI 卡片。
// 由 ui/mod.rs 的 ui_central 循环调用，故 pub(super)。

use super::super::{App, GRAB_COUNTDOWN_SECS};
use eframe::egui;

impl App {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn ui_slot(
        &mut self,
        ui: &mut egui::Ui,
        idx: usize,
        act_start: &mut Option<usize>,
        act_stop: &mut Option<usize>,
        act_grab: &mut Option<usize>,
        act_add: &mut Option<usize>,
        act_view: &mut Option<usize>,
    ) {
        let selected = self.hotkey_sm.selected().contains(&((idx + 1) as u8));
        let running = self.slots[idx].is_running();

        let frame = egui::Frame::group(ui.style()).fill(if selected {
            egui::Color32::from_rgb(60, 55, 20)
        } else {
            ui.style().visuals.faint_bg_color
        });

        let mut name_changed = false;
        let mut main_toggled: Option<bool> = None;
        frame.show(ui, |ui| {
            // 标题行
            ui.horizontal(|ui| {
                // 主窗口旗标（鼠标转发目标，全局互斥）
                let is_main = self.slots[idx].is_main;
                if ui
                    .small_button(
                        egui::RichText::new("⚑").color(if is_main {
                            egui::Color32::GOLD
                        } else {
                            egui::Color32::DARK_GRAY
                        }),
                    )
                    .on_hover_text(if is_main {
                        "取消主窗口标记"
                    } else {
                        "设为主窗口（鼠标转发目标）"
                    })
                    .clicked()
                {
                    main_toggled = Some(!is_main);
                }
                ui.strong(format!("{}.", idx + 1));
                // 可编辑的窗口名（语音指称用）
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.slots[idx].name)
                        .desired_width(90.0)
                        .hint_text(format!("窗口{}", idx + 1)),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    name_changed = true;
                }
                if resp.lost_focus() {
                    name_changed = true;
                }
                if running {
                    ui.colored_label(egui::Color32::GREEN, "● 运行中");
                } else {
                    ui.colored_label(egui::Color32::GRAY, "○ 空闲");
                }

                // 抓取/清除窗口
                let grabbing_this =
                    self.grabbing_slot == Some(idx) && self.grabbing_since.is_some();
                if grabbing_this {
                    let remain = GRAB_COUNTDOWN_SECS.saturating_sub(
                        self.grabbing_since.unwrap().elapsed().as_secs(),
                    );
                    ui.label(format!("⏳ {}s", remain));
                } else if ui.small_button("抓取窗口").clicked() {
                    *act_grab = Some(idx);
                }
            });

            // 窗口标题显示
            if self.slots[idx].is_bound() {
                ui.horizontal(|ui| {
                    ui.label("目标:");
                    ui.monospace(
                        egui::RichText::new(&self.slots[idx].title)
                            .color(egui::Color32::LIGHT_BLUE),
                    );
                });
            } else {
                ui.colored_label(egui::Color32::DARK_GRAY, "未绑定窗口");
            }

            // 方案集
            ui.horizontal(|ui| {
                ui.label("方案:");
                if ui.small_button("+ 添加方案").clicked() {
                    *act_add = Some(idx);
                }
            });

            let scheme_count = self.slots[idx].schemes.len();
            if scheme_count == 0 {
                ui.colored_label(egui::Color32::DARK_GRAY, "  (无方案)");
            } else {
                let marked = self.slots[idx].marked;
                let mut set_marked: Option<usize> = None;
                let mut remove: Option<usize> = None;

                for s in 0..scheme_count {
                    ui.horizontal(|ui| {
                        // 标识单选（★）
                        let is_marked = marked == Some(s);
                        let star = if is_marked { "★" } else { "☆" };
                        if ui
                            .small_button(star)
                            .on_hover_text("设为标识方案")
                            .clicked()
                        {
                            set_marked = Some(s);
                        }

                        let name = &self.slots[idx].schemes[s].script_name;
                        if is_marked {
                            ui.colored_label(egui::Color32::GOLD, name);
                        } else {
                            ui.label(name);
                        }

                        // 查看源码（在脚本池中找同名）
                        if ui.small_button("查看").clicked() {
                            if let Some(p) =
                                self.scripts.iter().position(|sf| &sf.name == name)
                            {
                                *act_view = Some(p);
                            }
                        }
                        if ui.small_button("移除").clicked() {
                            remove = Some(s);
                        }
                    });
                }

                if let Some(s) = set_marked {
                    self.slots[idx].set_marked(s);
                    self.save_config();
                }
                if let Some(s) = remove {
                    self.slots[idx].remove_scheme(s);
                    self.save_config();
                }
            }

            // 启停按钮
            ui.horizontal(|ui| {
                let can_start = self.slots[idx].is_bound()
                    && self.slots[idx].marked_scheme().is_some();
                if ui
                    .add_enabled(!running && can_start, egui::Button::new("▶ 启动标识方案"))
                    .clicked()
                {
                    *act_start = Some(idx);
                }
                if ui
                    .add_enabled(running, egui::Button::new("⏹ 停止"))
                    .clicked()
                {
                    *act_stop = Some(idx);
                }
            });
        });
        ui.add_space(4.0);

        // 窗口名编辑完成后持久化（空则回退默认名）
        if name_changed {
            if self.slots[idx].name.trim().is_empty() {
                self.slots[idx].name = format!("窗口{}", idx + 1);
            }
            self.save_config();
        }

        // 主窗口标记切换（互斥：先清全部再按需设置）
        if let Some(make_main) = main_toggled {
            for s in &mut self.slots {
                s.is_main = false;
            }
            if make_main {
                self.slots[idx].is_main = true;
            }
            self.save_config();
            // 转发在跑时切换主窗口 = 换跟踪目标
            if self.overlay.is_some() {
                self.stop_overlay();
                if make_main {
                    self.start_overlay(); // 新主窗口未绑定时内部会给出提示
                } else {
                    self.status = "🖱 已取消主窗口标记，鼠标转发停止".to_string();
                }
            }
        }
    }
}
