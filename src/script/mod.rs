// 脚本引擎模块
pub mod token;
pub mod parser;
pub mod ast;
pub mod executor;
pub mod loader;

pub use token::{Token, Tokenizer};
pub use parser::Parser;
pub use ast::{
    Command, MouseButton, Coord, FindArea, Value, CompareOp, BoolExpr,
};
pub use executor::ScriptExecutor;
pub use loader::{ScriptFile, load_dir};

#[cfg(test)]
mod tests;
