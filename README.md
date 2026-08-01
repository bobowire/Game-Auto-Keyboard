# Game Auto Keyboard

**游戏自动按键工具** - Windows 平台下的多窗口自动化键鼠操作工具

[![Rust](https://img.shields.io/badge/Rust-1.70+-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## ✨ 功能特性

### 核心功能
- 🎮 **多窗口管理**：支持同时管理 8 个游戏窗口
- 📜 **脚本系统**：自定义 .ag 脚本语言，支持键盘/鼠标/条件/循环
- 🎤 **语音控制**：说"小助手，窗口1跟随我"即可执行脚本（✨ 新功能）
- ⌨️ **全局热键**：
  - `Ctrl+Shift+[1-8]` 选择窗口
  - `Ctrl+Shift+9` 循环启动 / `0` 停止
  - `Ctrl+Shift+-` 单次执行
  - `Ctrl+Shift+Insert` 即兴发送任意键
- 🎯 **即兴发送**：无需脚本，快速向窗口发送单个按键
- 🖱 **鼠标/键盘转发（覆盖窗模式）**：半透明覆盖窗盖住主窗口，把鼠标/键盘操作转发给一个或多个绑定窗口（多开可同步）；可配置右键移动广播、键盘转发、键盘仅主窗口等开关
- 📊 **智能状态管理**：
  - 窗口失效自动检测
  - 热键前缀选择（3秒超时）
  - 拒绝抓取自身窗口
- 💾 **配置持久化**：窗口绑定和方案自动保存

### 脚本语言特性
```ag
# 脚本设置（可选）
setting(audio_onlyonce)  # 语音场景下仅执行一次（执行一轮后自动停止）

# 键盘操作
click(H)              # 单击按键
click_ms(H, 100)      # 长按 100ms
down(1) / up(1)       # 按下/弹起

# 鼠标操作
mouse_move(100, 200)           # 绝对坐标移动
mouse_move_center(0, 50)       # 相对中心偏移
mouse_move_percent(50, 60)     # 百分比定位
mouse_click(left, 100, 200)    # 鼠标点击
mouse_down/mouse_up            # 按下/弹起

# 窗口消息
send_window_active()           # 发送窗口激活消息

# 流程控制
delay_ms(1000)                 # 延迟
if (条件) { ... } else { ... } # 条件判断
loop { ... }                   # 循环
find_color(...)                # 颜色查找（规划中）
```

### 脚本设置项

| 设置 | 作用 |
|------|------|
| `setting(audio_onlyonce)` | 语音场景下仅执行一次。语音触发时执行一轮后自动停止，适合"跟随"、"加血"等单次动作 |

**示例**：
```ag
// 跟随.ag - 语音单次执行
setting(audio_onlyonce)

click(1)
delay_ms(500)
```

语音指令 `"窗口1跟随我"` → 执行一轮后自动停止

## 🚀 快速开始

### 环境要求
- Windows 10/11
- Rust 1.70+ ([安装指南](https://www.rust-lang.org/tools/install))
- Git

### 构建运行
```bash
# 克隆仓库
git clone git@gitee.com:wireboy/game-multi-utils.git
cd game-multi-utils

# 调试构建
cargo build

# Release 构建（推荐）
cargo build --release

# 运行（以管理员权限运行以绕过 UIPI 限制）
cargo run --release
```

### 使用构建脚本（Windows）
```cmd
REM 运行 build.bat，选择对应选项
build.bat

[1] Debug build    - 调试构建
[2] Release build  - 发布构建
[3] Run main       - 运行主程序
[4] Run tests      - 运行测试
```

## 📖 使用说明

### 1. 抓取窗口
1. 点击某个槽位的 **"抓取窗口"** 按钮
2. 3 秒倒计时内切换到目标窗口
3. 倒计时结束后自动绑定

### 2. 添加脚本方案
1. 在 `scripts/` 目录下创建 `.ag` 脚本文件
2. 点击 **"重载脚本"** 刷新脚本列表
3. 点击槽位的 **"添加方案"** 按钮选择脚本
4. 设置某个方案为 **标识方案**（★）

### 3. 执行脚本
**通过 UI**：
- 点击 **"启动"** 开始循环执行
- 点击 **"停止"** 停止执行

**通过热键**（需先选择标识方案）：
```
Ctrl+Shift+1          # 选中窗口1（再按一次停止该窗口）
Ctrl+Shift+9          # 启动选中的窗口（无选择则全部启动）
Ctrl+Shift+0          # 停止选中的窗口（无选择则全部停止）
Ctrl+Shift+-          # 单次执行选中窗口的脚本
```

### 4. 即兴发送任意键
```
Ctrl+Shift+1          # 选中窗口1（可选）
Ctrl+Shift+Insert     # 进入发送模式
Ctrl+Shift+H          # 2秒内按任意键（支持A-Z/0-9/F1-F12/空格等）
```

### 5. 语音控制（✨ 新功能）

#### 初次设置
1. **训练唤醒词**（仅需一次）
   ```bash
   # 使用 build.bat 选择 [a] wakeword_test
   # 或直接运行
   cargo run --example wakeword_test --release
   
   # 按提示录制4遍"小助手"，生成 wakeword_model.rpw
   ```

2. **配置百度语音识别**
   - 点击工具栏 **"⚙ 语音设置"**
   - 填写百度 API Key / Secret Key（[申请地址](https://console.bce.baidu.com/ai/#/ai/speech/overview/index)）
   - 点击 **"💾 保存密钥"**

3. **准备窗口和脚本**
   - 抓取窗口、添加脚本（如 `跟随.ag`、`加血.ag`）
   - 可编辑窗口名：点击槽位标题行的窗口名，改成"主号"、"辅助"等

#### 使用语音指令
1. 点击工具栏 **"🎤 语音: 关"** 切换为 **"🎤 语音: 开"**
2. 底部状态显示 "🎤 语音待命：说\"小助手\"唤醒"
3. 说出指令：

```
"小助手，窗口1跟随我"          → 窗口1执行包含"跟随"的脚本
"小助手，窗口1快加血"          → 窗口1执行包含"加血"的脚本
"小助手，所有人停止"           → 停止全部窗口
"小助手，窗口1停止"            → 停止窗口1
```

**说明**：
- 动作关键词（如"跟随我"）会匹配脚本名包含该词的脚本（"跟随.ag"）
- 脚本需要先添加到对应窗口的方案集
- 支持中文数字："窗口一"自动识别为"窗口1"
- 日志输出到 `voice_debug.log`，可查看识别过程

**故障排查**：详见 [VOICE_DEBUG.md](VOICE_DEBUG.md)

### 6. 鼠标/键盘转发（覆盖窗模式）

适合「一边在别的窗口干活，一边操作主号游戏窗口」。覆盖窗持焦时把鼠标/键盘消息经 PostMessage 转发给绑定窗口（多开可同步操作）。

1. 把某个槽位标记为**主窗口**（点槽位左侧 **⚑**，全局互斥）
2. 点工具行 **🖱 转发** 开关 → 半透明覆盖窗盖住主窗口客户区
3. 在覆盖窗上的鼠标点击/拖拽/双击/滚轮即转发给绑定窗口；按 **`Ctrl+Q`** 关闭转发

**转发配置**（设置 → 🖱 转发，默认全关，改动保存后需关闭再开启「🖱 转发」才生效）：

| 开关 | 作用 |
|------|------|
| 右键按下时广播鼠标移动 | 关闭后，按住右键拖动期间不转发鼠标移动（规避右键拖视角的反馈环）；右键按下/弹起仍转发 |
| 转发键盘消息 | 开启后覆盖窗持焦时的按键转发给目标窗口（`Ctrl+Q` 仍为关闭快捷键，不转发） |
| 键盘只发给主窗口 | 开 = 键盘只发给 ⚑ 主窗口；关 = 广播给所有绑定窗口（鼠标消息不受影响） |

> 设计细节与限制（如不转发 WM_CHAR、目标集合为开启时的快照、raw input 游戏不响应等）详见 [docs/12-mouse-forwarding.md](docs/12-mouse-forwarding.md)。

## 📁 项目结构

```
GameAutoKeyboard/
├── src/
│   ├── app.rs                    # 主应用 UI 和逻辑
│   ├── config.rs                 # 配置管理
│   ├── runner.rs                 # 脚本执行线程
│   ├── window_slot.rs            # 窗口槽位数据结构
│   ├── tray.rs                   # 系统托盘
│   ├── overlay.rs                # 鼠标/键盘转发覆盖窗
│   ├── event_bus.rs              # 统一事件总线（后台事件唤醒主线程）
│   ├── color_picker.rs           # 取色器
│   ├── capture/                  # 截图与颜色捕获（后台截图 + 颜色匹配）
│   ├── hotkey/
│   │   ├── manager.rs            # 热键注册和消息循环
│   │   └── state_machine.rs     # 热键状态机
│   ├── input/
│   │   ├── backend.rs            # 输入后端 trait
│   │   ├── post_message.rs       # PostMessage 实现
│   │   └── keymap.rs             # 按键映射
│   ├── script/
│   │   ├── ast.rs                # 抽象语法树
│   │   ├── parser.rs             # 脚本解析器
│   │   ├── executor.rs           # 脚本执行器
│   │   ├── loader.rs             # 脚本加载器
│   │   └── token.rs              # 词法分析
│   ├── voice/                    # 语音控制模块（✨ 新增）
│   │   ├── capture.rs            # 麦克风采集
│   │   ├── ring_buffer.rs        # 环形缓冲
│   │   ├── wakeword.rs           # 唤醒词检测
│   │   ├── vad.rs                # VAD 静音检测
│   │   ├── baidu_asr.rs          # 百度语音识别
│   │   ├── intent.rs             # 意图解析
│   │   ├── runtime.rs            # 语音运行时
│   │   └── vlog.rs               # 调试日志
│   └── utils/
│       └── win32.rs              # Windows API 工具函数
├── scripts/                      # 脚本目录 (.ag 文件)
├── config/                       # 配置目录 (config.json)
├── docs/                         # 完整设计文档
├── examples/                     # 示例和测试
│   ├── wakeword_test.rs          # 唤醒词训练工具
│   └── asr_test.rs               # ASR 测试
├── VOICE_DEBUG.md                # 语音调试指南
└── wakeword_model.rpw            # 唤醒词模型（需训练生成）

```

## 🛠️ 技术栈

- **语言**: Rust 2021
- **UI 框架**: egui 0.29 + eframe
- **Windows API**: windows-rs 0.58
- **配置序列化**: serde + serde_json
- **系统托盘**: tray-icon 0.19
- **资源嵌入**: winres 0.1
- **语音控制** (✨ 新增):
  - 音频采集: cpal 0.15 (WASAPI)
  - 唤醒词检测: rustpotter 3.0.2
  - VAD 静音检测: webrtc-vad 0.4
  - 语音识别: 百度短语音识别 API
  - HTTP 客户端: ureq 2

## 🔧 高级配置

### 权限问题
如果目标窗口以管理员权限运行，本程序也需要管理员权限才能发送消息（Windows UIPI 限制）。

**解决方案**：
- 右键以管理员身份运行
- 或修改 `manifest.xml` 自动请求权限

### 脚本开发
查看 `scripts/example.ag` 了解完整语法：
```ag
# 示例：自动打怪循环
loop {
    # 技能1
    click(1)
    delay_ms(500)
    
    # 技能2
    click(2)
    delay_ms(300)
    
    # 条件判断（需实现颜色查找）
    if (find_color(100, 100, 50, 50, 0xFF0000) == true) {
        click(H)  # 血量低了嗑药
    }
}
```

## 📝 开发文档

详细文档见 [docs/](docs/) 目录：
- [架构设计总览](docs/README.md)
- [输入后端设计](docs/03-input-backend.md)
- [脚本系统设计](docs/04-script-system.md)
- [热键管理](docs/05-hotkey-management.md)
- [鼠标事件转发（覆盖窗）](docs/12-mouse-forwarding.md)
- [实施路线图](docs/09-implementation-roadmap.md)

## 🗺️ 开发路线

### 已完成 ✅
- [x] 多窗口槽位管理
- [x] PostMessage 输入后端
- [x] 脚本解析和执行引擎
- [x] 全局热键系统
- [x] 即兴发送任意键
- [x] 系统托盘支持
- [x] 窗口失效检测
- [x] 鼠标移动命令
- [x] 窗口激活消息
- [x] 颜色查找功能 (find_color) - 后台截图 + 颜色匹配
- [x] 鼠标/键盘转发覆盖窗（ForwardConfig 三开关：右键移动广播 / 键盘转发 / 键盘仅主窗口）
- [x] 语音控制完整流水线
  - [x] 麦克风采集（WASAPI 共享模式）
  - [x] 环形缓冲（3秒，防指令丢失）
  - [x] 唤醒词检测（rustpotter，本地模板匹配）
  - [x] VAD 静音检测（webrtc-vad）
  - [x] 百度语音识别 API
  - [x] 规则匹配意图解析（窗口名+动作关键词）
  - [x] 脚本自动匹配与执行
  - [x] 可编辑窗口名
  - [x] 语音设置 UI（密钥配置、状态显示）
  - [x] 全链路调试日志（voice_debug.log）

### 进行中 🚧
- [x] 语音控制系统（Phase 1-3 已完成）
  - [x] 音频采集与唤醒词检测
  - [x] 百度语音识别集成
  - [x] 规则匹配意图识别
  - [ ] Whisper 离线 ASR（待 LLVM 环境）

### 规划中 📋
- [ ] SendInput 输入后端（前台窗口）
- [ ] 脚本调试器
- [ ] 图形化脚本编辑器
- [ ] 录制宏功能
- [ ] 更多条件判断（图像识别）

### 未来扩展 🌟

**已实现的语音控制系统** 采用规则匹配方案，低延迟、高准确率。未来可扩展的方向：

1. **离线 ASR**
   - 集成 Whisper 本地推理（需解决 LLVM 依赖）
   - 完全离线，保护隐私
   
2. **AI 意图理解**（可选升级）
   - 接入 LLM（GPT/Claude/本地模型）理解复杂指令
   - 示例："打完这波怪就切换到采集" → 自动串联多个动作
   - 上下文记忆：学习用户习惯
   
3. **语音反馈**
   - TTS 播报执行结果（edge-tts / piper-tts）
   - 双向语音交互
   
4. **多轮对话**
   - 支持追问和确认
   - 模糊指令补全

**当前语音控制已支持**：
```
"小助手，窗口1跟随我"     → ✅ 执行包含"跟随"的脚本
"小助手，所有人停止"       → ✅ 停止全部窗口
"小助手，窗口1快加血"     → ✅ 执行包含"加血"的脚本
```

## ⚠️ 免责声明

本工具仅供学习和研究 Windows 自动化技术使用。请遵守游戏服务条款，不要用于破坏游戏平衡或违反用户协议的行为。

## 📄 许可证

MIT License - 详见 [LICENSE](LICENSE) 文件

## 🤝 贡献

欢迎提交 Issue 和 Pull Request！

---

Co-Authored-By: Claude Opus 4.8 (1M context)
