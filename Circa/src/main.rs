mod ast;
mod env;
mod interpreter;
mod lexer;
mod parser;
mod value;

use std::env as std_env;
use std::fs;

fn main() {
    let args: Vec<String> = std_env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: circa <file.ca>");
        std::process::exit(1);
    }

    let filename = &args[1];
    let src = fs::read_to_string(filename).unwrap_or_else(|e| {
        eprintln!("Error reading {}: {}", filename, e);
        std::process::exit(1);
    });

    let tokens = match parser::lex(&src) {
        Ok(t) => t,
        Err(errs) => {
            for e in errs {
                eprintln!("Lex error: {:?}", e);
            }
            std::process::exit(1);
        }
    };

    let program = match parser::parse(tokens) {
        Ok(p) => p,
        Err(errs) => {
            for e in errs {
                eprintln!("Parse error: {:?}", e);
            }
            std::process::exit(1);
        }
    };

    let mut interp = interpreter::Interpreter::new();
    if let Err(e) = interp.run(&program) {
        eprintln!("Runtime error: {}", e);
        std::process::exit(1);
    }
}
