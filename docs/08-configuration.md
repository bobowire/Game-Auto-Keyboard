# 配置管理

应用所有可持久化的设置都集中在一个 `config.json` 文件里。唯一事实来源是 `src/config.rs`，本文档与之对齐。

配置只存"意图"，不存运行时状态（如窗口 HWND）。重启后：方案绑定、标识方案、主窗口标记、各开关都会自动恢复；窗口句柄需要用户重新抓取。

---

## 配置文件位置

- 文件名固定为 `config.json`。
- 目录为**可执行文件所在目录**（通过 `utils::get_exe_dir()` 取得）。
- 取不到 exe 目录时降级到当前工作目录。

```rust
// src/config.rs
const CONFIG_FILENAME: &str = "config.json";

fn get_config_path() -> PathBuf {
    if let Ok(exe_dir) = crate::utils::get_exe_dir() {
        exe_dir.join(CONFIG_FILENAME)
    } else {
        PathBuf::from(CONFIG_FILENAME) // 降级：当前目录
    }
}
```

---

## 配置文件结构

根对象 `AppConfig` 包含四块：`slots`（8 个窗口槽位）、`baidu`（语音识别）、`hotkey`（热键）、`general`（通用开关）。

```json
{
  "slots": [
    {
      "name": "主号",
      "scheme_names": ["farming.ag", "combat.ag"],
      "marked": 0,
      "is_main": true
    },
    {
      "name": "",
      "scheme_names": ["fishing.ag"],
      "marked": 0,
      "is_main": false
    },
    { "name": "", "scheme_names": [], "marked": null, "is_main": false },
    { "name": "", "scheme_names": [], "marked": null, "is_main": false },
    { "name": "", "scheme_names": [], "marked": null, "is_main": false },
    { "name": "", "scheme_names": [], "marked": null, "is_main": false },
    { "name": "", "scheme_names": [], "marked": null, "is_main": false },
    { "name": "", "scheme_names": [], "marked": null, "is_main": false }
  ],
  "baidu": {
    "api_key": "",
    "secret_key": ""
  },
  "hotkey": {
    "enabled": true,
    "impromptu_enabled": true
  },
  "general": {
    "log_enabled": true,
    "save_wakeword_samples": false,
    "save_asr_audio": false,
    "pinyin_assist": false
  },
  "forward": {
    "rbutton_broadcast_move": false,
    "keyboard_broadcast": false,
    "keyboard_marked_only": false
  }
}
```

说明：

- `slots` 恰好 8 个元素，顺序即窗口 1~8。
- 每个槽位的四个字段都会被写出（无 `skip_serializing_if`），即便值为空。
- `marked` 是 `Option<usize>`：无标识方案时写 `null`，否则写其在 `scheme_names` 中的索引。
- 窗口名等于默认"窗口N"时，保存阶段会写空串（见下文"运行时映射"），保持 JSON 干净。

---

## 字段说明与默认值

所有字段都带 `#[serde(default)]`，因此**旧配置缺字段时不会报错**，缺失部分按类型默认值（`""`、`0`、`[]`、`null`、`false`）补齐；布尔开关字段另注明了真实默认。

### SlotConfig（单槽位）

| 字段 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `name` | `String` | `""` | 自定义窗口名（语音指称用，如"主号"）。为空则运行时显示默认名"窗口N" |
| `scheme_names` | `Vec<String>` | `[]` | 该槽位绑定的方案脚本文件名列表，顺序即显示顺序 |
| `marked` | `Option<usize>` | `null` | 标识方案在 `scheme_names` 中的索引；`null` 表示无标识方案 |
| `is_main` | `bool` | `false` | 主窗口标记。全局互斥，至多一个（鼠标事件转发目标） |

### BaiduConfig（百度语音识别 API）

| 字段 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `api_key` | `String` | `""` | 百度智能云控制台申请的 API Key |
| `secret_key` | `String` | `""` | Secret Key |

### HotkeyConfig（热键）

| 字段 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `enabled` | `bool` | `true` | 热键总开关，禁用后所有热键不响应 |
| `impromptu_enabled` | `bool` | `true` | 即兴发送热键开关（单独控制 Ctrl+Shift+Insert） |

### GeneralConfig（通用）

| 字段 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `log_enabled` | `bool` | `true` | 日志文件开关，禁用后不写入 `vlog.txt` |
| `save_wakeword_samples` | `bool` | `false` | 唤醒词训练样本保存开关，启用后写入 `wakeword_samples` 目录 |
| `save_asr_audio` | `bool` | `false` | ASR 音频保存开关，启用后将发送给 ASR 的音频保存到 `sendvoice` 目录 |
| `pinyin_assist` | `bool` | `false` | 拼音辅助匹配开关，启用后在字符匹配之外再做一轮忽略声调的拼音匹配，取更优结果 |

### ForwardConfig（鼠标转发覆盖窗）

| 字段 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `rbutton_broadcast_move` | `bool` | `false` | 右键按下时是否广播鼠标移动。关 = 右键拖动期间不向任何窗口转发 `WM_MOUSEMOVE`（规避右键拖视角反馈环）；右键按下/弹起仍转发。⚠️ 默认关，相比旧版「总是广播移动」是行为变化 |
| `keyboard_broadcast` | `bool` | `false` | 键盘消息转发开关，启用后覆盖窗持焦期间的按键转发给目标窗口（Ctrl+Q 仍为关闭快捷键，不转发） |
| `keyboard_marked_only` | `bool` | `false` | 键盘是否只发给 ⚑ 主窗口；`false` = 广播给全部绑定窗口。鼠标消息不受此开关影响 |

---

## 配置加载与持久化

**位置**: `src/config.rs`

### 加载（容错）

`AppConfig::load()` 从默认路径读取。文件不存在或读取失败时**静默回退默认配置**；解析失败时打印到 stderr 并回退默认配置。无论哪条路径，最终都会过一遍 `normalize()`。

```rust
pub fn load_from(path: &Path) -> Self {
    match std::fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str::<AppConfig>(&content) {
            Ok(mut cfg) => { cfg.normalize(); cfg }
            Err(e) => {
                eprintln!("配置解析失败，使用默认配置: {}", e);
                AppConfig::default()
            }
        },
        Err(_) => AppConfig::default(),
    }
}
```

`AppConfig::default()` 会生成 8 个全空槽位、各开关取真实默认值（`enabled`/`impromptu_enabled`/`log_enabled` 为 `true`，其余为 `false`）的配置。

### 保存

`AppConfig::save()` 用 `serde_json::to_string_pretty` 写回默认路径，输出即上文示例那种带缩进的 JSON。写前会确保父目录存在。

```rust
pub fn save_to(&self, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {}", e))?;
    }
    let json = serde_json::to_string_pretty(self).map_err(|e| format!("序列化配置失败: {}", e))?;
    std::fs::write(path, json).map_err(|e| format!("写入配置失败: {}", e))?;
    Ok(())
}
```

保存失败仅在调用处打印（`App::save_config` 内 `eprintln!("保存配置失败: {}", e)`），不弹窗、不中断。

---

## normalize 规则

`normalize()` 在加载后强制整理 `slots`，保证运行时不变量：

1. **长度补齐**：`slots.resize(8, SlotConfig::default())`，不足补空槽，多余截断。
2. **修正越界 `marked`**：逐槽位检查。
   - 若 `marked = Some(m)` 且 `m >= scheme_names.len()`：`scheme_names` 为空则改写为 `None`，否则改写为 `Some(0)`。
   - 若 `marked = None` 但 `scheme_names` 非空：补成 `Some(0)`。
3. **`is_main` 全局互斥**：自上而下扫描，只保留第一个 `is_main = true` 的槽位，其后全部改写为 `false`。

```rust
fn normalize(&mut self) {
    self.slots.resize(8, SlotConfig::default());
    for slot in &mut self.slots {
        if let Some(m) = slot.marked {
            if m >= slot.scheme_names.len() {
                slot.marked = if slot.scheme_names.is_empty() { None } else { Some(0) };
            }
        } else if !slot.scheme_names.is_empty() {
            slot.marked = Some(0);
        }
    }
    // 主窗口标记全局互斥：只保留第一个
    let mut seen_main = false;
    for slot in &mut self.slots {
        if slot.is_main {
            if seen_main { slot.is_main = false; } else { seen_main = true; }
        }
    }
}
```

---

## 运行时与持久化的映射

运行时的窗口槽位 `WindowSlot`（含 HWND 等不可持久化状态）位于 `src/app.rs`。`App::save_config` 与 `App::new` 负责 `WindowSlot` 与 `SlotConfig` 之间的双向映射。

### 保存：`App::save_config`（src/app.rs，约 286 行）

从默认 `AppConfig` 起步，逐槽位填入运行时数据后调用 `cfg.save()`。注意两点：

- **窗口名归一**：当运行时名等于默认 `"窗口N"` 时存空串，避免 JSON 里出现冗余的默认名。
- **方案按文件名存**：只存 `script_name`（脚本文件名），命令本身不入配置——重启后按文件名从脚本池重新加载最新内容。

```rust
fn save_config(&self) {
    let mut cfg = AppConfig::default();
    for (i, slot) in self.slots.iter().enumerate() {
        // 与默认"窗口N"相同则存空串，保持配置干净
        cfg.slots[i].name = if slot.name == format!("窗口{}", i + 1) {
            String::new()
        } else {
            slot.name.clone()
        };
        cfg.slots[i].scheme_names = slot.schemes.iter().map(|s| s.script_name.clone()).collect();
        cfg.slots[i].marked = slot.marked;
        cfg.slots[i].is_main = slot.is_main;
    }
    cfg.baidu.api_key = self.baidu_api_key.clone();
    cfg.baidu.secret_key = self.baidu_secret_key.clone();
    cfg.hotkey.enabled = self.hotkey_enabled;
    cfg.hotkey.impromptu_enabled = self.hotkey_impromptu_enabled;
    cfg.general.log_enabled = self.log_enabled;
    cfg.general.save_wakeword_samples = self.save_wakeword_samples;
    cfg.general.save_asr_audio = self.save_asr_audio;
    cfg.general.pinyin_assist = self.pinyin_assist;

    vlog::set_enabled(self.log_enabled); // 同步日志开关到 vlog 模块

    if let Err(e) = cfg.save() {
        eprintln!("保存配置失败: {}", e);
    }
}
```

### 恢复：`App::new`（src/app.rs，约 178 行）

加载配置后按槽位还原。窗口名：配置里为空则用默认 `"窗口N"`，非空则覆盖。方案按文件名从当前脚本池匹配重建命令；脚本文件已不存在的静默跳过。`marked` 在此再做一次越界保护（与 `normalize` 一致）。

```rust
let config = AppConfig::load();
let mut slots = Vec::with_capacity(SLOT_COUNT);
for i in 0..SLOT_COUNT {
    let mut slot = WindowSlot::default();
    slot.name = format!("窗口{}", i + 1); // 默认名，配置里有非空自定义名则覆盖
    if let Some(sc) = config.slots.get(i) {
        if !sc.name.trim().is_empty() {
            slot.name = sc.name.clone();
        }
        for name in &sc.scheme_names {
            if let Some(sf) = scripts.iter().find(|s| &s.name == name) {
                if let Some(cmds) = &sf.commands {
                    slot.schemes.push(Scheme {
                        script_name: sf.name.clone(),
                        commands: cmds.clone(),
                        settings: sf.settings.clone(),
                    });
                }
            }
            // 脚本文件已不存在则静默跳过
        }
        slot.marked = match sc.marked {
            Some(m) if m < slot.schemes.len() => Some(m),
            _ if !slot.schemes.is_empty() => Some(0),
            _ => None,
        };
        slot.is_main = sc.is_main;
    }
    slots.push(slot);
}
```

百度、热键、通用等字段随后从同一份 `config` 拷进 `App` 的各字段；`log_enabled` 还会通过 `vlog::set_enabled` 同步到日志模块。
