# 文档索引

本目录包含游戏自动按键软件的完整架构设计文档。

## 快速导航

### 📚 核心文档

| 文档 | 说明 | 适合人群 |
|------|------|---------|
| [README](./README.md) | 项目概述和文档导航 | 所有人 |
| [01-项目结构](./01-project-structure.md) | 文件组织和模块划分 | 开发者 |
| [02-数据结构](./02-data-structures.md) | 核心类型定义 | 开发者 |
| [09-实施路线](./09-implementation-roadmap.md) | 分阶段开发计划 | 项目管理者 |

### 🔧 技术设计

| 文档 | 说明 | 重点内容 |
|------|------|---------|
| [03-输入后端](./03-input-backend.md) | 输入抽象层设计 | PostMessage vs SendInput |
| [04-脚本系统](./04-script-system.md) | .ag 脚本语法和解析器 | 脚本语言设计 |
| [05-热键管理](./05-hotkey-management.md) | 热键状态机 | 1-8 前缀选择逻辑 |
| [06-执行引擎](./06-execution-engine.md) | 多线程执行架构 | 独立线程 + 可中断 |
| [07-UI设计](./07-ui-design.md) | egui 界面布局 | 窗口列表和脚本浏览 |
| [08-配置管理](./08-configuration.md) | 配置文件格式 | 持久化和热重载 |

### 📖 参考资料

| 文档 | 说明 |
|------|------|
| [10-技术细节](./10-technical-details.md) | Windows API 注意事项、调试技巧、常见问题 |
| [ADR-架构决策记录](./ADR.md) | 关键技术决策及理由（✅ 含 Trait 抽象设计） |

---

## 阅读建议

### 如果你是项目发起人
1. 先读 [README](./README.md) 了解整体架构
2. 看 [ADR](./ADR.md) 了解关键技术决策（特别是 Trait 抽象设计）
3. 看 [09-实施路线](./09-implementation-roadmap.md) 评估工作量
4. 浏览 [03-输入后端](./03-input-backend.md) 了解技术实现（✅ PostMessage 已验证）

### 如果你是开发者
1. 从 [01-项目结构](./01-project-structure.md) 开始理解模块划分
2. 阅读 [ADR](./ADR.md) 了解关键架构决策（为什么用 Trait？）
3. 阅读 [02-数据结构](./02-data-structures.md) 了解核心类型
4. 按开发顺序阅读技术文档（03→04→05→06→07→08）
5. 遇到问题查阅 [10-技术细节](./10-technical-details.md)

### 如果你想快速上手
1. 直接看 [09-实施路线](./09-implementation-roadmap.md) 的"下一步行动"
2. 参考 [04-脚本系统](./04-script-system.md) 的示例脚本
3. 查看 [10-技术细节](./10-technical-details.md) 的常见问题

---

## 设计原则

本项目遵循以下原则：

1. **模块化**: 每个功能独立模块，职责清晰
2. **可测试性**: 核心逻辑与 Windows API 解耦
3. **扩展性**: trait 抽象关键接口，支持多种实现
4. **性能**: 独立线程执行脚本，不阻塞 UI
5. **安全性**: Rust 类型系统保证内存安全和线程安全

---

## 关键技术决策

### ✅ 已确定

1. **语言**: Rust (性能 + 安全性)
2. **UI 框架**: egui (轻量 + 即时模式)
3. **输入方式**: PostMessage 为主，SendInput 备用
4. **脚本语言**: 自定义 .ag 格式（简单直观）
5. **并发模型**: 每窗口独立线程

### ⏳ 待决定

1. **截图方式**: BitBlt / PrintWindow / Windows.Graphics.Capture
2. **热键库**: 自实现 vs 第三方库
3. **配置格式**: JSON vs TOML
4. **打包方式**: 单文件 exe vs 安装包

---

## 开发环境

### 必需工具
- Rust 1.70+ (推荐最新稳定版)
- Git
- Visual Studio Build Tools (Windows API 支持)

### 推荐工具
- Visual Studio Code + rust-analyzer
- Spy++ (消息监控)
- Process Explorer (进程查看)

### 运行环境
- Windows 10+ (64位)
- .NET Framework 4.7.2+ (egui 依赖)

---

## 文档维护

### 更新原则
- 设计变更时同步更新文档
- 每个阶段完成后补充实际经验
- 记录遇到的坑和解决方案

### 版本历史
- v1.0 (2026-07-24): 初始设计文档

---

## 联系方式

有问题或建议？

- 提交 Issue
- 发起 Pull Request
- 查看项目 Wiki

---

## 许可证

本文档与项目代码采用相同许可证。
