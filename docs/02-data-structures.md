# 核心数据结构

> 本文对齐 `src/` 下的真实代码。所有 Rust 定义均为简化形式，**字段名与类型以代码为准**。

## 1. 窗口槽位 (WindowSlot) 与方案 (Scheme)

**位置**: `src/window_slot.rs`

### 1.1 方案 (Scheme)

绑定到某个窗口的一个方案，引用一份已解析的脚本。

```rust
use crate::script::{Command, ScriptSettings};

/// 一个绑定到窗口的方案（引用某个脚本文件）
#[derive(Clone)]
pub struct Scheme {
    /// 脚本文件名（作为唯一标识，与 ScriptFile.name 对应）
    pub script_name: String,
    /// 解析好的命令（从 ScriptFile 拷贝，执行时用）
    pub commands: Vec<Command>,
    /// 脚本设置项（从 ScriptFile 拷贝）
    pub settings: ScriptSettings,
}
```

### 1.2 窗口槽位 (WindowSlot)

```rust
use crate::runner::Runner;

/// 单个窗口槽位（最多 8 个）
pub struct WindowSlot {
    /// 自定义窗口名（语音指称用，如"窗口1"、"主号"）
    pub name: String,
    /// 目标窗口句柄（isize 形式，便于跨线程传递；None 表示未绑定）
    pub hwnd: Option<isize>,
    /// 窗口标题（用于显示）
    pub title: String,
    /// 该窗口的方案集
    pub schemes: Vec<Scheme>,
    /// 标识方案的索引（默认执行方案），指向 schemes；None 表示未设标识
    pub marked: Option<usize>,
    /// 是否标记为主窗口（鼠标事件转发目标，全局互斥，至多一个）
    pub is_main: bool,
    /// 当前后台运行器（None 表示空闲）
    pub runner: Option<Runner>,
}
```

**字段说明**:

| 字段 | 类型 | 说明 |
|---|---|---|
| `name` | `String` | 用户可编辑的窗口名；空则 UI 回退为"窗口N"。语音指令按此匹配 |
| `hwnd` | `Option<isize>` | 用 `isize` 而非 `HWND`，是为了能安全跨线程传递到执行线程 |
| `schemes` | `Vec<Scheme>` | 该窗口已添加的方案；从全局脚本池按文件名加载 |
| `marked` | `Option<usize>` | 标识方案索引（★）。第一个加入的方案自动成为标识；热键 9 / 单次执行 / 语音动作都跑它 |
| `is_main` | `bool` | 主窗口标记（⚑）。全局互斥，鼠标转发覆盖窗以此为转发目标 |
| `runner` | `Option<Runner>` | 持有后台执行线程；`is_running()` 由它派生，替代了旧的 `ExecutionState` 枚举 |

**关键方法**:

- `is_bound()` / `is_running()` —— 是否已绑定窗口 / 是否正在执行
- `add_scheme()` —— 添加方案（同名跳过）；首个自动设为标识
- `remove_scheme()` —— 移除方案并自动修正 `marked`
- `set_marked()` / `marked_scheme()` —— 设置 / 获取标识方案
- `stop()` —— 停止并 join 当前运行器

> 旧设计中的 `index`、`selected_scheme`、`marked_scheme: usize`、`ExecutionState` 均已移除：标识索引改为可空的 `marked: Option<usize>`；执行状态改为由 `runner` 是否存在 + `Runner::is_running()` 推导。

---

## 2. 脚本 AST (Command 与相关类型)

**位置**: `src/script/ast.rs`

代码里没有独立的 `Script` / `Statement` 包裹类型，顶层就是一个 `Vec<Command>`（见 `ScriptFile.commands` 与 `Scheme.commands`）。条件分支以 `Command::If` 内联表达。

### 2.1 顶层命令枚举

```rust
/// 顶层命令 / 语句
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    // 设置（通常在脚本开头，加载时提取到 ScriptSettings，执行时跳过）
    Setting(Setting),

    // 键盘（按键名以字符串给出，如 "1"、"a"、"f1"、"space"）
    Down(String),              // down(1)
    Up(String),                // up(1)
    Click(String),             // click(2)
    ClickMs(String, u32),      // click_ms(2,50)
    DelayMs(u32),              // delay_ms(500)

    // 窗口消息
    SendWindowActive,          // send_window_active() 发送激活消息

    // 鼠标（坐标统一用 Coord 表达三种定位方式）
    MouseMove(Coord),                // mouse_move / _center / _percent
    MouseDown(MouseButton, Coord),   // mouse_down / _center / _percent
    MouseUp(MouseButton),            // mouse_up(left)
    MouseClick(MouseButton, Coord),  // mouse_click / _center / _percent

    // 条件分支（if / else_if；无独立 else，靠 else_if 中的 Bool 字面量实现）
    If {
        condition: BoolExpr,
        then_block: Vec<Command>,
        else_if_blocks: Vec<(BoolExpr, Vec<Command>)>,
    },
}
```

> 旧设计里的 `Down(Key)` / `MouseClickCenter` / `MouseDownPercent` 等一长串鼠标变体已被合并：坐标定位方式抽成统一的 `Coord` 枚举，按键用字符串名而非 `Key` 枚举。

### 2.2 坐标与鼠标按钮

```rust
/// 坐标定位方式
#[derive(Debug, Clone, PartialEq)]
pub enum Coord {
    /// 窗口客户区绝对坐标
    Absolute { x: i32, y: i32 },
    /// 相对窗口中心点的偏移
    Center { dx: i32, dy: i32 },
    /// 窗口宽/高的百分比 (0-100)
    Percent { px: i32, py: i32 },
}

/// 鼠标按钮
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}
```

### 2.3 条件表达式

```rust
/// 颜色查找区域的定位方式
#[derive(Debug, Clone, PartialEq)]
pub enum FindArea {
    Absolute { x: i32, y: i32, w: i32, h: i32 }, // find_color(x,y,w,h,color)
    Center   { dx: i32, dy: i32, w: i32, h: i32 }, // find_color_center(...)
    Percent  { px: i32, py: i32, w: i32, h: i32 }, // find_color_percent(...)
}

/// 值表达式：颜色查找或布尔字面量
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    FindColor { area: FindArea, color: u32 }, // color 为 0xRRGGBB
    Bool(bool),
}

/// 比较运算符
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompareOp {
    Eq,   // ==
    Ne,   // !=
}

/// 布尔表达式：左值 op 右值
#[derive(Debug, Clone, PartialEq)]
pub struct BoolExpr {
    pub left: Value,
    pub op: CompareOp,
    pub right: Value,
}
```

### 2.4 脚本设置项

```rust
/// 脚本设置项（出现在脚本开头的 setting 指令）
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Setting {
    /// 语音场景下仅执行一次（执行一轮后自动停止）
    AudioOnlyOnce,
}
```

---

## 3. 脚本文件与设置 (ScriptFile / ScriptSettings)

**位置**: `src/script/loader.rs`

全局脚本池在启动时由 `load_dir()` 递归扫描 `scripts/` 目录得到，每个 `.ag` 文件对应一个 `ScriptFile`。

```rust
use std::path::PathBuf;
use crate::script::ast::Command;

/// 脚本设置项（从脚本中的 setting 指令提取）
#[derive(Debug, Clone, Default)]
pub struct ScriptSettings {
    /// 语音场景下仅执行一次
    pub audio_only_once: bool,
}

/// 一个已加载的脚本方案
#[derive(Debug, Clone)]
pub struct ScriptFile {
    /// 文件名（含扩展名），如 "farming.ag"
    pub name: String,
    /// 完整路径
    pub path: PathBuf,
    /// 分类路径（相对脚本根目录的子目录，根目录文件为 "通用"）
    pub category: String,
    /// 原始文本内容（供 UI 浏览）
    pub source: String,
    /// 解析后的命令；解析失败时为 None，错误存到 parse_error
    pub commands: Option<Vec<Command>>,
    /// 解析错误信息
    pub parse_error: Option<String>,
    /// 脚本设置项（从 setting 指令提取）
    pub settings: ScriptSettings,
}
```

**说明**:

- `commands` 为 `Option`：解析失败不致命，UI 仍能展示源码并标注"无效"，只是不能加入方案集
- `ScriptSettings` 是扁平的 `bool` 集合（不是 `Setting` 枚举的列表），由 `extract_settings()` 在加载时从命令流里提取
- 槽位添加方案时，会把 `commands` 和 `settings` 拷贝进 `Scheme`，运行时只读这份拷贝

> 旧设计的 `Scheme { id, display_name, file_path, script: Option<Script> }` 已不存在。运行期方案就是 §1.1 的 `Scheme`，磁盘上的脚本文件就是这里的 `ScriptFile`。

---

## 4. 热键与事件总线

代码里没有单一的 `HotkeyEvent` 枚举。热键链路是：底层按键 → `HotkeyKey` → 经 `MainEvent::Hotkey` 投递 → 状态机产出 `HotkeyAction` → App 执行。

### 4.1 原始热键 (HotkeyKey)

**位置**: `src/hotkey/manager.rs`

```rust
/// 热键原始事件（由热键线程注册的 Ctrl+Shift+<键> 触发）
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HotkeyKey {
    Digit(u8),          // Ctrl+Shift+0~9
    Minus,              // Ctrl+Shift+- （单次执行）
    Insert,             // Ctrl+Shift+Insert （进入发送模式）
    Letter(char),       // Ctrl+Shift+A-Z
    FKey(u8),           // Ctrl+Shift+F1-F12
    Special(SpecialKey),// Ctrl+Shift+Space/Enter/Tab/Escape
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpecialKey { Space, Enter, Tab, Escape }
```

### 4.2 热键动作 (HotkeyAction)

**位置**: `src/hotkey/state_machine.rs`

`HotkeyStateMachine` 消费 `HotkeyKey`，结合当前选择集与发送模式，产出高层动作：

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum HotkeyAction {
    StartWindows(Vec<u8>),   // 循环启动指定窗口（1-8）的标识方案
    StopWindows(Vec<u8>),    // 停止指定窗口
    StartAll,                // 循环启动所有已绑定窗口
    StopAll,                 // 停止所有窗口
    RunOnceWindows(Vec<u8>), // 单次执行指定窗口的标识方案
    RunOnceAll,              // 单次执行所有已绑定窗口
    SendKey { windows: Vec<u8>, key_name: String }, // 即兴发送单个按键
}
```

### 4.3 主事件 (MainEvent)

**位置**: `src/event_bus.rs`

所有后台事件源（热键 / 托盘 / 语音 / 覆盖窗）统一走 `MainEventBus`，投递时自动 `PostMessage(WM_PAINT)` 唤醒主窗口，因此隐藏到托盘后仍即时响应。

```rust
pub enum MainEvent {
    Tray(TrayCommand),     // 托盘：显示 / 退出
    Hotkey(HotkeyKey),     // 热键原始按键
    Voice(VoiceEvent),     // 语音：唤醒 / 识别结果 / 状态 / 错误
    Overlay(OverlayEvent), // 鼠标转发覆盖窗：目标失效 / 请求关闭
}
```

---

## 5. 脚本运行器 (Runner)

**位置**: `src/runner.rs`

> 旧设计里的 `RunnerCommand { Start(Arc<Script>), Stop }` 消息枚举已不存在。当前实现中，`Runner` 是每个槽位各自持有的后台线程句柄，通过方法直接控制（内部用 `AtomicBool` 停止标志 + `JoinHandle` 管理），不走消息通道。

```rust
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread::JoinHandle;

pub struct Runner {
    stop_flag: Arc<AtomicBool>,      // 停止信号
    handle: Option<JoinHandle<()>>,  // 执行线程句柄
}
```

**启动方式**（按"循环/单次" × "是否延迟"组合）:

| 方法 | 行为 |
|---|---|
| `Runner::start(hwnd, commands)` | 循环执行，跑完一轮自动重来 |
| `Runner::start_once(hwnd, commands)` | 单次执行，跑完一轮即停 |
| `Runner::start_delayed(hwnd, commands, delay_ms)` | 循环执行，延迟若干毫秒后开始（热键触发用，给用户松开修饰键的时间） |
| `Runner::start_once_delayed(...)` | 单次执行 + 延迟 |

- `is_running()` —— 是否仍在跑（停止标志未置且线程未结束）
- `stop()` —— 请求停止（非阻塞）
- `stop_and_join()` —— 停止并等待线程结束
- `Drop` 自动 `stop_and_join()`，确保线程不会泄漏

执行线程内部构造 `PostMessageBackend` 作为输入后端，每条命令前检查停止标志；`DelayMs` 被切成 10ms 分段以便及时响应停止。

---

## 6. 持久化配置 (AppConfig)

**位置**: `src/config.rs`

只存"方案绑定意图"，不存运行时状态（HWND 等）。重启后方案配置自动恢复，脚本命令按文件名从当前脚本池重新加载（以磁盘最新内容为准），窗口需用户手动重新抓取。

### 6.1 槽位配置

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SlotConfig {
    /// 自定义窗口名（语音指称用）。空则 UI 显示默认名"窗口N"
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
```

> 这里的 `marked` / `is_main` 与 §1.2 `WindowSlot` 同名字段一一对应：保存时从 `WindowSlot` 拷出，加载时写回 `WindowSlot`。`AppConfig::normalize()` 会修正越界的 `marked`，并强制全局只保留第一个 `is_main`。

### 6.2 顶层配置

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub slots: Vec<SlotConfig>, // 固定 8 个
    #[serde(default)]
    pub baidu: BaiduConfig,
    #[serde(default)]
    pub hotkey: HotkeyConfig,
    #[serde(default)]
    pub general: GeneralConfig,
}
```

`AppConfig::default()` 产出 8 个默认 `SlotConfig`；`load()` 在文件缺失/损坏时回退默认值并调用 `normalize()`。

### 6.3 语音 / 热键 / 通用配置

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BaiduConfig {
    #[serde(default)] pub api_key: String,
    #[serde(default)] pub secret_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    /// 热键总开关（禁用后所有热键不响应）
    #[serde(default = "default_true")] pub enabled: bool,
    /// 即兴发送热键开关（单独控制 Ctrl+Shift+Insert）
    #[serde(default = "default_true")] pub impromptu_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    /// 日志文件开关（禁用后不写入 voice_debug.log）
    #[serde(default = "default_true")] pub log_enabled: bool,
    /// 唤醒词训练样本保存开关（启用后写入 wakeword_samples/）
    #[serde(default)] pub save_wakeword_samples: bool,
    /// ASR 音频保存开关（启用后写入 sendvoice/）
    #[serde(default)] pub save_asr_audio: bool,
    /// 拼音辅助匹配开关（字符匹配之外再做一轮忽略声调的拼音匹配，取更优结果）
    #[serde(default)] pub pinyin_assist: bool,
}
```

`HotkeyConfig` / `GeneralConfig` 的 `Default` 实现把 `default_true` 字段初始化为 `true`，其余 bool 为 `false`。

---

## 7. 主应用状态 (App)

**位置**: `src/app/mod.rs`（App 结构体、`new`、配置读写与 `update` 编排；各业务方法分见 `app/` 下 `events` / `slots` / `overlay` / `voice_ctrl` / `wakeword_train` / `grab` / `ui` 子模块）

> 旧设计里的 `AutoKeyboardApp` 已重命名为 `App`。它不再持有 `SchemeManager` / `ExecutorManager` / `InputManager`：脚本池直接以 `Vec<ScriptFile>` 形式存放；每个槽位自带 `Option<Runner>`（见 §1.2、§5），不再有独立的执行器管理器。

```rust
pub struct App {
    // 统一事件总线（托盘/热键/语音/覆盖窗 都往这里投事件，自动唤醒主窗口）
    events: MainEventBus,

    // 全局脚本候选池
    scripts: Vec<ScriptFile>,
    scripts_dir: PathBuf,

    // 8 个窗口槽位
    slots: Vec<WindowSlot>,

    // 抓取窗口 / 取色倒计时
    grabbing_slot: Option<usize>,
    grabbing_since: Option<Instant>,
    picking_since: Option<Instant>,
    last_validity_check: Instant,

    // UI 状态
    viewing_script: Option<usize>,
    adding_scheme_for: Option<usize>,
    show_hotkey_help: bool,
    show_settings: bool,
    settings_tab: SettingsTab,
    show_voice_help: bool,
    show_baidu_guide: bool,
    show_wakeword_guide: bool,
    wakeword_training: Option<WakewordTrainingState>,
    status: String,

    // 取色器
    color_picker: ColorPicker,

    // 热键：_hotkey_mgr 仅保生命周期（drop 注销全局热键），事件走总线
    _hotkey_mgr: Option<HotkeyManager>,
    hotkey_sm: HotkeyStateMachine,

    // 托盘 / 退出控制
    tray: Option<Tray>,
    quitting: bool,
    wake_pending: u8,

    // 语音控制运行时 / 鼠标转发覆盖窗
    voice: Option<VoiceRuntime>,
    overlay: Option<OverlayWindow>,

    // 从 config 加载、供 UI 编辑的配置镜像
    baidu_api_key: String,
    baidu_secret_key: String,
    last_voice_text: String,
    hotkey_enabled: bool,
    hotkey_impromptu_enabled: bool,
    log_enabled: bool,
    save_wakeword_samples: bool,
    save_asr_audio: bool,
    pinyin_assist: bool,
}
```

**说明**:

- 配置项在 App 里以独立 bool/String 字段镜像存放（供 egui 直接可变借用编辑）；保存时由 `save_config()` 重新组装回 `AppConfig`
- 输入后端：`Runner` 内部直接 `PostMessageBackend::new()`，不走任何管理器
- `InputManager`（定义于 `src/input/mod.rs`，含多后端切换能力）目前**预留 / 未接线**到 App；当前唯一后端是 `PostMessageBackend`（基于 `PostMessage` 的后台发送），`SendInput` 这类前台注入后端尚未实现

---

## 数据流示意

### 启动方案流程（热键触发）

```
用户按 Ctrl+Shift+9
    ↓
HotkeyManager 线程收到 WM_HOTKEY
    ↓
EventSender.send(MainEvent::Hotkey(HotkeyKey::Digit(9)))
   （入队后 PostMessage(WM_PAINT) 唤醒主窗口）
    ↓
App.dispatch_events() → App.handle_hotkey(HotkeyKey::Digit(9))
    ↓
HotkeyStateMachine.on_start() → HotkeyAction::StartWindows([2])（或 StartAll）
    ↓
App.apply_action() → App.start_slot(1)
    ↓
读 WindowSlot[1].marked → 取标识方案 Scheme
    ↓
WindowSlot[1].stop()（先停旧的）→ Runner::start_delayed(hwnd, commands, 1000)
    ↓
执行线程：每条 Command 前检查 stop_flag，循环直到停止
```

### 脚本执行流程（执行线程内）

```
Runner 线程 spawn
    ↓
构造 PostMessageBackend + HWND
    ↓
loop {
    ScriptExecutor.execute_interruptible(&commands, &stop_flag)
        ↓
    遍历 commands
        ↓
    匹配 Command 类型：
        Setting(_)        → 跳过（加载时已提取）
        Down/Up/Click...  → InputBackend.send_key_down/up()
        MouseMove/Down/Up/Click → resolve_coord(Coord) → InputBackend.send_mouse_xxx()
        SendWindowActive  → InputBackend.send_window_active()
        DelayMs(ms)       → 分段 sleep（每 10ms 检查停止标志）
        If { .. }         → eval BoolExpr → 命中则执行 then_block / else_if 块
        ↓
    每条命令前检查 stop_flag
    ↓
    单次模式 / 空脚本 → 退出；否则循环回到开头
}
```
