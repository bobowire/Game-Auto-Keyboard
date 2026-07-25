// 语法解析器 - 递归下降，将 token 流转换为 AST

use crate::script::token::{Token, Tokenizer};
use crate::script::ast::*;

/// 如果错误信息尚未带行号（tokenizer 的错误已自带），则补上行号前缀
fn with_line(line: u32, msg: String) -> String {
    if msg.starts_with("第 ") {
        msg
    } else {
        format!("第 {} 行: {}", line, msg)
    }
}

pub struct Parser {
    tk: Tokenizer,
}

impl Parser {
    pub fn new(content: &str) -> Result<Self, String> {
        Ok(Self {
            tk: Tokenizer::new(content)?,
        })
    }

    /// 解析整个脚本为顶层命令列表
    pub fn parse(&mut self) -> Result<Vec<Command>, String> {
        let mut commands = Vec::new();
        loop {
            match self.tk.peek() {
                Token::Eof => break,
                _ => {
                    let line = self.tk.current_line();
                    match self.parse_command() {
                        Ok(cmd) => commands.push(cmd),
                        Err(e) => return Err(with_line(line, e)),
                    }
                }
            }
        }
        Ok(commands)
    }

    /// 解析单条命令（可能是键鼠命令，也可能是 if 块）
    fn parse_command(&mut self) -> Result<Command, String> {
        let ident = match self.tk.next() {
            Token::Ident(s) => s,
            other => return Err(format!("期望命令名，得到: {:?}", other)),
        };

        match ident.as_str() {
            "if_start" => self.parse_if_block(),
            "if_end" => Err("if_end 没有匹配的 if_start".to_string()),
            "else_if" => Err("else_if 没有匹配的 if_start".to_string()),
            _ => self.parse_action(&ident),
        }
    }

    /// 解析键鼠动作命令（带括号参数）
    fn parse_action(&mut self, name: &str) -> Result<Command, String> {
        match name {
            "down" => {
                let key = self.parse_paren_single_ident()?;
                Ok(Command::Down(key))
            }
            "up" => {
                let key = self.parse_paren_single_ident()?;
                Ok(Command::Up(key))
            }
            "click" => {
                let key = self.parse_paren_single_ident()?;
                Ok(Command::Click(key))
            }
            "click_ms" => {
                self.expect(Token::LParen)?;
                let key = self.expect_ident_or_number()?;
                self.expect(Token::Comma)?;
                let ms = self.expect_number()?;
                self.expect(Token::RParen)?;
                Ok(Command::ClickMs(key, ms as u32))
            }
            "delay_ms" => {
                self.expect(Token::LParen)?;
                let ms = self.expect_number()?;
                self.expect(Token::RParen)?;
                Ok(Command::DelayMs(ms as u32))
            }
            "send_window_active" => {
                self.expect(Token::LParen)?;
                self.expect(Token::RParen)?;
                Ok(Command::SendWindowActive)
            }
            "mouse_up" => {
                self.expect(Token::LParen)?;
                let btn = self.expect_mouse_button()?;
                self.expect(Token::RParen)?;
                Ok(Command::MouseUp(btn))
            }
            "mouse_move" | "mouse_move_center" | "mouse_move_percent" => {
                let coord = self.parse_coord_args(name)?;
                Ok(Command::MouseMove(coord))
            }
            "mouse_down" | "mouse_down_center" | "mouse_down_percent" => {
                let (btn, coord) = self.parse_mouse_pos_args(name)?;
                Ok(Command::MouseDown(btn, coord))
            }
            "mouse_click" | "mouse_click_center" | "mouse_click_percent" => {
                let (btn, coord) = self.parse_mouse_pos_args(name)?;
                Ok(Command::MouseClick(btn, coord))
            }
            other => Err(format!("未知命令: {}", other)),
        }
    }

    /// 解析形如 (a, b) 的坐标参数（无鼠标按钮），根据命令后缀决定坐标类型
    fn parse_coord_args(&mut self, name: &str) -> Result<Coord, String> {
        self.expect(Token::LParen)?;
        let a = self.expect_number()?;
        self.expect(Token::Comma)?;
        let b = self.expect_number()?;
        self.expect(Token::RParen)?;

        Ok(if name.ends_with("_center") {
            Coord::Center { dx: a, dy: b }
        } else if name.ends_with("_percent") {
            Coord::Percent { px: a, py: b }
        } else {
            Coord::Absolute { x: a, y: b }
        })
    }

    /// 解析形如 (button, a, b) 的鼠标定位参数，根据命令后缀决定坐标类型
    fn parse_mouse_pos_args(&mut self, name: &str) -> Result<(MouseButton, Coord), String> {
        self.expect(Token::LParen)?;
        let btn = self.expect_mouse_button()?;
        self.expect(Token::Comma)?;
        let a = self.expect_number()?;
        self.expect(Token::Comma)?;
        let b = self.expect_number()?;
        self.expect(Token::RParen)?;

        let coord = if name.ends_with("_center") {
            Coord::Center { dx: a, dy: b }
        } else if name.ends_with("_percent") {
            Coord::Percent { px: a, py: b }
        } else {
            Coord::Absolute { x: a, y: b }
        };

        Ok((btn, coord))
    }

    /// 解析 if_start[cond] ... [else_if[cond] ...] if_end
    fn parse_if_block(&mut self) -> Result<Command, String> {
        // if_start 后面紧跟 [条件]
        let condition = self.parse_bracket_condition()?;

        let mut then_block = Vec::new();
        let mut else_if_blocks: Vec<(BoolExpr, Vec<Command>)> = Vec::new();

        // 当前正在收集的分支目标：None=then, Some(idx)=else_if_blocks[idx]
        let mut current_else_if: Option<usize> = None;

        loop {
            match self.tk.peek() {
                Token::Eof => return Err("if_start 没有匹配的 if_end".to_string()),
                Token::Ident(kw) if kw == "if_end" => {
                    self.tk.next(); // 消费 if_end
                    break;
                }
                Token::Ident(kw) if kw == "else_if" => {
                    self.tk.next(); // 消费 else_if
                    let cond = self.parse_bracket_condition()?;
                    else_if_blocks.push((cond, Vec::new()));
                    current_else_if = Some(else_if_blocks.len() - 1);
                }
                _ => {
                    // 普通命令或嵌套 if，归入当前分支
                    let cmd = self.parse_command()?;
                    match current_else_if {
                        None => then_block.push(cmd),
                        Some(idx) => else_if_blocks[idx].1.push(cmd),
                    }
                }
            }
        }

        Ok(Command::If {
            condition,
            then_block,
            else_if_blocks,
        })
    }

    /// 解析 [ 布尔表达式 ]
    fn parse_bracket_condition(&mut self) -> Result<BoolExpr, String> {
        self.expect(Token::LBracket)?;
        let expr = self.parse_bool_expr()?;
        self.expect(Token::RBracket)?;
        Ok(expr)
    }

    /// 解析布尔表达式：value (== | !=) value
    fn parse_bool_expr(&mut self) -> Result<BoolExpr, String> {
        let left = self.parse_value()?;

        let op = match self.tk.next() {
            Token::Eq => CompareOp::Eq,
            Token::Ne => CompareOp::Ne,
            other => return Err(format!("条件中期望 '==' 或 '!='，得到: {:?}", other)),
        };

        let right = self.parse_value()?;

        Ok(BoolExpr { left, op, right })
    }

    /// 解析值：true / false / find_color(...) / find_color_center(...) / find_color_percent(...)
    fn parse_value(&mut self) -> Result<Value, String> {
        let ident = match self.tk.next() {
            Token::Ident(s) => s,
            other => return Err(format!("条件中期望值，得到: {:?}", other)),
        };

        match ident.as_str() {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            "find_color" | "find_color_center" | "find_color_percent" => {
                self.parse_find_color(&ident)
            }
            other => Err(format!("无效的条件值: {}", other)),
        }
    }

    /// 解析 find_color 系列：(a, b, w, h, #color)
    fn parse_find_color(&mut self, name: &str) -> Result<Value, String> {
        self.expect(Token::LParen)?;
        let a = self.expect_number()?;
        self.expect(Token::Comma)?;
        let b = self.expect_number()?;
        self.expect(Token::Comma)?;
        let w = self.expect_number()?;
        self.expect(Token::Comma)?;
        let h = self.expect_number()?;
        self.expect(Token::Comma)?;
        let color = self.expect_hex_color()?;
        self.expect(Token::RParen)?;

        let area = if name.ends_with("_center") {
            FindArea::Center { dx: a, dy: b, w, h }
        } else if name.ends_with("_percent") {
            FindArea::Percent { px: a, py: b, w, h }
        } else {
            FindArea::Absolute { x: a, y: b, w, h }
        };

        Ok(Value::FindColor { area, color })
    }

    // ===== 辅助方法 =====

    /// 解析 (标识符)，用于 down/up/click 的单参数
    fn parse_paren_single_ident(&mut self) -> Result<String, String> {
        self.expect(Token::LParen)?;
        let key = self.expect_ident_or_number()?;
        self.expect(Token::RParen)?;
        Ok(key)
    }

    fn expect(&mut self, expected: Token) -> Result<(), String> {
        let got = self.tk.next();
        if got == expected {
            Ok(())
        } else {
            Err(format!("期望 {:?}，得到 {:?}", expected, got))
        }
    }

    fn expect_number(&mut self) -> Result<i32, String> {
        match self.tk.next() {
            Token::Number(n) => Ok(n),
            other => Err(format!("期望数字，得到 {:?}", other)),
        }
    }

    fn expect_hex_color(&mut self) -> Result<u32, String> {
        match self.tk.next() {
            Token::HexColor(c) => Ok(c),
            other => Err(format!("期望颜色值(#RRGGBB)，得到 {:?}", other)),
        }
    }

    /// 按键参数可以是标识符(a, space)或数字(1, 2)，统一转成字符串
    fn expect_ident_or_number(&mut self) -> Result<String, String> {
        match self.tk.next() {
            Token::Ident(s) => Ok(s),
            Token::Number(n) => Ok(n.to_string()),
            other => Err(format!("期望按键名，得到 {:?}", other)),
        }
    }

    fn expect_mouse_button(&mut self) -> Result<MouseButton, String> {
        match self.tk.next() {
            Token::Ident(s) => match s.to_ascii_lowercase().as_str() {
                "left" => Ok(MouseButton::Left),
                "right" => Ok(MouseButton::Right),
                "middle" => Ok(MouseButton::Middle),
                other => Err(format!("无效鼠标按钮: {}", other)),
            },
            other => Err(format!("期望鼠标按钮，得到 {:?}", other)),
        }
    }
}
