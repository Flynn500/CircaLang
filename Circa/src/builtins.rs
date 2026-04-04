use crate::ast::TypeAnno;
use crate::env::Env;
use crate::value::{NativeFn, Value};

#[derive(Clone, Copy)]
pub struct BuiltinSpec {
    pub name: &'static str,
    pub arity: usize,
    pub func: NativeFn,
    pub guarantees_tol: bool,
    pub ty: fn() -> TypeAnno,
}

impl BuiltinSpec {
    pub fn type_anno(&self) -> TypeAnno {
        (self.ty)()
    }
}

fn ty_tolerance() -> TypeAnno {
    TypeAnno::Fn {
        params: vec![TypeAnno::Float],
        ret: Box::new(TypeAnno::Float),
    }
}

fn ty_panic() -> TypeAnno {
    TypeAnno::Fn {
        params: vec![TypeAnno::Str],
        ret: Box::new(TypeAnno::None),
    }
}

fn ty_print() -> TypeAnno {
    TypeAnno::Fn {
        params: vec![TypeAnno::Str],
        ret: Box::new(TypeAnno::None),
    }
}

fn ty_snap() -> TypeAnno {
    TypeAnno::Fn {
        params: vec![TypeAnno::Float],
        ret: Box::new(TypeAnno::Float),
    }
}

fn ty_len() -> TypeAnno {
    TypeAnno::Fn {
        params: vec![TypeAnno::AnyVec],
        ret: Box::new(TypeAnno::Int),
    }
}

const PRELUDE_BUILTINS: &[BuiltinSpec] = &[
    BuiltinSpec { name: "tolerance", arity: 1, func: builtin_tolerance, guarantees_tol: false, ty: ty_tolerance },
    BuiltinSpec { name: "panic",     arity: 1, func: builtin_panic,     guarantees_tol: false, ty: ty_panic },
    BuiltinSpec { name: "print",     arity: 1, func: builtin_print,     guarantees_tol: false, ty: ty_print },
    BuiltinSpec { name: "snap",      arity: 1, func: builtin_snap,      guarantees_tol: false, ty: ty_snap },
    BuiltinSpec { name: "len",       arity: 1, func: builtin_len,       guarantees_tol: false, ty: ty_len },
];

/// Return the list of native builtins that belong to a given module.
/// Unknown module names return an empty slice (user files have no native builtins).
pub fn builtins_for_module(module: &str) -> &'static [BuiltinSpec] {
    match module {
        "prelude" => PRELUDE_BUILTINS,
        _ => &[],
    }
}

/// Register the native builtins for a specific module into the environment.
pub fn register_module_builtins(env: &mut Env, module: &str) {
    for spec in builtins_for_module(module) {
        env.define_value(
            spec.name.to_string(),
            Value::NativeFunc {
                name: spec.name,
                arity: spec.arity,
                func: spec.func,
                guarantees_tol: spec.guarantees_tol,
                ty: spec.type_anno(),
            },
            spec.type_anno(),
            false,
        );
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
    match &args[0] {
        Value::String(msg) => Err(format!("panic: {}", msg)),
        other => Err(format!("panic: expected a string, got {}", other)),
    }
}

fn builtin_print(args: &[Value], _caller_tol: Option<f64>) -> Result<Value, String> {
    match &args[0] {
        Value::String(msg) => {
            println!("{}", msg);
            Ok(Value::None)
        }
        other => Err(format!("print: expected a string, got {}", other)),
    }
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
        other => Err(format!("len: expected a vector, got {}", other)),
    }
}
