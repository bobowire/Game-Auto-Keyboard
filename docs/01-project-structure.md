# 项目结构

## 目录组织

```
game-auto-keyboard/
├── Cargo.toml                  # 项目配置和依赖
├── Cargo.lock                  # 依赖锁定
├── README.md                   # 项目说明
├── CHANGELOG.md                # 版本变更记录
├── LICENSE                     # 开源协议
├── build.rs                    # 构建脚本（Windows 资源/图标，winres）
├── manifest.xml                # UAC 清单（请求管理员权限）
├── docs/                       # 设计文档（本目录）
├── scripts/                    # 脚本与构建脚本目录（.ag / .bat）
│   ├── example.ag
│   ├── test_setting.ag
│   ├── 跟随.ag
│   └── build.bat
├── examples/                   # 独立示例与冒烟测试
│   ├── simple_test.rs
│   ├── script_test.rs
│   ├── capture_test.rs
│   ├── audio_test.rs
│   ├── asr_test.rs
│   ├── wakeword_test.rs
│   └── overlay_smoke.rs        # 覆盖窗冒烟测试
├── assets/                     # 资源文件（提示音 wav 等）
│   ├── beep_success.wav
│   └── beep_fail.wav
└── src/
    ├── main.rs                 # 程序入口
    ├── lib.rs                  # 模块声明与常用类型重导出
    ├── app/                    # 主应用（App 状态机 + UI + 各业务子系统）
    │   ├── mod.rs              #   App 结构体、构造、配置读写、eframe::App::update 编排
    │   ├── events.rs           #   事件分发枢纽（dispatch / hotkey / tray / apply_action）
    │   ├── slots.rs            #   槽位/窗口执行（Runner 启停与批量调度）
    │   ├── overlay.rs          #   鼠标转发覆盖窗开启/关闭/事件处理
    │   ├── voice_ctrl.rs       #   语音编排（启停 / 意图解析 / 脚本匹配执行）
    │   ├── wakeword_train.rs   #   唤醒词训练（录音 → 裁剪静音 → 训练模型）
    │   ├── grab.rs             #   抓取窗口 / 取色倒计时
    │   └── ui/                 #   egui 界面渲染
    │       ├── mod.rs          #     状态栏/源码面板/设置窗口外壳/中央面板编排
    │       ├── slot.rs         #     单槽位卡片 UI
    │       ├── settings.rs     #     设置窗口各标签页（通用/语音/转发/热键/关于）
    │       └── guides.rs       #     帮助/引导弹窗（语音帮助/百度/唤醒词）
    ├── config.rs               # 配置持久化（方案绑定，不存运行时 HWND）
    ├── runner.rs               # 执行引擎（Runner，单窗口执行线程）
    ├── window_slot.rs          # 窗口槽位与方案（WindowSlot / Scheme）
    ├── event_bus.rs            # 统一事件总线（MainEventBus）
    ├── overlay.rs              # 鼠标事件转发覆盖窗（layered 窗口）
    ├── tray.rs                 # 系统托盘（图标 + 右键菜单）
    ├── color_picker.rs         # 取色器（截图取色，记录坐标/颜色）
    │
    ├── hotkey/                 # 热键模块
    │   ├── mod.rs              # 模块入口和导出
    │   ├── manager.rs          # HotkeyManager（注册和轮询）
    │   └── state_machine.rs    # 状态机（1-8前缀选择逻辑）
    │
    ├── input/                  # 输入后端模块
    │   ├── mod.rs              # 模块入口与类型重导出（InputBackend / PostMessageBackend / keymap）
    │   ├── backend.rs          # InputBackend trait
    │   ├── post_message.rs     # PostMessage 实现（唯一实际后端）
    │   └── keymap.rs           # 按键名 ↔ VK 码 / 鼠标按钮解析
    │
    ├── script/                 # 脚本模块
    │   ├── mod.rs
    │   ├── token.rs            # 词法 token
    │   ├── parser.rs           # .ag 解析器
    │   ├── ast.rs              # AST 定义
    │   ├── executor.rs         # 脚本执行器
    │   ├── loader.rs           # 脚本加载器（扫描目录）
    │   └── tests.rs            # 单元测试
    │
    ├── capture/                # 截图找色模块
    │   ├── mod.rs
    │   ├── backend.rs          # CaptureBackend trait
    │   ├── print_window.rs     # PrintWindow 实现（后台窗口截图）
    │   └── color.rs            # 位图与颜色匹配（Bitmap / color_exists_in_area）
    │
    ├── voice/                  # 语音控制子系统
    │   ├── mod.rs              # 模块入口与类型重导出
    │   ├── runtime.rs          # VoiceRuntime（统一调度）
    │   ├── capture.rs          # 音频采集（cpal）
    │   ├── ring_buffer.rs      # 环形音频缓冲
    │   ├── wakeword.rs         # 唤醒词检测（rustpotter）
    │   ├── vad.rs              # 语音端点检测（webrtc-vad）+ 录音状态机
    │   ├── dsp.rs              # 信号处理（含 RNNoise 降噪）
    │   ├── audio_util.rs       # 音频工具（trim_silence / rms）
    │   ├── baidu_asr.rs        # 百度语音识别
    │   ├── intent.rs           # 意图解析与脚本匹配
    │   └── vlog.rs             # 语音模块专用日志宏
    │
    └── utils/                  # 工具模块
        ├── mod.rs
        └── win32.rs            # Windows API 封装
```

## 模块职责

### 核心模块

| 模块 | 职责 | 依赖 |
|------|------|------|
| `main.rs` | 程序入口，初始化 eframe 应用 | `app`, `utils` |
| `lib.rs` | 声明所有子模块，重导出常用类型 | 所有模块 |
| `app/` | App 状态机 + UI + 业务子系统编排（结构体与 `update` 在 `app/mod.rs`，实现分见各子模块） | 所有模块 |

### 功能模块

| 模块 | 职责 | 主要类型 |
|------|------|---------|
| `hotkey` | 全局热键注册和状态机 | `HotkeyManager`, `HotkeyStateMachine` |
| `window_slot` | 窗口槽位与方案绑定 | `WindowSlot`, `Scheme` |
| `script` | 脚本解析、AST定义、执行 | `Script`, `Parser`, `ScriptExecutor` |
| `input` | 输入后端抽象和实现 | `InputBackend`, `PostMessageBackend` |
| `capture` | 后台窗口截图和颜色查找 | `CaptureBackend`, `PrintWindowCapture` |
| `runner` | 单窗口多线程执行引擎 | `Runner` |
| `config` | 方案配置持久化（config.json） | `AppConfig` |
| `event_bus` | 统一事件总线，后台事件源唤醒主窗口 | `MainEventBus`, `EventSender`, `MainEvent` |
| `overlay` | 鼠标事件转发覆盖窗（焦点模型，零钩子） | `OverlayWindow`, `OverlayEvent` |
| `tray` | 系统托盘图标与右键菜单 | `TrayCommand` |
| `color_picker` | 截图取色器，记录坐标/颜色 | `PickedColor` |
| `voice` | 语音控制子系统（采集→唤醒→VAD→ASR→意图） | `VoiceRuntime`, `VoiceEvent`, `VoiceConfig` |
| `utils` | 通用工具和 Win32 封装 | `get_window_title`, `is_window_valid` |

### 关于输入后端的说明

`input` 模块用 `InputBackend` trait 抽象输入策略，当前唯一实现是 `PostMessageBackend`（`Runner` / `overlay` / `app` 直接实例化，无管理器；早期预留的 `InputManager` 已作为死代码移除）。
设计文档最初设想的 SendInput 前端后端为**设计预留、未实现**，对应文件（`send_input.rs`）并不存在。

## 模块间依赖关系

```
              ┌─────────┐
              │  main   │
              └────┬────┘
                   │
              ┌────▼────┐
   后台事件 ──►│   app   │◄──────────────┐
              └────┬────┘               │
                   │                    │
     ┌─────────┬───┴────┬─────────┐     │
     │         │        │         │     │
 ┌──▼──┐  ┌───▼────┐ ┌──▼───┐ ┌───▼──┐  │
 │hotkey│ │window_ │ │runner│ │voice │  │
 └──────┘ │ slot   │ └──┬───┘ └───┬──┘  │
          └────────┘    │         │     │
                     ┌──▼────┐    │     │
                     │script │◄───┘     │
                     └──┬────┘          │
                        │               │
                     ┌──▼────┐          │
                     │ input │          │
                     └───────┘          │
                                        │
   事件总线：hotkey/voice/overlay/tray ──┘
   经 EventSender 投递 MainEvent，
   PostMessage(WM_PAINT) 唤醒主窗口
```

`event_bus` 是后台事件源（热键、语音、覆盖窗、托盘）与主线程 `App::update` 之间的统一通道：
窗口隐藏到托盘时不再产生 WM_PAINT，事件总线通过 `PostMessageW(WM_PAINT)` 强制产生一帧
`update` 来消费队列，避免轮询点失效（详见 `docs/12-mouse-forwarding.md` 与事件总线源码注释）。

`overlay`（鼠标转发覆盖窗）和 `voice`（语音控制）均为独立子系统，详见各自设计文档：
`docs/12-mouse-forwarding.md`、`docs/11-voice-control-system.md`。

## 编译和运行

### 依赖 (Cargo.toml)

```toml
[package]
name = "game-auto-keyboard"
version = "0.1.0"
edition = "2021"

[dependencies]
# UI
eframe = "0.29"
egui = "0.29"

# Windows API
windows = { version = "0.58", features = [
    "Win32_Foundation",
    "Win32_UI_WindowsAndMessaging",
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_Graphics_Gdi",
    "Win32_System_Threading",
    "Win32_System_LibraryLoader",
    "Win32_Storage_Xps",
    "Win32_Media_Audio",
    "Win32_Media",
] }

# 并发
crossbeam-channel = "0.5"

# 序列化（配置保存）
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# 日志
log = "0.4"
env_logger = "0.11"

# 系统托盘
tray-icon = "0.19"
raw-window-handle = "0.6.2"   # 取主窗口 HWND

# 音频采集与语音控制
cpal = "0.15"
rustpotter = "3.0.2"          # 唤醒词
webrtc-vad = "0.4.0"          # 语音端点检测
ureq = { version = "2", features = ["json"] }   # 百度 ASR HTTP
base64 = "0.22"
nnnoiseless = { version = "0.5", default-features = false }   # RNNoise 降噪

# 拼音（语音指令辅助匹配）
pinyin = { version = "0.11", default-features = false, features = ["plain", "heteronym"] }

[build-dependencies]
winres = "0.1"
```

### 构建

```bash
# 开发构建
cargo build

# 发布构建（优化）
cargo build --release

# 运行
cargo run
```
