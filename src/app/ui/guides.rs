// 帮助 / 引导弹窗：语音指令帮助、百度密钥申请、唤醒词训练。
// 由 update() 直接调用，故 pub(in crate::app)（super=crate::app::ui 不够，update 在 crate::app）。

use super::super::App;
use eframe::egui;

impl App {
    /// 语音帮助文档窗口
    pub(in crate::app) fn ui_voice_help_window(&mut self, ctx: &egui::Context) {
        if !self.show_voice_help {
            return;
        }
        let mut open = true;

        egui::Window::new("📖 语音指令帮助")
            .collapsible(false)
            .resizable(true)
            .default_width(480.0)
            .open(&mut open)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("支持的语音指令");
                    ui.add_space(8.0);

                    ui.label(egui::RichText::new("1. 执行动作").strong());
                    ui.monospace("  小助手，窗口1跟随我");
                    ui.label("  → 窗口1执行名字含「跟随」的脚本");
                    ui.monospace("  小助手，窗口1加血");
                    ui.label("  → 窗口1执行名字含「加血」的脚本");
                    ui.add_space(4.0);

                    ui.label(egui::RichText::new("2. 停止指令").strong());
                    ui.monospace("  小助手，所有人停止");
                    ui.label("  → 停止全部窗口");
                    ui.monospace("  小助手，窗口1停止");
                    ui.label("  → 停止窗口1");
                    ui.add_space(8.0);

                    ui.label(egui::RichText::new("使用说明").strong());
                    ui.label("• 脚本需先在对应窗口「+ 添加方案」");
                    ui.label("• 窗口名可在各槽位标题处编辑（如改成「主号」）");
                    ui.label("• 动作按脚本名（去扩展名）包含匹配，优先匹配度高的");
                    ui.label("• 支持中文数字：「窗口一」自动识别为「窗口1」");
                    ui.add_space(8.0);

                    ui.label(egui::RichText::new("故障排查").strong());
                    ui.label("• 查看日志文件 voice_debug.log 了解识别过程");
                    ui.label("• 详细排查指南见 VOICE_DEBUG.md");
                });
            });

        if !open {
            self.show_voice_help = false;
        }
    }

    /// 百度申请引导窗口
    pub(in crate::app) fn ui_baidu_guide_window(&mut self, ctx: &egui::Context) {
        if !self.show_baidu_guide {
            return;
        }

        let viewport_id = egui::ViewportId::from_hash_of("baidu_guide_viewport");
        let builder = egui::ViewportBuilder::default()
            .with_title("📝 如何申请百度语音识别密钥")
            .with_inner_size([520.0, 600.0]);

        let mut should_close = false;
        ctx.show_viewport_immediate(viewport_id, builder, |ctx, _class| {
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("百度智能云控制台");
                    ui.add_space(4.0);

                    ui.label("点击下方链接打开百度智能云控制台：");
                    ui.hyperlink_to(
                        "🔗 https://console.bce.baidu.com/ai/#/ai/speech/overview/index",
                        "https://console.bce.baidu.com/ai/#/ai/speech/overview/index"
                    );

                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.label(egui::RichText::new("申请步骤：").strong());
                    ui.add_space(4.0);

                    ui.label("1️⃣ 登录百度账号（没有则先注册）");
                    ui.add_space(2.0);

                    ui.label("2️⃣ 完成实名认证（必须，否则无免费额度）");
                    ui.indent("auth_tip", |ui| {
                        ui.colored_label(
                            egui::Color32::from_rgb(200, 80, 0),
                            "⚠ 未实名认证的账号无法使用免费额度"
                        );
                    });
                    ui.add_space(2.0);

                    ui.label("3️⃣ 进入「语音技术」→「短语音识别」");
                    ui.add_space(2.0);

                    ui.label("4️⃣ 点击「创建应用」");
                    ui.add_space(2.0);

                    ui.label("5️⃣ 填写应用信息：");
                    ui.indent("app_info", |ui| {
                        ui.label("• 应用名称：随意填写（如「游戏助手」）");
                        ui.label("• 接口选择：勾选「短语音识别」");
                        ui.label("• 应用归属：个人");
                    });
                    ui.add_space(2.0);

                    ui.label("6️⃣ 创建成功后，在应用列表查看：");
                    ui.indent("keys", |ui| {
                        ui.label("• API Key（复制到设置窗口）");
                        ui.label("• Secret Key（复制到设置窗口）");
                    });

                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.label("💡 提示：");
                    ui.label("• 实名认证后每个账号有免费额度（每天50,000次调用）");
                    ui.label("• 超出免费额度后按次计费");
                    ui.label("• 详细定价见官网文档");

                    ui.add_space(12.0);
                    if ui.button("✅ 我已了解").clicked() {
                        should_close = true;
                    }
                });
            });

            if ctx.input(|i| i.viewport().close_requested()) {
                should_close = true;
            }
        });

        if should_close {
            self.show_baidu_guide = false;
        }
    }

    /// 唤醒词训练引导窗口
    pub(in crate::app) fn ui_wakeword_guide_window(&mut self, ctx: &egui::Context) {
        if !self.show_wakeword_guide {
            return;
        }

        let viewport_id = egui::ViewportId::from_hash_of("wakeword_guide_viewport");
        let builder = egui::ViewportBuilder::default()
            .with_title("🎤 唤醒词训练")
            .with_inner_size([450.0, 350.0]);

        let mut should_close = false;
        let mut start_training = false;
        let mut start_recording = false;

        ctx.show_viewport_immediate(viewport_id, builder, |ctx, _class| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_space(8.0);

                // 如果没有训练状态，显示开始界面
                if self.wakeword_training.is_none() {
                    ui.heading("训练唤醒词「小助手」");
                    ui.add_space(12.0);

                    ui.label("需要录制 4 遍唤醒词来训练识别模型");
                    ui.add_space(8.0);

                    ui.label("📝 录制要求：");
                    ui.indent("requirements", |ui| {
                        ui.label("• 每次录制 1.5 秒");
                        ui.label("• 环境安静，发音清晰");
                        ui.label("• 4 遍读音保持一致");
                        ui.label("• 点击按钮后立即说话");
                    });

                    ui.add_space(20.0);

                    ui.horizontal(|ui| {
                        if ui.button("✅ 开始训练").clicked() {
                            start_training = true;
                        }
                        ui.add_space(8.0);
                        if ui.button("❌ 取消").clicked() {
                            should_close = true;
                        }
                    });
                } else {
                    // 训练进行中
                    if let Some(training) = &self.wakeword_training {
                        ui.heading(format!("第 {}/{} 遍", training.current_round, training.total_rounds));
                        ui.add_space(12.0);

                        ui.label(
                            egui::RichText::new("请清晰地说：小助手")
                                .size(20.0)
                                .color(egui::Color32::from_rgb(100, 150, 255))
                        );
                        ui.add_space(16.0);

                        // 显示状态
                        if training.is_recording {
                            if let Some(start) = training.record_start {
                                let elapsed = start.elapsed().as_secs_f32();
                                let progress = (elapsed / training.record_duration).min(1.0);

                                ui.add(egui::ProgressBar::new(progress)
                                    .text(format!("🔴 录音中... {:.1}/{:.1}秒", elapsed, training.record_duration))
                                    .desired_width(350.0));
                            }
                        } else {
                            ui.colored_label(egui::Color32::GREEN, &training.status_msg);
                            ui.add_space(12.0);

                            if training.current_round <= training.total_rounds {
                                if ui.button("🎙 点击开始录制").clicked() {
                                    start_recording = true;
                                }
                            }
                        }

                        ui.add_space(16.0);
                        ui.separator();
                        ui.add_space(8.0);

                        // 显示已完成的样本
                        ui.label(format!("已完成: {}/{}", training.samples.len(), training.total_rounds));
                    }
                }
            });

            if ctx.input(|i| i.viewport().close_requested()) {
                should_close = true;
            }
        });

        // 处理动作
        if start_training {
            self.start_wakeword_training();
        }

        if start_recording {
            self.start_wakeword_recording();
        }

        if should_close {
            self.show_wakeword_guide = false;
            self.wakeword_training = None;
        }
    }
}
