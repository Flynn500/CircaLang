use crate::env::Env;
use crate::value::{NativeFn, Value};

/// Register all built-in native functions into the given environment.
pub fn register_builtins(env: &mut Env) {
    let builtins: &[(&'static str, usize, NativeFn, bool)] = &[
        ("tolerance", 1, builtin_tolerance, false),
        ("panic",     1, builtin_panic,     false),
        ("print",     1, builtin_print,     false),
        ("snap",      1, builtin_snap,      false),
    ];

    for &(name, arity, func, guarantees_tol) in builtins {
        env.define(name.to_string(), Value::NativeFunc { name, arity, func, guarantees_tol });
    }
}

fn builtin_tolerance(args: &[Value], _caller_tol: Option<f32>) -> Result<Value, String> {
    match &args[0] {
        Value::Number { tol, .. } => Ok(Value::number(tol.unwrap_or(0.0))),
        other => Err(format!("tolerance: expected a number, got {}", other)),
    }
}

fn builtin_panic(args: &[Value], _caller_tol: Option<f32>) -> Result<Value, String> {
    Err(format!("panic: {}", args[0]))
}

fn builtin_print(args: &[Value], _caller_tol: Option<f32>) -> Result<Value, String> {
    println!("{}", args[0]);
    Ok(Value::Bool(false))
}

fn builtin_snap(args: &[Value], _caller_tol: Option<f32>) -> Result<Value, String> {
    match &args[0] {
        Value::Number { val, .. } => Ok(Value::number(*val)),
        other => Err(format!("snap: expected a number, got {}", other)),
    }
}
