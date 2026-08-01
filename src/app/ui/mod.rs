// UI 编排：update() 每帧调用的顶层 UI 入口。
//
// 子模块按区域划分：slot（槽位卡片）、settings（设置标签页）、guides（帮助/引导弹窗）。
// 本文件保留状态栏、源码面板、添加方案弹窗、热键说明、设置窗口外壳、中央面板编排。

mod guides;
mod settings;
mod slot;

use super::{App, GRAB_COUNTDOWN_SECS, SLOT_COUNT, SettingsTab};
use std::time::Instant;

use crate::window_slot::Scheme;
use eframe::egui;

impl App {
    pub(super) fn ui_bottom_status(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                // 取色倒计时提示（醒目）
                if let Some(since) = self.picking_since {
                    let elapsed = since.elapsed().as_secs();
                    let remaining = GRAB_COUNTDOWN_SECS.saturating_sub(elapsed);
                    ui.colored_label(
                        egui::Color32::from_rgb(0, 200, 255),
                        format!("🎨 取色倒计时: {} 秒（请切换到目标窗口）", remaining),
                    );
                    ui.separator();
                }

                // 发送模式提示
                if self.hotkey_sm.in_send_mode() {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 140, 0),
                        "🎯 发送模式激活中（2秒内按任意键）",
                    );
                    ui.separator();
                }

                // 热键选择集提示
                let sel = self.hotkey_sm.selected();
                if !sel.is_empty() {
                    let list: Vec<String> = sel.iter().map(|n| n.to_string()).collect();
                    ui.colored_label(
                        egui::Color32::from_rgb(200, 120, 0),
                        format!("已选窗口 [{}]，按 Ctrl+Shift+9 启动 / +0 停止", list.join(",")),
                    );
                    ui.separator();
                }
                ui.label(&self.status);
            });
            ui.add_space(2.0);
        });
    }

    pub(super) fn ui_source_panel(&mut self, ctx: &egui::Context) {
        let Some(idx) = self.viewing_script else { return };
        if idx >= self.scripts.len() {
            self.viewing_script = None;
            return;
        }
        egui::SidePanel::right("source_panel")
            .default_width(360.0)
            .show(ctx, |ui| {
                let sf = &self.scripts[idx];
                ui.horizontal(|ui| {
                    ui.heading(&sf.name);
                    if ui.button("关闭").clicked() {
                        self.viewing_script = None;
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut sf.source.clone())
                            .code_editor()
                            .desired_width(f32::INFINITY)
                            .interactive(false),
                    );
                });
            });
    }

    /// 为某槽位添加方案的弹窗：列出所有可用脚本，点击加入
    pub(super) fn ui_add_scheme_window(&mut self, ctx: &egui::Context) {
        let Some(slot_idx) = self.adding_scheme_for else { return };
        let mut open = true;

        // 先收集所有脚本信息，避免借用冲突
        let script_list: Vec<(usize, String, String, bool, Option<Vec<crate::script::Command>>, Option<String>, crate::script::ScriptSettings)> =
            self.scripts.iter().enumerate().map(|(i, sf)| {
                (i, sf.name.clone(), sf.category.clone(), sf.is_valid(), sf.commands.clone(), sf.parse_error.clone(), sf.settings.clone())
            }).collect();

        let mut to_add: Option<(usize, String, Vec<crate::script::Command>, crate::script::ScriptSettings)> = None;

        egui::Window::new(format!("为窗口 {} 添加方案", slot_idx + 1))
            .collapsible(false)
            .resizable(true)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("点击脚本加入该窗口的方案集：");
                ui.separator();

                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    // 按分类分组
                    let mut categories: std::collections::BTreeMap<String, Vec<(usize, String, bool, Option<Vec<crate::script::Command>>, Option<String>, crate::script::ScriptSettings)>> =
                        std::collections::BTreeMap::new();
                    for (i, name, category, valid, commands, parse_error, settings) in &script_list {
                        categories.entry(category.clone())
                            .or_insert_with(Vec::new)
                            .push((*i, name.clone(), *valid, commands.clone(), parse_error.clone(), settings.clone()));
                    }

                    // 显示每个分类
                    for (category, scripts) in categories {
                        egui::CollapsingHeader::new(&category)
                            .default_open(true)
                            .show(ui, |ui| {
                                for (i, name, valid, commands, parse_error, settings) in scripts {
                                    ui.horizontal(|ui| {
                                        // 状态文本标签
                                        if valid {
                                            ui.colored_label(egui::Color32::from_rgb(0, 180, 0), "有效")
                                                .on_hover_text("脚本解析成功");
                                        } else {
                                            let error_text = parse_error.unwrap_or_else(|| "未知错误".to_string());
                                            ui.colored_label(egui::Color32::from_rgb(220, 0, 0), "无效")
                                                .on_hover_text(format!("语法错误:\n{}", error_text));
                                        }
                                        ui.label(&name);
                                        // 仅解析成功的脚本可加入
                                        if valid {
                                            if ui.small_button("加入").clicked() {
                                                if let Some(cmds) = commands {
                                                    to_add = Some((i, name.clone(), cmds, settings));
                                                }
                                            }
                                        }
                                    });
                                }
                            });
                    }
                });
            });

        // 处理添加操作
        if let Some((script_idx, script_name, commands, settings)) = to_add {
            let scheme = Scheme {
                script_name,
                commands,
                settings,
            };
            if self.slots[slot_idx].add_scheme(scheme) {
                self.status = format!("窗口 {} 已添加方案: {}", slot_idx + 1, self.scripts[script_idx].name);
                self.save_config();
            } else {
                self.status = format!("方案已存在: {}", self.scripts[script_idx].name);
            }
        }

        if !open {
            self.adding_scheme_for = None;
        }
    }

    /// 热键说明窗口
    pub(super) fn ui_hotkey_help_window(&mut self, ctx: &egui::Context) {
        if !self.show_hotkey_help {
            return;
        }

        egui::Window::new("🎮 热键使用说明")
            .collapsible(false)
            .resizable(true)
            .default_width(600.0)
            .open(&mut self.show_hotkey_help)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("所有热键都以 Ctrl+Shift 开头，支持前缀选择和批量操作")
                        .strong()
                        .color(egui::Color32::from_rgb(100, 150, 255)),
                );
                ui.add_space(8.0);

                egui::ScrollArea::vertical().max_height(500.0).show(ui, |ui| {
                    // 场景1：窗口选择
                    ui.collapsing("📌 场景一：选择窗口（前缀选择）", |ui| {
                        ui.label("用于指定后续操作作用于哪些窗口。");
                        ui.add_space(4.0);
                        ui.label("• 热键：Ctrl+Shift+[1~8]");
                        ui.label("• 首次按下：加入选择集");
                        ui.label("• 重复按下：立即停止该窗口（移出选择集）");
                        ui.label("• 可连续按多个数字选择多个窗口");
                        ui.label("• 选中后状态栏会显示黄色提示");
                        ui.label("• 3秒内未触发操作则自动清空选择");
                        ui.add_space(4.0);
                        ui.group(|ui| {
                            ui.label(egui::RichText::new("示例：").strong());
                            ui.monospace("  Ctrl+Shift+1");
                            ui.label("  → 选中窗口1，状态栏显示「已选窗口 [1]」");
                            ui.add_space(2.0);
                            ui.monospace("  Ctrl+Shift+1 (再按一次)");
                            ui.label("  → 停止窗口1的脚本执行");
                            ui.add_space(2.0);
                            ui.monospace("  Ctrl+Shift+1, Ctrl+Shift+3");
                            ui.label("  → 选中窗口1和3");
                        });
                    });

                    ui.add_space(8.0);

                    // 场景2：循环执行
                    ui.collapsing("🔁 场景二：循环执行脚本", |ui| {
                        ui.label("启动窗口的标识方案，脚本会循环执行直到手动停止。");
                        ui.add_space(4.0);
                        ui.label("• 热键：Ctrl+Shift+9");
                        ui.label("• 有前缀选择 → 启动选中的窗口");
                        ui.label("• 无前缀选择 → 启动所有已绑定窗口");
                        ui.label("• 延迟1秒后开始执行（给你时间松开按键）");
                        ui.add_space(4.0);
                        ui.group(|ui| {
                            ui.label(egui::RichText::new("示例：").strong());
                            ui.monospace("  Ctrl+Shift+9");
                            ui.label("  → 启动所有窗口的标识方案");
                            ui.add_space(2.0);
                            ui.monospace("  Ctrl+Shift+2, Ctrl+Shift+9");
                            ui.label("  → 只启动窗口2的标识方案");
                        });
                    });

                    ui.add_space(8.0);

                    // 场景3：停止执行
                    ui.collapsing("⏹ 场景三：停止执行", |ui| {
                        ui.label("停止正在运行的脚本。");
                        ui.add_space(4.0);
                        ui.label("• 热键：Ctrl+Shift+0");
                        ui.label("• 有前缀选择 → 停止选中的窗口");
                        ui.label("• 无前缀选择 → 停止所有窗口");
                        ui.add_space(4.0);
                        ui.group(|ui| {
                            ui.label(egui::RichText::new("示例：").strong());
                            ui.monospace("  Ctrl+Shift+0");
                            ui.label("  → 停止所有正在运行的窗口");
                            ui.add_space(2.0);
                            ui.monospace("  Ctrl+Shift+1, Ctrl+Shift+3, Ctrl+Shift+0");
                            ui.label("  → 只停止窗口1和3");
                        });
                    });

                    ui.add_space(8.0);

                    // 场景4：单次执行
                    ui.collapsing("▶ 场景四：单次执行脚本", |ui| {
                        ui.label("执行一次标识方案后自动停止（不循环）。");
                        ui.add_space(4.0);
                        ui.label("• 热键：Ctrl+Shift+-（减号键）");
                        ui.label("• 有前缀选择 → 单次执行选中的窗口");
                        ui.label("• 无前缀选择 → 单次执行所有已绑定窗口");
                        ui.label("• 延迟1秒后开始执行");
                        ui.add_space(4.0);
                        ui.group(|ui| {
                            ui.label(egui::RichText::new("示例：").strong());
                            ui.monospace("  Ctrl+Shift+-");
                            ui.label("  → 所有窗口的标识方案各执行一次");
                            ui.add_space(2.0);
                            ui.monospace("  Ctrl+Shift+5, Ctrl+Shift+-");
                            ui.label("  → 只有窗口5执行一次");
                        });
                    });

                    ui.add_space(8.0);

                    // 场景5：即兴发送
                    ui.collapsing("🎯 场景五：即兴发送任意按键", |ui| {
                        ui.label("快速向窗口发送单个按键，无需编写脚本。");
                        ui.add_space(4.0);
                        ui.label("• 步骤1：Ctrl+Shift+Insert（进入发送模式）");
                        ui.label("• 步骤2：2秒内按 Ctrl+Shift+<任意键>");
                        ui.label("• 支持：字母A-Z、数字0-9、F1-F12、空格/回车/Tab/ESC");
                        ui.label("• 有前缀选择 → 发给选中窗口");
                        ui.label("• 无前缀选择 → 发给所有已绑定窗口");
                        ui.label("• 发送前会先停止目标窗口正在运行的脚本");
                        ui.label("• 延迟1秒后发送");
                        ui.add_space(4.0);
                        ui.group(|ui| {
                            ui.label(egui::RichText::new("示例：").strong());
                            ui.monospace("  Ctrl+Shift+Insert → Ctrl+Shift+H");
                            ui.label("  → 所有窗口收到按键 'H'");
                            ui.add_space(2.0);
                            ui.monospace("  Ctrl+Shift+2 → Ctrl+Shift+Insert → Ctrl+Shift+F5");
                            ui.label("  → 窗口2收到 'F5'（若窗口2在跑脚本，会先停止）");
                        });
                    });

                    ui.add_space(8.0);

                    // 场景6：快捷开关
                    ui.collapsing("🎚 场景六：语音/转发快捷开关", |ui| {
                        ui.label("用热键快速开关语音控制或消息转发，无需切回主窗口点按钮。");
                        ui.add_space(4.0);
                        ui.label("• 语音开关：Ctrl+Shift+F1");
                        ui.label("• 转发开关：Ctrl+Shift+F2");
                        ui.label("• 开启播成功音、关闭播失败音；开启失败（如缺密钥/未标记主窗口）也播失败音");
                        ui.add_space(4.0);
                        ui.group(|ui| {
                            ui.label(egui::RichText::new("注意：").strong());
                            ui.label("• 这两个热键也受「启用热键」总开关控制");
                            ui.label("• 即兴发送模式下 F1/F2 会被当作要发送的按键，不触发开关");
                        });
                    });

                    ui.add_space(12.0);

                    // 小贴士
                    ui.group(|ui| {
                        ui.label(egui::RichText::new("💡 小贴士").strong());
                        ui.add_space(2.0);
                        ui.label("• 所有热键触发操作都会延迟1秒执行，避免修饰键冲突");
                        ui.label("• 发送模式激活后，状态栏会显示黄色提示");
                        ui.label("• 前缀选择3秒自动清空，发送模式2秒自动退出");
                        ui.label("• 窗口需先绑定方案并设置标识（★）才能执行");
                        ui.label("• 抓取窗口时不能选择本程序自己的窗口");
                        ui.label("• 同一窗口数字键按两次即可快速停止其脚本");
                    });
                });
            });
    }

    /// 统一配置窗口
    pub(super) fn ui_settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        let mut act_save = false;

        egui::Window::new("⚙ 设置")
            .collapsible(false)
            .resizable(true)
            .default_width(500.0)
            .open(&mut open)
            .show(ctx, |ui| {
                // 标签页选择
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.settings_tab, SettingsTab::General, "🔧 通用");
                    ui.selectable_value(&mut self.settings_tab, SettingsTab::Voice, "🎤 语音控制");
                    ui.selectable_value(&mut self.settings_tab, SettingsTab::Forward, "🖱 转发");
                    ui.selectable_value(&mut self.settings_tab, SettingsTab::Hotkey, "⌨️ 热键配置");
                    ui.selectable_value(&mut self.settings_tab, SettingsTab::About, "ℹ️ 关于");
                });
                ui.separator();

                match self.settings_tab {
                    SettingsTab::General => self.ui_settings_general(ui, &mut act_save),
                    SettingsTab::Voice => self.ui_settings_voice(ui, &mut act_save),
                    SettingsTab::Forward => self.ui_settings_forward(ui, &mut act_save),
                    SettingsTab::Hotkey => self.ui_settings_hotkey(ui, &mut act_save),
                    SettingsTab::About => self.ui_settings_about(ui),
                }

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("💾 保存").clicked() {
                        act_save = true;
                    }
                });
            });

        if act_save {
            self.save_config();
            self.status = "配置已保存".to_string();
        }
        if !open {
            self.show_settings = false;
        }
    }

    pub(super) fn ui_central(&mut self, ctx: &egui::Context) {
        // 收集待执行的动作，避免在借用 slots 时修改 self
        let mut act_start: Option<usize> = None;
        let mut act_stop: Option<usize> = None;
        let mut act_grab: Option<usize> = None;
        let mut act_add: Option<usize> = None;
        let mut act_view: Option<usize> = None;
        let mut act_reload = false;
        let mut act_start_all = false;
        let mut act_stop_all = false;
        let mut act_pick = false;
        let mut act_toggle_voice = false;
        let mut act_toggle_overlay = false;

        egui::CentralPanel::default().show(ctx, |ui| {
            // 顶部工具行
            ui.horizontal(|ui| {
                ui.heading("窗口方案管理");
                if ui.button("🔄 重载脚本").clicked() {
                    act_reload = true;
                }
                if ui.button("🎨 取色").clicked() {
                    act_pick = true;
                }
                // 语音控制开关
                let voice_on = self.voice.is_some();
                let voice_label = if voice_on { "🎤 语音: 开" } else { "🎤 语音: 关" };
                if ui
                    .button(egui::RichText::new(voice_label).color(if voice_on {
                        egui::Color32::GREEN
                    } else {
                        egui::Color32::GRAY
                    }))
                    .on_hover_text("开启/关闭语音控制")
                    .clicked()
                {
                    act_toggle_voice = true;
                }
                // 鼠标转发开关
                let overlay_on = self.overlay.is_some();
                let overlay_label = if overlay_on { "🖱 转发: 开" } else { "🖱 转发: 关" };
                if ui
                    .button(egui::RichText::new(overlay_label).color(if overlay_on {
                        egui::Color32::GREEN
                    } else {
                        egui::Color32::GRAY
                    }))
                    .on_hover_text("用半透明覆盖窗盖住主窗口客户区，把鼠标操作转发给主窗口")
                    .clicked()
                {
                    act_toggle_overlay = true;
                }
                if ui.button("⚙ 设置").clicked() {
                    self.show_settings = true;
                }
                ui.separator();
                if ui.button("▶ 全部启动").clicked() {
                    act_start_all = true;
                }
                if ui.button("⏹ 全部停止").clicked() {
                    act_stop_all = true;
                }
            });
            ui.label(
                egui::RichText::new(
                    "热键: Ctrl+Shift+[1-8] 选窗口 → +9 循环启动 / +0 停止 / +- 单次执行 / +Insert 发送任意键；不选则作用于全部",
                )
                .small()
                .color(egui::Color32::GRAY),
            );
            ui.separator();

            egui::ScrollArea::vertical().show(ui, |ui| {
                for idx in 0..SLOT_COUNT {
                    self.ui_slot(
                        ui,
                        idx,
                        &mut act_start,
                        &mut act_stop,
                        &mut act_grab,
                        &mut act_add,
                        &mut act_view,
                    );
                }
            });
        });

        // 统一处理动作
        if act_reload {
            self.reload_scripts();
        }
        if act_pick {
            self.picking_since = Some(Instant::now());
            self.status = "取色：3 秒内切换到目标窗口...".to_string();
        }
        if act_start_all {
            self.start_all();
        }
        if act_stop_all {
            self.stop_all();
        }
        if let Some(i) = act_grab {
            self.grabbing_slot = Some(i);
            self.grabbing_since = Some(Instant::now());
            self.status = format!("窗口 {}: 倒计时中，请切换到目标窗口", i + 1);
        }
        if let Some(i) = act_add {
            self.adding_scheme_for = Some(i);
        }
        if let Some(i) = act_view {
            self.viewing_script = Some(i);
        }
        if let Some(i) = act_start {
            self.start_slot(i);
        }
        if let Some(i) = act_stop {
            self.stop_slot(i);
        }
        if act_toggle_voice {
            if self.voice.is_some() {
                self.stop_voice();
            } else {
                self.start_voice();
            }
        }
        if act_toggle_overlay {
            if self.overlay.is_some() {
                self.stop_overlay();
            } else {
                self.start_overlay();
            }
        }
    }
}
