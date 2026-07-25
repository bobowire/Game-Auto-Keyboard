# 输入后端设计

## 设计理念

使用 **策略模式 (Strategy Pattern)** 通过 trait 抽象输入方式，核心优势：

1. **可插拔**: 运行时切换输入后端，无需重启程序
2. **可扩展**: 新增后端只需实现 `InputBackend` trait
3. **可测试**: 可以 mock 后端进行单元测试
4. **解耦**: 脚本执行器不关心具体实现细节

### 支持的实现

- **PostMessage** ✅: 后台发送（已验证可用）
- **SendInput**: 前台发送（备用方案，高兼容性）
- **驱动级**: 内核驱动（预留接口，应对特殊需求）

## 核心 Trait

**位置**: `src/input/backend.rs`

### 设计目标

**为什么要抽象成 trait？**

1. **隔离变化点**: Windows 输入 API 有多种方式（PostMessage、SendInput、驱动），未来可能还有新方案
2. **运行时切换**: 用户可在 UI 中动态切换后端，无需重启
3. **渐进式开发**: 先实现 PostMessage，后续再补充其他方案
4. **单元测试**: 可以创建 `MockBackend` 进行测试，不依赖真实窗口

### Trait 定义

```rust
use windows::Win32::Foundation::HWND;
use crate::script::ast::{Key, MouseButton};

/// 输入后端 trait（策略模式）
pub trait InputBackend: Send + Sync {
    /// 后端名称（用于 UI 显示/切换）
    fn name(&self) -> &str;
    
    /// 是否支持后台发送（窗口非激活状态）
    fn supports_background(&self) -> bool;
    
    // ===== 键盘接口 =====
    
    /// 发送键盘按下事件
    fn send_key_down(&self, hwnd: HWND, key: Key) -> Result<(), String>;
    
    /// 发送键盘弹起事件
    fn send_key_up(&self, hwnd: HWND, key: Key) -> Result<(), String>;
    
    // ===== 鼠标接口 =====
    
    /// 发送鼠标按下事件（客户区坐标）
    fn send_mouse_down(
        &self,
        hwnd: HWND,
        button: MouseButton,
        x: i32,
        y: i32,
    ) -> Result<(), String>;
    
    /// 发送鼠标弹起事件
    fn send_mouse_up(&self, hwnd: HWND, button: MouseButton) -> Result<(), String>;
}
```

### Trait 要求说明

| 方法 | 说明 | 返回值 |
|------|------|--------|
| `name()` | 后端唯一标识，用于 UI 显示和配置存储 | `&str` |
| `supports_background()` | 是否支持后台发送（影响 UI 提示） | `bool` |
| `send_key_down()` | 发送按键按下事件 | `Result<(), String>` |
| `send_key_up()` | 发送按键弹起事件 | `Result<(), String>` |
| `send_mouse_down()` | 发送鼠标按下事件（客户区坐标） | `Result<(), String>` |
| `send_mouse_up()` | 发送鼠标弹起事件 | `Result<(), String>` |

### 关键约束

- **线程安全**: `Send + Sync` 保证可以在执行线程中调用
- **错误处理**: 所有方法返回 `Result`，便于上层处理失败情况
- **坐标系统**: 鼠标坐标统一使用**客户区坐标**，具体实现负责转换

---

## 实现 1: PostMessage 后端

**位置**: `src/input/post_message.rs`

### 特点
- ✅ 支持后台发送
- ✅ **已验证可用**（用户确认）
- ✅ 无需激活窗口
- ⚠️ 兼容性：对普通程序/老游戏有效，现代 3D 游戏可能需要其他方案

### 实现原理

```rust
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_KEYDOWN, WM_KEYUP};
use windows::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, MAPVK_VK_TO_VSC};

pub struct PostMessageBackend;

impl InputBackend for PostMessageBackend {
    fn name(&self) -> &str {
        "PostMessage (后台)"
    }
    
    fn supports_background(&self) -> bool {
        true
    }
    
    fn send_key_down(&self, hwnd: HWND, key: Key) -> Result<(), String> {
        let vk = self.key_to_vk(key)?;
        let scan_code = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) };
        
        // LPARAM = (scan_code << 16) | repeat_count(1)
        let lparam = ((scan_code as isize) << 16) | 1;
        
        unsafe {
            PostMessageW(hwnd, WM_KEYDOWN, vk as usize, lparam)
                .map_err(|e| format!("PostMessage 失败: {:?}", e))?;
        }
        
        Ok(())
    }
    
    fn send_key_up(&self, hwnd: HWND, key: Key) -> Result<(), String> {
        let vk = self.key_to_vk(key)?;
        let scan_code = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) };
        
        // LPARAM = (scan_code << 16) | 0xC0000001 (transition & previous state)
        let lparam = ((scan_code as isize) << 16) | 0xC0000001;
        
        unsafe {
            PostMessageW(hwnd, WM_KEYUP, vk as usize, lparam)
                .map_err(|e| format!("PostMessage 失败: {:?}", e))?;
        }
        
        Ok(())
    }
    
    // ... 鼠标实现类似（WM_LBUTTONDOWN 等）
}

impl PostMessageBackend {
    fn key_to_vk(&self, key: Key) -> Result<u8, String> {
        match key {
            Key::VirtualKey(vk) => Ok(vk),
            Key::Char(c) => {
                // 使用 VkKeyScanW 转换字符到 VK 码
                let result = unsafe { VkKeyScanW(c as u16) };
                if result == -1 {
                    Err(format!("无法转换字符 '{}' 到虚拟键码", c))
                } else {
                    Ok((result & 0xFF) as u8)
                }
            }
        }
    }
}
```

### 鼠标消息

```rust
fn send_mouse_down(&self, hwnd: HWND, button: MouseButton, x: i32, y: i32) 
    -> Result<(), String> 
{
    let msg = match button {
        MouseButton::Left => WM_LBUTTONDOWN,
        MouseButton::Right => WM_RBUTTONDOWN,
        MouseButton::Middle => WM_MBUTTONDOWN,
    };
    
    // WPARAM = 鼠标按键状态（MK_LBUTTON 等）
    // LPARAM = MAKELPARAM(x, y)
    let lparam = ((y as isize) << 16) | (x as isize & 0xFFFF);
    
    unsafe {
        PostMessageW(hwnd, msg, 0, lparam)
            .map_err(|e| format!("发送鼠标消息失败: {:?}", e))?;
    }
    
    Ok(())
}
```

---

## 实现 2: SendInput 后端

**位置**: `src/input/send_input.rs`

### 特点
- ❌ 不支持后台（窗口必须在前台）
- ✅ 兼容性强（所有程序都能收到）
- ⚠️ 需要先激活窗口

### 实现原理

```rust
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
};

pub struct SendInputBackend;

impl InputBackend for SendInputBackend {
    fn name(&self) -> &str {
        "SendInput (前台)"
    }
    
    fn supports_background(&self) -> bool {
        false
    }
    
    fn send_key_down(&self, hwnd: HWND, key: Key) -> Result<(), String> {
        // 1. 先激活窗口
        self.activate_window(hwnd)?;
        
        // 2. 构造 INPUT 结构
        let vk = self.key_to_vk(key)?;
        let mut input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk as u16,
                    wScan: 0,
                    dwFlags: 0,  // 按下
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        
        // 3. 发送输入
        unsafe {
            let sent = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
            if sent != 1 {
                return Err("SendInput 失败".to_string());
            }
        }
        
        Ok(())
    }
    
    fn send_key_up(&self, hwnd: HWND, key: Key) -> Result<(), String> {
        self.activate_window(hwnd)?;
        
        let vk = self.key_to_vk(key)?;
        let mut input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk as u16,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,  // 弹起
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        
        unsafe {
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
        
        Ok(())
    }
}

impl SendInputBackend {
    fn activate_window(&self, hwnd: HWND) -> Result<(), String> {
        unsafe {
            SetForegroundWindow(hwnd);
            // 等待窗口激活（可选）
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Ok(())
    }
}
```

---

## 输入管理器

**位置**: `src/input/mod.rs`

```rust
use std::sync::Arc;

/// 全局输入后端管理器
pub struct InputManager {
    current: Arc<dyn InputBackend>,
    available: Vec<Arc<dyn InputBackend>>,
}

impl InputManager {
    pub fn new() -> Self {
        let backends: Vec<Arc<dyn InputBackend>> = vec![
            Arc::new(PostMessageBackend::new()),
            Arc::new(SendInputBackend::new()),
        ];
        
        let current = backends[0].clone();  // 默认 PostMessage
        
        Self {
            current,
            available: backends,
        }
    }
    
    /// 获取当前后端
    pub fn current(&self) -> Arc<dyn InputBackend> {
        self.current.clone()
    }
    
    /// 切换后端
    pub fn switch_backend(&mut self, name: &str) -> Result<(), String> {
        for backend in &self.available {
            if backend.name() == name {
                self.current = backend.clone();
                return Ok(());
            }
        }
        Err(format!("未找到后端: {}", name))
    }
    
    /// 获取所有可用后端名称
    pub fn available_backends(&self) -> Vec<String> {
        self.available.iter().map(|b| b.name().to_string()).collect()
    }
}
```

---

## 使用示例

### 在脚本执行器中使用

```rust
impl ScriptExecutor {
    pub fn execute_command(
        &self,
        cmd: &Command,
        hwnd: HWND,
        input_backend: &Arc<dyn InputBackend>,
    ) -> Result<(), String> {
        match cmd {
            Command::Down(key) => {
                input_backend.send_key_down(hwnd, *key)?;
            }
            Command::Up(key) => {
                input_backend.send_key_up(hwnd, *key)?;
            }
            Command::Click(key) => {
                input_backend.send_key_down(hwnd, *key)?;
                input_backend.send_key_up(hwnd, *key)?;
            }
            Command::ClickMs(key, delay) => {
                input_backend.send_key_down(hwnd, *key)?;
                std::thread::sleep(std::time::Duration::from_millis(*delay as u64));
                input_backend.send_key_up(hwnd, *key)?;
            }
            // ... 其他命令
        }
        Ok(())
    }
}
```

---

## 扩展新后端的步骤

### 示例：添加 AutoHotkey 后端

假设要集成 AutoHotkey 的 ControlSend 功能：

#### 1. 创建新文件 `src/input/ahk_backend.rs`

```rust
use crate::input::backend::InputBackend;
use windows::Win32::Foundation::HWND;
use crate::script::ast::{Key, MouseButton};

pub struct AhkBackend {
    // AHK 实例句柄或进程通信通道
}

impl InputBackend for AhkBackend {
    fn name(&self) -> &str {
        "AutoHotkey ControlSend"
    }
    
    fn supports_background(&self) -> bool {
        true  // AHK 的 ControlSend 支持后台
    }
    
    fn send_key_down(&self, hwnd: HWND, key: Key) -> Result<(), String> {
        // 调用 AHK COM 接口或命令行
        // ahk.exe script.ahk "ControlSend {a down}, , ahk_id %hwnd%"
        todo!()
    }
    
    // ... 实现其他方法
}
```

#### 2. 在 `src/input/mod.rs` 中注册

```rust
mod ahk_backend;
use ahk_backend::AhkBackend;

impl InputManager {
    pub fn new() -> Self {
        let backends: Vec<Arc<dyn InputBackend>> = vec![
            Arc::new(PostMessageBackend::new()),
            Arc::new(SendInputBackend::new()),
            Arc::new(AhkBackend::new()),  // ← 新增
        ];
        // ...
    }
}
```

#### 3. 无需修改其他代码

执行器、UI、配置系统自动支持新后端，只需在下拉框中选择即可。

---

## 扩展：驱动级后端（预留）

### 接口定义

```rust
/// 【阶段6】驱动级输入（需要安装内核驱动）
pub struct InterceptionBackend {
    driver_handle: Option<*mut c_void>,
}

impl InputBackend for InterceptionBackend {
    fn name(&self) -> &str {
        "Interception Driver (驱动级)"
    }
    
    fn supports_background(&self) -> bool {
        true
    }
    
    // 实现需要调用 interception.dll
}
```

### 注意事项
1. 需要安装内核驱动（需要管理员权限）
2. 部分反作弊会检测驱动
3. 兼容性最强，但复杂度高

---

## 测试工具设计

### 简单测试工具

```rust
// 位置: examples/test_input.rs

fn main() {
    let backend = PostMessageBackend::new();
    
    println!("请在 5 秒内点击目标窗口...");
    std::thread::sleep(Duration::from_secs(5));
    
    let hwnd = unsafe { GetForegroundWindow() };
    println!("目标窗口: {:?}", hwnd);
    
    println!("发送字符 'a'...");
    backend.send_key_down(hwnd, Key::Char('a')).unwrap();
    backend.send_key_up(hwnd, Key::Char('a')).unwrap();
    
    println!("测试完成，检查目标窗口是否收到输入");
}
```

运行测试：
```bash
cargo run --example test_input
```

---

## 对比总结

| 特性 | PostMessage ✅ | SendInput | Interception |
|------|---------------|-----------|--------------|
| 后台发送 | ✅ **已验证** | ❌ | ✅ |
| 兼容性 | ✅ 普通程序<br>⚠️ 部分游戏 | ✅ 通用 | ✅ 通用 |
| 安装要求 | 无 | 无 | 需要驱动 |
| 反作弊风险 | 低 | 低 | 高 |
| 实现优先级 | **P0** (立即实现) | P1 (备用) | P2 (可选) |
| 适用场景 | 普通程序/老游戏/2D游戏 | 前台自动化 | 高要求场景 |

### 开发建议

**阶段 1**: 只实现 `PostMessageBackend`
- 已确认可用，先让核心功能跑通
- 其他后端作为接口预留，阶段 4 再补充

**阶段 4**: 补充 `SendInputBackend`
- 作为 PostMessage 的补充方案
- 给用户提供切换选项

**阶段 6+**: 按需添加驱动级或其他方案
- 根据用户反馈决定是否需要

