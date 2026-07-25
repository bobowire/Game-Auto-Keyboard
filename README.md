# Game Auto Keyboard

**游戏自动按键工具** - Windows 平台下的多窗口自动化键鼠操作工具

[![Rust](https://img.shields.io/badge/Rust-1.70+-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## ✨ 功能特性

### 核心功能
- 🎮 **多窗口管理**：支持同时管理 8 个游戏窗口
- 📜 **脚本系统**：自定义 .ag 脚本语言，支持键盘/鼠标/条件/循环
- ⌨️ **全局热键**：
  - `Ctrl+Shift+[1-8]` 选择窗口
  - `Ctrl+Shift+9` 循环启动 / `0` 停止
  - `Ctrl+Shift+-` 单次执行
  - `Ctrl+Shift+Insert` 即兴发送任意键
- 🎯 **即兴发送**：无需脚本，快速向窗口发送单个按键
- 📊 **智能状态管理**：
  - 窗口失效自动检测
  - 热键前缀选择（3秒超时）
  - 拒绝抓取自身窗口
- 💾 **配置持久化**：窗口绑定和方案自动保存

### 脚本语言特性
```ag
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

## 📁 项目结构

```
GameAutoKeyboard/
├── src/
│   ├── app.rs                    # 主应用 UI 和逻辑
│   ├── config.rs                 # 配置管理
│   ├── runner.rs                 # 脚本执行线程
│   ├── window_slot.rs            # 窗口槽位数据结构
│   ├── tray.rs                   # 系统托盘
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
│   └── utils/
│       └── win32.rs              # Windows API 工具函数
├── scripts/                      # 脚本目录 (.ag 文件)
├── config/                       # 配置目录 (config.json)
├── docs/                         # 完整设计文档
└── examples/                     # 示例和测试

```

## 🛠️ 技术栈

- **语言**: Rust 2021
- **UI 框架**: egui 0.29 + eframe
- **Windows API**: windows-rs 0.58
- **配置序列化**: serde + serde_json
- **系统托盘**: tray-icon 0.19
- **资源嵌入**: winres 0.1

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

### 进行中 🚧
- [ ] 颜色查找功能 (find_color)

### 规划中 📋
- [ ] SendInput 输入后端（前台窗口）
- [ ] 脚本调试器
- [ ] 图形化脚本编辑器
- [ ] 录制宏功能
- [ ] 更多条件判断（图像识别）

## ⚠️ 免责声明

本工具仅供学习和研究 Windows 自动化技术使用。请遵守游戏服务条款，不要用于破坏游戏平衡或违反用户协议的行为。

## 📄 许可证

MIT License - 详见 [LICENSE](LICENSE) 文件

## 🤝 贡献

欢迎提交 Issue 和 Pull Request！

---

Co-Authored-By: Claude Opus 4.8 (1M context)
