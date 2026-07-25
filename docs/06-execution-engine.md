# 执行引擎设计

## 架构概览

```
ExecutorManager
    ↓
8 个 SchemeRunner (独立线程)
    ↓
ScriptExecutor (执行 AST)
    ↓
InputBackend (发送消息)
```

## 核心设计

### 1. 每窗口独立线程
- 每个窗口槽位对应一个 `SchemeRunner` 线程
- 线程在程序启动时创建，空闲时等待命令
- 避免多次创建/销毁线程的开销

### 2. 循环执行
- 脚本从头到尾执行一次后，立即重新开始
- 直到收到 `Stop` 命令或 `stop_flag` 为 true

### 3. 可中断延迟
- `delay_ms` 分段 sleep，每 10ms 检查 `stop_flag`
- 保证热键响应及时（最大延迟 10ms）

---

## SchemeRunner 实现

**位置**: `src/executor/runner.rs`

```rust
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread::{self, JoinHandle};
use crossbeam_channel::{Sender, Receiver, unbounded};
use windows::Win32::Foundation::HWND;
use crate::script::{Script, ScriptExecutor};
use crate::input::InputBackend;

/// 发送给执行线程的命令
#[derive(Clone)]
pub enum RunnerCommand {
    /// 启动执行指定脚本（循环执行）
    Start(Arc<Script>),
    
    /// 停止当前脚本
    Stop,
}

/// 单个窗口的执行器（独立线程）
pub struct SchemeRunner {
    window_index: u8,
    hwnd: HWND,
    
    /// 停止标志（原子操作）
    stop_flag: Arc<AtomicBool>,
    
    /// 线程句柄
    thread_handle: Option<JoinHandle<()>>,
    
    /// 命令发送端
    cmd_tx: Sender<RunnerCommand>,
}

impl SchemeRunner {
    pub fn new(
        window_index: u8,
        hwnd: HWND,
        input_backend: Arc<dyn InputBackend>,
    ) -> Self {
        let (cmd_tx, cmd_rx) = unbounded();
        let stop_flag = Arc::new(AtomicBool::new(false));
        
        // 启动后台线程
        let thread_handle = thread::spawn({
            let stop_flag = stop_flag.clone();
            let input_backend = input_backend.clone();
            move || Self::run_loop(window_index, hwnd, cmd_rx, stop_flag, input_backend)
        });
        
        Self {
            window_index,
            hwnd,
            stop_flag,
            thread_handle: Some(thread_handle),
            cmd_tx,
        }
    }
    
    /// 线程主循环
    fn run_loop(
        window_index: u8,
        hwnd: HWND,
        cmd_rx: Receiver<RunnerCommand>,
        stop_flag: Arc<AtomicBool>,
        input_backend: Arc<dyn InputBackend>,
    ) {
        log::info!("窗口 {} 执行线程启动", window_index);
        
        let executor = ScriptExecutor::new(input_backend);
        
        // 循环接收命令
        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                RunnerCommand::Start(script) => {
                    log::info!("窗口 {} 开始执行脚本", window_index);
                    stop_flag.store(false, Ordering::Relaxed);
                    
                    // 循环执行脚本，直到收到停止信号
                    executor.execute_loop(&script, hwnd, &stop_flag);
                    
                    log::info!("窗口 {} 脚本执行已停止", window_index);
                }
                RunnerCommand::Stop => {
                    stop_flag.store(true, Ordering::Relaxed);
                    log::info!("窗口 {} 收到停止命令", window_index);
                }
            }
        }
        
        log::info!("窗口 {} 执行线程退出", window_index);
    }
    
    /// 启动脚本执行
    pub fn start(&self, script: Arc<Script>) -> Result<(), String> {
        self.cmd_tx.send(RunnerCommand::Start(script))
            .map_err(|_| "发送启动命令失败".to_string())
    }
    
    /// 停止脚本执行
    pub fn stop(&self) -> Result<(), String> {
        self.stop_flag.store(true, Ordering::Relaxed);
        self.cmd_tx.send(RunnerCommand::Stop)
            .map_err(|_| "发送停止命令失败".to_string())
    }
    
    /// 检查是否正在运行
    pub fn is_running(&self) -> bool {
        !self.stop_flag.load(Ordering::Relaxed)
    }
    
    /// 更新窗口句柄（窗口重新绑定时）
    pub fn update_hwnd(&mut self, hwnd: HWND) {
        self.hwnd = hwnd;
    }
}

impl Drop for SchemeRunner {
    fn drop(&mut self) {
        // 确保线程停止
        self.stop().ok();
        
        // 等待线程退出
        if let Some(handle) = self.thread_handle.take() {
            handle.join().ok();
        }
    }
}
```

---

## ExecutorManager 实现

**位置**: `src/executor/mod.rs`

```rust
use std::sync::Arc;
use windows::Win32::Foundation::HWND;
use crate::script::Script;
use crate::input::InputBackend;

mod runner;
pub use runner::SchemeRunner;

/// 执行引擎管理器（管理所有窗口的 Runner）
pub struct ExecutorManager {
    runners: Vec<Option<SchemeRunner>>,
    input_backend: Arc<dyn InputBackend>,
}

impl ExecutorManager {
    pub fn new(input_backend: Arc<dyn InputBackend>) -> Self {
        Self {
            runners: vec![None; 8],
            input_backend,
        }
    }
    
    /// 为窗口创建或更新 Runner
    pub fn ensure_runner(&mut self, window_index: u8, hwnd: HWND) {
        let idx = (window_index - 1) as usize;
        
        match &mut self.runners[idx] {
            Some(runner) => {
                // 已存在，更新 HWND
                runner.update_hwnd(hwnd);
            }
            None => {
                // 创建新 Runner
                let runner = SchemeRunner::new(
                    window_index,
                    hwnd,
                    self.input_backend.clone(),
                );
                self.runners[idx] = Some(runner);
            }
        }
    }
    
    /// 启动窗口的脚本执行
    pub fn start(
        &mut self,
        window_index: u8,
        hwnd: HWND,
        script: Arc<Script>,
    ) -> Result<(), String> {
        self.ensure_runner(window_index, hwnd);
        
        let idx = (window_index - 1) as usize;
        if let Some(runner) = &self.runners[idx] {
            runner.start(script)
        } else {
            Err("Runner 不存在".to_string())
        }
    }
    
    /// 停止窗口的脚本执行
    pub fn stop(&self, window_index: u8) -> Result<(), String> {
        let idx = (window_index - 1) as usize;
        if let Some(runner) = &self.runners[idx] {
            runner.stop()
        } else {
            Err("Runner 不存在".to_string())
        }
    }
    
    /// 停止所有窗口
    pub fn stop_all(&self) {
        for (i, runner) in self.runners.iter().enumerate() {
            if let Some(runner) = runner {
                if let Err(e) = runner.stop() {
                    log::warn!("停止窗口 {} 失败: {}", i + 1, e);
                }
            }
        }
    }
    
    /// 检查窗口是否正在运行
    pub fn is_running(&self, window_index: u8) -> bool {
        let idx = (window_index - 1) as usize;
        self.runners[idx]
            .as_ref()
            .map(|r| r.is_running())
            .unwrap_or(false)
    }
    
    /// 更新输入后端（所有 Runner 需要重建）
    pub fn update_input_backend(&mut self, input_backend: Arc<dyn InputBackend>) {
        // 停止所有运行中的 Runner
        self.stop_all();
        
        // 清空现有 Runner（Drop 会等待线程退出）
        self.runners.clear();
        self.runners.resize(8, None);
        
        // 更新后端
        self.input_backend = input_backend;
        
        log::info!("输入后端已更新，所有 Runner 已重置");
    }
}
```

---

## ScriptExecutor 详细实现

**位置**: `src/script/executor.rs`

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use windows::Win32::Foundation::HWND;
use crate::script::ast::*;
use crate::input::InputBackend;
use crate::utils::win32;

pub struct ScriptExecutor {
    input_backend: Arc<dyn InputBackend>,
}

impl ScriptExecutor {
    pub fn new(input_backend: Arc<dyn InputBackend>) -> Self {
        Self { input_backend }
    }
    
    /// 循环执行脚本（直到 stop_flag 为 true）
    pub fn execute_loop(
        &self,
        script: &Script,
        hwnd: HWND,
        stop_flag: &Arc<AtomicBool>,
    ) {
        while !stop_flag.load(Ordering::Relaxed) {
            // 检查窗口是否仍然有效
            if !win32::is_window_valid(hwnd) {
                log::warn!("窗口 {:?} 已关闭，停止执行", hwnd);
                break;
            }
            
            // 执行一轮脚本
            if let Err(e) = self.execute_once(script, hwnd, stop_flag) {
                log::error!("脚本执行错误: {}", e);
                // 出错后等待 1 秒再继续
                self.interruptible_sleep(1000, stop_flag);
            }
        }
    }
    
    /// 执行脚本一次（从头到尾）
    fn execute_once(
        &self,
        script: &Script,
        hwnd: HWND,
        stop_flag: &Arc<AtomicBool>,
    ) -> Result<(), String> {
        for statement in &script.statements {
            if stop_flag.load(Ordering::Relaxed) {
                return Ok(());
            }
            
            self.execute_statement(statement, hwnd, stop_flag)?;
        }
        Ok(())
    }
    
    fn execute_statement(
        &self,
        stmt: &Statement,
        hwnd: HWND,
        stop_flag: &Arc<AtomicBool>,
    ) -> Result<(), String> {
        match stmt {
            Statement::Command(cmd) => self.execute_command(cmd, hwnd, stop_flag),
            Statement::If(if_block) => self.execute_if(if_block, hwnd, stop_flag),
            Statement::Comment(_) => Ok(()),
        }
    }
    
    fn execute_command(
        &self,
        cmd: &Command,
        hwnd: HWND,
        stop_flag: &Arc<AtomicBool>,
    ) -> Result<(), String> {
        match cmd {
            Command::Down(key) => {
                self.input_backend.send_key_down(hwnd, *key)?;
            }
            Command::Up(key) => {
                self.input_backend.send_key_up(hwnd, *key)?;
            }
            Command::Click(key) => {
                self.input_backend.send_key_down(hwnd, *key)?;
                self.input_backend.send_key_up(hwnd, *key)?;
            }
            Command::ClickMs(key, delay) => {
                self.input_backend.send_key_down(hwnd, *key)?;
                self.interruptible_sleep(*delay, stop_flag);
                self.input_backend.send_key_up(hwnd, *key)?;
            }
            Command::DelayMs(ms) => {
                self.interruptible_sleep(*ms, stop_flag);
            }
            Command::MouseDown { button, x, y } => {
                self.input_backend.send_mouse_down(hwnd, *button, *x, *y)?;
            }
            Command::MouseUp { button } => {
                self.input_backend.send_mouse_up(hwnd, *button)?;
            }
            Command::MouseClick { button, x, y } => {
                self.input_backend.send_mouse_down(hwnd, *button, *x, *y)?;
                self.input_backend.send_mouse_up(hwnd, *button)?;
            }
            Command::MouseDownCenter { button, offset_x, offset_y } => {
                let (x, y) = win32::get_window_center_with_offset(hwnd, *offset_x, *offset_y)?;
                self.input_backend.send_mouse_down(hwnd, *button, x, y)?;
            }
            Command::MouseClickCenter { button, offset_x, offset_y } => {
                let (x, y) = win32::get_window_center_with_offset(hwnd, *offset_x, *offset_y)?;
                self.input_backend.send_mouse_down(hwnd, *button, x, y)?;
                self.input_backend.send_mouse_up(hwnd, *button)?;
            }
            Command::MouseDownPercent { button, percent_x, percent_y } => {
                let (x, y) = win32::get_window_percent_position(hwnd, *percent_x, *percent_y)?;
                self.input_backend.send_mouse_down(hwnd, *button, x, y)?;
            }
            Command::MouseClickPercent { button, percent_x, percent_y } => {
                let (x, y) = win32::get_window_percent_position(hwnd, *percent_x, *percent_y)?;
                self.input_backend.send_mouse_down(hwnd, *button, x, y)?;
                self.input_backend.send_mouse_up(hwnd, *button)?;
            }
        }
        Ok(())
    }
    
    fn execute_if(
        &self,
        if_block: &IfBlock,
        hwnd: HWND,
        stop_flag: &Arc<AtomicBool>,
    ) -> Result<(), String> {
        for branch in &if_block.branches {
            let should_execute = match &branch.condition {
                Some(expr) => self.eval_expression(expr, hwnd)?,
                None => true,  // else 分支
            };
            
            if should_execute {
                for stmt in &branch.body {
                    if stop_flag.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    self.execute_statement(stmt, hwnd, stop_flag)?;
                }
                break;  // 只执行第一个匹配的分支
            }
        }
        Ok(())
    }
    
    fn eval_expression(&self, expr: &Expression, hwnd: HWND) -> Result<bool, String> {
        match expr {
            Expression::Bool(b) => Ok(*b),
            Expression::Equals(left, right) => {
                let l = self.eval_expression(left, hwnd)?;
                let r = self.eval_expression(right, hwnd)?;
                Ok(l == r)
            }
            Expression::NotEquals(left, right) => {
                let l = self.eval_expression(left, hwnd)?;
                let r = self.eval_expression(right, hwnd)?;
                Ok(l != r)
            }
            Expression::FindColor { .. } => {
                // 阶段5实现
                log::warn!("find_color 暂未实现");
                Ok(false)
            }
            Expression::FindColorCenter { .. } => {
                log::warn!("find_color_center 暂未实现");
                Ok(false)
            }
            Expression::FindColorPercent { .. } => {
                log::warn!("find_color_percent 暂未实现");
                Ok(false)
            }
        }
    }
    
    /// 可中断的 sleep（每 10ms 检查一次 stop_flag）
    fn interruptible_sleep(&self, ms: u32, stop_flag: &Arc<AtomicBool>) {
        let chunks = ms / 10;
        let remainder = ms % 10;
        
        for _ in 0..chunks {
            if stop_flag.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        
        if remainder > 0 && !stop_flag.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(remainder as u64));
        }
    }
}
```

---

## 工具函数

**位置**: `src/utils/win32.rs`

```rust
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{IsWindow, GetClientRect};
use windows::Win32::Graphics::Gdi::ScreenToClient;

/// 检查窗口是否有效
pub fn is_window_valid(hwnd: HWND) -> bool {
    unsafe { IsWindow(hwnd).as_bool() }
}

/// 获取窗口中心点 + 偏移
pub fn get_window_center_with_offset(
    hwnd: HWND,
    offset_x: i32,
    offset_y: i32,
) -> Result<(i32, i32), String> {
    let mut rect = Default::default();
    unsafe {
        GetClientRect(hwnd, &mut rect)
            .map_err(|_| "获取窗口客户区失败".to_string())?;
    }
    
    let center_x = (rect.right - rect.left) / 2;
    let center_y = (rect.bottom - rect.top) / 2;
    
    Ok((center_x + offset_x, center_y + offset_y))
}

/// 获取窗口百分比位置
pub fn get_window_percent_position(
    hwnd: HWND,
    percent_x: u8,
    percent_y: u8,
) -> Result<(i32, i32), String> {
    let mut rect = Default::default();
    unsafe {
        GetClientRect(hwnd, &mut rect)
            .map_err(|_| "获取窗口客户区失败".to_string())?;
    }
    
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    
    let x = (width * percent_x as i32) / 100;
    let y = (height * percent_y as i32) / 100;
    
    Ok((x, y))
}
```

---

## 执行流程图

```
用户按 Ctrl+Shift+9
    ↓
App.start_window_scheme(2)
    ↓
ExecutorManager.start(2, hwnd, script)
    ↓
SchemeRunner[1].start(script)
    ↓
发送 RunnerCommand::Start 到线程
    ↓
线程接收命令，设置 stop_flag = false
    ↓
ScriptExecutor.execute_loop(script, hwnd, stop_flag)
    ↓
循环执行 script.statements
    ↓
每 10ms 检查 stop_flag
    ↓
用户按 Ctrl+Shift+0 → 设置 stop_flag = true
    ↓
循环退出，线程等待下一个命令
```

---

## 性能优化

### 1. 避免频繁检查窗口有效性
```rust
// 每 100 次循环检查一次
let mut check_counter = 0;
while !stop_flag.load(Ordering::Relaxed) {
    check_counter += 1;
    if check_counter % 100 == 0 {
        if !win32::is_window_valid(hwnd) {
            break;
        }
    }
    // ...
}
```

### 2. 缓存窗口尺寸
```rust
// 在 Runner 中缓存
struct WindowInfo {
    hwnd: HWND,
    width: i32,
    height: i32,
    center_x: i32,
    center_y: i32,
}

// 只在 HWND 更新时重新计算
```

### 3. 批量发送消息
```rust
// PostMessage 可以连续发送无需等待
self.input_backend.send_key_down(hwnd, key1)?;
self.input_backend.send_key_down(hwnd, key2)?;
// 不需要中间 sleep
```
