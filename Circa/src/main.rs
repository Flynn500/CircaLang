mod ast;
mod builtins;
mod env;
mod errors;
mod interpreter;
mod lexer;
mod parser;
mod resolver;
mod value;
mod optimize;
mod typecheck;

use std::env as std_env;
use std::fs;
use std::path::Path;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std_env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: circa <file.ca>");
        std::process::exit(1);
    }

    let filename = &args[1];
    let src = fs::read_to_string(filename).unwrap_or_else(|e| {
        errors::report_runtime_error(&format!("cannot read {}: {}", filename, e));
        std::process::exit(1);
    });

    let tokens = match parser::lex(&src) {
        Ok(t) => t,
        Err(errs) => {
            for e in &errs {
                errors::report_lex_error(filename, &src, e);
            }
            std::process::exit(1);
        }
    };

    let mut program = match parser::parse(tokens) {
        Ok(p) => p,
        Err(errs) => {
            for e in &errs {
                errors::report_parse_error(filename, &src, e);
            }
            std::process::exit(1);
        }
    };

    // Always import prelude first
    program.insert(0, ast::Stmt::Import { name: "prelude".into() });

    // Resolve all imports (prelude + user imports + transitive imports)
    let base_dir = Path::new(filename)
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();

    let resolved = match resolver::resolve(program, &base_dir) {
        Ok(r) => r,
        Err(e) => {
            errors::report_runtime_error(&e);
            std::process::exit(1);
        }
    };

    let full_program = optimize::optimize(resolved.program);

    // Type check
    let type_errors = typecheck::typecheck(&full_program);
    if !type_errors.is_empty() {
        for e in &type_errors {
            errors::report_runtime_error(e);
        }
        std::process::exit(1);
    }

    // Existing tree-walking path
    let mut interp = interpreter::Interpreter::new();
    for module in &resolved.imported_modules {
        builtins::register_module_builtins(&mut interp.env, module);
    }
    let start = Instant::now();
    if let Err(e) = interp.run(&full_program) {
        errors::report_runtime_error_with_stack(&e, &interp.call_stack);
        std::process::exit(1);
    }
    let duration = start.elapsed();
    println!("Execution time: {:?}", duration);

}