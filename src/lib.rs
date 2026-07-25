pub mod input;
pub mod script;
pub mod capture;
pub mod utils;
pub mod runner;
pub mod hotkey;
pub mod window_slot;
pub mod config;
pub mod tray;
pub mod color_picker;
pub mod app;

// 重新导出常用类型
pub use input::{InputManager, InputBackend, PostMessageBackend, MouseButton};
pub use script::{Parser, ScriptExecutor, Command, ScriptFile, load_dir};
pub use runner::Runner;
pub use hotkey::{HotkeyManager, HotkeyStateMachine, HotkeyAction};
pub use window_slot::{WindowSlot, Scheme};
pub use app::App;
