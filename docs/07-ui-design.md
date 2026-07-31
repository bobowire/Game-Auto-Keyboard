# UI 设计

> 本文描述的是 egui 0.29 即时模式 GUI 的实际实现。全部 UI 代码集中在 `src/app.rs`（约 2300+ 行），**没有独立的 `src/ui/` 目录**。槽位数据结构在 `src/window_slot.rs`，取色器在 `src/color_picker.rs`，鼠标转发覆盖窗在 `src/overlay.rs`。

## 整体布局

主窗口由一个顶部工具行 + 槽位滚动区 + 底部状态栏组成，并在需要时叠加多个 egui 弹窗与一个独立的取色器 viewport：

```
┌─────────────────────────────────────────────────────────────┐
│ 窗口方案管理  🔄重载  🎨取色  🎤语音:关  🖱转发:关  ⚙设置 │ ▶全部启动  ⏹全部停止 │
├─────────────────────────────────────────────────────────────┤
│ 热键: Ctrl+Shift+[1-8] 选窗口 → +9 循环启动 / +0 停止 …    │
├─────────────────────────────────────────────────────────────┤
│ ┌─ Frame::group ──────────────────────────────────────────┐ │
│ │ ⚑  1. [窗口名____]  ● 运行中  [抓取窗口]                │ │
│ │ 目标: GameWindow (monospace, 浅蓝)                      │ │
│ │ 方案: [+ 添加方案]                                      │ │
│ │     ★  自动采集        [查看] [移除]                    │ │
│ │     ☆  自动打怪        [查看] [移除]                    │ │
│ │ [▶ 启动标识方案]  [⏹ 停止]                              │ │
│ └─────────────────────────────────────────────────────────┘ │
│ ┌─ 槽位 2 (未绑定窗口) ───────────────────────────────────┐ │
│ │ ⚑  2. [窗口2___]  ○ 空闲  [抓取窗口]                    │ │
│ │ …                                                       │ │
│ └─────────────────────────────────────────────────────────┘ │
│ …（共 8 个槽位，ScrollArea::vertical）                       │
├─────────────────────────────────────────────────────────────┤
│ 状态栏：取色倒计时 / 发送模式 / 已选窗口集 / self.status     │
└─────────────────────────────────────────────────────────────┘
```

底部状态栏之上还可能浮起：右侧脚本源码面板、添加方案弹窗、热键/语音/百度/唤醒词帮助弹窗、设置弹窗、取色器 viewport。

---

## 主应用 UI

**位置**: `src/app.rs`，`impl eframe::App` 的 `update`（约 1108 行起）

每帧的执行顺序固定：先把后台事件从总线分发进来，再驱动时间相关逻辑，最后画各面板与弹窗。

```rust
fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    self.capture_main_hwnd(_frame);
    self.pump_pending_wake();
    // 所有后台事件（托盘/热键/语音）统一从总线取出分发
    self.dispatch_events(ctx);
    self.process_wakeword_training();
    self.handle_grabbing();      // 抓取窗口倒计时
    self.handle_picking();       // 取色倒计时
    self.check_window_validity();

    // 拦截关闭：点 X 时隐藏到托盘而非退出
    if ctx.input(|i| i.viewport().close_requested()) {
        if self.tray.is_some() && !self.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }

    // 时间驱动逻辑兜底（事件总线已会主动唤醒）
    ctx.request_repaint_after(std::time::Duration::from_millis(30));

    // 依次绘制各面板/弹窗
    self.ui_bottom_status(ctx);
    self.ui_source_panel(ctx);
    self.ui_add_scheme_window(ctx);
    self.ui_hotkey_help_window(ctx);
    self.ui_settings_window(ctx);
    self.ui_voice_help_window(ctx);
    self.ui_baidu_guide_window(ctx);
    self.ui_wakeword_guide_window(ctx);
    self.color_picker.ui(ctx);   // 独立 egui viewport
    self.ui_central(ctx);        // 主面板（工具行 + 槽位）
}
```

> 注意：抓取/取色倒计时、窗口有效性检查都依赖这里 `request_repaint_after(30ms)` 的兜底重绘，不能去掉。

---

## 主面板 ui_central

**位置**: `src/app.rs`，`ui_central`（约 1999 行起）

一个 `egui::CentralPanel`，自上而下依次是：顶部工具行 → 热键提示 → 槽位滚动区。所有按钮的副作用都先收进局部 `act_*` 变量，等借用结束后统一回放，避免在借用 `self.slots` 时修改 `self`。

### 顶部工具行

按钮**严格按以下顺序**排列（`ui.horizontal`）：

| # | 控件 | 类型 | 说明 |
|---|------|------|------|
| 0 | `窗口方案管理` | `ui.heading` | 标题，不是按钮 |
| 1 | `🔄 重载脚本` | `ui.button` | 重新扫描 scripts/ 目录 |
| 2 | `🎨 取色` | `ui.button` | 启动 3 秒取色倒计时，随后打开取色器 |
| 3 | `🎤 语音: 开` / `🎤 语音: 关` | `ui.button(RichText)` | **运行时着色按钮**：开启时文字绿色、关闭时灰色；点击切换语音控制 |
| 4 | `🖱 转发: 开` / `🖱 转发: 关` | `ui.button(RichText)` | **运行时着色按钮**：同上范式；启动/停止鼠标转发覆盖窗（见文末） |
| 5 | `⚙ 设置` | `ui.button` | 打开设置弹窗（`show_settings = true`） |
| — | `ui.separator` | 分隔线 | 视觉分组 |
| 6 | `▶ 全部启动` | `ui.button` | 启动所有有标识方案的槽位 |
| 7 | `⏹ 全部停止` | `ui.button` | 停止所有运行中的槽位 |

> "语音"和"转发"两个开关**不是 checkbox**，而是带颜色的普通按钮：开则绿、关则灰，每帧根据 `self.voice.is_some()` / `self.overlay.is_some()` 重新决定文字与颜色。鼠标转发的悬浮提示为"用半透明覆盖窗盖住主窗口客户区，把鼠标操作转发给主窗口"。

```rust
ui.horizontal(|ui| {
    ui.heading("窗口方案管理");
    if ui.button("🔄 重载脚本").clicked() { act_reload = true; }
    if ui.button("🎨 取色").clicked() { act_pick = true; }

    // 语音控制开关（运行时着色按钮范式）
    let voice_on = self.voice.is_some();
    let voice_label = if voice_on { "🎤 语音: 开" } else { "🎤 语音: 关" };
    if ui.button(egui::RichText::new(voice_label)
            .color(if voice_on { egui::Color32::GREEN } else { egui::Color32::GRAY }))
        .on_hover_text("开启/关闭语音控制")
        .clicked() { act_toggle_voice = true; }

    // 鼠标转发开关（同上范式）
    let overlay_on = self.overlay.is_some();
    let overlay_label = if overlay_on { "🖱 转发: 开" } else { "🖱 转发: 关" };
    if ui.button(egui::RichText::new(overlay_label)
            .color(if overlay_on { egui::Color32::GREEN } else { egui::Color32::GRAY }))
        .on_hover_text("用半透明覆盖窗盖住主窗口客户区，把鼠标操作转发给主窗口")
        .clicked() { act_toggle_overlay = true; }

    if ui.button("⚙ 设置").clicked() { self.show_settings = true; }
    ui.separator();
    if ui.button("▶ 全部启动").clicked() { act_start_all = true; }
    if ui.button("⏹ 全部停止").clicked() { act_stop_all = true; }
});
```

工具行下方是一行灰色小字热键提示：

```
热键: Ctrl+Shift+[1-8] 选窗口 → +9 循环启动 / +0 停止 / +- 单次执行 / +Insert 发送任意键；不选则作用于全部
```

再下方是 `egui::ScrollArea::vertical`，循环 `SLOT_COUNT`（= 8）次调用 `ui_slot`。

---

## 槽位面板 ui_slot

**位置**: `src/app.rs`，`ui_slot`（约 2134 行起）
**数据**: `src/window_slot.rs`，`WindowSlot`

每个槽位是一个 `egui::Frame::group`，当槽位被热键选中（`hotkey_sm.selected()` 含 `idx+1`）时填充深黄色背景 `(60, 55, 20)`，否则用 `faint_bg_color`。

### 标题行控件顺序（关键）

标题行（`ui.horizontal`）内**自左向右**依次为：

1. **⚑ 主窗口旗标按钮**（标题行第一个控件）— 见下节
2. `ui.strong("1.")` 序号（`idx + 1`，带粗体点号）
3. 窗口名 `TextEdit::singleline(&mut slots[idx].name)`（`desired_width(90.0)`，hint "窗口N"）— 可编辑，回车或失焦后持久化；空名自动回退为"窗口N"
4. 运行状态：运行中显示绿色 `● 运行中`，否则灰色 `○ 空闲`（均为 `colored_label`）
5. 抓取/倒计时：正在抓取时显示 `⏳ Ns`（倒计时秒数），否则显示 `[抓取窗口]` 小按钮

```rust
ui.horizontal(|ui| {
    // 1. 主窗口旗标（全局互斥，鼠标转发目标）
    let is_main = self.slots[idx].is_main;
    if ui.small_button(egui::RichText::new("⚑")
            .color(if is_main { egui::Color32::GOLD } else { egui::Color32::DARK_GRAY }))
        .on_hover_text(if is_main { "取消主窗口标记" } else { "设为主窗口（鼠标转发目标）" })
        .clicked() { main_toggled = Some(!is_main); }

    // 2. 序号
    ui.strong(format!("{}.", idx + 1));

    // 3. 可编辑窗口名（语音指称用）
    let resp = ui.add(
        egui::TextEdit::singleline(&mut self.slots[idx].name)
            .desired_width(90.0)
            .hint_text(format!("窗口{}", idx + 1)),
    );
    if resp.lost_focus() { name_changed = true; }

    // 4. 运行状态
    if running {
        ui.colored_label(egui::Color32::GREEN, "● 运行中");
    } else {
        ui.colored_label(egui::Color32::GRAY, "○ 空闲");
    }

    // 5. 抓取/清除窗口
    if grabbing_this {
        ui.label(format!("⏳ {}s", remain));
    } else if ui.small_button("抓取窗口").clicked() {
        *act_grab = Some(idx);
    }
});
```

标题行之后还有三块（仍在 Frame 内）：

- **目标行**：已绑定时显示 `目标:` + 窗口标题（`monospace`，浅蓝 `LIGHT_BLUE`）；未绑定显示灰色 `未绑定窗口`。
- **方案集行**：`方案:` 标签 + `[+ 添加方案]` 小按钮（点击弹出添加方案窗口）。若无方案显示灰色 `(无方案)`。
- **方案列表**：每个方案一行（见下节）。
- **启停行**：`[▶ 启动标识方案]`（仅当 `!running && is_bound && marked_scheme().is_some()` 时可用）与 `[⏹ 停止]`（仅运行时可用），用 `add_enabled` 控制可用性。

### ⚑ 主窗口标记按钮（新增）

标题行的第一个控件是 ⚑ 旗标，对应 `WindowSlot.is_main: bool`（字段定义见 `window_slot.rs`，序列化见 `config.rs`）。

- 金色 `GOLD` = 当前槽位是主窗口；深灰 `DARK_GRAY` = 非主窗口。
- 点击切换：`main_toggled = Some(!is_main)`，在 Frame 闭包外统一处理。
- **全局互斥**：切换时先把所有槽位的 `is_main` 清为 `false`，再按需把当前槽位设为 `true`，然后 `save_config()`。
- 若鼠标转发覆盖窗正在运行，切换主窗口会先 `stop_overlay()` 再（若新设为主窗口）`start_overlay()`，相当于换跟踪目标；取消主窗口则直接停止转发并提示。

### 方案列表与 ★/☆ 标识按钮

方案列表每个方案占一行（`ui.horizontal`），控件顺序：

1. **★/☆ 标识按钮**：`small_button`，当 `marked == Some(s)` 显示金色 `★`，否则 `☆`；点击调用 `set_marked(s)` 把该方案设为标识方案（默认执行方案）。悬浮提示"设为标识方案"。
2. 方案名（`schemes[s].script_name`）：标识方案用金色 `colored_label`，其它用普通 `label`。
3. `[查看]` 小按钮：在脚本池中找同名脚本，打开右侧源码面板。
4. `[移除]` 小按钮：调用 `remove_scheme(s)`（内部会修正 `marked` 索引）。

标识状态用局部变量收集：`set_marked: Option<usize>`、`remove: Option<usize>`，闭包结束后统一回放并 `save_config()`。

```rust
let marked = self.slots[idx].marked;           // Option<usize>
for s in 0..scheme_count {
    ui.horizontal(|ui| {
        let is_marked = marked == Some(s);
        let star = if is_marked { "★" } else { "☆" };
        if ui.small_button(star).on_hover_text("设为标识方案").clicked() {
            set_marked = Some(s);
        }
        let name = &self.slots[idx].schemes[s].script_name;
        if is_marked {
            ui.colored_label(egui::Color32::GOLD, name);
        } else {
            ui.label(name);
        }
        if ui.small_button("查看").clicked() { /* 打开源码面板 */ }
        if ui.small_button("移除").clicked() { remove = Some(s); }
    });
}
```

---

## ★/☆ 与 ⚑ 是两个不同概念

| 标记 | 字段 | 类型 | 作用域 | 含义 |
|------|------|------|--------|------|
| ★ / ☆ | `WindowSlot.marked` | `Option<usize>` | **单个槽位内**单选 | 该槽位的"标识方案"，即默认执行方案（启动按钮的目标） |
| ⚑ | `WindowSlot.is_main` | `bool` | **全局**至多一个 | 该槽位是"主窗口"，作为鼠标转发覆盖窗的转发目标 |

两者完全独立：一个槽位可以同时有标识方案且是主窗口，也可以只有其一。

---

## 槽位数据结构

**位置**: `src/window_slot.rs`

```rust
pub struct WindowSlot {
    pub name: String,               // 自定义窗口名（语音指称用）
    pub hwnd: Option<isize>,        // 目标窗口句柄，未绑定为 None
    pub title: String,              // 窗口标题（抓取后填入）
    pub schemes: Vec<Scheme>,       // 该窗口的方案集
    pub marked: Option<usize>,      // 标识方案的索引（指向 schemes）
    pub is_main: bool,              // 主窗口标记（全局互斥）
    pub runner: Option<Runner>,     // 当前后台运行器
}
```

关键方法：`is_bound()` / `is_running()` / `add_scheme()`（第一个方案自动成为标识）/ `remove_scheme()`（修正 `marked`）/ `set_marked()` / `marked_scheme()`。

---

## 底部状态栏 ui_bottom_status

**位置**: `src/app.rs`，`ui_bottom_status`（约 1146 行起）

`egui::TopBottomPanel::bottom("status_bar")`，单行水平排列，按需显示：

- 🎨 取色倒计时（青色 `(0,200,255)`）：`🎨 取色倒计时: N 秒（请切换到目标窗口）`
- 🎯 发送模式激活提示（橙色 `(255,140,0)`）：2 秒内按任意键发送
- 已选窗口集提示（`(200,120,0)`）：`已选窗口 [1,3]，按 Ctrl+Shift+9 启动 / +0 停止`
- `self.status` 通用状态文本（始终显示）

---

## 设置窗口 ui_settings_window

**位置**: `src/app.rs`，`ui_settings_window`（约 1447 行起）

`egui::Window::new("⚙ 设置")`，`collapsible(false)`、`resizable(true)`、`default_width(500.0)`，受 `show_settings` 开关控制。顶部用 `selectable_value` 切换四个标签页：

| 标签 | 枚举 | 处理函数 | 主要控件 |
|------|------|----------|----------|
| 🔧 通用 | `SettingsTab::General` | `ui_settings_general` | 启用日志文件 checkbox、保存唤醒词训练样本 checkbox、保存 ASR 音频 checkbox |
| 🎤 语音控制 | `SettingsTab::Voice` | `ui_settings_voice` | 百度 API Key / Secret Key（密码框）、唤醒词模型状态链接、拼音辅助匹配 checkbox、最近识别文本、语音指令帮助按钮 |
| ⌨️ 热键配置 | `SettingsTab::Hotkey` | `ui_settings_hotkey` | 启用热键 checkbox、热键说明按钮 |
| ℹ️ 关于 | `SettingsTab::About` | `ui_settings_about` | 版本/作者信息 |

底部固定一个 `[💾 保存]` 按钮，点击后 `save_config()` 并把状态栏置为"配置已保存"。关闭窗口会把 `show_settings` 置回 `false`。

```rust
ui.horizontal(|ui| {
    ui.selectable_value(&mut self.settings_tab, SettingsTab::General, "🔧 通用");
    ui.selectable_value(&mut self.settings_tab, SettingsTab::Voice,   "🎤 语音控制");
    ui.selectable_value(&mut self.settings_tab, SettingsTab::Hotkey,  "⌨️ 热键配置");
    ui.selectable_value(&mut self.settings_tab, SettingsTab::About,   "ℹ️ 关于");
});
ui.separator();
match self.settings_tab {
    SettingsTab::General => self.ui_settings_general(ui, &mut act_save),
    SettingsTab::Voice   => self.ui_settings_voice(ui, &mut act_save),
    SettingsTab::Hotkey  => self.ui_settings_hotkey(ui, &mut act_save),
    SettingsTab::About   => self.ui_settings_about(ui),
}
```

---

## 取色器（独立 egui viewport）

**位置**: `src/color_picker.rs`，`ColorPicker::ui`（在 `update` 末尾每帧调用）

取色器**不是原生 Win32 窗口**，而是用 egui 的 `ViewportBuilder` 开的独立 viewport：

```rust
let viewport_id = egui::ViewportId::from_hash_of("color_picker_viewport");
let builder = egui::ViewportBuilder::default()
    .with_title("🎨 取色器")
    .with_inner_size([960.0, 640.0]);
```

主面板点 `🎨 取色` 后启动 3 秒倒计时（`handle_picking`），到点截屏并把位图交给取色器 viewport 显示：鼠标悬停显示坐标/颜色，按 Ctrl 记录到右侧抽屉的取色列表。

---

## 添加方案弹窗 ui_add_scheme_window

**位置**: `src/app.rs`，`ui_add_scheme_window`（约 1215 行起）

当 `adding_scheme_for = Some(slot_idx)` 时显示，`egui::Window` 标题为"为窗口 N 添加方案"。列出所有脚本，按 `category` 用 `CollapsingHeader`（默认展开）分组，每行显示有效/无效状态（绿色"有效"/红色"无效"+ 错误悬浮提示）、脚本名、`[加入]` 小按钮（仅有效脚本可点）。加入后调用 `slots[slot_idx].add_scheme(...)`，重复同名会被跳过。

---

## 脚本源码面板 ui_source_panel

**位置**: `src/app.rs`，`ui_source_panel`（约 1186 行起）

当 `viewing_script = Some(idx)` 时显示在右侧 `egui::SidePanel::right("source_panel")`（`default_width(360.0)`）。顶部为脚本名 + `[关闭]` 按钮，主体是只读的多行 `TextEdit::multiline(...).code_editor()`（`interactive(false)`），用于查看脚本源码。

---

## 其它帮助/引导弹窗

均为 `egui::Window`，由对应标志位开关，互不影响：

- `ui_hotkey_help_window`（"🎮 热键使用说明"，`show_hotkey_help`）— 详解前缀选择、批量启停、发送模式等。
- `ui_voice_help_window`（"📖 语音指令帮助"，`show_voice_help`）— 列出支持的语音指令。
- `ui_baidu_guide_window`（独立 viewport，`show_baidu_guide`）— 引导申请百度语音密钥。
- `ui_wakeword_guide_window`（独立 viewport，`show_wakeword_guide`）— 唤醒词"小助手"训练向导，含每遍录制进度条。

---

## 鼠标转发覆盖窗（独立原生 Win32 窗口）

主面板工具行的 `🖱 转发` 开关会启动一个**独立的半透明原生 Win32 覆盖窗**（盖住主窗口客户区，把鼠标操作转发给主窗口），其实现在 `src/overlay.rs`，**不在 egui 里**。转发目标由槽位的 ⚑ 主窗口标记决定（全局至多一个）。覆盖窗的详细设计见 `docs/12`。

---

## 字体与样式

**位置**: `src/main.rs`

主窗口初始尺寸 `720 × 520`，标题"游戏自动按键工具"。为避免中文显示为方块，启动时 `setup_fonts` 按顺序尝试加载 Windows 自带字体（微软雅黑 → 黑体 → 宋体），命中后插入到 `Proportional` 与 `Monospace` 字体族首位。

```rust
let options = eframe::NativeOptions {
    viewport: egui::ViewportBuilder::default()
        .with_inner_size([720.0, 520.0])
        .with_title("游戏自动按键工具"),
    ..Default::default()
};
```
