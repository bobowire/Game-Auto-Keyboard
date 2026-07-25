# 项目结构

## 目录组织

```
game-auto-keyboard/
├── Cargo.toml                  # 项目配置和依赖
├── README.md                   # 项目说明
├── docs/                       # 设计文档（本目录）
├── scripts/                    # 脚本目录（.ag 文件）
│   └── example.ag
├── config/                     # 配置文件
│   └── window_config.json
└── src/
    ├── main.rs                 # 程序入口
    ├── app.rs                  # 主应用状态机
    │
    ├── hotkey/                 # 热键模块
    │   ├── mod.rs              # 模块入口和导出
    │   ├── manager.rs          # HotkeyManager（注册和轮询）
    │   └── state_machine.rs    # 状态机（1-8前缀选择逻辑）
    │
    ├── window/                 # 窗口管理模块
    │   ├── mod.rs
    │   ├── slot.rs             # WindowSlot（单个窗口槽位）
    │   └── selector.rs         # 窗口选择器（捕获HWND）
    │
    ├── script/                 # 脚本模块
    │   ├── mod.rs
    │   ├── parser.rs           # .ag 解析器
    │   ├── ast.rs              # AST 定义
    │   ├── executor.rs         # 脚本执行器
    │   └── loader.rs           # 脚本加载器（扫描目录）
    │
    ├── input/                  # 输入后端模块
    │   ├── mod.rs
    │   ├── backend.rs          # InputBackend trait
    │   ├── post_message.rs     # PostMessage 实现（后台）
    │   └── send_input.rs       # SendInput 实现（前台）
    │
    ├── capture/                # 【阶段5】截图找色模块
    │   ├── mod.rs
    │   ├── backend.rs          # CaptureBackend trait
    │   ├── bitblt.rs           # BitBlt 实现
    │   └── color_match.rs      # 颜色匹配算法
    │
    ├── scheme/                 # 方案管理模块
    │   ├── mod.rs
    │   ├── scheme.rs           # Scheme 结构
    │   └── manager.rs          # SchemeManager（全局方案管理）
    │
    ├── executor/               # 执行引擎模块
    │   ├── mod.rs
    │   └── runner.rs           # SchemeRunner（单窗口执行线程）
    │
    ├── ui/                     # UI 模块
    │   ├── mod.rs
    │   ├── window_list.rs      # 窗口列表面板
    │   ├── scheme_list.rs      # 方案列表面板
    │   └── script_viewer.rs    # 脚本浏览面板
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
| `app.rs` | 主应用状态，协调各模块 | 所有模块 |

### 功能模块

| 模块 | 职责 | 主要类型 |
|------|------|---------|
| `hotkey` | 全局热键注册和状态机 | `HotkeyManager`, `HotkeyStateMachine` |
| `window` | 窗口槽位管理和选择 | `WindowSlot`, `WindowSelector` |
| `script` | 脚本解析、AST定义、执行 | `Script`, `Parser`, `ScriptExecutor` |
| `input` | 输入后端抽象和实现 | `InputBackend`, `PostMessageBackend` |
| `capture` | 截图和颜色查找【后期】 | `CaptureBackend` |
| `scheme` | 方案加载和管理 | `Scheme`, `SchemeManager` |
| `executor` | 多线程执行引擎 | `SchemeRunner`, `ExecutorManager` |
| `ui` | egui 界面组件 | 各种 UI 函数 |
| `utils` | 通用工具和 Win32 封装 | `get_window_title`, `is_window_valid` |

## 模块间依赖关系

```
           ┌─────────┐
           │  main   │
           └────┬────┘
                │
           ┌────▼────┐
           │   app   │◄──────────────┐
           └────┬────┘               │
                │                    │
      ┌─────────┼─────────┐          │
      │         │         │          │
  ┌───▼──┐  ┌──▼───┐  ┌──▼────┐     │
  │hotkey│  │window│  │executor│     │
  └───┬──┘  └──┬───┘  └──┬────┘     │
      │        │         │          │
      │        │      ┌──▼──┐       │
      │        │      │runner│──────┘
      │        │      └──┬──┘
      │        │         │
      │     ┌──▼─────────▼──┐
      │     │    script     │
      │     └──┬────────────┘
      │        │
      │     ┌──▼────┐
      │     │ input │
      │     └───────┘
      │
   ┌──▼──┐
   │ ui  │
   └─────┘
```

## 编译和运行

### 依赖 (Cargo.toml)

```toml
[package]
name = "game-auto-keyboard"
version = "0.1.0"
edition = "2021"

[dependencies]
eframe = "0.29"
egui = "0.29"
windows = { version = "0.58", features = [
    "Win32_Foundation",
    "Win32_UI_WindowsAndMessaging",
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_Graphics_Gdi",
    "Win32_System_Threading",
] }
crossbeam-channel = "0.5"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
log = "0.4"
env_logger = "0.11"
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
