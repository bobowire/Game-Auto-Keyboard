pub mod backend;
pub mod post_message;
pub mod keymap;

use std::sync::Arc;
pub use backend::InputBackend;
pub use post_message::PostMessageBackend;
pub use keymap::{MouseButton, parse_key, parse_mouse_button};

/// 全局输入后端管理器
pub struct InputManager {
    current: Arc<dyn InputBackend>,
    available: Vec<Arc<dyn InputBackend>>,
}

impl InputManager {
    pub fn new() -> Self {
        let backends: Vec<Arc<dyn InputBackend>> = vec![
            Arc::new(PostMessageBackend::new()),
        ];

        let current = backends[0].clone();

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
