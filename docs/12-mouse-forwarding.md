# 12. 鼠标事件转发（覆盖窗）

## 背景与场景

游戏多开时，用户希望**一边在别的窗口（浏览器/工具）干活，一边用鼠标操作"主号"游戏窗口**。游戏窗口失去焦点后，正常的鼠标操作到不了它；而项目已验证 PostMessage 发鼠标消息（按下/弹起/移动）对目标游戏有效（与脚本自动化同一条路线）。

本功能给用户一个可视化的"后台操作通道"：

1. 把某个槽位标记为**主窗口**（⚑ 旗标，全局互斥，持久化）
2. 点工具行的 **🖱 转发** 开关
3. 一个 50% 半透明、带"鼠标事件转发模式"提示文字的覆盖窗精确盖住主窗口客户区
4. 用户在覆盖窗上的鼠标操作（点击/拖拽/双击/滚轮）经 `PostMessageW` 转发给主窗口

## 总体架构

```
UI 开关 (app.rs)
   │ start_overlay / stop_overlay
   ▼
OverlayWindow (src/overlay.rs)
   │ 独立线程：layered 窗口 + 原生 GetMessageW 消息循环
   │ WM_TIMER(50ms) 跟随主窗口客户区
   │ 收到鼠标消息 → PostMessageW(主窗口)
   │ 主窗口失效 → 自毁 + 回报 OverlayEvent::TargetLost
   ▼
MainEventBus (src/event_bus.rs)
   │ MainEvent::Overlay(ev) 入队 + PostMessage(WM_PAINT) 唤醒主窗口
   ▼
App::handle_overlay_event → 复位开关、状态栏提示
```

线程与启停范式照抄热键线程（`hotkey/manager.rs`）：启动时 bounded channel 回传创建结果 + Win32 线程 id；停止 = `PostThreadMessageW(WM_QUIT)` + `join`；`Drop` 兜底，幂等。

## 主窗口标记

| 层 | 字段 | 说明 |
|---|---|---|
| 持久化 | `SlotConfig.is_main: bool`（config.rs） | `#[serde(default)]`，旧配置兼容 |
| 运行时 | `WindowSlot.is_main: bool`（window_slot.rs） | — |
| UI | 槽位序号左侧 ⚑ small_button（app.rs `ui_slot`） | 金色=主窗口 / 深灰=非主；点一下设为主并清除其他槽，再点取消 |

`AppConfig::normalize()` 保证全局至多一个 `is_main`（加载旧配置或手动编辑 config.json 时的保险）。符合项目配置哲学：**只存绑定意图，不存运行时状态（HWND）**。

## 覆盖窗实现要点（src/overlay.rs）

### 窗口属性

| 属性 | 值 | 理由 |
|---|---|---|
| 扩展样式 | `WS_EX_LAYERED \| WS_EX_TOPMOST \| WS_EX_TOOLWINDOW` | 半透明；始终置顶（主窗口被挡时仍可操作）；不进任务栏/Alt-Tab |
| 样式 | `WS_POPUP` | 无边框 → 整个窗口即客户区，覆盖窗坐标与主窗口客户区坐标 1:1，转发无需换算 |
| 透明度 | `SetLayeredWindowAttributes(..., 128, LWA_ALPHA)` | 整体 50%（v1 简单方案） |
| 窗口类 | `CS_DBLCLKS \| CS_HREDRAW \| CS_VREDRAW` | 双击消息必需；尺寸变化整体重绘 |
| 绘制 | `WM_PAINT`：深蓝灰底 `RGB(30,60,80)` + 白色粗体微软雅黑居中文字 | 深底保证 50% 透明叠在亮色游戏画面上仍可读 |

### 跟随定时器（50ms）

`WM_TIMER` 里：

- `IsWindow` 失效 → 经事件总线回报 `TargetLost` + `DestroyWindow` 自毁
- `IsIconic` / 不可见 → `SW_HIDE`（恢复后自动回来）
- 否则 `GetClientRect + ClientToScreen` 算出主窗口客户区的屏幕矩形（天然排除边框/标题栏/菜单栏），变化时 `SetWindowPos(HWND_TOPMOST, ..., SWP_NOACTIVATE | SWP_SHOWWINDOW)`

### 状态存放

窗口过程是裸 `extern "system" fn`，状态（目标 hwnd、事件发送端、上次矩形）放堆上经 `GWLP_USERDATA` 存取；`into_raw/from_raw` 各一次，唯一释放点在线程收尾。`HWND` 非 Send，状态全程不出覆盖窗线程，跨界只传 `isize`。

## 焦点模型与鼠标转发（关键设计）

### 覆盖窗刻意接受激活/焦点

转发模式下覆盖窗**理应持有焦点**——点击覆盖窗即获焦，点回工作窗口焦点就回去，焦点跟着用户的点击走（Windows 原生语义，无额外逻辑）。

### 为什么必须如此：滚轮规则

Windows 对鼠标消息有两套投递规则：

- **点击/移动/双击**：按命中测试送给"光标下方的窗口"。覆盖窗物理上盖住主窗口，这些消息自动进入覆盖窗自己的消息循环——直接原样转发，不涉及任何拦截手段。
- **滚轮（`WM_MOUSEWHEEL`/`WM_MOUSEHWHEEL`）**：按文档**只送给焦点窗口**，不看光标位置。覆盖窗若不拿焦点，滚轮永远进不了自己的消息循环。

**被否决的方案：全局低级鼠标钩子（`WH_MOUSE_LL`）**。它能在消息管道上截住滚轮，但 `SetWindowsHookEx` 是反作弊软件重点监控的经典模式，会给项目引入不必要的检测风险。焦点方案下滚轮直接进入本窗口消息循环，**零钩子**，风险面与既有 PostMessage 路线完全一致。

### 消息映射表

| 覆盖窗收到 | 转发给主窗口 | 备注 |
|---|---|---|
| `WM_MOUSEMOVE` | 同名消息，wParam/lParam 原样 | OS 派发时 wParam 已带 `MK_*` 位 |
| `WM_LBUTTONDOWN` 等 | 同名消息原样 | 按下时 `SetCapture`，拖拽出界不断流 |
| `WM_LBUTTONUP` 等 | 同名消息原样 | 弹起时 `ReleaseCapture` |
| `WM_XBUTTONDOWN/UP` | 同名消息原样 | 按文档返回 TRUE（侧键 id 在 wParam 高字，原样传） |
| `WM_*BUTTONDBLCLK` | 同名消息原样 | 依赖 `CS_DBLCLKS` |
| `WM_MOUSEWHEEL/HWHEEL` | 同名消息原样 | 焦点在覆盖窗时由 OS 直接送达；wParam 的 delta+MK 位、lParam 屏幕坐标都由 OS 组好 |
| `WM_CLOSE` | **吞掉** | 覆盖窗持焦时 Alt+F4 不应销毁它（只能由开关/主窗口关闭来停） |

### 键盘（预留）

覆盖窗持有焦点时键盘消息（`WM_KEYDOWN/UP`）同样送达本窗口，现阶段忽略。后续若需要，可直接用项目已有的 PostMessage 发键机制（`send_key_down/up`，已验证）转发给主窗口。

## 事件回报与生命周期

- `OverlayEvent::TargetLost` 经 `MainEventBus` 回报（自动获得"入队 + 唤醒主窗口"能力，托盘隐藏期间也不丢）
- `handle_overlay_event` 开头有 `if self.overlay.is_none() { return; }` 竞态保护（同 `handle_voice_event`）：UI 侧 stop 会 join 线程，但线程退出前发出的残留事件会被安全丢弃
- 联动场景：
  - 托盘退出 → `stop_overlay()`
  - 窗口失效巡检发现主窗口槽失效 → `stop_overlay()`（与覆盖窗线程 50ms 自检双保险）
  - 重抓主窗口槽 → 停旧起新（换跟踪目标）；抓到本程序自身清绑定 → 停
  - 转发中切换 ⚑ 标记 → 换目标或停止

## 验证

- 冒烟：`cargo run --example overlay_smoke`（3 秒倒计时后覆盖前台窗口，肉眼验证半透明/文字/跟随/最小化/关闭自毁）
- 端到端靶子推荐**记事本**（Edit 控件完整消费 PostMessage 鼠标）：抓取 → ⚑ → 开转发 → 单击定位光标、拖拽出选区（出界不断流）、双击选词、滚轮滚动、移动/缩放跟随、最小化隐藏/恢复回来、关闭窗口后状态栏提示且按钮回灰

## 限制与风险

- raw input / DirectInput 的游戏不响应 PostMessage 鼠标（与既有功能同边界；用户目标游戏已验证可行）
- 独占全屏盖不过（仅窗口化/无边框有效）
- 覆盖窗上是系统箭头，无法镜像游戏自绘光标（游戏内光标按转发坐标绘制正常）
- 50ms 跟随有轻微拖影；均匀 50% 透明下文字也半透明
- 覆盖窗持有焦点期间，键盘输入不会到达其他窗口（焦点模型使然；后续做键盘转发后即为功能）

## 未来演进（v2 可选）

- `UpdateLayeredWindow` 每像素 alpha：膜透字不透
- `SetWinEventHook(EVENT_OBJECT_LOCATIONCHANGE)` 替换定时器：跟随无拖影
- 键盘消息转发：覆盖窗焦点期间的按键同步给主窗口
