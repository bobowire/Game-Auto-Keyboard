// 脚本执行引擎 - 解释执行 AST

use crate::script::ast::*;
use crate::input::InputBackend;
use crate::capture::{CaptureBackend, PrintWindowCapture, color_exists_in_area};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

/// find_color 颜色匹配的默认容差（每通道 ±10）
const COLOR_TOLERANCE: u8 = 10;

pub struct ScriptExecutor<'a> {
    input: &'a dyn InputBackend,
    hwnd: HWND,
    stop_flag: Option<&'a AtomicBool>,
    capture: Box<dyn CaptureBackend>,
}

impl<'a> ScriptExecutor<'a> {
    pub fn new(input: &'a dyn InputBackend, hwnd: HWND) -> Self {
        Self {
            input,
            hwnd,
            stop_flag: None,
            capture: Box::new(PrintWindowCapture::new()),
        }
    }

    /// 一次性执行所有命令（无中断）
    pub fn execute(&self, commands: &[Command]) -> Result<(), String> {
        for cmd in commands {
            self.execute_command(cmd)?;
        }
        Ok(())
    }

    /// 可中断执行：每条命令前检查停止标志，delay 分段可中断
    pub fn execute_interruptible(
        &self,
        commands: &[Command],
        stop_flag: &'a AtomicBool,
    ) -> Result<(), String> {
        let exec = ScriptExecutor {
            input: self.input,
            hwnd: self.hwnd,
            stop_flag: Some(stop_flag),
            capture: Box::new(PrintWindowCapture::new()),
        };
        for cmd in commands {
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }
            exec.execute_command(cmd)?;
        }
        Ok(())
    }

    /// 是否已请求停止
    fn should_stop(&self) -> bool {
        self.stop_flag
            .map_or(false, |f| f.load(Ordering::Relaxed))
    }

    /// 可中断的睡眠：分成 10ms 片段，便于及时响应停止
    fn sleep_ms(&self, ms: u32) {
        let mut remaining = ms;
        while remaining > 0 {
            if self.should_stop() {
                return;
            }
            let chunk = remaining.min(10);
            thread::sleep(Duration::from_millis(chunk as u64));
            remaining -= chunk;
        }
    }

    fn execute_command(&self, cmd: &Command) -> Result<(), String> {
        match cmd {
            Command::Setting(_) => {
                // 设置项在加载时已提取，执行时跳过
            }
            Command::Down(key) => {
                println!("[执行] down({})", key);
                self.input.send_key_down(self.hwnd, key)?;
            }
            Command::Up(key) => {
                println!("[执行] up({})", key);
                self.input.send_key_up(self.hwnd, key)?;
            }
            Command::Click(key) => {
                println!("[执行] click({})", key);
                self.input.send_key_down(self.hwnd, key)?;
                self.input.send_key_up(self.hwnd, key)?;
            }
            Command::ClickMs(key, ms) => {
                println!("[执行] click_ms({},{})", key, ms);
                self.input.send_key_down(self.hwnd, key)?;
                self.sleep_ms(*ms);
                self.input.send_key_up(self.hwnd, key)?;
            }
            Command::DelayMs(ms) => {
                println!("[执行] delay_ms({})", ms);
                self.sleep_ms(*ms);
            }
            Command::SendWindowActive => {
                println!("[执行] send_window_active()");
                self.input.send_window_active(self.hwnd)?;
            }
            Command::MouseMove(coord) => {
                let (x, y) = self.resolve_coord(coord)?;
                println!("[执行] mouse_move({}, {})", x, y);
                self.input.send_mouse_move(self.hwnd, x, y)?;
            }
            Command::MouseDown(btn, coord) => {
                let (x, y) = self.resolve_coord(coord)?;
                println!("[执行] mouse_down({:?}, {}, {})", btn, x, y);
                self.input.send_mouse_down(self.hwnd, *btn, x, y)?;
            }
            Command::MouseUp(btn) => {
                println!("[执行] mouse_up({:?})", btn);
                self.input.send_mouse_up(self.hwnd, *btn)?;
            }
            Command::MouseClick(btn, coord) => {
                let (x, y) = self.resolve_coord(coord)?;
                println!("[执行] mouse_click({:?}, {}, {})", btn, x, y);
                self.input.send_mouse_down(self.hwnd, *btn, x, y)?;
                self.input.send_mouse_up(self.hwnd, *btn)?;
            }
            Command::If { condition, then_block, else_if_blocks } => {
                if self.eval_bool_expr(condition)? {
                    println!("[执行] if 条件为真");
                    self.execute_block(then_block)?;
                } else {
                    let mut matched = false;
                    for (cond, block) in else_if_blocks {
                        if self.eval_bool_expr(cond)? {
                            println!("[执行] else_if 条件为真");
                            self.execute_block(block)?;
                            matched = true;
                            break;
                        }
                    }
                    if !matched {
                        println!("[执行] if/else_if 均不满足，跳过");
                    }
                }
            }
        }
        Ok(())
    }

    /// 执行一个命令块（if/else_if 分支体），每条命令前检查停止标志
    fn execute_block(&self, commands: &[Command]) -> Result<(), String> {
        for cmd in commands {
            if self.should_stop() {
                break;
            }
            self.execute_command(cmd)?;
        }
        Ok(())
    }

    /// 计算实际客户区坐标
    fn resolve_coord(&self, coord: &Coord) -> Result<(i32, i32), String> {
        match coord {
            Coord::Absolute { x, y } => Ok((*x, *y)),
            Coord::Center { dx, dy } => {
                let (w, h) = self.client_size()?;
                Ok((w / 2 + dx, h / 2 + dy))
            }
            Coord::Percent { px, py } => {
                let (w, h) = self.client_size()?;
                Ok((w * px / 100, h * py / 100))
            }
        }
    }

    /// 获取窗口客户区尺寸
    fn client_size(&self) -> Result<(i32, i32), String> {
        let mut rect = RECT::default();
        unsafe {
            GetClientRect(self.hwnd, &mut rect)
                .map_err(|e| format!("获取窗口尺寸失败: {:?}", e))?;
        }
        Ok((rect.right - rect.left, rect.bottom - rect.top))
    }

    fn eval_bool_expr(&self, expr: &BoolExpr) -> Result<bool, String> {
        let left = self.eval_value(&expr.left)?;
        let right = self.eval_value(&expr.right)?;
        Ok(match expr.op {
            CompareOp::Eq => left == right,
            CompareOp::Ne => left != right,
        })
    }

    /// 求值为布尔
    fn eval_value(&self, value: &Value) -> Result<bool, String> {
        match value {
            Value::Bool(b) => Ok(*b),
            Value::FindColor { area, color } => self.eval_find_color(area, *color),
        }
    }

    /// 执行颜色查找：截图 → 计算区域 → 逐像素匹配
    fn eval_find_color(&self, area: &FindArea, color: u32) -> Result<bool, String> {
        // 截取窗口客户区
        let bitmap = self.capture.capture(self.hwnd)?;

        // 把 FindArea 转成位图内的绝对矩形 (x, y, w, h)
        let (x, y, w, h) = self.resolve_find_area(area)?;

        let found = color_exists_in_area(&bitmap, x, y, w, h, color, COLOR_TOLERANCE);
        println!(
            "  [条件] find_color 区域({},{},{},{}) 颜色#{:06x} => {}",
            x, y, w, h, color, found
        );
        Ok(found)
    }

    /// 将 FindArea 的三种定位方式转换为位图内的绝对矩形
    fn resolve_find_area(&self, area: &FindArea) -> Result<(i32, i32, i32, i32), String> {
        Ok(match area {
            FindArea::Absolute { x, y, w, h } => (*x, *y, *w, *h),
            FindArea::Center { dx, dy, w, h } => {
                let (cw, ch) = self.client_size()?;
                (cw / 2 + dx, ch / 2 + dy, *w, *h)
            }
            FindArea::Percent { px, py, w, h } => {
                let (cw, ch) = self.client_size()?;
                (cw * px / 100, ch * py / 100, *w, *h)
            }
        })
    }
}
