// 词法分析器 - 字符级扫描，将 .ag 脚本切分成 token 流
// 支持括号式语法：down(1), click_ms(2,50), find_color(...), if_start[... == true]

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),   // 标识符：down, click_ms, find_color, left, true...
    Number(i32),     // 整数：100, -5
    HexColor(u32),   // #ff00ff -> 0xff00ff
    LParen,          // (
    RParen,          // )
    LBracket,        // [
    RBracket,        // ]
    Comma,           // ,
    Eq,              // ==
    Ne,              // !=
    Eof,
}

pub struct Tokenizer {
    chars: Vec<char>,
    pos: usize,
    line: u32,
    tokens: Vec<Token>,
    /// 与 tokens 一一对应的行号
    token_lines: Vec<u32>,
    cursor: usize,
}

impl Tokenizer {
    /// 预处理：去掉注释和空白，一次性扫描出所有 token
    pub fn new(content: &str) -> Result<Self, String> {
        // 逐行去掉 // 注释，再拼回字符流
        let mut cleaned = String::new();
        for line in content.lines() {
            let code = match line.find("//") {
                Some(idx) => &line[..idx],
                None => line,
            };
            cleaned.push_str(code);
            cleaned.push('\n');
        }

        let mut tk = Self {
            chars: cleaned.chars().collect(),
            pos: 0,
            line: 1,
            tokens: Vec::new(),
            token_lines: Vec::new(),
            cursor: 0,
        };
        tk.scan_all()?;
        Ok(tk)
    }

    /// 记录 token 及其所在行号
    fn push(&mut self, tok: Token) {
        self.tokens.push(tok);
        self.token_lines.push(self.line);
    }

    fn scan_all(&mut self) -> Result<(), String> {
        while self.pos < self.chars.len() {
            let c = self.chars[self.pos];

            if c == '\n' {
                self.line += 1;
                self.pos += 1;
                continue;
            }
            if c.is_whitespace() {
                self.pos += 1;
                continue;
            }

            match c {
                '(' => { self.push(Token::LParen); self.pos += 1; }
                ')' => { self.push(Token::RParen); self.pos += 1; }
                '[' => { self.push(Token::LBracket); self.pos += 1; }
                ']' => { self.push(Token::RBracket); self.pos += 1; }
                ',' => { self.push(Token::Comma); self.pos += 1; }
                '=' => {
                    if self.peek_char(1) == Some('=') {
                        self.push(Token::Eq);
                        self.pos += 2;
                    } else {
                        return Err(format!("第 {} 行: 单个 '=' 无效，条件比较请用 '=='", self.line));
                    }
                }
                '!' => {
                    if self.peek_char(1) == Some('=') {
                        self.push(Token::Ne);
                        self.pos += 2;
                    } else {
                        return Err(format!("第 {} 行: 单个 '!' 无效，不等于请用 '!='", self.line));
                    }
                }
                '#' => self.scan_hex_color()?,
                '-' | '0'..='9' => self.scan_number()?,
                c if c.is_alphabetic() || c == '_' => self.scan_ident(),
                other => return Err(format!("第 {} 行: 无法识别的字符: '{}'", self.line, other)),
            }
        }

        self.push(Token::Eof);
        Ok(())
    }

    fn peek_char(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn scan_hex_color(&mut self) -> Result<(), String> {
        self.pos += 1; // 跳过 '#'
        let start = self.pos;
        while self.pos < self.chars.len() && self.chars[self.pos].is_ascii_hexdigit() {
            self.pos += 1;
        }
        let hex: String = self.chars[start..self.pos].iter().collect();
        if hex.is_empty() {
            return Err("'#' 后缺少十六进制颜色值".to_string());
        }
        let value = u32::from_str_radix(&hex, 16)
            .map_err(|_| format!("第 {} 行: 无效颜色值: #{}", self.line, hex))?;
        self.push(Token::HexColor(value));
        Ok(())
    }

    fn scan_number(&mut self) -> Result<(), String> {
        let start = self.pos;
        if self.chars[self.pos] == '-' {
            self.pos += 1;
        }
        while self.pos < self.chars.len() && self.chars[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        let num_str: String = self.chars[start..self.pos].iter().collect();
        let value = num_str.parse::<i32>()
            .map_err(|_| format!("第 {} 行: 无效数字: {}", self.line, num_str))?;
        self.push(Token::Number(value));
        Ok(())
    }

    fn scan_ident(&mut self) {
        let start = self.pos;
        while self.pos < self.chars.len() {
            let c = self.chars[self.pos];
            if c.is_alphanumeric() || c == '_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let ident: String = self.chars[start..self.pos].iter().collect();
        self.push(Token::Ident(ident));
    }

    // ===== 供 parser 使用的游标接口 =====

    pub fn next(&mut self) -> Token {
        let tok = self.tokens.get(self.cursor).cloned().unwrap_or(Token::Eof);
        if self.cursor < self.tokens.len() {
            self.cursor += 1;
        }
        tok
    }

    pub fn peek(&self) -> Token {
        self.tokens.get(self.cursor).cloned().unwrap_or(Token::Eof)
    }

    /// 当前游标处 token 的行号（用于报错）
    pub fn current_line(&self) -> u32 {
        self.token_lines
            .get(self.cursor)
            .copied()
            .or_else(|| self.token_lines.last().copied())
            .unwrap_or(1)
    }
}
