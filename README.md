# 游戏自动按键工具

Windows 平台下的自动化键鼠消息发送工具。

## 当前进度

**阶段 1 - 核心框架** ✅ 进行中

- ✅ 项目结构搭建
- ✅ InputBackend trait 定义
- ✅ PostMessageBackend 实现
- ✅ ScriptExecutor 实现
- ⏳ 简单脚本解析器（待实现）
- ⏳ 基础 UI 界面（待实现）

## 快速开始

### 构建项目

```bash
cargo build
```

### 运行测试工具

测试 PostMessage 输入是否工作：

```bash
cargo run --example test_input
```

**使用方法**：
1. 运行命令后有 5 秒准备时间
2. 快速点击目标窗口（如记事本）
3. 程序会向该窗口发送 "Hello" 字符
4. 查看窗口是否收到输入

### 运行主程序

```bash
cargo run
```

目前主程序只是一个骨架，会输出状态信息。

## 项目结构

```
src/
├── script/          # 脚本系统
│   ├── ast.rs       # AST 定义
│   └── mod.rs
├── input/           # 输入后端
│   ├── backend.rs   # Trait 定义
│   ├── post_message.rs  # PostMessage 实现
│   └── mod.rs
├── executor/        # 执行引擎
│   ├── executor.rs  # 脚本执行器
│   └── mod.rs
├── lib.rs           # 库入口
└── main.rs          # 主程序

examples/
└── test_input.rs    # PostMessage 测试工具

docs/                # 完整设计文档
```

## 功能特性

### 已实现

- ✅ **InputBackend Trait**: 可扩展的输入后端抽象
- ✅ **PostMessage 后端**: 支持后台窗口发送（已验证可用）
- ✅ **ScriptExecutor**: 脚本执行引擎，支持可中断循环
- ✅ **基础命令**: down/up/click/click_ms/delay_ms

### 规划中

- ⏳ 脚本解析器（.ag 文件格式）
- ⏳ UI 界面（egui）
- ⏳ 多窗口管理
- ⏳ 热键系统
- ⏳ 条件判断（if/else）
- ⏳ 截图找色

## 开发文档

详细的架构设计和实施计划见 [docs/](docs/) 目录：

- [架构设计总览](docs/README.md)
- [输入后端设计](docs/03-input-backend.md)
- [架构决策记录](docs/ADR.md)
- [实施路线图](docs/09-implementation-roadmap.md)

## 技术栈

- **语言**: Rust 2021
- **UI**: egui + eframe
- **Windows API**: windows-rs 0.58
- **并发**: crossbeam-channel
- **日志**: env_logger

## 许可证

MIT
