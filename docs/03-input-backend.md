# 输入后端设计

## 设计理念

使用 **策略模式 (Strategy Pattern)** 通过 trait 抽象输入方式，核心优势：

1. **可插拔**: `InputBackend` trait 是后端切换的扩展点（早期预留的 `InputManager` 管理器已作为死代码移除，trait 抽象保留）
2. **可扩展**: 新增后端只需实现 `InputBackend` trait
3. **可测试**: 可以 mock 后端进行单元测试
4. **解耦**: 脚本执行器不关心具体实现细节

### 支持的实现

- **PostMessage** ✅: 后台发送（**当前唯一实现**，已验证可用）
- **SendInput**: 前台发送（**设计预留，代码未实现**，无 `src/input/send_input.rs`）
- **驱动级**: 内核驱动（纯接口构想，未实现）

> 现状提示：trait 设计为可切换，但实际运行时只有 `PostMessageBackend` 一种实现，由 `Runner` / `overlay` / `app` 直接 `new` 使用，无后端管理器（早期预留的 `InputManager` 已移除，详见下文「输入管理器」一节）。

## 核心 Trait

**位置**: `src/input/backend.rs`

### 设计目标

**为什么要抽象成 trait？**

1. **隔离变化点**: Windows 输入 API 有多种方式（PostMessage、SendInput、驱动），未来可能还有新方案
2. **预留切换能力**: `InputBackend` trait 为运行时切换留好接口（管理器 `InputManager` 已移除，需要时重新引入即可）
3. **渐进式开发**: 先实现 PostMessage，后续再补充其他方案
4. **单元测试**: 可以创建 `MockBackend` 进行测试，不依赖真实窗口

### 字符串驱动而非枚举

一个关键设计：**trait 的键盘入参是 `&str`，不是 `Key` 枚举**。脚本层（`script::ast::Command::Down(String)` 等）从头到尾都把按键名当字符串传递，由 `input::keymap::parse_key` 在后端内部解析为虚拟键码 (VK)。这样做的好处是脚本解析器和后端彻底解耦，新增按键名只需改 keymap，不需要改 AST 和执行器。

### Trait 定义

以下与 `src/input/backend.rs` 完全一致：

```rust
use windows::Win32::Foundation::HWND;
use crate::input::keymap::MouseButton;

/// 输入后端 trait（策略模式）
///
/// 脚本引擎以字符串名称驱动按键（如 "A"、"space"、"left"），
/// 各后端负责把名称解析为具体的 VK 码/鼠标消息并发送。
pub trait InputBackend: Send + Sync {
    /// 后端名称（用于 UI 显示/切换）
    fn name(&self) -> &str;

    /// 是否支持后台发送（窗口非激活状态）
    fn supports_background(&self) -> bool;

    // ===== 键盘接口 =====

    /// 发送键盘按下事件（key 为按键名，如 "A"、"space"、"f1"）
    fn send_key_down(&self, hwnd: HWND, key: &str) -> Result<(), String>;

    /// 发送键盘弹起事件
    fn send_key_up(&self, hwnd: HWND, key: &str) -> Result<(), String>;

    // ===== 鼠标接口 =====

    /// 发送鼠标移动事件（客户区坐标）
    fn send_mouse_move(&self, hwnd: HWND, x: i32, y: i32) -> Result<(), String>;

    /// 发送鼠标按下事件（客户区坐标）
    fn send_mouse_down(
        &self,
        hwnd: HWND,
        button: MouseButton,
        x: i32,
        y: i32,
    ) -> Result<(), String>;

    /// 发送鼠标弹起事件（弹起不需要坐标）
    fn send_mouse_up(&self, hwnd: HWND, button: MouseButton) -> Result<(), String>;

    // ===== 窗口消息 =====

    /// 发送窗口激活消息（欺骗窗口使其认为自己被激活）
    fn send_window_active(&self, hwnd: HWND) -> Result<(), String>;
}
```

### Trait 方法说明

| 方法 | 说明 | 返回值 |
|------|------|--------|
| `name()` | 后端唯一标识，用于 UI 显示和配置存储 | `&str` |
| `supports_background()` | 是否支持后台发送（影响 UI 提示） | `bool` |
| `send_key_down()` | 发送按键按下事件，`key` 为按键名字符串 | `Result<(), String>` |
| `send_key_up()` | 发送按键弹起事件 | `Result<(), String>` |
| `send_mouse_move()` | 发送鼠标移动（客户区坐标） | `Result<(), String>` |
| `send_mouse_down()` | 发送鼠标按下事件（客户区坐标） | `Result<(), String>` |
| `send_mouse_up()` | 发送鼠标弹起事件（不带坐标） | `Result<(), String>` |
| `send_window_active()` | 发送窗口激活消息（`WM_ACTIVATE` + `WM_SETFOCUS`） | `Result<(), String>` |

### 关键约束

- **线程安全**: `Send + Sync` 保证可以在执行线程中调用
- **错误处理**: 所有方法返回 `Result`，便于上层处理失败情况
- **坐标系统**: 鼠标坐标统一使用**客户区坐标**，由调用方（`ScriptExecutor::resolve_coord`）负责转换
- **字符串驱动**: 键盘入参是 `&str`，由后端内部调 `keymap::parse_key` 解析为 VK

---

## 键鼠名称与 keymap

**位置**: `src/input/keymap.rs`

trait 把"按键名 → VK"的解析放在后端内部，统一走 keymap 模块。其中两个函数尤其重要：

### `parse_key(name: &str) -> Result<u16, String>`

把按键名解析为虚拟键码，支持：

- 单字符：字母 `a`-`z`、数字 `0`-`9`
- 功能键：`f1`-`f24`
- 特殊键：`space`/`spacebar`、`enter`/`return`、`tab`、`esc`/`escape`、`backspace`/`back`、`shift`、`ctrl`/`control`、`alt`/`menu`、`capslock`/`caps`
- 方向键：`up`、`down`、**`left`、`right`**（注意：解析为 `VK_LEFT` / `VK_RIGHT` 方向键）
- 编辑键：`home`、`end`、`pageup`/`pgup`、`pagedown`/`pgdn`、`insert`/`ins`、`delete`/`del`
- 小键盘：`num0`-`num9`

### `parse_mouse_button(name: &str) -> Option<MouseButton>`

把按键名解析为鼠标按钮：

| 名称 | 解析结果 |
|------|----------|
| `left` / `lbutton` / `mouse_left` | `MouseButton::Left` |
| `right` / `rbutton` / `mouse_right` | `MouseButton::Right` |
| `middle` / `mbutton` / `mouse_middle` | `MouseButton::Middle` |
| 其他 | `None` |

### ⚠️ 键鼠歧义（重要）

注意 `parse_key` 和 `parse_mouse_button` 对 `"left"` / `"right"` 这两个名字的解读**冲突**：

- `parse_key("left")` → `VK_LEFT`（方向键）
- `parse_mouse_button("left")` → `MouseButton::Left`（鼠标左键）

消歧发生在 **AST 解析层**而非后端：脚本的 `mouse_click(left)` 在解析时就被 `parse_mouse_button` 认成 `Command::MouseClick(MouseButton::Left, Coord)`，带着枚举进入执行器，不经过字符串歧义；而键盘 `click(left)` 解析为 `Command::Click("left")`，执行器走 `send_key_down/up`，被 `parse_key` 解释成**方向键**。两者命令类型不同，不会混淆。

---

## 唯一实现：PostMessage 后端

**位置**: `src/input/post_message.rs`

### 特点
- ✅ 支持后台发送
- ✅ **已验证可用**（用户确认）
- ✅ 无需激活窗口
- ⚠️ 兼容性：对普通程序/老游戏有效，现代 3D 游戏可能需要其他方案
- ✅ 鼠标消息 `wParam` 合成 `MK_*` 按键状态位（按下/弹起/移动）

### 键盘实现

通过 `parse_key` 拿到 VK，再用 `MapVirtualKeyW(MAPVK_VK_TO_VSC)` 反查扫描码构造 `lParam`：

```rust
fn send_key_down(&self, hwnd: HWND, key: &str) -> Result<(), String> {
    let vk = parse_key(key)?;
    let scan_code = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) };

    // LPARAM = (scan_code << 16) | repeat_count(1)
    let lparam = LPARAM(((scan_code as isize) << 16) | 1);

    unsafe {
        PostMessageW(hwnd, WM_KEYDOWN, WPARAM(vk as usize), lparam)
            .map_err(|e| format!("PostMessage 失败: {:?}", e))?;
    }

    Ok(())
}

fn send_key_up(&self, hwnd: HWND, key: &str) -> Result<(), String> {
    let vk = parse_key(key)?;
    let scan_code = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) };

    // LPARAM = (scan_code << 16) | 0xC0000001 (transition & previous state)
    let lparam = LPARAM(((scan_code as isize) << 16) | 0xC0000001u32 as isize);

    unsafe {
        PostMessageW(hwnd, WM_KEYUP, WPARAM(vk as usize), lparam)
            .map_err(|e| format!("PostMessage 失败: {:?}", e))?;
    }

    Ok(())
}
```

| 消息 | `wParam` | `lParam` 构造 |
|------|----------|---------------|
| `WM_KEYDOWN` | VK 码 | `(scan_code << 16) \| 1` |
| `WM_KEYUP` | VK 码 | `(scan_code << 16) \| 0xC0000001` |

### 鼠标实现

```rust
fn send_mouse_down(&self, hwnd: HWND, button: MouseButton, x: i32, y: i32)
    -> Result<(), String>
{
    let msg = match button {
        MouseButton::Left => WM_LBUTTONDOWN,
        MouseButton::Right => WM_RBUTTONDOWN,
        MouseButton::Middle => WM_MBUTTONDOWN,
    };

    // LPARAM = MAKELPARAM(x, y)
    let lparam = LPARAM(((y as isize) << 16) | (x as isize & 0xFFFF));

    unsafe {
        PostMessageW(hwnd, msg, WPARAM(0), lparam)
            .map_err(|e| format!("发送鼠标消息失败: {:?}", e))?;
    }

    Ok(())
}
```

| 消息 | `wParam` | `lParam` 构造 |
|------|----------|---------------|
| `WM_LBUTTONDOWN` / `WM_RBUTTONDOWN` / `WM_MBUTTONDOWN` | **恒为 `0`** | `MAKELPARAM(x, y) = (y << 16) \| (x & 0xFFFF)` |
| `WM_LBUTTONUP` / `WM_RBUTTONUP` / `WM_MBUTTONUP` | **恒为 `0`** | `MAKELPARAM(x, y)` |
| `WM_MOUSEMOVE` | **恒为 `0`** | `MAKELPARAM(x, y)` |

### 窗口激活实现

```rust
fn send_window_active(&self, hwnd: HWND) -> Result<(), String> {
    unsafe {
        // WM_ACTIVATE: WPARAM = 激活类型 (WA_ACTIVE=1)，LPARAM = 上一个激活窗口（传 0）
        PostMessageW(hwnd, WM_ACTIVATE, WPARAM(1), LPARAM(0))
            .map_err(|e| format!("发送 WM_ACTIVATE 失败: {:?}", e))?;

        // WM_SETFOCUS: 通知窗口获得键盘焦点
        PostMessageW(hwnd, WM_SETFOCUS, WPARAM(0), LPARAM(0))
            .map_err(|e| format!("发送 WM_SETFOCUS 失败: {:?}", e))?;
    }

    Ok(())
}
```

### 鼠标消息的 wParam（MK_* 状态位）

按 Win32 规范，`WM_LBUTTONDOWN` 等鼠标消息的 `wParam` 应为按键状态标志（`MK_LBUTTON` / `MK_SHIFT` 等）。后端用 `GetKeyState` 读取当前物理按键/修饰键状态合成 `MK_*` 位，并对正在模拟的按键额外强制置位/清位：

- `send_mouse_down`：`current_mk_state() | button_mk(button)`（强制置上本次按下位）
- `send_mouse_up`：`current_mk_state() & !button_mk(button)`（清掉本次释放位）
- `send_mouse_move`：`current_mk_state()`

> 注意：`GetKeyState` 反映调用线程消息队列的状态；后台 Runner 线程通常无消息循环，物理按键位多返回 0，因此主要靠强制置位保证按下语义。`MK_*` 用字面量常量，未引入 `Win32_System_SystemServices` feature。

---

## 调用链：ScriptExecutor 如何使用后端

**位置**: `src/script/executor.rs`

`ScriptExecutor` 持有 `input: &'a dyn InputBackend` 和目标 `hwnd`，`execute_command` 按 `Command` 变体分发：

```rust
fn execute_command(&self, cmd: &Command) -> Result<(), String> {
    match cmd {
        Command::Down(key)       => self.input.send_key_down(self.hwnd, key)?,
        Command::Up(key)         => self.input.send_key_up(self.hwnd, key)?,
        Command::Click(key)      => {
            // 键盘点击：直接 down/up（"left" 在此会被 parse_key 解释为方向键）
            self.input.send_key_down(self.hwnd, key)?;
            self.input.send_key_up(self.hwnd, key)?;
        }
        Command::ClickMs(key, ms)=> {
            self.input.send_key_down(self.hwnd, key)?;
            self.sleep_ms(*ms);
            self.input.send_key_up(self.hwnd, key)?;
        }
        Command::SendWindowActive => self.input.send_window_active(self.hwnd)?,
        Command::MouseMove(coord) => {
            let (x, y) = self.resolve_coord(coord)?;
            self.input.send_mouse_move(self.hwnd, x, y)?;
        }
        Command::MouseDown(btn, coord) => {
            let (x, y) = self.resolve_coord(coord)?;
            self.input.send_mouse_down(self.hwnd, *btn, x, y)?;
        }
        Command::MouseUp(btn) => {
            self.input.send_mouse_up(self.hwnd, *btn)?;
        }
        Command::MouseClick(btn, coord) => {
            let (x, y) = self.resolve_coord(coord)?;
            self.input.send_mouse_down(self.hwnd, *btn, x, y)?;
            self.input.send_mouse_up(self.hwnd, *btn)?;
        }
        Command::Setting(_) | Command::If { .. } => { /* 略 */ }
    }
    Ok(())
}
```

### 坐标转换：`resolve_coord`

`Coord`（来自 AST）有三种变体，`resolve_coord` 借助 `GetClientRect` 取客户区尺寸后统一转成客户区绝对像素坐标：

| `Coord` 变体 | 转换公式 |
|--------------|----------|
| `Absolute { x, y }` | 直接使用 `(x, y)` |
| `Center { dx, dy }` | `(w/2 + dx, h/2 + dy)` |
| `Percent { px, py }` | `(w * px / 100, h * py / 100)` |

### `MouseButton` 枚举（已统一）

`MouseButton` 只有一份定义：`input::keymap::MouseButton`（Left/Right/Middle）。`script::ast` 通过 `pub use` 重导出复用，AST 与后端直接共用同一类型，无需桥接转换（历史上有过重复定义与 `to_input_btn` 桥接函数，已移除）。

---

## 输入管理器（已移除）

**历史**：早期曾定义 `InputManager`（管理多后端注册与运行时切换）作为 ADR-001 的扩展点，但从未接入运行链路——`Runner`、`overlay`、`app` 均直接 `PostMessageBackend::new()`。作为死代码已于 2026-08 移除，`src/lib.rs` 的重导出同步删除。

`InputBackend` trait 抽象保留（`ScriptExecutor` / `overlay` 仍以 trait 接收后端）。若未来需要运行时切换后端，重新引入一个管理器即可，trait 接口不变。

### 真实运行链路

实际运行时由 `Runner` 直接实例化后端：

**位置**: `src/runner.rs`

```rust
fn spawn(hwnd_raw: isize, commands: Vec<Command>, once: bool, initial_delay_ms: u64) -> Self {
    // ...
    let handle = thread::spawn(move || {
        // ...
        let backend = PostMessageBackend::new();           // ← 直接 new
        let hwnd = windows::Win32::Foundation::HWND(hwnd_raw as *mut _);

        loop {
            if stop_clone.load(Ordering::Relaxed) { break; }
            let executor = ScriptExecutor::new(&backend, hwnd);   // ← 借给执行器
            if let Err(e) = executor.execute_interruptible(&commands, &stop_clone) {
                eprintln!("脚本执行出错: {}", e);
                break;
            }
            if once || commands.is_empty() { break; }
        }
    });
    // ...
}
```

此外 `src/app/events.rs`（即兴发送）和 `src/overlay.rs`（鼠标转发覆盖窗）也各自直接 `PostMessageBackend::new()`。

---

## 使用示例

### 在脚本执行器中使用（实际签名）

`ScriptExecutor::new` 接收 `&dyn InputBackend`，执行器内部按上面“调用链”一节的方式分发：

```rust
impl<'a> ScriptExecutor<'a> {
    pub fn new(input: &'a dyn InputBackend, hwnd: HWND) -> Self {
        Self {
            input,
            hwnd,
            stop_flag: None,
            capture: Box::new(PrintWindowCapture::new()),
        }
    }
}
```

### 简单测试工具

**位置**: `examples/script_test.rs`（实际存在）

```rust
use game_auto_keyboard::input::PostMessageBackend;

fn main() {
    // ...
    let backend = PostMessageBackend::new();
    // ...
}
```

运行：
```bash
cargo run --example script_test
```

---

## 扩展新后端的步骤

### 示例：添加 AutoHotkey 后端

假设要集成 AutoHotkey 的 ControlSend 功能：

#### 1. 创建新文件 `src/input/ahk_backend.rs`

```rust
use crate::input::backend::InputBackend;
use windows::Win32::Foundation::HWND;
use crate::input::keymap::MouseButton;

pub struct AhkBackend {
    // AHK 实例句柄或进程通信通道
}

impl InputBackend for AhkBackend {
    fn name(&self) -> &str { "AutoHotkey ControlSend" }

    fn supports_background(&self) -> bool { true }  // AHK 的 ControlSend 支持后台

    fn send_key_down(&self, hwnd: HWND, key: &str) -> Result<(), String> {
        // 注意：key 是字符串，由后端自行决定如何解析（可复用 keymap::parse_key）
        // ahk.exe script.ahk "ControlSend {a down}, , ahk_id %hwnd%"
        todo!()
    }

    fn send_key_up(&self, hwnd: HWND, key: &str) -> Result<(), String> { todo!() }
    fn send_mouse_move(&self, hwnd: HWND, x: i32, y: i32) -> Result<(), String> { todo!() }
    fn send_mouse_down(&self, hwnd: HWND, button: MouseButton, x: i32, y: i32) -> Result<(), String> { todo!() }
    fn send_mouse_up(&self, hwnd: HWND, button: MouseButton) -> Result<(), String> { todo!() }
    fn send_window_active(&self, hwnd: HWND) -> Result<(), String> { todo!() }
}
```

#### 2. 接入运行链路

> 注：早期版本在 `src/input/mod.rs` 提供 `InputManager` 管理多后端注册，已于 2026-08 作为死代码移除。新后端无需注册步骤，直接在调用处实例化即可；若需要运行时切换，再重新引入一个管理器。

目前 `Runner` / `overlay` / `app` 都直接 `PostMessageBackend::new()`。接入新后端最简做法是给 `Runner::spawn`（及即兴发送、overlay 启动处）加一个后端选择参数：

```rust
// Runner::spawn 增加参数，按需 new 对应后端
let backend: Arc<dyn InputBackend> = match backend_kind {
    BackendKind::PostMessage => Arc::new(PostMessageBackend::new()),
    BackendKind::Ahk => Arc::new(AhkBackend::new()),   // ← 新增
};
```

#### 3. 接通 UI 切换入口（当前未做）

有了后端选择参数后，在设置窗口加一个下拉选择，把用户选择传给 `Runner::spawn` 即可。

---

## 扩展：SendInput / 驱动级后端（均未实现）

### SendInput 后端（设计预留，代码未实现）

⚠️ **当前不存在 `src/input/send_input.rs`，也没有 `SendInputBackend` 类型。** 以下是设想中的设计要点：

- ❌ 不支持后台（窗口必须在前台）
- ✅ 兼容性强（所有程序都能收到）
- ⚠️ 需要先激活窗口（`SetForegroundWindow`）
- 预期走 `SendInput` + `INPUT_KEYBOARD` / `INPUT_MOUSE`

实现时需补齐 trait 全部方法（`send_key_down/up`、`send_mouse_move/down/up`、`send_window_active`）。

### 驱动级后端（接口构想）

- 预期走 Interception 驱动（`interception.dll`）
- 需要安装内核驱动（需要管理员权限）
- 部分反作弊会检测驱动
- 兼容性最强，但复杂度高

---

## 对比总结

| 特性 | PostMessage ✅ | SendInput | Interception |
|------|---------------|-----------|--------------|
| 实现状态 | **✅ 已实现并验证** | ❌ 未实现（无对应文件） | ❌ 未实现（纯构想） |
| 后台发送 | ✅ **已验证** | ❌（需前台） | ✅ |
| 兼容性 | ✅ 普通程序<br>⚠️ 部分 3D 游戏 | ✅ 通用 | ✅ 通用 |
| 安装要求 | 无 | 无 | 需要驱动 |
| 反作弊风险 | 低 | 低 | 高 |
| 进入运行链路 | ✅ Runner 直接实例化 | — | — |
| 适用场景 | 普通程序/老游戏/2D游戏 | 前台自动化（预留） | 高要求场景（预留） |

### 开发建议

- **当前**: 仅 `PostMessageBackend` 一种实现，由 `Runner` / `overlay` / `app` 直接实例化，无后端管理器
- **后续补充 `SendInputBackend` 时**：
  1. 新建 `src/input/send_input.rs`，实现 `InputBackend` 全部方法
  2. 给 `Runner::spawn`（及 overlay / 即兴发送处）加后端选择参数；若需运行时切换，再重新引入后端管理器（原 `InputManager` 已移除）
  3. 接通 UI 切换入口
- **驱动级**: 按需评估，复杂度高，暂不排期
