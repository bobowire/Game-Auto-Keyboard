// 配置持久化 - 只存方案绑定意图，不存运行时状态（HWND 等）
//
// 重启后：方案配置自动恢复，脚本命令按文件名从当前脚本池重新加载
// （以磁盘最新内容为准），窗口需用户手动重新抓取。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const CONFIG_FILENAME: &str = "config.json";

/// 获取配置文件的完整路径（基于可执行文件目录）
fn get_config_path() -> PathBuf {
    if let Ok(exe_dir) = crate::utils::get_exe_dir() {
        exe_dir.join(CONFIG_FILENAME)
    } else {
        // 降级方案：使用当前目录
        PathBuf::from(CONFIG_FILENAME)
    }
}

/// 单个槽位的持久化配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SlotConfig {
    /// 自定义窗口名（语音指称用，如"窗口1"、"主号"）。空则显示默认名
    #[serde(default)]
    pub name: String,
    /// 该槽位绑定的方案脚本文件名列表（顺序即显示顺序）
    #[serde(default)]
    pub scheme_names: Vec<String>,
    /// 标识方案在 scheme_names 中的索引
    #[serde(default)]
    pub marked: Option<usize>,
    /// 是否标记为主窗口（鼠标事件转发目标，全局互斥，至多一个）
    #[serde(default)]
    pub is_main: bool,
}

/// 百度语音识别 API 配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BaiduConfig {
    /// API Key（在百度智能云控制台申请）
    #[serde(default)]
    pub api_key: String,
    /// Secret Key
    #[serde(default)]
    pub secret_key: String,
}

/// 热键配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    /// 热键总开关（禁用后所有热键不响应）
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 即兴发送热键开关（单独控制 Ctrl+Shift+Insert）
    #[serde(default = "default_true")]
    pub impromptu_enabled: bool,
}

/// 鼠标转发覆盖窗的消息转发配置
///
/// 三个开关默认全关（`#[serde(default)]`，旧 config.json 缺失字段走 false）。
/// 注意：`rbutton_broadcast_move` 默认关意味着右键拖动期间默认不广播移动
/// （仅右键按下期间抑制 WM_MOUSEMOVE；右键的按下/弹起仍照常转发）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ForwardConfig {
    /// 右键按下时是否广播鼠标移动消息（关 = 右键拖动期间不广播移动，规避视角反馈环）
    #[serde(default)]
    pub rbutton_broadcast_move: bool,
    /// 是否广播键盘消息（覆盖窗焦点期间的按键转发给目标窗口）
    #[serde(default)]
    pub keyboard_broadcast: bool,
    /// 键盘消息是否只广播给标记（主窗口）窗口；false = 广播给全部绑定窗口
    #[serde(default)]
    pub keyboard_marked_only: bool,
}

impl Default for ForwardConfig {
    fn default() -> Self {
        Self {
            rbutton_broadcast_move: false,
            keyboard_broadcast: false,
            keyboard_marked_only: false,
        }
    }
}

/// 通用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    /// 日志文件开关（禁用后不写入 vlog.txt）
    #[serde(default = "default_true")]
    pub log_enabled: bool,
    /// 唤醒词训练样本保存开关（禁用后不创建 wakeword_samples 目录）
    #[serde(default)]
    pub save_wakeword_samples: bool,
    /// ASR 音频保存开关（启用后将发送给 ASR 的音频保存到 sendvoice 目录）
    #[serde(default)]
    pub save_asr_audio: bool,
    /// 拼音辅助匹配开关（字符匹配之外再做一轮忽略声调的拼音匹配，取更优结果）
    #[serde(default)]
    pub pinyin_assist: bool,
}

fn default_true() -> bool {
    true
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            impromptu_enabled: true,
        }
    }
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            log_enabled: true,
            save_wakeword_samples: false,
            save_asr_audio: false,
            pinyin_assist: false,
        }
    }
}

/// 应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// 8 个槽位的配置
    #[serde(default)]
    pub slots: Vec<SlotConfig>,
    /// 百度语音识别配置
    #[serde(default)]
    pub baidu: BaiduConfig,
    /// 热键配置
    #[serde(default)]
    pub hotkey: HotkeyConfig,
    /// 通用配置
    #[serde(default)]
    pub general: GeneralConfig,
    /// 鼠标转发配置
    #[serde(default)]
    pub forward: ForwardConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            slots: vec![SlotConfig::default(); 8],
            baidu: BaiduConfig::default(),
            hotkey: HotkeyConfig::default(),
            general: GeneralConfig::default(),
            forward: ForwardConfig::default(),
        }
    }
}

impl AppConfig {
    /// 从默认路径加载；文件不存在或损坏时返回默认配置
    pub fn load() -> Self {
        Self::load_from(&get_config_path())
    }

    pub fn load_from(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(content) => match serde_json::from_str::<AppConfig>(&content) {
                Ok(mut cfg) => {
                    cfg.normalize();
                    cfg
                }
                Err(e) => {
                    eprintln!("配置解析失败，使用默认配置: {}", e);
                    AppConfig::default()
                }
            },
            Err(_) => AppConfig::default(),
        }
    }

    /// 保存到默认路径
    pub fn save(&self) -> Result<(), String> {
        self.save_to(&get_config_path())
    }

    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        // 确保父目录存在
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建配置目录失败: {}", e))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("序列化配置失败: {}", e))?;
        std::fs::write(path, json)
            .map_err(|e| format!("写入配置失败: {}", e))?;
        Ok(())
    }

    /// 保证 slots 长度恰好为 8，修正越界的 marked
    fn normalize(&mut self) {
        self.slots.resize(8, SlotConfig::default());
        for slot in &mut self.slots {
            if let Some(m) = slot.marked {
                if m >= slot.scheme_names.len() {
                    slot.marked = if slot.scheme_names.is_empty() {
                        None
                    } else {
                        Some(0)
                    };
                }
            } else if !slot.scheme_names.is_empty() {
                slot.marked = Some(0);
            }
        }
        // 主窗口标记全局互斥：只保留第一个
        let mut seen_main = false;
        for slot in &mut self.slots {
            if slot.is_main {
                if seen_main {
                    slot.is_main = false;
                } else {
                    seen_main = true;
                }
            }
        }
    }

    /// 便捷：返回配置文件路径
    pub fn path() -> PathBuf {
        get_config_path()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinyin_assist_default_and_roundtrip() {
        // 旧配置（无 pinyin_assist 字段）应反序列化为 false（#[serde(default)]）
        let old = serde_json::from_str::<GeneralConfig>(
            r#"{"log_enabled":true,"save_wakeword_samples":false,"save_asr_audio":false}"#,
        )
        .unwrap();
        assert!(!old.pinyin_assist);

        // 往返序列化应保留 pinyin_assist = true
        let cfg = GeneralConfig {
            log_enabled: true,
            save_wakeword_samples: false,
            save_asr_audio: true,
            pinyin_assist: true,
        };
        let s = serde_json::to_string(&cfg).unwrap();
        let back: GeneralConfig = serde_json::from_str(&s).unwrap();
        assert!(back.pinyin_assist);
    }

    #[test]
    fn forward_config_default_and_roundtrip() {
        // 旧配置（无 forward 段）应反序列化为全 false（#[serde(default)]）
        let old = serde_json::from_str::<AppConfig>(r#"{"slots":[]}"#).unwrap();
        assert!(!old.forward.rbutton_broadcast_move);
        assert!(!old.forward.keyboard_broadcast);
        assert!(!old.forward.keyboard_marked_only);

        // 往返序列化应保留 true
        let cfg = ForwardConfig {
            rbutton_broadcast_move: true,
            keyboard_broadcast: true,
            keyboard_marked_only: false,
        };
        let s = serde_json::to_string(&cfg).unwrap();
        let back: ForwardConfig = serde_json::from_str(&s).unwrap();
        assert!(back.rbutton_broadcast_move);
        assert!(back.keyboard_broadcast);
        assert!(!back.keyboard_marked_only);
    }

    #[test]
    fn slot_config_is_main_default_false() {
        // 旧配置（无 is_main 字段）应反序列化为 false（#[serde(default)]）
        let sc = serde_json::from_str::<SlotConfig>(r#"{"name":"主号"}"#).unwrap();
        assert!(!sc.is_main);
    }

    #[test]
    fn normalize_keeps_only_first_is_main() {
        let mut cfg = AppConfig::default();
        cfg.slots[1].is_main = true;
        cfg.slots[5].is_main = true;
        cfg.normalize();
        assert!(!cfg.slots[0].is_main);
        assert!(cfg.slots[1].is_main);
        assert!(!cfg.slots[5].is_main);
    }
}
