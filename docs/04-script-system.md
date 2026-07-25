# 脚本系统设计

## .ag 脚本语言

### 语法概览

```
// 单行注释

// 键盘命令
down(1)                     // 按下键盘1
up(1)                       // 弹起键盘1
click(2)                    // 点击键盘2（按下+立即弹起）
click_ms(2, 50)             // 点击键盘2（按下+延迟50ms+弹起）

// 鼠标命令（绝对坐标）
mouse_down(left, 100, 200)
mouse_up(left)
mouse_click(right, 100, 200)

// 鼠标命令（中心偏移）
mouse_down_center(left, 50, -30)
mouse_click_center(left, 0, 100)

// 鼠标命令（百分比坐标）
mouse_down_percent(left, 50, 60)
mouse_click_percent(right, 25, 75)

// 延迟
delay_ms(500)

// 条件判断
if_start[find_color(100, 200, 10, 20, #ff00ff) == true]
    click(1)
    delay_ms(100)
else_if[find_color(200, 300, 20, 30, #00ff00) == true]
    click(2)
if_end
```

---

## 解析器设计

**位置**: `src/script/parser.rs`

### 解析流程

```
源代码文本
    ↓
词法分析（Tokenizer）
    ↓
Token 流
    ↓
语法分析（Parser）
    ↓
AST（Script 结构）
```

### Token 定义

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    // 标识符和字面量
    Identifier(String),     // down, click, if_start
    Number(i32),            // 100, -50
    HexColor(u32),          // #ff00ff
    
    // 符号
    LeftParen,              // (
    RightParen,             // )
    LeftBracket,            // [
    RightBracket,           // ]
    Comma,                  // ,
    
    // 运算符
    Equals,                 // ==
    NotEquals,              // !=
    
    // 关键字
    True,
    False,
    
    // 特殊
    Newline,
    Comment(String),
    Eof,
}
```

### Tokenizer 实现

```rust
pub struct Tokenizer<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Tokenizer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }
    
    pub fn next_token(&mut self) -> Result<Token, String> {
        self.skip_whitespace();
        
        if self.pos >= self.input.len() {
            return Ok(Token::Eof);
        }
        
        let ch = self.current_char();
        
        match ch {
            '/' if self.peek_char() == '/' => self.read_comment(),
            '#' => self.read_hex_color(),
            '(' => { self.advance(); Ok(Token::LeftParen) }
            ')' => { self.advance(); Ok(Token::RightParen) }
            '[' => { self.advance(); Ok(Token::LeftBracket) }
            ']' => { self.advance(); Ok(Token::RightBracket) }
            ',' => { self.advance(); Ok(Token::Comma) }
            '=' if self.peek_char() == '=' => {
                self.advance();
                self.advance();
                Ok(Token::Equals)
            }
            '!' if self.peek_char() == '=' => {
                self.advance();
                self.advance();
                Ok(Token::NotEquals)
            }
            '0'..='9' | '-' => self.read_number(),
            'a'..='z' | 'A'..='Z' | '_' => self.read_identifier(),
            _ => Err(format!("未知字符: {}", ch)),
        }
    }
    
    fn read_identifier(&mut self) -> Result<Token, String> {
        let start = self.pos;
        while self.pos < self.input.len() {
            let ch = self.current_char();
            if ch.is_alphanumeric() || ch == '_' {
                self.advance();
            } else {
                break;
            }
        }
        
        let ident = &self.input[start..self.pos];
        match ident {
            "true" => Ok(Token::True),
            "false" => Ok(Token::False),
            _ => Ok(Token::Identifier(ident.to_string())),
        }
    }
    
    fn read_number(&mut self) -> Result<Token, String> {
        let start = self.pos;
        if self.current_char() == '-' {
            self.advance();
        }
        while self.pos < self.input.len() && self.current_char().is_ascii_digit() {
            self.advance();
        }
        
        let num_str = &self.input[start..self.pos];
        num_str.parse::<i32>()
            .map(Token::Number)
            .map_err(|_| format!("无效数字: {}", num_str))
    }
    
    fn read_hex_color(&mut self) -> Result<Token, String> {
        self.advance(); // 跳过 '#'
        let start = self.pos;
        for _ in 0..6 {
            if self.pos >= self.input.len() || !self.current_char().is_ascii_hexdigit() {
                return Err("无效颜色值".to_string());
            }
            self.advance();
        }
        
        let hex_str = &self.input[start..self.pos];
        u32::from_str_radix(hex_str, 16)
            .map(Token::HexColor)
            .map_err(|_| "颜色解析失败".to_string())
    }
    
    // ... 辅助方法
}
```

### Parser 实现

```rust
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }
    
    pub fn parse(&mut self) -> Result<Script, String> {
        let mut statements = Vec::new();
        
        while !self.is_at_end() {
            if self.match_token(&Token::Newline) {
                continue;
            }
            statements.push(self.parse_statement()?);
        }
        
        Ok(Script { statements })
    }
    
    fn parse_statement(&mut self) -> Result<Statement, String> {
        match self.current_token() {
            Token::Comment(text) => {
                let comment = Statement::Comment(text.clone());
                self.advance();
                Ok(comment)
            }
            Token::Identifier(name) if name == "if_start" => {
                self.parse_if_block()
            }
            Token::Identifier(_) => {
                self.parse_command()
            }
            _ => Err(format!("意外的 token: {:?}", self.current_token())),
        }
    }
    
    fn parse_command(&mut self) -> Result<Statement, String> {
        let name = match self.current_token() {
            Token::Identifier(n) => n.clone(),
            _ => return Err("期望命令名".to_string()),
        };
        self.advance();
        
        self.expect(Token::LeftParen)?;
        
        let cmd = match name.as_str() {
            "down" => {
                let key = self.parse_key()?;
                Command::Down(key)
            }
            "up" => {
                let key = self.parse_key()?;
                Command::Up(key)
            }
            "click" => {
                let key = self.parse_key()?;
                Command::Click(key)
            }
            "click_ms" => {
                let key = self.parse_key()?;
                self.expect(Token::Comma)?;
                let delay = self.parse_number()?;
                Command::ClickMs(key, delay as u32)
            }
            "delay_ms" => {
                let ms = self.parse_number()?;
                Command::DelayMs(ms as u32)
            }
            "mouse_click" => {
                let button = self.parse_mouse_button()?;
                self.expect(Token::Comma)?;
                let x = self.parse_number()?;
                self.expect(Token::Comma)?;
                let y = self.parse_number()?;
                Command::MouseClick { button, x, y }
            }
            // ... 其他命令
            _ => return Err(format!("未知命令: {}", name)),
        };
        
        self.expect(Token::RightParen)?;
        Ok(Statement::Command(cmd))
    }
    
    fn parse_if_block(&mut self) -> Result<Statement, String> {
        let mut branches = Vec::new();
        
        // if_start[condition]
        self.expect_identifier("if_start")?;
        self.expect(Token::LeftBracket)?;
        let condition = self.parse_expression()?;
        self.expect(Token::RightBracket)?;
        
        let mut body = Vec::new();
        loop {
            if self.check_identifier("else_if") || self.check_identifier("if_end") {
                break;
            }
            body.push(self.parse_statement()?);
        }
        
        branches.push(Branch {
            condition: Some(condition),
            body,
        });
        
        // else_if 分支
        while self.match_identifier("else_if") {
            self.expect(Token::LeftBracket)?;
            let condition = self.parse_expression()?;
            self.expect(Token::RightBracket)?;
            
            let mut body = Vec::new();
            loop {
                if self.check_identifier("else_if") || self.check_identifier("if_end") {
                    break;
                }
                body.push(self.parse_statement()?);
            }
            
            branches.push(Branch {
                condition: Some(condition),
                body,
            });
        }
        
        self.expect_identifier("if_end")?;
        
        Ok(Statement::If(IfBlock { branches }))
    }
    
    fn parse_expression(&mut self) -> Result<Expression, String> {
        let left = self.parse_primary_expression()?;
        
        // 检查二元运算符
        if self.match_token(&Token::Equals) {
            let right = self.parse_primary_expression()?;
            Ok(Expression::Equals(Box::new(left), Box::new(right)))
        } else if self.match_token(&Token::NotEquals) {
            let right = self.parse_primary_expression()?;
            Ok(Expression::NotEquals(Box::new(left), Box::new(right)))
        } else {
            Ok(left)
        }
    }
    
    fn parse_primary_expression(&mut self) -> Result<Expression, String> {
        match self.current_token() {
            Token::True => {
                self.advance();
                Ok(Expression::Bool(true))
            }
            Token::False => {
                self.advance();
                Ok(Expression::Bool(false))
            }
            Token::Identifier(name) if name == "find_color" => {
                self.parse_find_color()
            }
            // ... 其他表达式
            _ => Err(format!("意外的表达式 token: {:?}", self.current_token())),
        }
    }
    
    fn parse_find_color(&mut self) -> Result<Expression, String> {
        self.advance(); // 跳过 find_color
        self.expect(Token::LeftParen)?;
        
        let x = self.parse_number()?;
        self.expect(Token::Comma)?;
        let y = self.parse_number()?;
        self.expect(Token::Comma)?;
        let w = self.parse_number()? as u32;
        self.expect(Token::Comma)?;
        let h = self.parse_number()? as u32;
        self.expect(Token::Comma)?;
        let color = self.parse_color()?;
        
        self.expect(Token::RightParen)?;
        
        Ok(Expression::FindColor { x, y, w, h, color })
    }
    
    // ... 辅助方法
}
```

---

## 脚本执行器

**位置**: `src/script/executor.rs`

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::Foundation::HWND;

pub struct ScriptExecutor {
    input_backend: Arc<dyn InputBackend>,
    capture_backend: Option<Arc<dyn CaptureBackend>>,  // 阶段5添加
}

impl ScriptExecutor {
    pub fn new(input_backend: Arc<dyn InputBackend>) -> Self {
        Self {
            input_backend,
            capture_backend: None,
        }
    }
    
    /// 执行脚本（循环直到 stop_flag 为 true）
    pub fn execute_loop(
        &self,
        script: &Script,
        hwnd: HWND,
        stop_flag: &Arc<AtomicBool>,
    ) {
        while !stop_flag.load(Ordering::Relaxed) {
            for statement in &script.statements {
                if stop_flag.load(Ordering::Relaxed) {
                    return;
                }
                
                if let Err(e) = self.execute_statement(statement, hwnd, stop_flag) {
                    log::error!("执行语句失败: {}", e);
                }
            }
        }
    }
    
    fn execute_statement(
        &self,
        stmt: &Statement,
        hwnd: HWND,
        stop_flag: &Arc<AtomicBool>,
    ) -> Result<(), String> {
        match stmt {
            Statement::Command(cmd) => self.execute_command(cmd, hwnd),
            Statement::If(if_block) => self.execute_if(if_block, hwnd, stop_flag),
            Statement::Comment(_) => Ok(()),
        }
    }
    
    fn execute_command(&self, cmd: &Command, hwnd: HWND) -> Result<(), String> {
        match cmd {
            Command::Down(key) => {
                self.input_backend.send_key_down(hwnd, *key)?;
            }
            Command::Up(key) => {
                self.input_backend.send_key_up(hwnd, *key)?;
            }
            Command::Click(key) => {
                self.input_backend.send_key_down(hwnd, *key)?;
                self.input_backend.send_key_up(hwnd, *key)?;
            }
            Command::ClickMs(key, delay) => {
                self.input_backend.send_key_down(hwnd, *key)?;
                std::thread::sleep(std::time::Duration::from_millis(*delay as u64));
                self.input_backend.send_key_up(hwnd, *key)?;
            }
            Command::DelayMs(ms) => {
                // 分段 sleep，每 10ms 检查一次 stop_flag
                self.interruptible_sleep(*ms);
            }
            Command::MouseClick { button, x, y } => {
                self.input_backend.send_mouse_down(hwnd, *button, *x, *y)?;
                self.input_backend.send_mouse_up(hwnd, *button)?;
            }
            // ... 其他命令
        }
        Ok(())
    }
    
    fn execute_if(
        &self,
        if_block: &IfBlock,
        hwnd: HWND,
        stop_flag: &Arc<AtomicBool>,
    ) -> Result<(), String> {
        for branch in &if_block.branches {
            let should_execute = match &branch.condition {
                Some(expr) => self.eval_expression(expr, hwnd)?,
                None => true,  // else 分支
            };
            
            if should_execute {
                for stmt in &branch.body {
                    self.execute_statement(stmt, hwnd, stop_flag)?;
                }
                break;  // 只执行第一个匹配的分支
            }
        }
        Ok(())
    }
    
    fn eval_expression(&self, expr: &Expression, hwnd: HWND) -> Result<bool, String> {
        match expr {
            Expression::Bool(b) => Ok(*b),
            Expression::Equals(left, right) => {
                let l = self.eval_expression(left, hwnd)?;
                let r = self.eval_expression(right, hwnd)?;
                Ok(l == r)
            }
            Expression::NotEquals(left, right) => {
                let l = self.eval_expression(left, hwnd)?;
                let r = self.eval_expression(right, hwnd)?;
                Ok(l != r)
            }
            Expression::FindColor { .. } => {
                // 阶段5实现
                log::warn!("find_color 暂未实现");
                Ok(false)
            }
            // ... 其他表达式
        }
    }
    
    /// 可中断的 sleep
    fn interruptible_sleep(&self, ms: u32) {
        let chunks = ms / 10;
        let remainder = ms % 10;
        
        for _ in 0..chunks {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        
        if remainder > 0 {
            std::thread::sleep(std::time::Duration::from_millis(remainder as u64));
        }
    }
}
```

---

## 脚本加载器

**位置**: `src/script/loader.rs`

```rust
use std::fs;
use std::path::{Path, PathBuf};

pub struct ScriptLoader {
    scripts_dir: PathBuf,
}

impl ScriptLoader {
    pub fn new(scripts_dir: impl AsRef<Path>) -> Self {
        Self {
            scripts_dir: scripts_dir.as_ref().to_path_buf(),
        }
    }
    
    /// 扫描目录，加载所有 .ag 文件
    pub fn load_all(&self) -> Result<Vec<Scheme>, String> {
        let mut schemes = Vec::new();
        
        let entries = fs::read_dir(&self.scripts_dir)
            .map_err(|e| format!("无法读取脚本目录: {}", e))?;
        
        for entry in entries {
            let entry = entry.map_err(|e| format!("读取目录项失败: {}", e))?;
            let path = entry.path();
            
            if path.extension().and_then(|s| s.to_str()) == Some("ag") {
                match self.load_scheme(&path) {
                    Ok(scheme) => schemes.push(scheme),
                    Err(e) => log::warn!("加载脚本失败 {:?}: {}", path, e),
                }
            }
        }
        
        Ok(schemes)
    }
    
    fn load_scheme(&self, path: &Path) -> Result<Scheme, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("读取文件失败: {}", e))?;
        
        let id = path.file_name()
            .and_then(|s| s.to_str())
            .ok_or("无效文件名")?
            .to_string();
        
        let display_name = path.file_stem()
            .and_then(|s| s.to_str())
            .ok_or("无效文件名")?
            .to_string();
        
        // 解析脚本（懒加载可以在这里跳过）
        let script = Parser::parse_from_string(&content)?;
        
        Ok(Scheme {
            id,
            display_name,
            file_path: path.to_path_buf(),
            script: Some(script),
        })
    }
}
```

---

## 示例脚本

### 示例1: 循环按键

```
// 自动采集循环
click(e)              // 按 E 键采集
delay_ms(2000)        // 等待 2 秒
click(w)              // 向前移动
delay_ms(500)
click(space)          // 跳跃
delay_ms(1000)
```

### 示例2: 条件判断

```
// 根据颜色执行不同操作
if_start[find_color(100, 100, 50, 50, #ff0000) == true]
    click(1)          // 发现红色 -> 按 1
    delay_ms(500)
else_if[find_color(100, 100, 50, 50, #00ff00) == true]
    click(2)          // 发现绿色 -> 按 2
    delay_ms(500)
if_end

delay_ms(1000)
```

### 示例3: 鼠标操作

```
// 点击屏幕中心
mouse_click_center(left, 0, 0)
delay_ms(100)

// 点击指定百分比位置
mouse_click_percent(left, 50, 30)
delay_ms(500)
```
