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
//   "全部窗口跟随我" / "所有人加血"     → 所有窗口各自动作匹配（RunActionAll）
//
// 脚本匹配支持"拼音辅助"（match_script_ex）：ASR 常把自定义脚本名听成同音错字
// （如"加血"→"加雪"），字符匹配对此零容忍。开启后在字符轮之外再做一轮以单字音节
// 为单位的拼音匹配（忽略声调、多音字展开全部读音），两轮按仲裁规则取最优。

use pinyin::ToPinyinMulti;

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
    /// 在所有窗口执行动作。剥掉"全部/所有"前缀后的剩余文本为动作，
    /// 由上层对每个窗口各自的脚本列表分别做包含匹配。
    RunActionAll { action: String },
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

/// 从"全部/所有…"类指令中剥出动作文本。
///
/// 命中最长的 ALL_WORD（"所有人"优先于"所有"）后，再剥掉残留的指称
/// 填充词前缀（"所有窗口"→"窗口"、"全部人"→"人"等），剩余即动作。
/// 例如 "全部窗口跟随我" → "跟随我"、"所有人加血" → "加血"。
fn extract_all_action(norm: &str) -> Option<String> {
    // 选命中最长的 ALL_WORD
    let mut matched: Option<&str> = None;
    for &w in ALL_WORDS {
        if norm.contains(w)
            && matched.map_or(true, |m| w.chars().count() > m.chars().count())
        {
            matched = Some(w);
        }
    }
    let matched = matched?;
    let mut rest = norm.replacen(matched, "", 1).to_string();
    // 剥掉残留指称填充词前缀（仅前缀，动作中后段出现的"人"等不动）
    let fillers = ["窗口", "人", "们", "的"];
    loop {
        match fillers.iter().find(|f| rest.starts_with(**f)) {
            Some(f) => rest = rest[f.len()..].to_string(),
            None => break,
        }
    }
    let action = rest.trim().to_string();
    if action.is_empty() {
        None
    } else {
        Some(action)
    }
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

    // "全部/所有…" + 动作（不含停止词）→ 所有窗口各自动作匹配。
    // 须在窗口名循环之前判：否则"所有窗口"里的"窗口"可能与默认窗口名
    // "窗口N"产生前缀歧义，或干脆匹配不到导致指令落空。
    if ALL_WORDS.iter().any(|w| norm.contains(w)) {
        if let Some(action) = extract_all_action(&norm) {
            return Some(VoiceIntent::RunActionAll { action });
        }
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

// ---------------------------------------------------------------------------
// 脚本匹配（字符轮 + 可选拼音轮 + 仲裁）
// ---------------------------------------------------------------------------

/// 单轮冠军记录
#[derive(Debug, Clone, Copy)]
struct Champion {
    idx: usize,
    score: f32,
    base_len: usize,
}

/// 更新冠军：得分高者优先；得分相同取 base 更短者（更精确，沿用原 tie-break）
fn consider(best: &mut Option<Champion>, cand: Champion) {
    if best.map_or(true, |b| {
        cand.score > b.score || (cand.score == b.score && cand.base_len < b.base_len)
    }) {
        *best = Some(cand);
    }
}

/// 两个 token 序列的最长公共连续段长度（DP 滚动数组，O(n·m)）。
///
/// `eq` 为"位置匹配"判定：字符轮用 `a == b`；拼音轮用音节集交集非空。
fn longest_common_run_len<T, F>(a: &[T], b: &[T], eq: F) -> usize
where
    F: Fn(&T, &T) -> bool,
{
    let mut prev = vec![0usize; b.len() + 1];
    let mut best = 0;
    for x in a {
        let mut curr = vec![0usize; b.len() + 1];
        for (j, y) in b.iter().enumerate() {
            if eq(x, y) {
                curr[j + 1] = prev[j] + 1;
                if curr[j + 1] > best {
                    best = curr[j + 1];
                }
            }
        }
        prev = curr;
    }
    best
}

/// 单个候选脚本打分：Some((匹配长度, 匹配度 = 匹配长度 / base token 数))；零匹配 → None。
fn token_score<T, F>(action: &[T], base: &[T], eq: F) -> Option<(usize, f32)>
where
    F: Fn(&T, &T) -> bool,
{
    if base.is_empty() {
        return None;
    }
    let len = longest_common_run_len(action, base, eq);
    if len == 0 {
        return None;
    }
    Some((len, len as f32 / base.len() as f32))
}

/// 命中来自哪一轮
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchSource {
    /// 中文字符匹配轮
    Char,
    /// 拼音匹配轮
    Pinyin,
}

/// 单轮冠军（供日志诊断）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoundBest {
    pub index: usize,
    pub score: f32,
}

/// 仲裁后的赢家
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScriptMatch {
    pub index: usize,
    pub source: MatchSource,
    pub score: f32,
}

/// 完整匹配结果。即使无赢家也保留两轮得分——匹配失败时最需要看"差多少没配上"。
#[derive(Debug, Clone, PartialEq)]
pub struct MatchResult {
    pub winner: Option<ScriptMatch>,
    /// 字符轮冠军（None = 零匹配）
    pub char_best: Option<RoundBest>,
    /// 拼音轮冠军（开关关闭时恒为 None）
    pub pinyin_best: Option<RoundBest>,
}

impl MatchResult {
    /// 胜出脚本索引（None = 无匹配），与 `match_script` 返回语义一致
    pub fn index(&self) -> Option<usize> {
        self.winner.as_ref().map(|w| w.index)
    }
}

/// 拼音轮得分下限：低于此值即使严格胜出也不采纳，防垃圾识别强行配上脚本。
pub const PINYIN_MIN_RATIO: f32 = 0.5;

/// 在给定脚本名列表中匹配动作文本，返回两轮得分与仲裁赢家。
///
/// 字符轮总是执行：脚本名（去扩展名）与动作的最长公共连续子串 / 脚本名长度。
/// `pinyin_assist = true` 时追加拼音轮：两侧文本转为音节序列（单字为单位、忽略声调、
/// 多音字展开全部读音），最长公共连续音节段 / 脚本名音节数。
///
/// 仲裁规则：
/// - 开关关闭 → 仅字符轮（与 `match_script` 逐位等价）；
/// - 拼音轮得分**严格大于**字符轮冠军且 `>= PINYIN_MIN_RATIO` → 拼音轮胜出；
/// - 否则（含平局、未过下限）→ 字符轮胜出（中文优先）。
///
/// 例如：动作 "佳雪"（ASR 同音错字），脚本 ["加血.ag"]
/// - 字符轮：零匹配
/// - 拼音轮：jia-xue 对 jia-xue/xie 全中 → 1.0 → 拼音轮胜出
pub fn match_script_ex<'a, I>(action: &str, script_names: I, pinyin_assist: bool) -> MatchResult
where
    I: IntoIterator<Item = &'a str>,
{
    let action_chars: Vec<char> = action.chars().collect();
    let action_syl = if pinyin_assist {
        to_syllable_seq(action)
    } else {
        Vec::new()
    };

    let mut char_champ: Option<Champion> = None;
    let mut pin_champ: Option<Champion> = None;

    for (i, name) in script_names.into_iter().enumerate() {
        let base = name.rsplit_once('.').map(|(b, _)| b).unwrap_or(name);
        if base.is_empty() {
            continue;
        }

        // 字符轮（总是执行）
        let base_chars: Vec<char> = base.chars().collect();
        if let Some((_, ratio)) = token_score(&action_chars, &base_chars, |a, b| a == b) {
            consider(
                &mut char_champ,
                Champion {
                    idx: i,
                    score: ratio,
                    base_len: base_chars.len(),
                },
            );
        }

        // 拼音轮（仅开关开启时）
        if pinyin_assist {
            let base_syl = to_syllable_seq(base);
            if let Some((_, ratio)) =
                token_score(&action_syl, &base_syl, |a, b| sets_overlap(a, b))
            {
                consider(
                    &mut pin_champ,
                    Champion {
                        idx: i,
                        score: ratio,
                        base_len: base_syl.len(),
                    },
                );
            }
        }
    }

    let char_best = char_champ.map(|c| RoundBest {
        index: c.idx,
        score: c.score,
    });
    let pinyin_best = pin_champ.map(|c| RoundBest {
        index: c.idx,
        score: c.score,
    });

    let winner = if !pinyin_assist {
        char_champ.map(|c| ScriptMatch {
            index: c.idx,
            source: MatchSource::Char,
            score: c.score,
        })
    } else {
        // 字符轮无冠军时基线记 0.0：拼音轮任意正分都算严格大于，但仍须过下限
        let baseline = char_champ.map(|c| c.score).unwrap_or(0.0);
        if let Some(p) = pin_champ.filter(|p| p.score > baseline && p.score >= PINYIN_MIN_RATIO) {
            Some(ScriptMatch {
                index: p.idx,
                source: MatchSource::Pinyin,
                score: p.score,
            })
        } else {
            // 平局 / 未过下限 → 中文优先
            char_champ.map(|c| ScriptMatch {
                index: c.idx,
                source: MatchSource::Char,
                score: c.score,
            })
        }
    };

    MatchResult {
        winner,
        char_best,
        pinyin_best,
    }
}

/// 在给定脚本名列表中，找出与动作文本匹配的脚本索引。
///
/// 匹配规则：脚本名（去扩展名）包含动作中的任意连续子串，计算匹配度（匹配长度/脚本名长度）。
/// 取匹配度最高的脚本。如果匹配度相同，取脚本名最短的（更精确）。
///
/// 例如：
/// - 动作 "跟随我"，脚本 ["蜀门-自动跟随.ag", "跟随.ag"]
/// - "蜀门-自动跟随" 包含 "跟随" → 匹配度 2/7 ≈ 0.29
/// - "跟随" 包含 "跟随" → 匹配度 2/2 = 1.0
/// - 选择 "跟随.ag"
///
/// 等价于 `match_script_ex(action, script_names, false).index()`（纯字符匹配，无拼音轮）。
pub fn match_script<'a, I>(action: &str, script_names: I) -> Option<usize>
where
    I: IntoIterator<Item = &'a str>,
{
    match_script_ex(action, script_names, false).index()
}

/// 阿拉伯数字 0-9 的拼音（与中文数字经词典得到的音节对齐）
const DIGIT_PINYIN: [&str; 10] = [
    "ling", "yi", "er", "san", "si", "wu", "liu", "qi", "ba", "jiu",
];

/// 是否跳过该字符（标点/空白不参与音节匹配）
fn is_skipped(c: char) -> bool {
    c.is_whitespace()
        || c.is_ascii_punctuation()
        || matches!(
            c,
            '，' | '。'
                | '、'
                | '！'
                | '？'
                | '；'
                | '：'
                | '\u{201c}'
                | '\u{201d}'
                | '\u{2018}'
                | '\u{2019}'
                | '（'
                | '）'
                | '《'
                | '》'
                | '—'
                | '…'
                | '·'
        )
}

/// 文本 → 音节集合序列：每个字一个读音集合（多音字全展开、忽略声调、去重）。
///
/// 分派规则（action 与脚本名两侧同一变换，天然对齐）：
/// - `0-9` → 显式表 ling/yi/…/jiu（normalize 已把中文数字转阿拉伯，
///   脚本名里的中文数字 一/二/… 经词典得到同音节）；
/// - ASCII 字母 → 小写自身（一字母一音节，大小写不敏感）；
/// - 汉字 → `to_pinyin_multi()` 全部读音的无声调形式；
/// - 词典未收录 → 字符自身兜底（拼音轮对这些字退化为字符相等）；
/// - 标点/空白 → 跳过。
///
/// 数字/字母先于词典分派，行为由本函数显式定义，不依赖 crate 对非汉字的处理。
fn to_syllable_seq(text: &str) -> Vec<Vec<String>> {
    let mut seq = Vec::new();
    for c in text.chars() {
        if is_skipped(c) {
            continue;
        }
        let set = if let Some(d) = c.to_digit(10) {
            vec![DIGIT_PINYIN[d as usize].to_string()]
        } else if c.is_ascii_alphabetic() {
            vec![c.to_ascii_lowercase().to_string()]
        } else {
            match c.to_pinyin_multi() {
                Some(multi) => {
                    let mut set: Vec<String> =
                        multi.into_iter().map(|p| p.plain().to_string()).collect();
                    set.sort_unstable();
                    set.dedup();
                    if set.is_empty() {
                        vec![c.to_string()]
                    } else {
                        set
                    }
                }
                None => vec![c.to_string()], // 词典未收录兜底
            }
        };
        seq.push(set);
    }
    seq
}

/// 音节集匹配判定：两侧读音集合交集非空（含多音字的任一读音相同）
fn sets_overlap(a: &[String], b: &[String]) -> bool {
    a.iter().any(|s| b.contains(s))
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
    fn run_action_all_variants() {
        // "全部/所有…" + 动作 → 所有窗口各自动作匹配
        assert_eq!(
            parse_intent("全部窗口跟随我", &wins()),
            Some(VoiceIntent::RunActionAll {
                action: "跟随我".to_string()
            })
        );
        assert_eq!(
            parse_intent("所有人加血", &wins()),
            Some(VoiceIntent::RunActionAll {
                action: "加血".to_string()
            })
        );
        assert_eq!(
            parse_intent("全部加血", &wins()),
            Some(VoiceIntent::RunActionAll {
                action: "加血".to_string()
            })
        );
        assert_eq!(
            parse_intent("所有窗口跟随", &wins()),
            Some(VoiceIntent::RunActionAll {
                action: "跟随".to_string()
            })
        );
        // 唤醒词前缀
        assert_eq!(
            parse_intent("小助手全部窗口跟随", &wins()),
            Some(VoiceIntent::RunActionAll {
                action: "跟随".to_string()
            })
        );
        // 只有"全部"无动作 → 仍无法执行（落空到 None）
        assert_eq!(parse_intent("全部窗口", &wins()), None);
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

    #[test]
    fn script_matching_with_prefix() {
        // 新测试：带前缀的脚本名也能匹配
        let scripts = vec!["蜀门-自动跟随.ag", "跟随.ag", "加血.ag"];
        // "跟随我"应匹配"跟随.ag"（匹配度100%）而非"蜀门-自动跟随.ag"（匹配度约29%）
        assert_eq!(match_script("跟随我", scripts.iter().copied()), Some(1));
        // "自动跟随"同样匹配"跟随.ag"：匹配度 2/2=1.0 高于"蜀门-自动跟随"的 4/7≈0.57。
        // （原断言 Some(0) 与打分规则矛盾，属历史遗留错误断言，HEAD 上即失败，此处修正）
        assert_eq!(match_script("自动跟随", scripts.iter().copied()), Some(1));
    }

    // ---------- 拼音辅助匹配 ----------

    fn ex(action: &str, names: &[&str], assist: bool) -> MatchResult {
        match_script_ex(action, names.iter().copied(), assist)
    }

    #[test]
    fn pinyin_homophone_hit() {
        // ASR 把"加血"听成同音错字"佳雪"：字符轮零匹配，拼音轮救回
        let scripts = ["跟随.ag", "加血.ag", "回城.ag"];
        let r = ex("佳雪", &scripts, true);
        let w = r.winner.expect("拼音轮应命中");
        assert_eq!(w.index, 1);
        assert_eq!(w.source, MatchSource::Pinyin);
        assert!((w.score - 1.0).abs() < f32::EPSILON);
        assert_eq!(r.char_best, None);
        assert_eq!(
            r.pinyin_best,
            Some(RoundBest {
                index: 1,
                score: 1.0
            })
        );
    }

    #[test]
    fn pinyin_strictly_greater_wins() {
        // "加雪"与"加血"共享字符"加"（字符轮 0.5），拼音轮 1.0 严格大于 → 拼音胜出
        let scripts = ["跟随.ag", "加血.ag", "回城.ag"];
        let r = ex("加雪", &scripts, true);
        let w = r.winner.expect("拼音轮应命中");
        assert_eq!(w.index, 1);
        assert_eq!(w.source, MatchSource::Pinyin);
        assert_eq!(
            r.char_best,
            Some(RoundBest {
                index: 1,
                score: 0.5
            })
        );
    }

    #[test]
    fn pinyin_off_keeps_old_behavior() {
        let scripts = ["跟随.ag", "加血.ag", "回城.ag"];
        // 开关关闭：不跑拼音轮，无法救回同音错字
        let r = ex("佳雪", &scripts, false);
        assert_eq!(r.winner, None);
        assert_eq!(r.pinyin_best, None);

        // 与旧 match_script 逐样本等价
        let samples: &[(&str, &[&str])] = &[
            ("跟随我", &["跟随.ag"]),
            ("快加血", &["加血.ag"]),
            ("原地不动", &["加血.ag"]),
            ("自动跟随", &["蜀门-自动跟随.ag", "跟随.ag"]),
            ("佳雪", &["加血.ag"]),
        ];
        for (action, names) in samples.iter().copied() {
            assert_eq!(
                ex(action, names, false).index(),
                match_script(action, names.iter().copied()),
                "action={} 时新旧行为不一致",
                action
            );
        }
    }

    #[test]
    fn pinyin_polyphonic() {
        // 血有 xuè/xiě 两读：用户说"jiāxiě"、ASR 写成"加写"，
        // 拼音轮 写(xie) ∩ 血{xue,xie} ≠ ∅ → 命中
        let r = ex("加写", &["加血.ag"], true);
        let w = r.winner.expect("多音字应命中");
        assert_eq!(w.source, MatchSource::Pinyin);
        assert!((w.score - 1.0).abs() < f32::EPSILON);
        // 字符轮仅共享"加" → 0.5
        assert_eq!(
            r.char_best,
            Some(RoundBest {
                index: 0,
                score: 0.5
            })
        );
    }

    #[test]
    fn pinyin_digit_alignment() {
        // normalize 把中文数字转阿拉伯；拼音轮 2→er、二→er（词典），两侧对齐
        let r1 = ex("副本2", &["副本二.ag"], true);
        assert_eq!(r1.winner.unwrap().source, MatchSource::Pinyin);
        assert!((r1.winner.unwrap().score - 1.0).abs() < f32::EPSILON);

        let r2 = ex("副本二", &["副本2.ag"], true);
        assert_eq!(r2.winner.unwrap().source, MatchSource::Pinyin);
        assert!((r2.winner.unwrap().score - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn tie_prefers_char_round() {
        // 两轮同分 1.0（不严格大于）→ 中文优先
        let r = ex("加血", &["加血.ag", "加雪.ag"], true);
        let w = r.winner.unwrap();
        assert_eq!(w.index, 0);
        assert_eq!(w.source, MatchSource::Char);
    }

    #[test]
    fn threshold_blocks_weak_pinyin() {
        // 拼音轮 1/3 < 0.5：即使字符轮无匹配也被下限否决
        let r = ex("雪", &["加加血.ag"], true);
        assert_eq!(r.winner, None);
        assert!(r.pinyin_best.is_some()); // 拼音轮有得分但被否决，日志仍可见
        assert_eq!(r.char_best, None);
    }

    #[test]
    fn threshold_boundary_passes() {
        // 1/2 == 0.5：边界放行
        let r = ex("雪", &["加血.ag"], true);
        let w = r.winner.expect("0.5 == 下限应放行");
        assert_eq!(w.source, MatchSource::Pinyin);
        assert!((w.score - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn mixed_ascii_case_insensitive() {
        // pk/PK 小写对齐 + 同音救回
        let r = ex("pk加雪", &["PK加血.ag"], true);
        let w = r.winner.unwrap();
        assert_eq!(w.source, MatchSource::Pinyin);
        assert!((w.score - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn dict_absent_char_falls_back_to_self() {
        // ★ 不在拼音词典中，应走兜底分支：to_syllable_seq 返回字符自身作为唯一音节
        let syl = to_syllable_seq("★血");
        assert_eq!(syl[0], vec!["★".to_string()]); // 兜底
        // 血的读音集合应包含 xue 或 xie（多音字展开）
        let blood_syl = &syl[1];
        assert!(blood_syl.contains(&"xue".to_string()) || blood_syl.contains(&"xie".to_string()));
        // 整体匹配不应 panic，且 ★ 两侧对齐 → 命中
        let r = ex("★血", &["★血.ag"], true);
        assert!(r.winner.is_some());
    }

    #[test]
    fn pinyin_can_pick_different_script_than_char_round() {
        // 字符轮冠军："雪人"（共享"雪" 0.5，idx 0）
        // 拼音轮冠军："加血"（1.0，idx 1）严格高于字符轮 → 跨脚本胜出
        // 这是同音救回的设计行为：拼音轮冠军可以与字符轮冠军是不同脚本
        let r = ex("加雪", &["雪人.ag", "加血.ag"], true);
        let w = r.winner.expect("应命中");
        assert_eq!(w.index, 1); // 加血
        assert_eq!(w.source, MatchSource::Pinyin);
        // 字符轮冠军仍是"雪人"
        assert_eq!(r.char_best.unwrap().index, 0);
        assert!((r.char_best.unwrap().score - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn pinyin_score_never_below_char_for_same_script() {
        // 不变量：对同一脚本，拼音轮得分恒 ≥ 字符轮得分
        // （每个字符匹配位置必是音节匹配位置，且 base 音节 ≤ base 字符）
        let cases = [
            ("加雪", "加血"),
            ("自动跟随", "蜀门-自动跟随"),
            ("pk加雪", "PK加血"),
            ("副本2", "副本二"),
            ("加写", "加血"),
        ];
        for (action, base) in cases {
            let a_chars: Vec<char> = action.chars().collect();
            let b_chars: Vec<char> = base.chars().collect();
            let char_score = token_score(&a_chars, &b_chars, |a, b| a == b)
                .map(|(_, r)| r)
                .unwrap_or(0.0);
            let a_syl = to_syllable_seq(action);
            let b_syl = to_syllable_seq(base);
            let pin_score = token_score(&a_syl, &b_syl, |a, b| sets_overlap(a, b))
                .map(|(_, r)| r)
                .unwrap_or(0.0);
            assert!(
                pin_score + 1e-6 >= char_score,
                "action={action} base={base}: 拼音得分 {pin_score} < 字符得分 {char_score}（违反不变量）"
            );
        }
    }

    #[test]
    fn multi_dot_extension() {
        // 去扩展名取最后一个点之前；base 内 '.' 在拼音轮跳过
        let r = ex("副本二", &["副本2.1.ag"], true);
        let w = r.winner.unwrap();
        assert_eq!(w.source, MatchSource::Pinyin);
        assert!((w.score - 0.75).abs() < f32::EPSILON); // fu,ben,er 中 3 / base 4 音节
    }
}
