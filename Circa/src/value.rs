use std::fmt;
use crate::ast::Stmt;
use std::rc::Rc;
/// Signature for a native (Rust-implemented) Circa function.
/// The second argument is the caller-provided tolerance, if any.
pub type NativeFn = fn(&[Value], Option<f64>) -> Result<Value, String>;

/// Runtime value in Circa.
#[derive(Debug, Clone)]
pub enum Value {
    /// A number with an optional tolerance.
    Number {
        val: f64,
        tol: Option<f64>,
    },
    Bool(bool),
    /// A user-defined function (captures param names + body).
    Func {
        name: Rc<str>,
        params: Rc<[String]>,
        body: Rc<[Stmt]>,
        tol_param: Option<Rc<str>>,
    },
    /// A native (Rust-implemented) function.
    NativeFunc {
        name: &'static str,
        arity: usize,
        func: NativeFn,
        guarantees_tol: bool,
    },
    /// A vector of values.
    Vector(Vec<Value>),
    /// None value — represents absence of a value (used for optional tolerance).
    None,
}

impl Value {
    pub fn number(val: f64) -> Self {
        Value::Number { val, tol: None }
    }

    pub fn number_with_tol(val: f64, tol: f64) -> Self {
        Value::Number { val, tol: Some(tol) }
    }

    /// Approximate equality: |a - b| <= tolerance.
    /// Uses the tolerance from `other` if it has one (the RHS in `==`),
    /// otherwise falls back to `self`'s tolerance, otherwise exact.
    pub fn approx_eq(&self, other: &Value) -> Option<bool> {
        match (self, other) {
            (Value::Number { val: a, tol: tol_a }, Value::Number { val: b, tol: tol_b }) => {
                let tolerance = tol_b.or(*tol_a).unwrap_or(0.0);
                Some((a - b).abs() <= tolerance)
            }
            (Value::Bool(a), Value::Bool(b)) => Some(a == b),
            (Value::None, Value::None) => Some(true),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number { val, .. } => Some(*val),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            // Truthy: non-zero numbers
            Value::Number { val, .. } => Some(*val != 0.0),
            Value::None => Some(false),
            _ => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number { val, tol: Some(t) } => write!(f, "{} ~ {}", val, t),
            Value::Number { val, tol: None } => write!(f, "{}", val),
            Value::Bool(b) => write!(f, "{}", if *b { "True" } else { "False" }),
            Value::Func { name, .. } => write!(f, "<fn {}>", name),
            Value::NativeFunc { name, .. } => write!(f, "<native fn {}>", name),
            Value::Vector(elems) => {
                let parts: Vec<String> = elems.iter().map(|v| v.to_string()).collect();
                write!(f, "[{}]", parts.join(", "))
            },
            Value::None => write!(f, "None"),
        }
    }
}