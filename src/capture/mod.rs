// 截图与颜色查找模块

pub mod backend;
pub mod print_window;
pub mod color;

pub use backend::CaptureBackend;
pub use print_window::PrintWindowCapture;
pub use color::{Bitmap, color_exists_in_area};
