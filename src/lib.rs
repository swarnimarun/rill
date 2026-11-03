pub mod ast;
pub mod backend;
pub mod c_import;
pub mod check;
mod display;
pub mod formatter;
pub mod interpreter;
pub mod lexer;
pub mod modules;
pub mod parser;
pub mod source;
pub mod source_map;
pub mod syntax;
pub mod typed;

/// Parse and check a program before it reaches a backend.
pub fn compile(source: &str) -> Result<String, String> {
    let program = parser::parse(source)?;
    backend::emit(&program)
}
