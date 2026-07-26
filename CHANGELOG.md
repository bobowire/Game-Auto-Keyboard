# 更新日志

所有重要的项目变更都会记录在这个文件中。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/)，
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [未发布]

## [1.0.0] - 2026-07-26

### 新增
- ✨ **语音控制系统**（Phase 1-3 完整实现）
  - 麦克风音频采集（WASAPI 共享模式，与游戏语音软件共存）
  - 环形缓冲（3秒，防止连读时指令开头丢失）
  - 唤醒词检测（rustpotter 本地模板匹配，需用户录制训练）
  - VAD 静音检测（webrtc-vad，自动识别语音结束）
  - 百度语音识别 API（Token 30天自动缓存）
  - 规则匹配意图解析（窗口名+动作关键词）
  - 中文数字归一化（"窗口一" → "窗口1"）
  - 脚本名包含匹配（"跟随我" 匹配 "跟随.ag"）
  - 可编辑窗口名（点击槽位标题修改）
  - 语音设置 UI（密钥配置、模型状态、使用说明）
  - 全链路调试日志（voice_debug.log）
  - 唤醒词训练 GUI 集成
- 🛠️ 构建脚本优化
  - 构建后自动部署 exe 到指定目录
  - 方案选择界面显示脚本有效性
- ⚙️ 通用配置系统
  - 新增日志开关配置
  - 热键配置支持（统一设置窗口）
- 🔧 引导流程优化
  - 语音控制前置检查（密钥、模型）
  - 百度密钥申请引导（独立 OS 窗口）
  - 首次使用引导流程

### 修复
- 🐛 修复隐藏到托盘后菜单"显示/退出"无响应
- 🐛 修复脚本匹配中文字符边界 panic
- 🐛 优化引导窗口显示，确保独立弹出

### 变更
- 📝 更新 README，添加完整语音控制使用说明
- 📝 新增 VOICE_DEBUG.md 调试指南
- 📝 完善技术文档（docs/11-voice-control-system.md）

## [0.9.0] - 2026-07-15

### 新增
- 🎨 系统托盘支持
  - 最小化到托盘
  - 托盘菜单（显示/退出）
  - 托盘图标（嵌入资源）
- 🔍 颜色查找功能
  - PrintWindow 后台截图
  - 精确颜色匹配 + 容差匹配
  - find_color 脚本表达式集成
- 🖱️ 鼠标操作命令
  - mouse_move（绝对坐标）
  - mouse_move_center（相对中心）
  - mouse_move_percent（百分比定位）
  - mouse_click（左/右/中键）
  - mouse_down/mouse_up

### 修复
- 🐛 窗口失效检测优化
- 🐛 PostMessage scan code 修正

## [0.8.0] - 2026-07-10

### 新增
- ⌨️ 即兴发送功能
  - Ctrl+Shift+Insert 进入发送模式
  - 2秒内按任意键发送到选中窗口
  - 支持 A-Z/0-9/F1-F12/空格等常用键
- 🎯 窗口激活消息
  - send_window_active() 脚本命令
  - WM_ACTIVATEAPP 消息发送

### 变更
- 🔧 热键系统重构
  - 状态机优化
  - 选择超时机制（3秒）
  - 拒绝抓取自身窗口

## [0.7.0] - 2026-07-05

### 新增
- 📜 完整脚本系统
  - if/else 条件判断（支持嵌套）
  - loop 无限循环
  - 表达式评估器
  - 多方案管理
- 💾 配置持久化
  - config/config.json 自动保存
  - 窗口绑定、方案关联持久化
  - 启动时自动加载

## [0.6.0] - 2026-07-01

### 新增
- 🎮 多窗口支持（8个槽位）
- 🔥 全局热键系统
  - Ctrl+Shift+[1-8] 选择窗口
  - Ctrl+Shift+9 循环启动
  - Ctrl+Shift+0 停止
  - Ctrl+Shift+- 单次执行
- 📂 脚本加载器（扫描 scripts/ 目录）
- 🪟 窗口选择器（3秒倒计时抓取）

## [0.5.0] - 2026-06-25

### 新增
- 🎨 egui UI 界面
- ⚙️ InputBackend trait 抽象
- 📤 PostMessage 实现（后台输入）
- 🧪 单窗口脚本执行

## [0.1.0] - 2026-06-20

### 新增
- 🚀 项目初始化
- 📝 脚本解析器（Tokenizer + Parser）
- 🔧 基础 AST 结构
- ✅ 单元测试框架

---

## 版本说明

### v1.0.0 - 正式版
功能完整，稳定可用，语音控制系统全面集成。适合生产环境部署。

### v0.9.0 - 功能完善
核心功能全部实现，增加系统托盘、颜色查找等高级特性。

### v0.5.0 - MVP
最小可用版本，单窗口脚本执行可用。

---

[未发布]: https://gitee.com/wireboy/game-multi-utils/compare/v1.0.0...HEAD
[1.0.0]: https://gitee.com/wireboy/game-multi-utils/compare/v0.9.0...v1.0.0
[0.9.0]: https://gitee.com/wireboy/game-multi-utils/compare/v0.8.0...v0.9.0
[0.8.0]: https://gitee.com/wireboy/game-multi-utils/compare/v0.7.0...v0.8.0
[0.7.0]: https://gitee.com/wireboy/game-multi-utils/compare/v0.6.0...v0.7.0
[0.6.0]: https://gitee.com/wireboy/game-multi-utils/compare/v0.5.0...v0.6.0
[0.5.0]: https://gitee.com/wireboy/game-multi-utils/compare/v0.1.0...v0.5.0
[0.1.0]: https://gitee.com/wireboy/game-multi-utils/releases/tag/v0.1.0
