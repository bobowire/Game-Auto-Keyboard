# UI 设计

## 整体布局

使用 egui 的即时模式 GUI，主窗口分为三个主要面板：

```
┌─────────────────────────────────────────┐
│  游戏自动按键工具                        │
├─────────────────────────────────────────┤
│                                         │
│  [窗口列表面板]                          │
│  ┌──────────────────────────────┐      │
│  │ [1] 游戏窗口1  ● 空闲          │      │
│  │    方案: [自动采集 ▼]  ★       │      │
│  │ [2] 未绑定                    │      │
│  └──────────────────────────────┘      │
│                                         │
│  [输入后端]  PostMessage (后台) ▼       │
│                                         │
│  [查看脚本列表]  [添加窗口 (Ctrl+Alt+A)] │
│                                         │
└─────────────────────────────────────────┘
```

---

## 主应用 UI

**位置**: `src/app.rs`

```rust
impl eframe::App for AutoKeyboardApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. 处理热键事件
        self.process_hotkeys();
        
        // 2. 主面板
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("游戏自动按键工具");
            ui.separator();
            
            // 窗口列表
            self.ui_window_list(ui);
            
            ui.separator();
            
            // 输入后端选择
            self.ui_input_backend_selector(ui);
            
            ui.separator();
            
            // 底部按钮
            ui.horizontal(|ui| {
                if ui.button("📋 查看脚本列表").clicked() {
                    self.show_script_viewer = true;
                }
                
                if ui.button("➕ 添加窗口 (Ctrl+Alt+A)").clicked() {
                    self.is_selecting_window = true;
                }
                
                if ui.button("⏹ 停止所有").clicked() {
                    self.executor_manager.stop_all();
                }
            });
            
            // 提示信息
            ui.separator();
            ui.label("快捷键:");
            ui.label("  Ctrl+Shift+[1-8]: 选择窗口");
            ui.label("  Ctrl+Shift+9: 启动选中窗口（无选择则全部启动）");
            ui.label("  Ctrl+Shift+0: 停止选中窗口（无选择则全部停止）");
        });
        
        // 3. 脚本浏览窗口（可选）
        if self.show_script_viewer {
            self.ui_script_viewer_window(ctx);
        }
        
        // 4. 窗口选择提示（可选）
        if self.is_selecting_window {
            self.ui_window_selector_overlay(ctx);
        }
        
        // 5. 持续刷新（检查热键事件）
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}
```

---

## 窗口列表面板

**位置**: `src/ui/window_list.rs`

```rust
impl AutoKeyboardApp {
    pub fn ui_window_list(&mut self, ui: &mut egui::Ui) {
        ui.heading("窗口列表");
        
        egui::Grid::new("window_grid")
            .num_columns(4)
            .spacing([10.0, 8.0])
            .striped(true)
            .show(ui, |ui| {
                // 表头
                ui.label("编号");
                ui.label("窗口标题");
                ui.label("方案");
                ui.label("状态");
                ui.end_row();
                
                // 每个窗口槽位
                for i in 0..8 {
                    let slot = &mut self.windows[i];
                    let window_index = (i + 1) as u8;
                    
                    // 选择状态指示器
                    let is_selected = self.state_machine.is_selected(window_index);
                    if is_selected {
                        ui.colored_label(egui::Color32::YELLOW, format!("[{}]", window_index));
                    } else {
                        ui.label(format!("[{}]", window_index));
                    }
                    
                    // 窗口标题
                    if slot.hwnd.is_some() {
                        ui.label(&slot.title);
                    } else {
                        ui.colored_label(egui::Color32::GRAY, "未绑定");
                    }
                    
                    // 方案下拉框
                    if slot.hwnd.is_some() && !slot.schemes.is_empty() {
                        egui::ComboBox::from_id_source(format!("scheme_{}", i))
                            .selected_text(&slot.schemes[slot.selected_scheme].display_name)
                            .show_ui(ui, |ui| {
                                for (idx, scheme) in slot.schemes.iter().enumerate() {
                                    let is_marked = idx == slot.marked_scheme;
                                    let label = if is_marked {
                                        format!("★ {}", scheme.display_name)
                                    } else {
                                        scheme.display_name.clone()
                                    };
                                    
                                    if ui.selectable_label(idx == slot.selected_scheme, label).clicked() {
                                        slot.selected_scheme = idx;
                                    }
                                }
                            });
                        
                        // 标识按钮
                        if ui.small_button(if slot.marked_scheme == slot.selected_scheme { "★" } else { "☆" }).clicked() {
                            slot.marked_scheme = slot.selected_scheme;
                        }
                    } else {
                        ui.label("-");
                    }
                    
                    // 状态显示
                    let is_running = self.executor_manager.is_running(window_index);
                    if is_running {
                        ui.colored_label(egui::Color32::GREEN, "▶ 运行中");
                    } else if slot.hwnd.is_some() {
                        ui.label("● 空闲");
                    } else {
                        ui.label("");
                    }
                    
                    ui.end_row();
                }
            });
    }
}
```

---

## 输入后端选择器

**位置**: `src/ui/mod.rs`

```rust
impl AutoKeyboardApp {
    pub fn ui_input_backend_selector(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("输入模式:");
            
            let current_name = self.input_manager.current().name();
            
            egui::ComboBox::from_label("")
                .selected_text(current_name)
                .show_ui(ui, |ui| {
                    for backend_name in self.input_manager.available_backends() {
                        if ui.selectable_label(backend_name == current_name, &backend_name).clicked() {
                            if let Err(e) = self.input_manager.switch_backend(&backend_name) {
                                log::error!("切换输入后端失败: {}", e);
                            } else {
                                // 更新执行器的后端
                                self.executor_manager.update_input_backend(self.input_manager.current());
                                log::info!("已切换到: {}", backend_name);
                            }
                        }
                    }
                });
            
            // 后端说明
            let backend = self.input_manager.current();
            let desc = if backend.supports_background() {
                "✓ 支持后台发送"
            } else {
                "✗ 仅前台有效"
            };
            ui.colored_label(
                if backend.supports_background() {
                    egui::Color32::GREEN
                } else {
                    egui::Color32::YELLOW
                },
                desc
            );
        });
    }
}
```

---

## 脚本浏览窗口

**位置**: `src/ui/script_viewer.rs`

```rust
impl AutoKeyboardApp {
    pub fn ui_script_viewer_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("脚本列表")
            .default_width(600.0)
            .default_height(400.0)
            .open(&mut self.show_script_viewer)
            .show(ctx, |ui| {
                ui.label("所有可用脚本:");
                ui.separator();
                
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let schemes = self.scheme_manager.get_all_schemes();
                    
                    if schemes.is_empty() {
                        ui.label("未找到任何 .ag 脚本文件");
                        ui.label("请将脚本放入 scripts/ 目录");
                    } else {
                        for scheme in schemes {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.heading(&scheme.display_name);
                                    
                                    if ui.small_button("查看").clicked() {
                                        self.selected_script_id = Some(scheme.id.clone());
                                    }
                                });
                                
                                ui.label(format!("文件: {}", scheme.id));
                                
                                // 显示脚本内容（可选）
                                if self.selected_script_id.as_ref() == Some(&scheme.id) {
                                    ui.separator();
                                    
                                    if let Some(script) = &scheme.script {
                                        ui.label(format!("命令数: {}", script.statements.len()));
                                        
                                        // 显示脚本文本
                                        egui::ScrollArea::vertical()
                                            .max_height(200.0)
                                            .show(ui, |ui| {
                                                if let Ok(content) = std::fs::read_to_string(&scheme.file_path) {
                                                    ui.code(&content);
                                                }
                                            });
                                    }
                                }
                            });
                            
                            ui.add_space(5.0);
                        }
                    }
                });
                
                ui.separator();
                
                if ui.button("🔄 刷新列表").clicked() {
                    if let Err(e) = self.scheme_manager.reload() {
                        log::error!("重新加载脚本失败: {}", e);
                    }
                }
            });
    }
}
```

---

## 窗口选择覆盖层

**位置**: `src/ui/window_list.rs`

```rust
impl AutoKeyboardApp {
    pub fn ui_window_selector_overlay(&mut self, ctx: &egui::Context) {
        // 半透明遮罩
        egui::Area::new("window_selector_overlay")
            .fixed_pos(egui::pos2(0.0, 0.0))
            .show(ctx, |ui| {
                let screen_rect = ctx.screen_rect();
                ui.painter().rect_filled(
                    screen_rect,
                    0.0,
                    egui::Color32::from_black_alpha(180),
                );
                
                // 提示文本
                egui::Window::new("选择窗口")
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label("请点击要添加的目标窗口");
                        ui.label("(按 ESC 取消)");
                        
                        ui.separator();
                        
                        if ui.button("取消").clicked() {
                            self.is_selecting_window = false;
                        }
                    });
            });
        
        // 检查鼠标点击
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.is_selecting_window = false;
        }
        
        // 检测鼠标点击窗口
        if ctx.input(|i| i.pointer.primary_clicked()) {
            if let Some(hwnd) = self.capture_window_under_cursor() {
                self.add_window_to_slot(hwnd);
                self.is_selecting_window = false;
            }
        }
    }
    
    fn capture_window_under_cursor(&self) -> Option<HWND> {
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, WindowFromPoint};
            
            let mut point = Default::default();
            if GetCursorPos(&mut point).is_ok() {
                let hwnd = WindowFromPoint(point);
                if !hwnd.is_invalid() {
                    return Some(hwnd);
                }
            }
        }
        None
    }
    
    fn add_window_to_slot(&mut self, hwnd: HWND) {
        // 查找第一个空槽位
        for slot in &mut self.windows {
            if slot.hwnd.is_none() {
                slot.hwnd = Some(hwnd);
                slot.title = crate::utils::win32::get_window_title(hwnd)
                    .unwrap_or_else(|_| format!("窗口 {:?}", hwnd));
                
                // 加载方案列表
                slot.schemes = self.scheme_manager.get_all_schemes()
                    .iter()
                    .map(|s| SchemeRef {
                        id: s.id.clone(),
                        display_name: s.display_name.clone(),
                    })
                    .collect();
                
                if !slot.schemes.is_empty() {
                    slot.selected_scheme = 0;
                    slot.marked_scheme = 0;
                }
                
                log::info!("窗口 {} 已添加: {}", slot.index, slot.title);
                break;
            }
        }
    }
}
```

---

## 工具函数补充

**位置**: `src/utils/win32.rs`

```rust
use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;
use windows::core::HSTRING;

/// 获取窗口标题
pub fn get_window_title(hwnd: HWND) -> Result<String, String> {
    unsafe {
        let mut buffer = [0u16; 256];
        let len = GetWindowTextW(hwnd, &mut buffer);
        
        if len > 0 {
            Ok(String::from_utf16_lossy(&buffer[..len as usize]))
        } else {
            Err("获取窗口标题失败".to_string())
        }
    }
}
```

---

## 样式定制

**位置**: `src/main.rs`

```rust
fn main() {
    env_logger::init();
    
    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(800.0, 600.0)),
        ..Default::default()
    };
    
    eframe::run_native(
        "游戏自动按键工具",
        options,
        Box::new(|cc| {
            // 自定义样式
            cc.egui_ctx.set_visuals(egui::Visuals {
                window_rounding: 5.0.into(),
                ..Default::default()
            });
            
            Box::new(AutoKeyboardApp::new())
        }),
    ).expect("启动应用失败");
}
```

---

## 状态持久化（可选）

在退出时保存窗口配置：

```rust
impl Drop for AutoKeyboardApp {
    fn drop(&mut self) {
        // 保存配置
        if let Err(e) = self.save_config() {
            log::error!("保存配置失败: {}", e);
        }
    }
}

impl AutoKeyboardApp {
    fn save_config(&self) -> Result<(), String> {
        use serde::Serialize;
        
        #[derive(Serialize)]
        struct Config {
            windows: Vec<WindowConfig>,
            input_backend: String,
        }
        
        #[derive(Serialize)]
        struct WindowConfig {
            index: u8,
            title: String,
            selected_scheme: usize,
            marked_scheme: usize,
        }
        
        let config = Config {
            windows: self.windows.iter()
                .filter(|s| s.hwnd.is_some())
                .map(|s| WindowConfig {
                    index: s.index,
                    title: s.title.clone(),
                    selected_scheme: s.selected_scheme,
                    marked_scheme: s.marked_scheme,
                })
                .collect(),
            input_backend: self.input_manager.current().name().to_string(),
        };
        
        let json = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("序列化失败: {}", e))?;
        
        std::fs::write("config/window_config.json", json)
            .map_err(|e| format!("写入配置文件失败: {}", e))?;
        
        Ok(())
    }
}
```

---

## 响应式设计

根据窗口大小调整布局：

```rust
impl AutoKeyboardApp {
    pub fn ui_responsive(&mut self, ui: &mut egui::Ui) {
        let available_width = ui.available_width();
        
        if available_width < 600.0 {
            // 窄屏：垂直布局
            self.ui_window_list_vertical(ui);
        } else {
            // 宽屏：网格布局
            self.ui_window_list(ui);
        }
    }
}
```
