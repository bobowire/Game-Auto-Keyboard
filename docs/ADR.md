# 架构决策记录 (ADR)

## ADR-001: 使用 Trait 抽象输入后端

**日期**: 2026-07-24  
**状态**: ✅ 已采纳

### 背景

Windows 提供多种发送键鼠消息的方式：
- PostMessage/SendMessage
- SendInput
- keybd_event (已废弃)
- 驱动级方案 (Interception, 自定义驱动)

不同方式各有优劣，且用户场景多变（后台vs前台、普通程序vs游戏）。

### 决策

**采用 Rust trait 抽象输入后端，定义统一接口 `InputBackend`**

```rust
pub trait InputBackend: Send + Sync {
    fn name(&self) -> &str;
    fn supports_background(&self) -> bool;
    fn send_key_down(&self, hwnd: HWND, key: Key) -> Result<(), String>;
    fn send_key_up(&self, hwnd: HWND, key: Key) -> Result<(), String>;
    fn send_mouse_down(&self, hwnd: HWND, button: MouseButton, x: i32, y: i32) -> Result<(), String>;
    fn send_mouse_up(&self, hwnd: HWND, button: MouseButton) -> Result<(), String>;
}
```

### 理由

1. **隔离变化**: Windows API 选择是易变点，抽象后上层代码不受影响
2. **可扩展**: 新增后端只需实现 trait，无需修改现有代码（开闭原则）
3. **可测试**: 可创建 MockBackend 进行单元测试
4. **运行时切换**: 用户可在 UI 动态切换后端
5. **渐进式开发**: 先实现 PostMessage，后续按需补充

### 实现路径

#### 阶段 1（立即）
- 定义 `InputBackend` trait
- 实现 `PostMessageBackend`（✅ 已验证可用）
- 创建 `InputManager` 管理后端切换

#### 阶段 4（备用）
- 实现 `SendInputBackend`（前台方案）

#### 阶段 6+（可选）
- 按需实现驱动级后端

### 替代方案

#### 方案 A: 直接使用 PostMessage，不抽象
- ❌ 后续切换成本高
- ❌ 难以测试
- ✅ 代码更简单

#### 方案 B: 运行时根据配置判断（if/else）
```rust
if config.use_send_input {
    SendInput(...);
} else {
    PostMessage(...);
}
```
- ❌ 违反开闭原则
- ❌ 新增方式需修改多处代码
- ✅ 不需要 trait object 开销

#### 方案 C: 编译期特性开关（feature flags）
```rust
#[cfg(feature = "post_message")]
fn send_key(...) { PostMessage(...); }

#[cfg(feature = "send_input")]
fn send_key(...) { SendInput(...); }
```
- ❌ 无法运行时切换
- ❌ 需要为每种组合编译不同二进制
- ✅ 零开销抽象

### 结论

方案 C（trait 抽象）在**灵活性、可维护性、可测试性**上远优于其他方案。

Rust trait object 的性能开销（虚函数调用）在这个场景下完全可以忽略：
- 键鼠消息发送本身是系统调用，耗时远超虚函数调用
- 每秒发送消息数量级在 10-100 级别，不是性能瓶颈

### 后果

**正面影响**:
- 未来扩展成本低
- 用户可自由切换后端
- 代码可测试性强

**负面影响**:
- 需要额外定义 trait 和管理器
- 初始实现略复杂（但长期收益高）

---

## ADR-002: PostMessage 作为默认后端

**日期**: 2026-07-24  
**状态**: ✅ 已采纳

### 背景

用户已验证 PostMessage 对目标程序可用。

### 决策

**PostMessage 作为默认输入后端，其他方案作为备选**

### 理由

1. ✅ 已验证可用（实际测试通过）
2. ✅ 支持后台发送（核心需求）
3. ✅ 无需额外依赖
4. ✅ 实现简单

### 实现策略

```rust
impl InputManager {
    pub fn new() -> Self {
        let backends = vec![
            Arc::new(PostMessageBackend::new()),  // 默认
            // Arc::new(SendInputBackend::new()),  // 阶段4添加
        ];
        Self {
            current: backends[0].clone(),
            available: backends,
        }
    }
}
```

### 风险缓解

**风险**: PostMessage 对部分程序无效  
**缓解**: 
1. UI 中提供后端切换选项
2. 配置持久化记住用户选择
3. 阶段4补充 SendInput 备用方案

---

## ADR-003: 客户区坐标作为统一坐标系

**日期**: 2026-07-24  
**状态**: ✅ 已采纳

### 背景

Windows 有三种坐标系：
- 屏幕坐标（相对于整个屏幕）
- 窗口坐标（相对于窗口左上角，含标题栏）
- 客户区坐标（相对于窗口客户区，不含标题栏）

### 决策

**InputBackend trait 的鼠标方法统一使用客户区坐标**

```rust
fn send_mouse_down(&self, hwnd: HWND, button: MouseButton, x: i32, y: i32);
//                                                         ^^^^^^^^^ 客户区坐标
```

### 理由

1. **一致性**: PostMessage 的鼠标消息使用客户区坐标
2. **直观性**: 脚本作者看到的游戏内坐标就是客户区坐标
3. **简化脚本**: 不需要考虑标题栏高度

### 实现要求

- **PostMessageBackend**: 直接传递（PostMessage 本身用客户区坐标）
- **SendInputBackend**: 需要转换为屏幕坐标
  ```rust
  let screen_pos = client_to_screen(hwnd, x, y)?;
  ```

### 脚本示例

```
// 点击窗口 (100, 200) 位置
mouse_click(left, 100, 200)

// 直观、无需考虑窗口位置和标题栏
```

---

## ADR-004: 错误处理使用 Result<(), String>

**日期**: 2026-07-24  
**状态**: ✅ 已采纳

### 背景

InputBackend 的方法可能失败（窗口关闭、权限不足等）。

### 决策

**所有可能失败的方法返回 `Result<(), String>`**

### 理由

1. **简单**: String 错误信息足够（不需要复杂的错误类型）
2. **灵活**: 可以直接格式化 Windows 错误码
3. **UI友好**: String 可直接显示给用户

### 替代方案

#### 方案 A: 自定义错误类型
```rust
pub enum InputError {
    WindowClosed,
    PermissionDenied,
    ApiError(i32),
}
```
- ❌ 过度设计（当前阶段）
- ✅ 类型安全

#### 方案 B: anyhow::Error
- ❌ 额外依赖
- ✅ 生态标准

### 结论

阶段1使用 `String`，如果后续需要细化错误类型，再重构。

---

## ADR-005: 统一事件总线（自带窗口唤醒）

**日期**: 2026-07-26
**状态**: ✅ 已采纳

### 背景

程序有多个后台事件源（托盘菜单、全局热键、语音识别），原先每个源都持有自己的
channel，由 `App::update()` 逐个 `poll()`。这个模式在隐藏到托盘后失效：

窗口不可见 → Windows 不再产生 `WM_PAINT` → winit 收不到 `RedrawRequested`
→ eframe 不调用 `App::update()` → 所有 poll 点停摆。

表现为：托盘菜单点了没反应、热键按了不执行、语音识别完了不动。
`ctx.request_repaint()` 也救不了 —— winit 用 `RDW_INTERNALPAINT` 请求重绘，
不可见窗口不会因此收到 `WM_PAINT`。

托盘模块曾单独打过补丁（回调里自己 `PostMessage(WM_PAINT)`），但那是点状修复：
热键和语音各自还是哑的，而且以后每加一个事件源都得记得重写一遍唤醒逻辑。

### 决策

**引入 `src/event_bus.rs`：所有后台事件收敛到一条 `MainEvent` 队列，
投递动作内部自带窗口唤醒。**

```rust
pub enum MainEvent {
    Tray(TrayCommand),
    Hotkey(HotkeyKey),
    Voice(VoiceEvent),
}

// 后台线程侧：send 即自动唤醒主窗口
impl EventSender {
    pub fn send(&self, event: MainEvent) {
        let _ = self.tx.send(event);
        wake_main_window(&self.hwnd);   // PostMessage(WM_PAINT)
    }
}

// 主线程侧：每帧一次统一分发
fn dispatch_events(&mut self, ctx: &egui::Context) {
    for event in self.events.poll() {
        match event {
            MainEvent::Tray(cmd) => self.handle_tray(ctx, cmd),
            MainEvent::Hotkey(key) => self.handle_hotkey(key),
            MainEvent::Voice(ev) => self.handle_voice_event(ev),
        }
    }
}
```

配套 `WakeTicker`：给"必须连续跑 update 才能推进"的流程用（唤醒词训练要逐帧
抽麦克风数据），活着期间定时唤醒，drop 即停。

### 理由

1. **唤醒不再是每个事件源的责任**：新增事件源只要用 `EventSender::send()`，
   自动获得隐藏时也能即时响应的能力，不会漏
2. **消除三处重复的 poll 样板**：`process_hotkeys` / `process_tray` /
   `process_voice` 合并为一个 `dispatch_events`
3. **HWND 只登记一处**：原先托盘持有 HWND 槽位，语音要用还得再传一遍
4. **可测**：总线本身不依赖窗口，`poll` / 生命周期行为可单元测试

### 替代方案

#### 方案 A: 每个事件源自己 PostMessage
- ✅ 改动小
- ❌ 唤醒逻辑重复三份，新增事件源必然漏（这正是修复前的状态）
- ❌ HWND 要分发给每个事件源

#### 方案 B: 用 `eframe` 的 `Repaint` 信号 / `request_repaint`
- ❌ 不可见窗口收不到 `WM_PAINT`，根本无效（已验证）

#### 方案 C: 常驻消息窗口（`HWND_MESSAGE`）单独跑事件循环
- ✅ 彻底脱离 UI 可见性
- ❌ 需要维护第二个窗口和线程模型，改动远超收益
- ❌ 事件仍要跨回 UI 线程才能改 `App` 状态，问题没消失

### 后果

- `Tray::new()` / `HotkeyManager::start()` / `VoiceRuntime::start()` 签名
  各增加一个 `EventSender` 参数，各自的 `poll()` 移除
- `VoiceEvent` 需要 `Debug + Clone`（作为 `MainEvent` 的载荷）
- 所有事件源共用一条队列 → 停止语音后残留事件可能误伤新实例，
  `handle_voice_event()` 在 `self.voice.is_none()` 时丢弃事件

---

## 决策总结

| ADR | 决策 | 优先级 | 状态 |
|-----|------|-------|------|
| 001 | Trait 抽象输入后端 | P0 | ✅ 已采纳 |
| 002 | PostMessage 默认 | P0 | ✅ 已采纳 |
| 003 | 客户区坐标统一 | P0 | ✅ 已采纳 |
| 004 | Result<(), String> | P0 | ✅ 已采纳 |
| 005 | 统一事件总线（自带唤醒） | P0 | ✅ 已采纳 |

---

## 文档维护

有新的架构决策时，按以下格式添加：

```markdown
## ADR-XXX: 决策标题

**日期**: YYYY-MM-DD  
**状态**: 提议中 / 已采纳 / 已废弃

### 背景
（问题描述）

### 决策
（做什么）

### 理由
（为什么）

### 替代方案
（考虑过哪些方案）

### 后果
（影响是什么）
```
