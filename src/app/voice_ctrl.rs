// 语音控制编排：启停运行时、事件分发、识别文本 → 意图 → 脚本匹配执行。
//
// 依赖 slots（start_slot / stop_slot / run_slot_once / stop_all 作为执行出口），
// 是 slots 之上的一层。handle_voice_text / try_voice_action 等内部方法仅本模块调用。

use super::{
    App, ActionOutcome, SettingsTab, SLOT_COUNT, WAKEWORD_MODEL_PATH, WAKEWORD_THRESHOLD,
    play_sound,
};
use crate::vlog;
use crate::voice::{
    MatchSource, VoiceConfig, VoiceEvent, VoiceIntent, VoiceRuntime, match_script_ex, parse_intent,
};

impl App {
    /// 开启语音控制：校验前置条件后启动后台运行时
    pub(super) fn start_voice(&mut self) {
        if self.voice.is_some() {
            return;
        }

        // 检查百度密钥
        if self.baidu_api_key.trim().is_empty() || self.baidu_secret_key.trim().is_empty() {
            self.status = "⚠ 请先配置百度语音识别密钥".to_string();
            self.show_settings = true;
            self.settings_tab = SettingsTab::Voice;
            return;
        }

        // 检查唤醒词模型
        if !std::path::Path::new(WAKEWORD_MODEL_PATH).exists() {
            self.status = "⚠ 请先训练唤醒词模型".to_string();
            self.show_wakeword_guide = true;
            return;
        }

        let cfg = VoiceConfig {
            model_path: WAKEWORD_MODEL_PATH.to_string(),
            threshold: WAKEWORD_THRESHOLD,
            api_key: self.baidu_api_key.clone(),
            secret_key: self.baidu_secret_key.clone(),
            save_asr_audio: self.save_asr_audio,
        };
        self.voice = Some(VoiceRuntime::start(cfg, self.events.sender()));
        self.status = "语音控制已开启，正在初始化...".to_string();
    }

    /// 关闭语音控制
    pub(super) fn stop_voice(&mut self) {
        if let Some(mut v) = self.voice.take() {
            v.stop();
            self.status = "语音控制已关闭".to_string();
        }
    }

    /// 处理单个语音事件
    pub(super) fn handle_voice_event(&mut self, ev: VoiceEvent) {
        // 语音已关闭时丢弃残留事件：stop_voice() 会 join 线程，线程退出前发出的
        // Stopped/Status 可能还留在总线里。若不丢弃，这些旧事件会覆盖状态栏，
        // 甚至在"关闭→立刻重开"时把刚建好的 self.voice 误清成 None。
        if self.voice.is_none() {
            return;
        }

        let mut stopped = false;
        match ev {
            VoiceEvent::Status(s) => self.status = format!("🎤 {}", s),
            VoiceEvent::Woke => self.status = "🎤 已唤醒，请说指令...".to_string(),
            VoiceEvent::Recognized(text) => {
                self.last_voice_text = text.clone();
                self.handle_voice_text(&text);
            }
            VoiceEvent::Error(e) => self.status = format!("🎤 语音错误: {}", e),
            VoiceEvent::Stopped => stopped = true,
        }
        if stopped {
            // 后台线程已退出（多为出错自停），清理句柄
            self.voice = None;
        }
    }

    /// 解析识别文本为意图并执行
    fn handle_voice_text(&mut self, text: &str) {
        // 收集非空窗口名（默认名"窗口N"也算有效指称）
        let windows: Vec<(usize, String)> = self
            .slots
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s.name.clone()))
            .collect();

        vlog!("[intent] 原始文本: 「{}」", text);
        vlog!(
            "[intent] 当前窗口名: {:?}",
            windows.iter().map(|(i, n)| format!("{}={}", i + 1, n)).collect::<Vec<_>>()
        );

        match parse_intent(text, &windows) {
            Some(VoiceIntent::StopAll) => {
                vlog!("[intent] 匹配: 停止全部");
                self.stop_all();
                self.status = format!("🎤「{}」→ 停止全部", text);
                play_sound("beep_success.wav");
            }
            Some(VoiceIntent::StopWindow(idx)) => {
                vlog!("[intent] 匹配: 停止窗口 {}", idx + 1);
                self.stop_slot(idx);
                self.status = format!("🎤「{}」→ 停止 {}", text, self.slots[idx].name);
                play_sound("beep_success.wav");
            }
            Some(VoiceIntent::RunAction { window, action }) => {
                vlog!("[intent] 匹配: 窗口 {} 执行动作「{}」", window + 1, action);
                self.run_voice_action(window, &action, text);
            }
            Some(VoiceIntent::RunActionAll { action }) => {
                vlog!("[intent] 匹配: 所有窗口执行动作「{}」", action);
                self.run_voice_action_all(&action, text);
            }
            None => {
                vlog!("[intent] 未匹配到任何窗口名或停止指令");
                self.status = format!("🎤「{}」→ 未匹配到指令", text);
                play_sound("beep_fail.wav");
            }
        }
    }

    /// 在单个窗口按动作关键词匹配脚本并执行（核心：不含状态/响铃副作用）。
    ///
    /// 返回 `ActionOutcome` 供单窗口与全部窗口两个外壳各自汇总展示。
    fn try_voice_action(&mut self, idx: usize, action: &str) -> ActionOutcome {
        let win_name = self.slots[idx].name.clone();
        if !self.slots[idx].is_bound() {
            vlog!("[intent] 窗口 {}({}) 未绑定窗口句柄，跳过", idx + 1, win_name);
            return ActionOutcome::NotBound;
        }
        // 在该窗口已添加的方案里按动作关键词匹配脚本
        let names: Vec<String> = self.slots[idx]
            .schemes
            .iter()
            .map(|s| s.script_name.clone())
            .collect();
        vlog!(
            "[intent] 窗口 {}({}) 已添加脚本: {:?}",
            idx + 1,
            win_name,
            names
        );
        let result = match_script_ex(action, names.iter().map(|s| s.as_str()), self.pinyin_assist);
        match &result.winner {
            Some(m) => {
                let scheme_idx = m.index;
                let script_name = self.slots[idx].schemes[scheme_idx].script_name.clone();
                let audio_only_once = self.slots[idx].schemes[scheme_idx].settings.audio_only_once;

                let source = match m.source {
                    MatchSource::Char => "字符",
                    MatchSource::Pinyin => "拼音",
                };
                vlog!(
                    "[intent] 动作「{}」匹配到脚本「{}」（{}命中 得分 {:.2}；字符轮 {:?} 拼音轮 {:?}），启动",
                    action, script_name, source, m.score, result.char_best, result.pinyin_best
                );
                self.slots[idx].set_marked(scheme_idx);

                // 根据脚本设置选择执行模式
                let success = if audio_only_once {
                    vlog!("[intent] 脚本设置 audio_only_once=true，单次执行");
                    self.run_slot_once(idx)
                } else {
                    self.start_slot(idx)
                };

                if success {
                    vlog!("[intent] 已启动窗口 {} 的脚本「{}」", idx + 1, script_name);
                } else {
                    vlog!(
                        "[intent] start_slot/run_slot_once 返回 false（窗口失效/无标识方案），未执行"
                    );
                }
                ActionOutcome::Ran {
                    script: script_name,
                    success,
                }
            }
            None => {
                vlog!(
                    "[intent] 动作「{}」在窗口 {} 的脚本中未找到匹配（字符轮 {:?} 拼音轮 {:?}）",
                    action,
                    idx + 1,
                    result.char_best,
                    result.pinyin_best
                );
                ActionOutcome::NoMatch
            }
        }
    }

    /// 在指定窗口按动作关键词匹配脚本并执行（带状态提示与响铃）
    fn run_voice_action(&mut self, idx: usize, action: &str, raw: &str) {
        let win_name = self.slots[idx].name.clone();
        match self.try_voice_action(idx, action) {
            ActionOutcome::NotBound => {
                self.status = format!("🎤「{}」→ {} 未绑定窗口", raw, win_name);
                play_sound("beep_fail.wav");
            }
            ActionOutcome::NoMatch => {
                self.status = format!(
                    "🎤「{}」→ {} 未找到匹配「{}」的脚本",
                    raw, win_name, action
                );
                play_sound("beep_fail.wav");
            }
            ActionOutcome::Ran { script, success } => {
                if success {
                    self.status = format!("🎤「{}」→ {} 执行 {}", raw, win_name, script);
                    play_sound("beep_success.wav");
                } else {
                    play_sound("beep_fail.wav");
                }
            }
        }
    }

    /// 在所有已绑定窗口各自按动作关键词匹配脚本并执行。
    ///
    /// 每个窗口用各自的脚本列表独立匹配（未必命中同一脚本名），
    /// 命中即启动；最后统一汇总状态、响一次铃。
    fn run_voice_action_all(&mut self, action: &str, raw: &str) {
        let mut ran = 0usize;
        let mut failed = 0usize;
        let mut skipped = 0usize;
        for idx in 0..SLOT_COUNT {
            match self.try_voice_action(idx, action) {
                ActionOutcome::NotBound => skipped += 1,
                ActionOutcome::NoMatch => failed += 1,
                ActionOutcome::Ran { success, .. } => {
                    if success {
                        ran += 1;
                    } else {
                        failed += 1;
                    }
                }
            }
        }
        if ran > 0 {
            self.status = format!(
                "🎤「{}」→ 全部窗口执行「{}」：{} 个启动，{} 个未命中",
                raw, action, ran, failed
            );
            play_sound("beep_success.wav");
        } else {
            self.status = format!(
                "🎤「{}」→ 没有窗口可执行「{}」（未命中 {}，未绑定 {}）",
                raw, action, failed, skipped
            );
            play_sound("beep_fail.wav");
        }
    }
}
