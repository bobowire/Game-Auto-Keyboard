# 热键管理系统

## 热键方案

| 热键 | 功能 | 说明 |
|------|------|------|
| `Ctrl+Shift+1~8` | 选择窗口 | 按下后标记对应窗口，可连按选择多个 |
| `Ctrl+Shift+9` | 启动方案 | 启动选中窗口的标识方案（无选择则启动全部）|
| `Ctrl+Shift+0` | 停止方案 | 停止选中窗口的执行（无选择则停止全部）|
| `Ctrl+Shift+F1` | 语音开关 | 切换语音控制；开启播成功音、关闭播失败音（开启失败也播失败音）。不走状态机，在 `handle_hotkey` 直接处理 |
| `Ctrl+Shift+F2` | 转发开关 | 切换鼠标/键盘转发覆盖窗；音效规则同上 |
| `Ctrl+Alt+A` | 添加窗口 | 进入窗口捕获模式【可自定义】 |

## 状态机设计

### 状态转换图

```
         [初始状态]
            |
            | 按 Ctrl+Shift+[1-8]
            ↓
     [已选择窗口 N]
        /       \
       / 2秒超时  \
      ↓           ↓
  [重置]      [继续选择]
                 |
                 | 按 Ctrl+Shift+[1-8]
                 ↓
            [选择窗口 M]
                 |
                 | 按 Ctrl+Shift+9 或 0
                 ↓
            [执行命令]
                 |
                 ↓
              [重置]
```

### 核心逻辑

1. **选择前缀**：用户按 `Ctrl+Shift+[1-8]` 时，设置对应 bit 位
2. **超时重置**：2 秒内无操作，清空选择
3. **执行/停止**：按 `9` 或 `0` 时，根据选择掩码决定操作窗口列表
4. **无选择 = 全部**：如果掩码为 0，则操作所有 8 个窗口

---

## 状态机实现

**位置**: `src/hotkey/state_machine.rs`

```rust
use std::time::{Duration, Instant};

/// 热键状态机（处理 1-8 前缀选择）
pub struct HotkeyStateMachine {
    /// 选中的窗口集合（bit 0-7 对应窗口 1-8）
    selected_mask: u8,
    
    /// 最后一次选择时间
    last_select_time: Option<Instant>,
    
    /// 超时时长（默认 2 秒）
    timeout: Duration,
}

impl HotkeyStateMachine {
    pub fn new() -> Self {
        Self {
            selected_mask: 0,
            last_select_time: None,
            timeout: Duration::from_secs(2),
        }
    }
    
    /// 设置超时时长
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }
    
    /// 处理 Ctrl+Shift+[1-8] 按下
    pub fn on_select_key(&mut self, window_index: u8) {
        assert!(window_index >= 1 && window_index <= 8);
        
        self.check_timeout();
        
        // 设置对应 bit 位
        self.selected_mask |= 1 << (window_index - 1);
        self.last_select_time = Some(Instant::now());
        
        log::debug!("选择窗口 {}, 当前掩码: {:08b}", window_index, self.selected_mask);
    }
    
    /// 处理 Ctrl+Shift+9（启动）
    pub fn on_start_key(&mut self) -> Vec<u8> {
        self.check_timeout();
        
        let result = if self.selected_mask == 0 {
            // 无选择 -> 返回所有窗口
            vec![1, 2, 3, 4, 5, 6, 7, 8]
        } else {
            // 返回选中的窗口列表
            self.get_selected_windows()
        };
        
        log::info!("启动方案: 窗口 {:?}", result);
        self.reset();
        result
    }
    
    /// 处理 Ctrl+Shift+0（停止）
    pub fn on_stop_key(&mut self) -> Vec<u8> {
        self.check_timeout();
        
        let result = if self.selected_mask == 0 {
            vec![1, 2, 3, 4, 5, 6, 7, 8]
        } else {
            self.get_selected_windows()
        };
        
        log::info!("停止方案: 窗口 {:?}", result);
        self.reset();
        result
    }
    
    /// 获取当前选中的窗口列表
    pub fn get_selected_windows(&self) -> Vec<u8> {
        (1..=8)
            .filter(|i| self.selected_mask & (1 << (i - 1)) != 0)
            .collect()
    }
    
    /// 检查是否超时，如果超时则重置
    fn check_timeout(&mut self) {
        if let Some(last_time) = self.last_select_time {
            if last_time.elapsed() > self.timeout {
                log::debug!("选择超时，重置状态");
                self.reset();
            }
        }
    }
    
    /// 重置状态
    fn reset(&mut self) {
        self.selected_mask = 0;
        self.last_select_time = None;
    }
    
    /// 获取当前选择状态（用于 UI 显示）
    pub fn is_selected(&self, window_index: u8) -> bool {
        self.check_timeout();
        self.selected_mask & (1 << (window_index - 1)) != 0
    }
}
```

---

## 热键管理器

**位置**: `src/hotkey/manager.rs`

```rust
use windows::Win32::UI::WindowsAndMessaging::{
    RegisterHotKey, UnregisterHotKey, GetMessageW, MSG,
    MOD_CONTROL, MOD_SHIFT, MOD_ALT, WM_HOTKEY,
};
use windows::Win32::Foundation::HWND;
use crossbeam_channel::{Sender, Receiver, unbounded};
use std::thread;

const HOTKEY_ID_BASE: i32 = 1000;

/// 热键事件
#[derive(Clone, Debug)]
pub enum HotkeyEvent {
    SelectWindow(u8),       // 选择窗口 1-8
    Start,                  // 启动（Ctrl+Shift+9）
    Stop,                   // 停止（Ctrl+Shift+0）
    AddWindow,              // 添加窗口（Ctrl+Alt+A）
}

/// 热键管理器
pub struct HotkeyManager {
    hwnd: HWND,
    event_rx: Receiver<HotkeyEvent>,
    _thread_handle: Option<thread::JoinHandle<()>>,
}

impl HotkeyManager {
    pub fn new(hwnd: HWND) -> Result<Self, String> {
        let (event_tx, event_rx) = unbounded();
        
        // 在单独线程注册热键并监听消息
        let thread_handle = thread::spawn({
            let hwnd = hwnd.clone();
            move || Self::hotkey_thread(hwnd, event_tx)
        });
        
        Ok(Self {
            hwnd,
            event_rx,
            _thread_handle: Some(thread_handle),
        })
    }
    
    /// 热键监听线程
    fn hotkey_thread(hwnd: HWND, event_tx: Sender<HotkeyEvent>) {
        // 注册所有热键
        if let Err(e) = Self::register_hotkeys(hwnd) {
            log::error!("注册热键失败: {}", e);
            return;
        }
        
        // 消息循环
        unsafe {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, hwnd, 0, 0).as_bool() {
                if msg.message == WM_HOTKEY {
                    let hotkey_id = msg.wParam.0 as i32;
                    if let Some(event) = Self::hotkey_id_to_event(hotkey_id) {
                        event_tx.send(event).ok();
                    }
                }
            }
        }
        
        // 清理：注销热键
        Self::unregister_hotkeys(hwnd);
    }
    
    /// 注册所有热键
    fn register_hotkeys(hwnd: HWND) -> Result<(), String> {
        let modifiers = MOD_CONTROL | MOD_SHIFT;
        
        // 注册 Ctrl+Shift+0~9
        for i in 0..=9 {
            let vk = ('0' as u32) + i;
            let id = HOTKEY_ID_BASE + i as i32;
            
            unsafe {
                if !RegisterHotKey(hwnd, id, modifiers, vk).as_bool() {
                    return Err(format!("注册热键 Ctrl+Shift+{} 失败", i));
                }
            }
        }
        
        // 注册 Ctrl+Alt+A（添加窗口）
        unsafe {
            RegisterHotKey(
                hwnd,
                HOTKEY_ID_BASE + 100,
                MOD_CONTROL | MOD_ALT,
                'A' as u32,
            ).map_err(|_| "注册添加窗口热键失败".to_string())?;
        }
        
        log::info!("热键注册成功");
        Ok(())
    }
    
    /// 注销所有热键
    fn unregister_hotkeys(hwnd: HWND) {
        unsafe {
            for i in 0..=9 {
                UnregisterHotKey(hwnd, HOTKEY_ID_BASE + i);
            }
            UnregisterHotKey(hwnd, HOTKEY_ID_BASE + 100);
        }
        log::info!("热键已注销");
    }
    
    /// 将热键 ID 转换为事件
    fn hotkey_id_to_event(hotkey_id: i32) -> Option<HotkeyEvent> {
        let offset = hotkey_id - HOTKEY_ID_BASE;
        
        match offset {
            1..=8 => Some(HotkeyEvent::SelectWindow(offset as u8)),
            9 => Some(HotkeyEvent::Start),
            0 => Some(HotkeyEvent::Stop),
            100 => Some(HotkeyEvent::AddWindow),
            _ => None,
        }
    }
    
    /// 轮询热键事件（UI 线程调用）
    pub fn poll_events(&self) -> Vec<HotkeyEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            events.push(event);
        }
        events
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        // 发送退出消息到热键线程
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
            PostMessageW(self.hwnd, WM_QUIT, 0, 0).ok();
        }
    }
}
```

---

## 在应用中集成

**位置**: `src/app.rs`

```rust
impl AutoKeyboardApp {
    pub fn new() -> Self {
        // 创建隐藏窗口用于接收热键消息
        let hwnd = Self::create_message_window();
        
        let hotkey_manager = HotkeyManager::new(hwnd)
            .expect("创建热键管理器失败");
        
        let state_machine = HotkeyStateMachine::new();
        
        Self {
            hotkey_manager,
            state_machine,
            // ... 其他字段
        }
    }
    
    /// 创建消息窗口（不可见，仅用于接收热键消息）
    fn create_message_window() -> HWND {
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{
                CreateWindowExW, HWND_MESSAGE,
            };
            
            // 创建仅消息窗口
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("GameAutoKeyboard_HotkeyWindow"),
                Default::default(),
                0, 0, 0, 0,
                HWND_MESSAGE,  // 父窗口为 HWND_MESSAGE
                None,
                None,
                None,
            ).expect("创建消息窗口失败")
        }
    }
}

impl eframe::App for AutoKeyboardApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. 处理热键事件
        self.process_hotkeys();
        
        // 2. 渲染 UI
        // ...
    }
}

impl AutoKeyboardApp {
    fn process_hotkeys(&mut self) {
        for event in self.hotkey_manager.poll_events() {
            match event {
                HotkeyEvent::SelectWindow(index) => {
                    self.state_machine.on_select_key(index);
                }
                HotkeyEvent::Start => {
                    let windows = self.state_machine.on_start_key();
                    for idx in windows {
                        self.start_window_scheme(idx);
                    }
                }
                HotkeyEvent::Stop => {
                    let windows = self.state_machine.on_stop_key();
                    for idx in windows {
                        self.stop_window_scheme(idx);
                    }
                }
                HotkeyEvent::AddWindow => {
                    self.is_selecting_window = true;
                }
            }
        }
    }
    
    fn start_window_scheme(&mut self, window_index: u8) {
        let slot = &self.windows[window_index as usize - 1];
        
        if let Some(hwnd) = slot.hwnd {
            let scheme_id = &slot.schemes[slot.marked_scheme].id;
            let script = self.scheme_manager.get_script(scheme_id);
            
            self.executor_manager.start(window_index, hwnd, script);
            log::info!("启动窗口 {} 的方案: {}", window_index, scheme_id);
        } else {
            log::warn!("窗口 {} 未绑定", window_index);
        }
    }
    
    fn stop_window_scheme(&mut self, window_index: u8) {
        self.executor_manager.stop(window_index);
        log::info!("停止窗口 {} 的执行", window_index);
    }
}
```

---

## UI 状态显示

在窗口列表中显示选择状态：

```rust
impl AutoKeyboardApp {
    fn ui_window_list(&mut self, ui: &mut egui::Ui) {
        ui.heading("窗口列表");
        
        for i in 0..8 {
            let slot = &self.windows[i];
            let is_selected = self.state_machine.is_selected((i + 1) as u8);
            
            ui.horizontal(|ui| {
                // 显示选择状态
                if is_selected {
                    ui.colored_label(egui::Color32::YELLOW, "●");
                } else {
                    ui.label("○");
                }
                
                // 窗口编号和标题
                ui.label(format!("[{}] {}", i + 1, slot.title));
                
                // 执行状态
                match slot.state {
                    ExecutionState::Running => {
                        ui.colored_label(egui::Color32::GREEN, "▶ 运行中");
                    }
                    ExecutionState::Idle => {
                        ui.label("● 空闲");
                    }
                    _ => {}
                }
            });
        }
    }
}
```

---

## 热键冲突处理

### 问题
`RegisterHotKey` 可能因为已被其他程序占用而失败。

### 解决方案

```rust
impl HotkeyManager {
    fn register_hotkeys(hwnd: HWND) -> Result<(), String> {
        let mut failed = Vec::new();
        
        for i in 0..=9 {
            let vk = ('0' as u32) + i;
            let id = HOTKEY_ID_BASE + i as i32;
            
            unsafe {
                if !RegisterHotKey(hwnd, id, MOD_CONTROL | MOD_SHIFT, vk).as_bool() {
                    failed.push(format!("Ctrl+Shift+{}", i));
                }
            }
        }
        
        if !failed.is_empty() {
            return Err(format!("以下热键注册失败（可能被占用）: {}", failed.join(", ")));
        }
        
        Ok(())
    }
}
```

### 启动时提示

```rust
impl AutoKeyboardApp {
    pub fn new() -> Self {
        match HotkeyManager::new(hwnd) {
            Ok(manager) => manager,
            Err(e) => {
                // 显示错误对话框
                MessageBoxW(
                    None,
                    &HSTRING::from(format!("热键注册失败:\n{}", e)),
                    w!("错误"),
                    MB_OK | MB_ICONERROR,
                );
                panic!("热键注册失败");
            }
        }
    }
}
```

---

## 测试

### 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_state_machine_single_select() {
        let mut sm = HotkeyStateMachine::new();
        
        sm.on_select_key(2);
        assert_eq!(sm.get_selected_windows(), vec![2]);
        
        let started = sm.on_start_key();
        assert_eq!(started, vec![2]);
        
        // 启动后应重置
        assert_eq!(sm.get_selected_windows(), vec![]);
    }
    
    #[test]
    fn test_state_machine_multi_select() {
        let mut sm = HotkeyStateMachine::new();
        
        sm.on_select_key(1);
        sm.on_select_key(3);
        sm.on_select_key(5);
        
        let selected = sm.get_selected_windows();
        assert_eq!(selected, vec![1, 3, 5]);
    }
    
    #[test]
    fn test_state_machine_no_select() {
        let mut sm = HotkeyStateMachine::new();
        
        // 没有选择任何窗口，直接启动
        let started = sm.on_start_key();
        assert_eq!(started, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }
    
    #[test]
    fn test_state_machine_timeout() {
        use std::thread::sleep;
        
        let mut sm = HotkeyStateMachine::new();
        sm.set_timeout(Duration::from_millis(100));
        
        sm.on_select_key(1);
        sleep(Duration::from_millis(150));
        
        // 超时后应自动重置
        assert_eq!(sm.get_selected_windows(), vec![]);
    }
}
```
