# 配置管理

## 配置文件结构

### 窗口配置 (config/window_config.json)

```json
{
  "version": "1.0",
  "scripts_dir": "./scripts",
  "input_backend": "PostMessage (后台)",
  "windows": [
    {
      "index": 1,
      "title": "游戏窗口1",
      "schemes": [
        {
          "id": "farming.ag",
          "display_name": "自动采集"
        },
        {
          "id": "combat.ag",
          "display_name": "战斗模式"
        }
      ],
      "selected_scheme": 0,
      "marked_scheme": 0
    },
    {
      "index": 2,
      "title": "游戏窗口2",
      "schemes": [
        {
          "id": "fishing.ag",
          "display_name": "自动钓鱼"
        }
      ],
      "selected_scheme": 0,
      "marked_scheme": 0
    }
  ],
  "hotkey_timeout_ms": 2000
}
```

---

## 配置加载

**位置**: `src/app.rs`

```rust
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize)]
struct AppConfig {
    version: String,
    scripts_dir: String,
    input_backend: String,
    windows: Vec<WindowConfig>,
    #[serde(default = "default_hotkey_timeout")]
    hotkey_timeout_ms: u64,
}

#[derive(Serialize, Deserialize)]
struct WindowConfig {
    index: u8,
    title: String,
    schemes: Vec<SchemeRef>,
    selected_scheme: usize,
    marked_scheme: usize,
}

fn default_hotkey_timeout() -> u64 {
    2000
}

impl AutoKeyboardApp {
    pub fn load_config() -> Result<AppConfig, String> {
        let config_path = "config/window_config.json";
        
        if !Path::new(config_path).exists() {
            // 创建默认配置
            let default_config = AppConfig {
                version: "1.0".to_string(),
                scripts_dir: "./scripts".to_string(),
                input_backend: "PostMessage (后台)".to_string(),
                windows: Vec::new(),
                hotkey_timeout_ms: 2000,
            };
            
            Self::save_config_to_file(&default_config, config_path)?;
            return Ok(default_config);
        }
        
        let content = std::fs::read_to_string(config_path)
            .map_err(|e| format!("读取配置文件失败: {}", e))?;
        
        serde_json::from_str(&content)
            .map_err(|e| format!("解析配置文件失败: {}", e))
    }
    
    pub fn save_config(&self) -> Result<(), String> {
        let config = AppConfig {
            version: "1.0".to_string(),
            scripts_dir: "./scripts".to_string(),
            input_backend: self.input_manager.current().name().to_string(),
            windows: self.windows.iter()
                .filter(|s| s.hwnd.is_some())
                .map(|s| WindowConfig {
                    index: s.index,
                    title: s.title.clone(),
                    schemes: s.schemes.clone(),
                    selected_scheme: s.selected_scheme,
                    marked_scheme: s.marked_scheme,
                })
                .collect(),
            hotkey_timeout_ms: self.state_machine.timeout.as_millis() as u64,
        };
        
        Self::save_config_to_file(&config, "config/window_config.json")
    }
    
    fn save_config_to_file(config: &AppConfig, path: &str) -> Result<(), String> {
        // 确保目录存在
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建配置目录失败: {}", e))?;
        }
        
        let json = serde_json::to_string_pretty(config)
            .map_err(|e| format!("序列化配置失败: {}", e))?;
        
        std::fs::write(path, json)
            .map_err(|e| format!("写入配置文件失败: {}", e))?;
        
        Ok(())
    }
    
    pub fn apply_config(&mut self, config: AppConfig) {
        // 应用输入后端
        if let Err(e) = self.input_manager.switch_backend(&config.input_backend) {
            log::warn!("切换输入后端失败: {}, 使用默认", e);
        }
        
        // 应用热键超时
        self.state_machine.set_timeout(
            std::time::Duration::from_millis(config.hotkey_timeout_ms)
        );
        
        // 应用窗口配置（不含 HWND，需要用户重新绑定）
        for window_config in config.windows {
            let idx = (window_config.index - 1) as usize;
            if idx < 8 {
                let slot = &mut self.windows[idx];
                slot.title = window_config.title;
                slot.schemes = window_config.schemes;
                slot.selected_scheme = window_config.selected_scheme;
                slot.marked_scheme = window_config.marked_scheme;
            }
        }
        
        log::info!("配置已应用");
    }
}
```

---

## 方案管理器

**位置**: `src/scheme/manager.rs`

```rust
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use crate::script::{Script, ScriptLoader};
use crate::scheme::Scheme;

/// 方案管理器（全局方案管理）
pub struct SchemeManager {
    scripts_dir: PathBuf,
    schemes: HashMap<String, Scheme>,
}

impl SchemeManager {
    pub fn new(scripts_dir: impl Into<PathBuf>) -> Result<Self, String> {
        let scripts_dir = scripts_dir.into();
        let mut manager = Self {
            scripts_dir: scripts_dir.clone(),
            schemes: HashMap::new(),
        };
        
        manager.reload()?;
        Ok(manager)
    }
    
    /// 重新加载所有脚本
    pub fn reload(&mut self) -> Result<(), String> {
        let loader = ScriptLoader::new(&self.scripts_dir);
        let schemes = loader.load_all()?;
        
        self.schemes.clear();
        for scheme in schemes {
            self.schemes.insert(scheme.id.clone(), scheme);
        }
        
        log::info!("已加载 {} 个脚本", self.schemes.len());
        Ok(())
    }
    
    /// 获取脚本（懒加载）
    pub fn get_script(&mut self, id: &str) -> Result<Arc<Script>, String> {
        let scheme = self.schemes.get_mut(id)
            .ok_or_else(|| format!("脚本不存在: {}", id))?;
        
        // 如果脚本未解析，则解析
        if scheme.script.is_none() {
            let content = std::fs::read_to_string(&scheme.file_path)
                .map_err(|e| format!("读取脚本文件失败: {}", e))?;
            
            let script = crate::script::Parser::parse_from_string(&content)?;
            scheme.script = Some(script);
        }
        
        Ok(Arc::new(scheme.script.clone().unwrap()))
    }
    
    /// 获取所有方案列表
    pub fn get_all_schemes(&self) -> Vec<&Scheme> {
        self.schemes.values().collect()
    }
    
    /// 获取方案引用列表（用于 UI）
    pub fn get_scheme_refs(&self) -> Vec<SchemeRef> {
        self.schemes.values()
            .map(|s| SchemeRef {
                id: s.id.clone(),
                display_name: s.display_name.clone(),
            })
            .collect()
    }
}
```

---

## 脚本热重载（可选）

使用 `notify` crate 监听文件变化：

### Cargo.toml 添加依赖

```toml
[dependencies]
notify = "6"
```

### 实现文件监听

**位置**: `src/scheme/manager.rs`

```rust
use notify::{Watcher, RecursiveMode, Event};
use crossbeam_channel::{Sender, Receiver, unbounded};

pub struct SchemeManager {
    scripts_dir: PathBuf,
    schemes: HashMap<String, Scheme>,
    
    // 文件监听
    _watcher: Option<Box<dyn Watcher>>,
    reload_rx: Receiver<()>,
}

impl SchemeManager {
    pub fn new_with_watch(scripts_dir: impl Into<PathBuf>) -> Result<Self, String> {
        let scripts_dir = scripts_dir.into();
        let (reload_tx, reload_rx) = unbounded();
        
        // 创建文件监听器
        let mut watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
            if let Ok(_event) = res {
                reload_tx.send(()).ok();
            }
        }).map_err(|e| format!("创建文件监听器失败: {}", e))?;
        
        watcher.watch(&scripts_dir, RecursiveMode::NonRecursive)
            .map_err(|e| format!("监听目录失败: {}", e))?;
        
        let mut manager = Self {
            scripts_dir: scripts_dir.clone(),
            schemes: HashMap::new(),
            _watcher: Some(Box::new(watcher)),
            reload_rx,
        };
        
        manager.reload()?;
        Ok(manager)
    }
    
    /// 检查是否有文件变化（UI 循环调用）
    pub fn check_reload(&mut self) {
        if self.reload_rx.try_recv().is_ok() {
            log::info!("检测到脚本文件变化，重新加载");
            if let Err(e) = self.reload() {
                log::error!("重新加载失败: {}", e);
            }
        }
    }
}
```

### 在 UI 中集成

```rust
impl eframe::App for AutoKeyboardApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 检查脚本文件变化
        self.scheme_manager.check_reload();
        
        // ... 其他 UI 代码
    }
}
```

---

## 日志配置

### 环境变量配置

```rust
// main.rs
fn main() {
    // 设置日志级别
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info")
    ).init();
    
    log::info!("应用启动");
    
    // ...
}
```

### 运行时配置

```bash
# Windows PowerShell
$env:RUST_LOG="debug"
cargo run

# Windows CMD
set RUST_LOG=debug
cargo run

# 分模块配置
$env:RUST_LOG="game_auto_keyboard=debug,script::executor=trace"
```

### 日志输出到文件

```rust
use std::fs::File;

fn main() {
    let log_file = File::create("game_auto_keyboard.log")
        .expect("创建日志文件失败");
    
    env_logger::Builder::new()
        .target(env_logger::Target::Pipe(Box::new(log_file)))
        .filter_level(log::LevelFilter::Info)
        .init();
    
    // ...
}
```

---

## 配置验证

### 启动时验证

```rust
impl AutoKeyboardApp {
    pub fn new() -> Self {
        // 加载配置
        let config = match Self::load_config() {
            Ok(c) => c,
            Err(e) => {
                log::warn!("加载配置失败: {}, 使用默认配置", e);
                AppConfig::default()
            }
        };
        
        // 验证脚本目录
        if !Path::new(&config.scripts_dir).exists() {
            log::warn!("脚本目录不存在: {}, 尝试创建", config.scripts_dir);
            std::fs::create_dir_all(&config.scripts_dir).ok();
        }
        
        // 创建应用
        let mut app = Self::default();
        app.apply_config(config);
        app
    }
}
```

### 配置迁移

```rust
impl AppConfig {
    pub fn migrate(mut self) -> Self {
        // 版本 1.0 -> 1.1
        if self.version == "1.0" {
            self.version = "1.1".to_string();
            // 添加新字段的默认值
        }
        self
    }
}
```

---

## 预设配置模板

### 游戏配置模板

创建 `config/presets/` 目录存放预设：

```json
// config/presets/mmo_game.json
{
  "name": "MMO 游戏模板",
  "scripts_dir": "./scripts/mmo",
  "input_backend": "PostMessage (后台)",
  "hotkey_timeout_ms": 1500,
  "description": "适用于多开 MMO 游戏"
}
```

### 加载预设

```rust
impl AutoKeyboardApp {
    pub fn load_preset(&mut self, preset_name: &str) -> Result<(), String> {
        let path = format!("config/presets/{}.json", preset_name);
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("读取预设失败: {}", e))?;
        
        let config: AppConfig = serde_json::from_str(&content)
            .map_err(|e| format!("解析预设失败: {}", e))?;
        
        self.apply_config(config);
        Ok(())
    }
}
```

---

## 配置备份

### 自动备份

```rust
impl AutoKeyboardApp {
    fn save_config(&self) -> Result<(), String> {
        let config_path = "config/window_config.json";
        
        // 备份旧配置
        if Path::new(config_path).exists() {
            let backup_path = format!("{}.backup", config_path);
            std::fs::copy(config_path, backup_path).ok();
        }
        
        // 保存新配置
        // ...
    }
}
```

### 恢复备份

```rust
impl AutoKeyboardApp {
    pub fn restore_backup(&mut self) -> Result<(), String> {
        let backup_path = "config/window_config.json.backup";
        let config_path = "config/window_config.json";
        
        std::fs::copy(backup_path, config_path)
            .map_err(|e| format!("恢复备份失败: {}", e))?;
        
        let config = Self::load_config()?;
        self.apply_config(config);
        
        Ok(())
    }
}
```
