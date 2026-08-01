// 设置窗口的各标签页：通用 / 转发 / 语音 / 热键 / 关于。
// 由 ui/mod.rs 的 ui_settings_window 按 settings_tab 分发调用，故 pub(super)。

use super::super::{App, WAKEWORD_MODEL_PATH};
use eframe::egui;

impl App {
    /// 通用配置标签页
    pub(super) fn ui_settings_general(&mut self, ui: &mut egui::Ui, _act_save: &mut bool) {
        ui.label(egui::RichText::new("日志设置").strong());
        ui.add_space(4.0);

        ui.checkbox(&mut self.log_enabled, "启用日志文件");
        ui.add_space(2.0);
        ui.label("禁用后将不会写入 voice_debug.log 日志文件");

        if !self.log_enabled {
            ui.add_space(4.0);
            ui.colored_label(
                egui::Color32::from_rgb(200, 120, 0),
                "⚠ 日志已禁用，问题排查将受限"
            );
        }

        ui.add_space(12.0);
        ui.label(egui::RichText::new("唤醒词训练").strong());
        ui.add_space(4.0);

        ui.checkbox(&mut self.save_wakeword_samples, "保存训练样本");
        ui.add_space(2.0);
        ui.label("开启后，训练唤醒词时会保存录音样本到 wakeword_samples/ 目录");
        ui.label("关闭后，训练时使用临时文件，训练完成后自动删除");

        ui.add_space(12.0);
        ui.label(egui::RichText::new("语音识别调试").strong());
        ui.add_space(4.0);

        ui.checkbox(&mut self.save_asr_audio, "保存 ASR 音频");
        ui.add_space(2.0);
        ui.label("开启后，将发送给百度 ASR 的音频保存到 sendvoice/ 目录");
        ui.label("关闭后，不保存音频文件（默认）");
    }

    /// 鼠标转发配置标签页
    pub(super) fn ui_settings_forward(&mut self, ui: &mut egui::Ui, _act_save: &mut bool) {
        ui.label(egui::RichText::new("鼠标移动转发").strong());
        ui.add_space(4.0);

        ui.checkbox(&mut self.forward_rbutton_move, "右键按下时广播鼠标移动");
        ui.add_space(2.0);
        ui.label("关闭后，按住右键拖动期间不向任何窗口转发鼠标移动");
        ui.label("（用于规避右键拖视角的反馈环；右键的按下/弹起仍正常转发）");

        ui.add_space(12.0);
        ui.label(egui::RichText::new("键盘消息转发").strong());
        ui.add_space(4.0);

        ui.checkbox(&mut self.forward_keyboard, "转发键盘消息");
        ui.add_space(2.0);
        ui.label("开启后，覆盖窗持焦时按键转发给目标窗口");
        ui.label("Ctrl+Q 仍为关闭转发的快捷键，不会被转发");

        ui.add_space(8.0);
        ui.checkbox(&mut self.forward_marked_only, "键盘只发给主窗口（⚑）");
        ui.add_space(2.0);
        ui.label("开启后键盘只发给 ⚑ 标记的主窗口；关闭则广播给所有绑定窗口");
        ui.label("（鼠标消息不受此开关影响，始终广播给全部绑定窗口）");

        ui.add_space(8.0);
        ui.colored_label(
            egui::Color32::from_rgb(120, 120, 120),
            "💡 改动保存后，需关闭并重新打开「🖱 转发」开关才生效（目标窗口集合为开启时的快照）",
        );
    }

    /// 语音配置标签页
    pub(super) fn ui_settings_voice(&mut self, ui: &mut egui::Ui, _act_save: &mut bool) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("百度语音识别密钥")
                    .strong(),
            );
            ui.label("（");
            if ui.link("在百度智能云控制台申请").clicked() {
                self.show_baidu_guide = true;
            }
            ui.label("）");
        });
        ui.add_space(4.0);
        egui::Grid::new("baidu_keys").num_columns(2).show(ui, |ui| {
            ui.label("API Key:");
            ui.add(
                egui::TextEdit::singleline(&mut self.baidu_api_key)
                    .desired_width(320.0)
                    .password(true),
            );
            ui.end_row();
            ui.label("Secret Key:");
            ui.add(
                egui::TextEdit::singleline(&mut self.baidu_secret_key)
                    .desired_width(320.0)
                    .password(true),
            );
            ui.end_row();
        });

        ui.add_space(8.0);
        let model_ok = std::path::Path::new(WAKEWORD_MODEL_PATH).exists();
        ui.horizontal(|ui| {
            ui.label("唤醒词模型:");
            if model_ok {
                if ui.link(format!("✓ {}", WAKEWORD_MODEL_PATH))
                    .on_hover_text("点击查看训练指南")
                    .clicked() {
                    self.show_wakeword_guide = true;
                }
            } else {
                if ui.link(format!("✗ 缺失 {}", WAKEWORD_MODEL_PATH))
                    .on_hover_text("点击查看如何训练唤醒词模型")
                    .clicked() {
                    self.show_wakeword_guide = true;
                }
            }
        });

        ui.add_space(12.0);
        ui.label(egui::RichText::new("指令匹配").strong());
        ui.add_space(4.0);

        ui.checkbox(&mut self.pinyin_assist, "拼音辅助匹配");
        ui.add_space(2.0);
        ui.label("开启后，字符匹配之外再做一轮拼音匹配（忽略声调、多音字全读音），取更优结果");
        ui.label("可救回同音误识别（如“加血”被听成“加雪”）；打平时以字符匹配为准");

        if !self.last_voice_text.is_empty() {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("最近识别:");
                ui.colored_label(
                    egui::Color32::LIGHT_BLUE,
                    &self.last_voice_text,
                );
            });
        }

        ui.add_space(8.0);
        if ui.button("📖 查看语音指令帮助").clicked() {
            self.show_voice_help = true;
        }
    }

    /// 热键配置标签页
    pub(super) fn ui_settings_hotkey(&mut self, ui: &mut egui::Ui, _act_save: &mut bool) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.hotkey_enabled, "启用热键");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("❓ 热键说明").clicked() {
                    self.show_hotkey_help = true;
                }
            });
        });
        ui.add_space(4.0);

        if !self.hotkey_enabled {
            ui.colored_label(egui::Color32::from_rgb(200, 120, 0), "⚠ 热键已全局禁用");
            ui.add_space(4.0);
        }

        ui.label(egui::RichText::new("已注册热键列表").strong());
        ui.add_space(4.0);

        egui::Grid::new("hotkey_list")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                ui.label("功能");
                ui.label("热键");
                ui.end_row();

                ui.label("窗口选择");
                ui.monospace("Ctrl+Shift+1~8");
                ui.end_row();

                ui.label("循环启动");
                ui.monospace("Ctrl+Shift+9");
                ui.end_row();

                ui.label("全部停止");
                ui.monospace("Ctrl+Shift+0");
                ui.end_row();

                ui.label("单次执行");
                ui.monospace("Ctrl+Shift+- (减号)");
                ui.end_row();

                ui.label("即兴发送");
                ui.horizontal(|ui| {
                    ui.monospace("Ctrl+Shift+Insert + [A-Z/0-9/F1-F12/Space]");
                    ui.checkbox(&mut self.hotkey_impromptu_enabled, "");
                });
                ui.end_row();

                ui.label("语音开关");
                ui.monospace("Ctrl+Shift+F1");
                ui.end_row();

                ui.label("转发开关");
                ui.monospace("Ctrl+Shift+F2");
                ui.end_row();
            });

        ui.add_space(8.0);
        ui.label("💡 提示：");
        ui.label("• 即兴发送：按 Ctrl+Shift+Insert 进入发送模式，2秒内按任意支持的键");
        ui.label("• Ctrl+Shift+F1/F2 切换语音/转发，开启播成功音、关闭播失败音");
        ui.label("• 热键仅在程序运行时生效，关闭后自动注销");
        ui.label("• 点击右上角「❓ 热键说明」查看详细使用指南");
    }

    /// 关于页面
    pub(super) fn ui_settings_about(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(16.0);
            ui.heading("Game Auto Keyboard");
            ui.add_space(4.0);
            ui.label(egui::RichText::new(format!("版本 {}", env!("BUILD_DATE"))).weak());
            ui.add_space(16.0);
        });

        ui.add_space(8.0);

        ui.label(egui::RichText::new("作者").strong());
        ui.add_space(2.0);
        ui.label("wireboy");
        ui.add_space(12.0);

        ui.label(egui::RichText::new("源码仓库").strong());
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label("GitHub:");
            ui.hyperlink_to(
                "bobowire/Game-Auto-Keyboard",
                "https://github.com/bobowire/Game-Auto-Keyboard",
            );
        });
        ui.horizontal(|ui| {
            ui.label("Gitee:");
            ui.hyperlink_to(
                "wireboy/game-multi-utils",
                "https://gitee.com/wireboy/game-multi-utils",
            );
        });
        ui.add_space(12.0);

        ui.label(egui::RichText::new("功能简介").strong());
        ui.add_space(2.0);
        ui.label("• 多窗口脚本自动化执行");
        ui.label("• 语音控制（唤醒词 + ASR 识别）");
        ui.label("• 热键触发、颜色检测、自动循环等");
        ui.label("• 支持自定义脚本（.ag 文件）");
    }
}
