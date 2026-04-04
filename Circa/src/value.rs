use std::fmt;
use std::rc::Rc;
use crate::ast::{Stmt, TypeAnno};


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
    /// An integer (no tolerance).
    Integer(i64),
    /// A string.
    String(String),
    Bool(bool),
    /// A user-defined function (captures typed params + body).
    Func {
        name: Rc<str>,
        params: Rc<[(String, TypeAnno)]>,
        body: Rc<[Stmt]>,
        tol_param: Option<Rc<str>>,
        return_type: TypeAnno,
    },
    /// A native (Rust-implemented) function.
    NativeFunc {
        name: &'static str,
        arity: usize,
        func: NativeFn,
        guarantees_tol: bool,
        ty: TypeAnno,
    },

    /// A struct definition (blueprint): holds typed field names and methods.
    StructDef {
        name: Rc<str>,
        fields: Rc<[(String, TypeAnno)]>,
        methods: Rc<[(String, Value)]>,
    },
    /// A struct instance: holds the struct name and field values.
    StructInstance {
        struct_name: Rc<str>,
        fields: Vec<(String, Value)>,
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

    pub fn runtime_type(&self) -> TypeAnno {
        match self {
            Value::Number { .. } => TypeAnno::Float,
            Value::Integer(_) => TypeAnno::Int,
            Value::String(_) => TypeAnno::Str,
            Value::Bool(_) => TypeAnno::Bool,
            Value::Func { params, return_type, .. } => TypeAnno::Fn {
                params: params.iter().map(|(_, ty)| ty.clone()).collect(),
                ret: Box::new(return_type.clone()),
            },
            Value::NativeFunc { ty, .. } => ty.clone(),
            Value::StructDef { name, .. } => TypeAnno::Named(name.to_string()),
            Value::StructInstance { struct_name, .. } => TypeAnno::Named(struct_name.to_string()),
            Value::Vector(values) => {
                let inner = values.first()
                    .map(|v| v.runtime_type())
                    .unwrap_or(TypeAnno::None);
                TypeAnno::Vec(Box::new(inner))
            }
            Value::None => TypeAnno::None,
        }
    }

    /// Exact equality: values must be identical.
    /// Treat `~ 0.0` as equivalent to no tolerance.
    pub fn exact_eq(&self, other: &Value) -> Option<bool> {
        match (self, other) {
            (Value::Number { val: a, tol: tol_a }, Value::Number { val: b, tol: tol_b }) => {
                let ta = tol_a.unwrap_or(0.0);
                let tb = tol_b.unwrap_or(0.0);
                if ta != 0.0 || tb != 0.0 {
                    return Some(false);
                }
                Some(a == b)
            }
            (Value::Integer(a), Value::Integer(b)) => Some(a == b),
            (Value::String(a), Value::String(b)) => Some(a == b),
            (Value::Bool(a), Value::Bool(b)) => Some(a == b),
            (Value::None, Value::None) => Some(true),
            // int/float cross-comparison
            (Value::Integer(i), Value::Number { val, tol }) |
            (Value::Number { val, tol }, Value::Integer(i)) => {
                if tol.unwrap_or(0.0) != 0.0 { return Some(false); }
                Some(*val == *i as f64)
            }
            _ => None,
        }
    }

    /// Possible equality: do the tolerance ranges overlap?
    /// |a - b| <= tol_a + tol_b
    pub fn maybe_eq(&self, other: &Value) -> Option<bool> {
        match (self, other) {
            (Value::Number { val: a, tol: tol_a }, Value::Number { val: b, tol: tol_b }) => {
                let ta = tol_a.unwrap_or(0.0);
                let tb = tol_b.unwrap_or(0.0);
                Some((a - b).abs() <= ta + tb)
            }
            (Value::Integer(a), Value::Integer(b)) => Some(a == b),
            (Value::String(a), Value::String(b)) => Some(a == b),
            (Value::Bool(a), Value::Bool(b)) => Some(a == b),
            (Value::None, Value::None) => Some(true),
            // int/float cross-comparison
            (Value::Integer(i), Value::Number { val, tol }) |
            (Value::Number { val, tol }, Value::Integer(i)) => {
                let tb = tol.unwrap_or(0.0);
                Some((*val - *i as f64).abs() <= tb)
            }
            _ => None,
        }
    }

    /// Extract value and tolerance as a pair.
    pub fn as_f64_tol(&self) -> Option<(f64, f64)> {
        match self {
            Value::Number { val, tol } => Some((*val, tol.unwrap_or(0.0))),
            Value::Integer(i) => Some((*i as f64, 0.0)),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number { val, .. } => Some(*val),
            Value::Integer(i) => Some(*i as f64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::Number { val, .. } => Some(*val != 0.0),
            Value::Integer(i) => Some(*i != 0),
            Value::String(s) => Some(!s.is_empty()),
            Value::None => Some(false),
            _ => None,
        }
    }
}

pub fn types_compatible(declared: &TypeAnno, actual: &TypeAnno) -> bool {
    // Int is compatible with Float (promotion)
    if matches!(declared, TypeAnno::Float) && matches!(actual, TypeAnno::Int) {
        return true;
    }

    match (declared, actual) {
        (TypeAnno::None, TypeAnno::None) => true,
        (TypeAnno::AnyVec, TypeAnno::Vec(_)) => true,
        (TypeAnno::Optional(_inner), TypeAnno::None) => true,
        (TypeAnno::Optional(inner), TypeAnno::Optional(other)) => types_compatible(inner, other),
        (TypeAnno::Optional(inner), other) => types_compatible(inner, other),

        (TypeAnno::Int, TypeAnno::Int) => true,
        (TypeAnno::Float, TypeAnno::Float) => true,
        (TypeAnno::Bool, TypeAnno::Bool) => true,
        (TypeAnno::Str, TypeAnno::Str) => true,
        (TypeAnno::Named(a), TypeAnno::Named(b)) => a == b,
        (TypeAnno::Vec(a), TypeAnno::Vec(b)) => {
            matches!(**b, TypeAnno::None) || types_compatible(a, b)
        }
        (TypeAnno::Fn { params: pa, ret: ra }, TypeAnno::Fn { params: pb, ret: rb }) => {
            pa.len() == pb.len()
                && pa.iter().zip(pb.iter()).all(|(a, b)| types_compatible(a, b))
                && types_compatible(ra, rb)
        }
        _ => false,
    }
}

pub fn value_matches_type(value: &Value, expected: &TypeAnno) -> bool {
    match expected {
        TypeAnno::None => matches!(value, Value::None),
        TypeAnno::AnyVec => matches!(value, Value::Vector(_)),
        TypeAnno::Optional(inner) => matches!(value, Value::None) || value_matches_type(value, inner),
        TypeAnno::Int => matches!(value, Value::Integer(_)),
        TypeAnno::Float => matches!(value, Value::Number { .. } | Value::Integer(_)),
        TypeAnno::Bool => matches!(value, Value::Bool(_)),
        TypeAnno::Str => matches!(value, Value::String(_)),
        TypeAnno::Named(name) => {
            matches!(value,
                Value::StructInstance { struct_name, .. } if struct_name.as_ref() == name
            ) || matches!(value,
                Value::StructDef { name: struct_name, .. } if struct_name.as_ref() == name
            )
        }
        TypeAnno::Vec(inner) => match value {
            Value::Vector(values) => values.iter().all(|v| value_matches_type(v, inner)),
            _ => false,
        },
        TypeAnno::Fn { params, ret } => {
            let actual = value.runtime_type();
            types_compatible(
                &TypeAnno::Fn {
                    params: params.clone(),
                    ret: ret.clone(),
                },
                &actual,
            )
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number { val, tol: Some(t) } => write!(f, "{} ~ {}", val, t),
            Value::Number { val, tol: None } => write!(f, "{}", val),
            Value::Integer(i) => write!(f, "{}", i),
            Value::String(s) => write!(f, "{}", s),
            Value::Bool(b) => write!(f, "{}", if *b { "True" } else { "False" }),
            Value::Func { name, .. } => write!(f, "<fn {}>", name),
            Value::NativeFunc { name, .. } => write!(f, "<native fn {}>", name),
            Value::StructDef { name, .. } => write!(f, "<struct {}>", name),
            Value::StructInstance { struct_name, fields } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(k, v)| format!("{} = {}", k, v))
                    .collect();
                write!(f, "{} {{ {} }}", struct_name, parts.join(", "))
            },
            Value::Vector(elems) => {
                let parts: Vec<String> = elems.iter().map(|v| v.to_string()).collect();
                write!(f, "[{}]", parts.join(", "))
            },
            Value::None => write!(f, "None"),
        }
    }
}
