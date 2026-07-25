# 游戏自动按键软件 - 架构设计文档

## 项目概述

这是一个 Windows 平台下的自动化键鼠消息发送工具，支持向多个窗口（包括后台窗口）发送键盘和鼠标消息，通过自定义脚本控制执行逻辑，使用热键管理多窗口的自动化方案。

## 技术栈

- **语言**: Rust (edition 2021)
- **UI 框架**: egui + eframe (即时模式GUI)
- **Windows API**: windows-rs (官方 Windows API 绑定)
- **并发**: crossbeam-channel (线程间通信)
- **配置**: serde + serde_json

## 核心特性

1. **多窗口支持**: 最多同时管理 8 个目标窗口
2. **后台消息发送**: 通过 PostMessage 向非激活窗口发送键鼠消息（✅ 已验证可用）
3. **可扩展架构**: 输入后端抽象为 trait，支持运行时切换（PostMessage/SendInput/驱动级）
4. **脚本系统**: 自定义 .ag 脚本语言，支持条件判断和循环
5. **热键控制**: 复杂的热键状态机，支持窗口选择 + 执行控制
6. **多方案管理**: 每个窗口可绑定多套方案，标识默认方案

## 文档结构

- [项目结构](./01-project-structure.md) - 文件组织和模块划分
- [核心数据结构](./02-data-structures.md) - 主要类型定义
- [输入后端设计](./03-input-backend.md) - 输入抽象层和实现
- [脚本系统](./04-script-system.md) - .ag 脚本语法和执行器
- [热键管理](./05-hotkey-management.md) - 热键状态机和控制逻辑
- [执行引擎](./06-execution-engine.md) - 多线程执行架构
- [UI 设计](./07-ui-design.md) - egui 界面布局
- [配置管理](./08-configuration.md) - 配置文件格式
- [实施路线](./09-implementation-roadmap.md) - 分阶段开发计划
- [技术细节](./10-technical-details.md) - 重要注意事项

## 快速开始

参考 [实施路线](./09-implementation-roadmap.md) 了解分阶段开发计划。

## 设计原则

1. **模块化**: 每个功能独立模块，职责清晰
2. **可测试性**: 核心逻辑与 Windows API 解耦
3. **扩展性**: trait 抽象关键接口，支持多种实现
4. **性能**: 独立线程执行脚本，不阻塞 UI
5. **安全性**: Rust 类型系统保证内存安全和线程安全
