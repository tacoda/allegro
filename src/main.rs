// openai is unused until phase 4 (AI primitives); keep it compiling meanwhile.
#![allow(dead_code, unused_imports)]

mod ast;
mod interp;
mod lexer;
mod natives;
mod openai;
mod parser;
mod patterns;
mod prims;
mod scheduler;
mod token;
mod value;

use std::process::exit;

use interp::Interp;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = match args.get(1).map(|s| s.as_str()) {
        Some("run") => args.get(2),
        Some(p) if !p.starts_with('-') => args.get(1),
        _ => None,
    };
    let Some(path) = path else {
        eprintln!("usage: allegro run <file.al>");
        exit(2);
    };

    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {}: {}", path, e);
            exit(1);
        }
    };

    if let Err(e) = execute(&src) {
        eprintln!("error: {}", e);
        exit(1);
    }
}

fn execute(src: &str) -> Result<(), String> {
    let toks = lexer::lex(src)?;
    let program = parser::parse(toks)?;
    let mut interp = Interp::new();
    interp.run(&program)
}
