use crate::env::Env;
use crate::value::{NativeFn, Value};

/// Return the list of native builtins that belong to a given module.
/// Unknown module names return an empty slice (user files have no native builtins).
fn builtins_for_module(module: &str) -> &'static [(&'static str, usize, NativeFn, bool)] {
    match module {
        "prelude" => &[
            ("tolerance", 1, builtin_tolerance, false),
            ("panic",     1, builtin_panic,     false),
            ("print",     1, builtin_print,     false),
            ("snap",      1, builtin_snap,      false),
            ("len",       1, builtin_len,       false),
        ],
        // "trig" => &[ ("sin", 1, builtin_sin, false), ... ],
        // "math" => &[ ... ],
        _ => &[],
    }
}

/// Register the native builtins for a specific module into the environment.
pub fn register_module_builtins(env: &mut Env, module: &str) {
    for &(name, arity, func, guarantees_tol) in builtins_for_module(module) {
        env.define(name.to_string(), Value::NativeFunc { name, arity, func, guarantees_tol });
    }
}

fn builtin_tolerance(args: &[Value], _caller_tol: Option<f64>) -> Result<Value, String> {
    match &args[0] {
        Value::Number { tol, .. } => Ok(Value::number(tol.unwrap_or(0.0))),
        Value::Integer(_) => Ok(Value::number(0.0)),
        other => Err(format!("tolerance: expected a number, got {}", other)),
    }
}

fn builtin_panic(args: &[Value], _caller_tol: Option<f64>) -> Result<Value, String> {
    Err(format!("panic: {}", args[0]))
}

fn builtin_print(args: &[Value], _caller_tol: Option<f64>) -> Result<Value, String> {
    println!("{}", args[0]);
    Ok(Value::Bool(false))
}

fn builtin_snap(args: &[Value], _caller_tol: Option<f64>) -> Result<Value, String> {
    match &args[0] {
        Value::Number { val, .. } => Ok(Value::number(*val)),
        Value::Integer(i) => Ok(Value::number(*i as f64)),
        other => Err(format!("snap: expected a number, got {}", other)),
    }
}

fn builtin_len(args: &[Value], _caller_tol: Option<f64>) -> Result<Value, String> {
    match &args[0] {
        Value::Vector(elems) => Ok(Value::Integer(elems.len() as i64)),
        Value::String(s) => Ok(Value::Integer(s.len() as i64)),
        other => Err(format!("len: expected a vector or string, got {}", other)),
    }
}