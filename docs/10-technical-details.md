# 技术细节和注意事项

## Windows API 关键点

### 1. 窗口句柄 (HWND) 生命周期

#### 问题
窗口可能在执行过程中被关闭，导致 HWND 失效。

#### 解决方案
```rust
use windows::Win32::UI::WindowsAndMessaging::IsWindow;

pub fn is_window_valid(hwnd: HWND) -> bool {
    unsafe { IsWindow(hwnd).as_bool() }
}

// 在执行器中每 N 次循环检查一次
if counter % 100 == 0 && !is_window_valid(hwnd) {
    log::warn!("窗口已关闭");
    break;
}
```

---

### 2. 坐标系统

#### 坐标类型
- **屏幕坐标**: 相对于整个屏幕左上角
- **窗口坐标**: 相对于窗口左上角（包含标题栏）
- **客户区坐标**: 相对于窗口客户区左上角（不含标题栏）

#### 转换函数
```rust
use windows::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};
use windows::Win32::Foundation::POINT;

/// 客户区坐标 → 屏幕坐标
pub fn client_to_screen(hwnd: HWND, x: i32, y: i32) -> Result<(i32, i32), String> {
    let mut point = POINT { x, y };
    unsafe {
        ClientToScreen(hwnd, &mut point)
            .map_err(|_| "坐标转换失败".to_string())?;
    }
    Ok((point.x, point.y))
}
```

#### PostMessage 使用客户区坐标
```rust
// WM_LBUTTONDOWN 的坐标是客户区坐标
// LPARAM = MAKELPARAM(x, y)
let lparam = ((y as isize) << 16) | (x as isize & 0xFFFF);
PostMessageW(hwnd, WM_LBUTTONDOWN, 0, lparam)?;
```

---

### 3. 虚拟键码 (VK) 和扫描码 (Scan Code)

#### VK 码转换
```rust
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VkKeyScanW, MapVirtualKeyW, MAPVK_VK_TO_VSC
};

/// 字符 → VK 码
pub fn char_to_vk(c: char) -> Result<u8, String> {
    let result = unsafe { VkKeyScanW(c as u16) };
    if result == -1 {
        return Err(format!("无法转换字符: {}", c));
    }
    Ok((result & 0xFF) as u8)
}

/// VK 码 → 扫描码
pub fn vk_to_scan_code(vk: u8) -> u32 {
    unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) }
}
```

#### 常用 VK 码
```rust
pub mod vk_codes {
    pub const VK_RETURN: u8 = 0x0D;
    pub const VK_ESCAPE: u8 = 0x1B;
    pub const VK_SPACE: u8 = 0x20;
    pub const VK_LEFT: u8 = 0x25;
    pub const VK_UP: u8 = 0x26;
    pub const VK_RIGHT: u8 = 0x27;
    pub const VK_DOWN: u8 = 0x28;
    
    // 数字键 0-9
    pub const VK_0: u8 = 0x30;
    pub const VK_9: u8 = 0x39;
    
    // 字母键 A-Z
    pub const VK_A: u8 = 0x41;
    pub const VK_Z: u8 = 0x5A;
    
    // 功能键 F1-F12
    pub const VK_F1: u8 = 0x70;
    pub const VK_F12: u8 = 0x7B;
}
```

---

### 4. PostMessage LPARAM 构造

#### 键盘消息
```rust
/// WM_KEYDOWN LPARAM 格式：
/// - bit 0-15:  重复次数（通常为 1）
/// - bit 16-23: 扫描码
/// - bit 24:    扩展键标志
/// - bit 29:    上下文代码（Alt 键）
/// - bit 30:    前一状态（0=未按下）
/// - bit 31:    转换状态（0=按下，1=弹起）

pub fn make_key_down_lparam(scan_code: u32) -> isize {
    ((scan_code as isize) << 16) | 1
}

pub fn make_key_up_lparam(scan_code: u32) -> isize {
    ((scan_code as isize) << 16) | 0xC0000001
}
```

#### 鼠标消息
```rust
/// 鼠标 LPARAM: MAKELPARAM(x, y)
pub fn make_mouse_lparam(x: i32, y: i32) -> isize {
    ((y as isize) << 16) | (x as isize & 0xFFFF)
}

/// 鼠标 WPARAM: 按钮状态标志
pub const MK_LBUTTON: usize = 0x0001;
pub const MK_RBUTTON: usize = 0x0002;
pub const MK_SHIFT: usize = 0x0004;
pub const MK_CONTROL: usize = 0x0008;
```

---

## 线程安全

### 1. HWND 的线程安全性

#### 问题
HWND 本质是指针，可以跨线程传递，但 Windows 消息必须在创建窗口的线程发送。

#### 解决方案
PostMessage/SendInput 可以从任意线程调用：
```rust
// ✅ 安全：PostMessage 可以跨线程
std::thread::spawn(move || {
    PostMessageW(hwnd, WM_KEYDOWN, vk as usize, lparam).ok();
});

// ⚠️ 注意：GetWindowRect 等查询函数也是线程安全的
let mut rect = Default::default();
unsafe { GetClientRect(hwnd, &mut rect).ok(); }
```

---

### 2. 共享状态管理

#### 方案1: Arc + Mutex（简单但有锁竞争）
```rust
struct WindowSlot {
    hwnd: Arc<Mutex<Option<HWND>>>,
    state: Arc<Mutex<ExecutionState>>,
}
```

#### 方案2: 消息传递（推荐）
```rust
// UI 线程 → 执行线程
enum Message {
    Start(Arc<Script>),
    Stop,
    UpdateHwnd(HWND),
}

// 执行线程 → UI 线程
enum Event {
    StateChanged(ExecutionState),
    Error(String),
}
```

---

## 性能优化

### 1. 减少系统调用

#### 问题
频繁调用 `IsWindow`/`GetClientRect` 影响性能。

#### 优化
```rust
// 缓存窗口信息
struct WindowCache {
    hwnd: HWND,
    rect: RECT,
    last_update: Instant,
    ttl: Duration,
}

impl WindowCache {
    fn get_rect(&mut self) -> RECT {
        if self.last_update.elapsed() > self.ttl {
            unsafe { GetClientRect(self.hwnd, &mut self.rect).ok(); }
            self.last_update = Instant::now();
        }
        self.rect
    }
}
```

---

### 2. 批量发送消息

#### PostMessage 不阻塞
```rust
// ✅ 快速：连续发送不等待
for key in &[VK_A, VK_B, VK_C] {
    PostMessageW(hwnd, WM_KEYDOWN, *key as usize, lparam)?;
    PostMessageW(hwnd, WM_KEYUP, *key as usize, lparam_up)?;
}
```

#### SendInput 可以批量
```rust
// ✅ 更快：一次发送多个输入
let inputs = vec![
    make_key_input(VK_A, false),
    make_key_input(VK_A, true),
    make_key_input(VK_B, false),
    make_key_input(VK_B, true),
];
SendInput(&inputs, size_of::<INPUT>() as i32);
```

---

### 3. 可中断延迟的优化

#### 问题
每 10ms 检查 stop_flag 可能不够及时。

#### 优化
```rust
use std::sync::Condvar;

struct Sleeper {
    flag: Arc<AtomicBool>,
    condvar: Arc<Condvar>,
}

impl Sleeper {
    fn sleep(&self, ms: u32) {
        let deadline = Instant::now() + Duration::from_millis(ms as u64);
        let mut lock = self.condvar.wait_timeout(
            Mutex::new(()),
            deadline.duration_since(Instant::now())
        ).unwrap();
        
        // 立即响应停止信号
    }
    
    fn interrupt(&self) {
        self.flag.store(true, Ordering::Relaxed);
        self.condvar.notify_all();
    }
}
```

---

## 错误处理

### 1. Windows API 错误

#### 获取详细错误信息
```rust
use windows::core::Error as WinError;

fn post_message_with_error(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) 
    -> Result<(), String> 
{
    unsafe {
        PostMessageW(hwnd, msg, wparam, lparam)
            .map_err(|e: WinError| {
                let code = e.code().0;
                format!("PostMessage 失败: 错误码 0x{:08X}", code)
            })
    }
}
```

---

### 2. 脚本执行错误

#### 错误恢复策略
```rust
impl ScriptExecutor {
    fn execute_with_retry(&self, cmd: &Command, hwnd: HWND) -> Result<(), String> {
        const MAX_RETRIES: u32 = 3;
        
        for attempt in 1..=MAX_RETRIES {
            match self.execute_command(cmd, hwnd) {
                Ok(()) => return Ok(()),
                Err(e) if attempt < MAX_RETRIES => {
                    log::warn!("执行失败（尝试 {}/{}）: {}", attempt, MAX_RETRIES, e);
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(e),
            }
        }
        unreachable!()
    }
}
```

---

## 调试技巧

### 1. Spy++ 工具

使用 Visual Studio 的 Spy++ 查看窗口消息：
1. 启动 Spy++
2. 拖动查找器到目标窗口
3. 右键 → Messages
4. 运行程序，观察是否收到 WM_KEYDOWN

---

### 2. 日志输出

#### 详细日志
```rust
log::trace!("发送按键: {:?} to {:?}", key, hwnd);
log::debug!("LPARAM: 0x{:08X}", lparam);
```

#### 性能日志
```rust
use std::time::Instant;

let start = Instant::now();
self.execute_command(cmd, hwnd)?;
let elapsed = start.elapsed();
if elapsed > Duration::from_millis(100) {
    log::warn!("命令执行耗时过长: {:?}", elapsed);
}
```

---

### 3. 测试窗口

#### 创建测试接收窗口
```rust
// examples/test_receiver.rs
fn main() {
    // 创建窗口
    let hwnd = create_test_window();
    println!("测试窗口 HWND: {:?}", hwnd);
    
    // 消息循环，打印收到的消息
    loop {
        let mut msg = MSG::default();
        unsafe {
            if GetMessageW(&mut msg, hwnd, 0, 0).as_bool() {
                println!("收到消息: {:?}", msg.message);
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}
```

---

## 兼容性注意事项

### 1. 不同程序的响应差异

| 程序类型 | PostMessage | SendInput | 备注 |
|---------|------------|-----------|------|
| 记事本 | ✅ | ✅ | 完全支持 |
| 浏览器 | ✅ | ✅ | 需要客户区坐标 |
| UWP 应用 | ⚠️ | ✅ | PostMessage 可能失效 |
| DirectX 游戏 | ❌ | ⚠️ | 需要驱动级方案 |
| Electron 应用 | ✅ | ✅ | 完全支持 |

---

### 2. UAC 权限

#### 问题
向提升权限的程序发送消息需要管理员权限。

#### 解决方案
```xml
<!-- 添加 manifest 文件 -->
<requestedExecutionLevel level="requireAdministrator" uiAccess="false" />
```

或在代码中检测：
```rust
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION};

fn is_elevated_process(hwnd: HWND) -> bool {
    // 检查目标进程是否提升权限
    // 如果当前进程权限不足，返回错误提示
}
```

---

## 安全注意事项

### 1. 防止注入攻击

#### 问题
如果脚本路径可配置，用户可能指向恶意脚本。

#### 缓解
```rust
use std::path::Path;

fn validate_script_path(path: &Path) -> Result<(), String> {
    // 只允许指定目录下的脚本
    let canonical = path.canonicalize()
        .map_err(|_| "无效路径".to_string())?;
    
    let allowed_dir = Path::new("./scripts").canonicalize().unwrap();
    
    if !canonical.starts_with(allowed_dir) {
        return Err("脚本路径不在允许范围内".to_string());
    }
    
    Ok(())
}
```

---

### 2. 限制脚本权限

#### 禁用危险操作
```rust
// 脚本中不允许文件操作、网络请求
// 只允许键盘鼠标和延迟命令
```

---

## 常见问题排查

### Q1: PostMessage 发送成功但程序无响应

**可能原因**:
1. 目标程序使用 DirectInput/RawInput
2. LPARAM 构造错误（扫描码）
3. 窗口不是激活的客户区

**排查步骤**:
1. 用 Spy++ 确认消息是否到达
2. 对比手动按键和 PostMessage 的 LPARAM
3. 尝试 SendInput 对比

---

### Q2: 热键注册失败

**可能原因**:
1. 热键被其他程序占用
2. 消息窗口未创建

**解决方案**:
```rust
// 尝试备用热键
const HOTKEY_ALTERNATIVES: &[(u32, u32)] = &[
    (MOD_CONTROL | MOD_SHIFT, '9' as u32),
    (MOD_CONTROL | MOD_ALT, '9' as u32),
    (MOD_ALT | MOD_SHIFT, '9' as u32),
];

for (modifiers, vk) in HOTKEY_ALTERNATIVES {
    if RegisterHotKey(hwnd, id, *modifiers, *vk).is_ok() {
        println!("使用备用热键: {:?}", (modifiers, vk));
        break;
    }
}
```

---

### Q3: 脚本执行卡顿

**可能原因**:
1. delay_ms 过长且不可中断
2. find_color 频繁截图

**优化**:
1. 确保 delay 可中断（每 10ms 检查 stop_flag）
2. 限制 find_color 每秒查找次数
3. 使用区域截图而非全屏

---

## 推荐资源

### 官方文档
- [Windows API Index](https://docs.microsoft.com/en-us/windows/win32/api/)
- [windows-rs crate](https://docs.rs/windows/latest/windows/)
- [egui documentation](https://docs.rs/egui/latest/egui/)

### 工具
- **Spy++**: Windows 消息监控
- **Process Explorer**: 进程信息查看
- **WinDbg**: 调试工具

### 相关项目参考
- [global-hotkey](https://github.com/tauri-apps/global-hotkey) - 全局热键库
- [inputbot](https://github.com/obv-mikhail/InputBot) - 键鼠模拟
- [autopilot-rs](https://github.com/autopilot-rs/autopilot-rs) - 自动化框架
