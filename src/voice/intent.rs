// 语音意图解析 - 规则匹配
//
// 把 ASR 识别出的中文文本映射为一个可执行意图。识别文本通常带唤醒词
// 前缀（因唤醒后回溯 2 秒），这里先剥掉"小助手"再解析。
//
// 支持的指令形态（示例）：
//   "窗口1跟随我"        → 在"窗口1"执行动作关键词"跟随我"（由上层匹配脚本）
//   "窗口1快加血"        → 在"窗口1"执行动作"快加血"
//   "所有人停止" / "所有窗口停止执行"  → 停止全部
//   "窗口1停止" / "窗口1停止执行"       → 停止指定窗口

/// 解析出的语音意图
#[derive(Debug, Clone, PartialEq)]
pub enum VoiceIntent {
    /// 停止所有窗口
    StopAll,
    /// 停止指定窗口（槽位索引）
    StopWindow(usize),
    /// 在指定窗口执行动作。action 为窗口名之后的剩余文本，
    /// 由上层与该窗口已添加的脚本名做包含匹配。
    RunAction { window: usize, action: String },
}

/// 唤醒词（回溯补齐后识别文本常含此前缀）
const WAKE_WORD: &str = "小助手";

/// 停止关键词
const STOP_WORDS: &[&str] = &["停止", "停下", "暂停"];

/// "全部"类关键词（用于停止全部判断）
const ALL_WORDS: &[&str] = &["所有", "全部", "全体", "大家", "所有人"];

/// 规范化文本：去空白、剥唤醒词、去常见标点、中文数字转阿拉伯数字
fn normalize(text: &str) -> String {
    let mut s = text.trim().to_string();
    // 去掉唤醒词（可能出现多次，一并清掉）
    s = s.replace(WAKE_WORD, "");
    // 中文数字 → 阿拉伯数字（百度常把"窗口1"听成"窗口一"）
    s = s
        .replace("一", "1")
        .replace("二", "2")
        .replace("三", "3")
        .replace("四", "4")
        .replace("五", "5")
        .replace("六", "6")
        .replace("七", "7")
        .replace("八", "8")
        .replace("九", "9")
        .replace("零", "0");
    // 去空白与常见标点（中英文）
    s.chars()
        .filter(|c| {
            !c.is_whitespace()
                && !matches!(
                    c,
                    '，' | '。' | '、' | '！' | '？' | '；' | '：'
                        | ',' | '.' | '!' | '?' | ';' | ':'
                )
        })
        .collect()
}

/// 是否包含任一停止关键词
fn has_stop(s: &str) -> bool {
    STOP_WORDS.iter().any(|w| s.contains(w))
}

/// 是否为"停止全部"意图
fn is_stop_all(s: &str) -> bool {
    has_stop(s) && ALL_WORDS.iter().any(|w| s.contains(w))
}

/// 解析文本为意图。
///
/// `windows` 为 (槽位索引, 自定义窗口名) 列表；只有名字非空的槽位应传入。
/// 匹配窗口名时优先取最长名，避免"窗口1"命中"窗口11"这类前缀歧义。
pub fn parse_intent(text: &str, windows: &[(usize, String)]) -> Option<VoiceIntent> {
    let norm = normalize(text);
    if norm.is_empty() {
        return None;
    }

    // 先判停止全部（"所有窗口"可能与某个窗口名部分重叠，需优先）
    if is_stop_all(&norm) {
        return Some(VoiceIntent::StopAll);
    }

    // 按窗口名长度降序，长名优先匹配
    let mut names: Vec<&(usize, String)> =
        windows.iter().filter(|(_, n)| !n.is_empty()).collect();
    names.sort_by(|a, b| b.1.chars().count().cmp(&a.1.chars().count()));

    // 找到第一个作为子串出现的窗口名，取其之后的文本作为动作部分
    for (idx, name) in names {
        if let Some(pos) = norm.find(name.as_str()) {
            let after = &norm[pos + name.len()..];

            // 窗口名之后含停止关键词 → 停止该窗口
            if has_stop(after) {
                return Some(VoiceIntent::StopWindow(*idx));
            }

            let action = after.trim().to_string();
            if action.is_empty() {
                // 只喊了窗口名，没有动作，无法执行
                return None;
            }
            return Some(VoiceIntent::RunAction {
                window: *idx,
                action,
            });
        }
    }

    None
}

/// 在给定脚本名列表中，找出与动作文本匹配的脚本索引。
///
/// 匹配规则：脚本名去掉扩展名后的主名若是动作文本的子串即命中
/// （如脚本"跟随.ag"，动作"跟随我"含"跟随" → 命中）。
/// 有多个命中时取主名最长者，更精确。
pub fn match_script<'a, I>(action: &str, script_names: I) -> Option<usize>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut best: Option<(usize, usize)> = None; // (索引, 主名长度)
    for (i, name) in script_names.into_iter().enumerate() {
        let base = name.rsplit_once('.').map(|(b, _)| b).unwrap_or(name);
        if base.is_empty() {
            continue;
        }
        if action.contains(base) {
            let len = base.chars().count();
            if best.map_or(true, |(_, bl)| len > bl) {
                best = Some((i, len));
            }
        }
    }
    best.map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wins() -> Vec<(usize, String)> {
        vec![
            (0, "窗口1".to_string()),
            (1, "窗口2".to_string()),
            (2, "主号".to_string()),
        ]
    }

    #[test]
    fn run_action_with_wake_prefix() {
        let r = parse_intent("小助手窗口1跟随我", &wins());
        assert_eq!(
            r,
            Some(VoiceIntent::RunAction {
                window: 0,
                action: "跟随我".to_string()
            })
        );
    }

    #[test]
    fn run_action_add_blood() {
        let r = parse_intent("窗口1快加血", &wins());
        assert_eq!(
            r,
            Some(VoiceIntent::RunAction {
                window: 0,
                action: "快加血".to_string()
            })
        );
    }

    #[test]
    fn stop_all_variants() {
        assert_eq!(parse_intent("所有人停止", &wins()), Some(VoiceIntent::StopAll));
        assert_eq!(
            parse_intent("所有窗口停止执行", &wins()),
            Some(VoiceIntent::StopAll)
        );
    }

    #[test]
    fn stop_specific_window() {
        assert_eq!(
            parse_intent("窗口1停止", &wins()),
            Some(VoiceIntent::StopWindow(0))
        );
        assert_eq!(
            parse_intent("窗口2停止执行", &wins()),
            Some(VoiceIntent::StopWindow(1))
        );
    }

    #[test]
    fn custom_name() {
        assert_eq!(
            parse_intent("主号跟随", &wins()),
            Some(VoiceIntent::RunAction {
                window: 2,
                action: "跟随".to_string()
            })
        );
    }

    #[test]
    fn chinese_numbers() {
        // 百度常把"窗口1"听成"窗口一"
        assert_eq!(
            parse_intent("窗口一跟随", &wins()),
            Some(VoiceIntent::RunAction {
                window: 0,
                action: "跟随".to_string()
            })
        );
        assert_eq!(
            parse_intent("小助手窗口二加血", &wins()),
            Some(VoiceIntent::RunAction {
                window: 1,
                action: "加血".to_string()
            })
        );
    }

    #[test]
    fn no_match() {
        assert_eq!(parse_intent("今天天气不错", &wins()), None);
        assert_eq!(parse_intent("窗口1", &wins()), None); // 只有窗口名无动作
    }

    #[test]
    fn script_matching() {
        let scripts = vec!["跟随.ag", "加血.ag", "回城.ag"];
        assert_eq!(match_script("跟随我", scripts.iter().copied()), Some(0));
        assert_eq!(match_script("快加血", scripts.iter().copied()), Some(1));
        assert_eq!(match_script("原地不动", scripts.iter().copied()), None);
    }
}
