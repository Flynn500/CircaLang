use std::collections::HashSet;

use crate::ast::*;
use crate::builtins;
use crate::value::types_compatible;

#[derive(Debug, Clone)]
struct VarInfo {
    ty: TypeAnno,
    mutable: bool,
}

#[derive(Debug, Clone)]
struct StructInfo {
    name: String,
    fields: Vec<(String, TypeAnno)>,
    methods: Vec<(String, TypeAnno)>,
}



/// A type environment: maps variable names to their declared types and mutability,
/// plus known struct definitions.
struct TypeEnv {
    entries: Vec<(String, VarInfo)>,
    scope_starts: Vec<usize>,
    structs: Vec<StructInfo>,
}

impl TypeEnv {
    fn new() -> Self {
        TypeEnv {
            entries: Vec::new(),
            scope_starts: vec![0],
            structs: Vec::new(),
        }
    }

    fn push_scope(&mut self) {
        self.scope_starts.push(self.entries.len());
    }

    fn pop_scope(&mut self) {
        let start = self.scope_starts.pop().expect("cannot pop global scope");
        self.entries.truncate(start);
    }

    fn define(&mut self, name: String, ty: TypeAnno, mutable: bool) {
        self.entries.push((name, VarInfo { ty, mutable }));
    }

    fn get(&self, name: &str) -> Option<&VarInfo> {
        for (n, info) in self.entries.iter().rev() {
            if n == name {
                return Some(info);
            }
        }
        None
    }

    fn define_struct(&mut self, info: StructInfo) {
        if let Some(existing) = self.structs.iter_mut().find(|s| s.name == info.name) {
            *existing = info;
        } else {
            self.structs.push(info);
        }
    }

    fn struct_exists(&self, name: &str) -> bool {
        self.structs.iter().any(|s| s.name == name)
    }

    fn get_struct(&self, name: &str) -> Option<&StructInfo> {
        self.structs.iter().find(|s| s.name == name)
    }
}

#[derive(Debug, Clone, Copy)]
struct CheckCtx<'a> {
    current_fn: Option<(&'a str, &'a TypeAnno)>,
    loop_depth: usize,
}

impl<'a> CheckCtx<'a> {
    fn top_level() -> Self {
        Self {
            current_fn: None,
            loop_depth: 0,
        }
    }

    fn in_fn(self, name: &'a str, return_type: &'a TypeAnno) -> Self {
        Self {
            current_fn: Some((name, return_type)),
            loop_depth: self.loop_depth,
        }
    }

    fn in_loop(self) -> Self {
        Self {
            current_fn: self.current_fn,
            loop_depth: self.loop_depth + 1,
        }
    }
}

/// Run type checking on a resolved program. Returns a list of errors.
pub fn typecheck(program: &Program, imported_modules: &HashSet<String>) -> Vec<String> {
    let mut env = TypeEnv::new();
    let mut errors = Vec::new();

    for module in imported_modules {
        for spec in builtins::builtins_for_module(module) {
            env.define(spec.name.to_string(), spec.type_anno(), false);
        }
    }

    for stmt in program {
        check_stmt(stmt, &mut env, &mut errors, CheckCtx::top_level());
    }

    errors
}

/// Validate that a TypeAnno refers to known types.
fn validate_type(ty: &TypeAnno, env: &TypeEnv) -> Result<(), String> {
    match ty {
        TypeAnno::Int | TypeAnno::Float | TypeAnno::Bool | TypeAnno::Str | TypeAnno::None | TypeAnno::AnyVec => Ok(()),
        TypeAnno::Optional(inner) => validate_type(inner, env),
        TypeAnno::Named(name) => {
            if env.struct_exists(name) {
                Ok(())
            } else {
                Err(format!("unknown type '{}'", name))
            }
        }
        TypeAnno::Vec(inner) => validate_type(inner, env),
        TypeAnno::Fn { params, ret } => {
            for p in params {
                validate_type(p, env)?;
            }
            validate_type(ret, env)
        }
    }
}



trait AnnotationRef {
    fn as_annotation(&self) -> Option<&TypeAnno>;
}

impl AnnotationRef for TypeAnno {
    fn as_annotation(&self) -> Option<&TypeAnno> {
        Some(self)
    }
}

impl AnnotationRef for Option<TypeAnno> {
    fn as_annotation(&self) -> Option<&TypeAnno> {
        self.as_ref()
    }
}

fn require_annotation<A: AnnotationRef + ?Sized>(
    annotation: &A,
    context: &str,
    env: &TypeEnv,
    errors: &mut Vec<String>,
) -> TypeAnno {
    match annotation.as_annotation() {
        Some(ty) => {
            if let Err(e) = validate_type(ty, env) {
                errors.push(format!("{}: {}", context, e));
            }
            ty.clone()
        }
        None => {
            errors.push(format!("{}: missing type annotation", context));
            TypeAnno::None
        }
    }
}

fn fn_return_type<A: AnnotationRef + ?Sized>(annotation: &A, context: &str, env: &TypeEnv, errors: &mut Vec<String>) -> TypeAnno {
    match annotation.as_annotation() {
        Some(ty) => {
            if let Err(e) = validate_type(ty, env) {
                errors.push(format!("{}: {}", context, e));
            }
            ty.clone()
        }
        None => TypeAnno::None,
    }
}

fn is_numeric(ty: &TypeAnno) -> bool {
    matches!(ty, TypeAnno::Int | TypeAnno::Float)
}

fn is_numeric_like(ty: &TypeAnno) -> bool {
    is_numeric(ty) || is_optional_float(ty)
}

fn is_stringifiable(ty: &TypeAnno) -> bool {
    matches!(
        ty,
        TypeAnno::Str
            | TypeAnno::Int
            | TypeAnno::Float
            | TypeAnno::Bool
            | TypeAnno::None
            | TypeAnno::Optional(_)
            | TypeAnno::Vec(_)
            | TypeAnno::Named(_)
    )
}

fn is_bool_compatible(ty: &TypeAnno) -> bool {
    matches!(ty, TypeAnno::Bool | TypeAnno::Optional(_))
}

fn is_optional_float(ty: &TypeAnno) -> bool {
    matches!(ty, TypeAnno::Optional(inner) if matches!(inner.as_ref(), TypeAnno::Float))
}

fn find_struct_field<'a>(info: &'a StructInfo, field: &str) -> Option<&'a TypeAnno> {
    info.fields.iter().find(|(name, _)| name == field).map(|(_, ty)| ty)
}

fn find_struct_method<'a>(info: &'a StructInfo, method: &str) -> Option<&'a TypeAnno> {
    info.methods.iter().find(|(name, _)| name == method).map(|(_, ty)| ty)
}

fn truthy_narrowing(condition: &Expr, env: &TypeEnv) -> Option<(String, TypeAnno)> {
    match condition {
        Expr::Ident(name) => {
            let info = env.get(name)?;
            match &info.ty {
                TypeAnno::Optional(inner) => Some((name.clone(), (**inner).clone())),
                _ => None,
            }
        }
        _ => None,
    }
}
/// Infer and validate the type of an expression.
fn infer_expr(expr: &Expr, env: &TypeEnv, errors: &mut Vec<String>) -> TypeAnno {
    match expr {
        Expr::Number(_) => TypeAnno::Float,
        Expr::Integer(_) => TypeAnno::Int,
        Expr::StringLiteral(_) => TypeAnno::Str,
        Expr::Bool(_) => TypeAnno::Bool,
        Expr::None => TypeAnno::None,

        Expr::Ident(name) => match env.get(name) {
            Some(info) => info.ty.clone(),
            None => {
                errors.push(format!("undefined variable: {}", name));
                TypeAnno::None
            }
        },

        Expr::WithTolerance { value, tolerance } => {
            let value_ty = infer_expr(value, env, errors);
            let tol_ty = infer_expr(tolerance, env, errors);

            if !is_numeric(&value_ty) && !matches!(value_ty, TypeAnno::None) {
                errors.push(format!(
                    "cannot apply tolerance to value of type {:?}",
                    value_ty
                ));
            }

            if !is_numeric_like(&tol_ty) && !matches!(tol_ty, TypeAnno::None) {
                errors.push(format!(
                    "tolerance must be numeric or None, got {:?}",
                    tol_ty
                ));
            }

            value_ty
        }

        Expr::Unary { expr, .. } => {
            let inner = infer_expr(expr, env, errors);
            if !is_numeric_like(&inner) {
                errors.push(format!("unary '-' expects a numeric value, got {:?}", inner));
            }
            inner
        }

        Expr::BinOp { left, op, right } => {
            let left_ty = infer_expr(left, env, errors);
            let right_ty = infer_expr(right, env, errors);

            match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                    match (&left_ty, &right_ty, op) {
                        _ if matches!(op, BinOp::Add)
                            && (matches!(left_ty, TypeAnno::Str) || matches!(right_ty, TypeAnno::Str)) =>
                        {
                            if !is_stringifiable(&left_ty) {
                                errors.push(format!(
                                    "left operand of + cannot be converted to string, got {:?}",
                                    left_ty
                                ));
                            }
                            if !is_stringifiable(&right_ty) {
                                errors.push(format!(
                                    "right operand of + cannot be converted to string, got {:?}",
                                    right_ty
                                ));
                            }
                            TypeAnno::Str
                        }
                        (TypeAnno::Str, TypeAnno::Int, BinOp::Mul)
                        | (TypeAnno::Int, TypeAnno::Str, BinOp::Mul) => TypeAnno::Str,
                        _ => {
                            if !is_numeric_like(&left_ty) {
                                errors.push(format!(
                                    "left operand of {:?} must be numeric, got {:?}",
                                    op, left_ty
                                ));
                            }
                            if !is_numeric_like(&right_ty) {
                                errors.push(format!(
                                    "right operand of {:?} must be numeric, got {:?}",
                                    op, right_ty
                                ));
                            }

                            if matches!(left_ty, TypeAnno::Float | TypeAnno::Optional(_))
                                || matches!(right_ty, TypeAnno::Float | TypeAnno::Optional(_))
                            {
                                TypeAnno::Float
                            } else if matches!(left_ty, TypeAnno::Int) && matches!(right_ty, TypeAnno::Int) {
                                TypeAnno::Int
                            } else {
                                TypeAnno::None
                            }
                        }
                    }
                }

                BinOp::Eq | BinOp::Neq | BinOp::MaybeEq | BinOp::MaybeNeq => {
                    if !types_compatible(&left_ty, &right_ty) && !types_compatible(&right_ty, &left_ty) {
                        errors.push(format!(
                            "cannot compare values of type {:?} and {:?}",
                            left_ty, right_ty
                        ));
                    }
                    TypeAnno::Bool
                }

                BinOp::Lt | BinOp::Gt | BinOp::Lte | BinOp::Gte
                | BinOp::MaybeLt | BinOp::MaybeGt | BinOp::MaybeLte | BinOp::MaybeGte => {
                    if !is_numeric_like(&left_ty) {
                        errors.push(format!(
                            "left operand of {:?} must be numeric, got {:?}",
                            op, left_ty
                        ));
                    }
                    if !is_numeric_like(&right_ty) {
                        errors.push(format!(
                            "right operand of {:?} must be numeric, got {:?}",
                            op, right_ty
                        ));
                    }
                    TypeAnno::Bool
                }
            }
        }

        Expr::Call { func, args, tolerance } => {
            let func_ty = infer_expr(func, env, errors);

            if let Some(tol) = tolerance {
                let tol_ty = infer_expr(tol, env, errors);
                if !is_numeric_like(&tol_ty) && !matches!(tol_ty, TypeAnno::None) {
                    errors.push(format!(
                        "call tolerance must be numeric or None, got {:?}",
                        tol_ty
                    ));
                }
            }

            let arg_types: Vec<TypeAnno> = args.iter().map(|arg| infer_expr(arg, env, errors)).collect();

            match func_ty {
                TypeAnno::Fn { params, ret } => {
                    if params.len() != arg_types.len() {
                        errors.push(format!(
                            "function expected {} args, got {}",
                            params.len(),
                            arg_types.len()
                        ));
                    }

                    for (idx, (expected, actual)) in params.iter().zip(arg_types.iter()).enumerate() {
                        if !types_compatible(expected, actual) {
                            errors.push(format!(
                                "argument {} type mismatch: expected {:?}, got {:?}",
                                idx + 1,
                                expected,
                                actual
                            ));
                        }
                    }

                    *ret
                }
                other => {
                    errors.push(format!("cannot call value of type {:?}", other));
                    TypeAnno::None
                }
            }
        }

        Expr::Lambda { params, body, tol_param, return_type } => {
            let param_types: Vec<TypeAnno> = params
                .iter()
                .map(|(name, ty)| require_annotation(ty, &format!("lambda parameter '{}'", name), env, errors))
                .collect();
            let ret_ty = fn_return_type(return_type, "lambda return", env, errors);

            let lambda_name = "<lambda>";
            let lambda_fn_ty = TypeAnno::Fn {
                params: param_types.clone(),
                ret: Box::new(ret_ty.clone()),
            };

            let mut body_env = TypeEnv {
                entries: env.entries.clone(),
                scope_starts: env.scope_starts.clone(),
                structs: env.structs.clone(),
            };
            body_env.push_scope();
            for ((name, _), ty) in params.iter().zip(param_types.iter()) {
                body_env.define(name.clone(), ty.clone(), true);
            }
            if let Some(tol_name) = tol_param {
                body_env.define(tol_name.clone(), TypeAnno::Optional(Box::new(TypeAnno::Float)), false);
            }
            for stmt in body {
                check_stmt(stmt, &mut body_env, errors, CheckCtx::top_level().in_fn(lambda_name, &ret_ty));
            }
            body_env.pop_scope();

            lambda_fn_ty
        }

        Expr::VecLiteral(elems) => {
            if elems.is_empty() {
                return TypeAnno::Vec(Box::new(TypeAnno::None));
            }

            let elem_types: Vec<TypeAnno> = elems.iter().map(|e| infer_expr(e, env, errors)).collect();
            let first = elem_types[0].clone();

            for (idx, ty) in elem_types.iter().enumerate().skip(1) {
                if !types_compatible(&first, ty) && !types_compatible(ty, &first) {
                    errors.push(format!(
                        "vector element {} has type {:?}, expected compatible with {:?}",
                        idx + 1,
                        ty,
                        first
                    ));
                }
            }

            let inner = if elem_types.iter().any(|t| matches!(t, TypeAnno::Float)) {
                if elem_types.iter().all(|t| matches!(t, TypeAnno::Int | TypeAnno::Float | TypeAnno::None)) {
                    TypeAnno::Float
                } else {
                    first
                }
            } else {
                first
            };

            TypeAnno::Vec(Box::new(inner))
        }

        Expr::Index { vec, index } => {
            let vec_ty = infer_expr(vec, env, errors);
            let idx_ty = infer_expr(index, env, errors);

            if !matches!(idx_ty, TypeAnno::Int) {
                errors.push(format!("index must be int, got {:?}", idx_ty));
            }

            match vec_ty {
                TypeAnno::Vec(inner) => *inner,
                TypeAnno::Str => TypeAnno::Str,
                other => {
                    errors.push(format!("cannot index into value of type {:?}", other));
                    TypeAnno::None
                }
            }
        }

        Expr::StructInit { name, fields } => {
            let info = match env.get_struct(name) {
                Some(info) => info,
                None => {
                    errors.push(format!("unknown struct '{}'", name));
                    return TypeAnno::None;
                }
            };

            let mut seen = Vec::<&str>::new();
            for (field_name, field_expr) in fields {
                if seen.contains(&field_name.as_str()) {
                    errors.push(format!("struct {} initializer repeats field '{}'", name, field_name));
                }
                seen.push(field_name.as_str());

                let actual = infer_expr(field_expr, env, errors);
                match find_struct_field(info, field_name) {
                    Some(expected) => {
                        if !types_compatible(expected, &actual) {
                            errors.push(format!(
                                "struct {} field '{}' expects {:?}, got {:?}",
                                name, field_name, expected, actual
                            ));
                        }
                    }
                    None => errors.push(format!("struct {} has no field '{}'", name, field_name)),
                }
            }

            TypeAnno::Named(name.clone())
        }

        Expr::FieldAccess { object, field } => {
            let object_ty = infer_expr(object, env, errors);
            match object_ty {
                TypeAnno::Named(name) => match env.get_struct(&name) {
                    Some(info) => match find_struct_field(info, field) {
                        Some(field_ty) => field_ty.clone(),
                        None => {
                            errors.push(format!("struct {} has no field '{}'", name, field));
                            TypeAnno::None
                        }
                    },
                    None => {
                        errors.push(format!("unknown struct '{}'", name));
                        TypeAnno::None
                    }
                },
                other => {
                    errors.push(format!("type {:?} has no fields", other));
                    TypeAnno::None
                }
            }
        }

        Expr::MethodCall { receiver, method, args } => {
            let receiver_ty = infer_expr(receiver, env, errors);
            let arg_types: Vec<TypeAnno> = args.iter().map(|a| infer_expr(a, env, errors)).collect();

            match &receiver_ty {
                TypeAnno::Named(name) => {
                    let info = match env.get_struct(name) {
                        Some(info) => info,
                        None => {
                            errors.push(format!("unknown struct '{}'", name));
                            return TypeAnno::None;
                        }
                    };

                    match find_struct_method(info, method) {
                        Some(TypeAnno::Fn { params, ret }) => {
                            if params.is_empty() {
                                errors.push(format!(
                                    "method {}.{} is missing its self parameter",
                                    name, method
                                ));
                                return TypeAnno::None;
                            }

                            if !types_compatible(&params[0], &receiver_ty) {
                                errors.push(format!(
                                    "method {}.{} expects self of type {:?}, got {:?}",
                                    name, method, params[0], receiver_ty
                                ));
                            }

                            let expected_args = &params[1..];
                            if expected_args.len() != arg_types.len() {
                                errors.push(format!(
                                    "method {}.{} expects {} args, got {}",
                                    name, method, expected_args.len(), arg_types.len()
                                ));
                            }

                            for (idx, (expected, actual)) in expected_args.iter().zip(arg_types.iter()).enumerate() {
                                if !types_compatible(expected, actual) {
                                    errors.push(format!(
                                        "method {}.{} argument {} mismatch: expected {:?}, got {:?}",
                                        name, method, idx + 1, expected, actual
                                    ));
                                }
                            }

                            (**ret).clone()
                        }
                        Some(other) => {
                            errors.push(format!("{}.{} is not callable (type {:?})", name, method, other));
                            TypeAnno::None
                        }
                        None => {
                            errors.push(format!("struct {} has no method '{}'", name, method));
                            TypeAnno::None
                        }
                    }
                }

                TypeAnno::Str => infer_string_method(method, &arg_types, errors),
                TypeAnno::Vec(inner) => infer_vector_method(method, &arg_types, inner.as_ref(), errors),

                other => {
                    errors.push(format!("type {:?} has no method '{}'", other, method));
                    TypeAnno::None
                }
            }
        }
    }
}

fn infer_string_method(method: &str, arg_types: &[TypeAnno], errors: &mut Vec<String>) -> TypeAnno {
    match method {
        "len" => {
            if !arg_types.is_empty() {
                errors.push("string.len expects 0 arguments".into());
            }
            TypeAnno::Int
        }
        "upper" | "lower" | "trim" => {
            if !arg_types.is_empty() {
                errors.push(format!("string.{} expects 0 arguments", method));
            }
            TypeAnno::Str
        }
        "contains" => {
            if arg_types.len() != 1 {
                errors.push("string.contains expects 1 argument".into());
            } else if !types_compatible(&TypeAnno::Str, &arg_types[0]) {
                errors.push(format!("string.contains expects string, got {:?}", arg_types[0]));
            }
            TypeAnno::Bool
        }
        "split" => {
            if arg_types.len() != 1 {
                errors.push("string.split expects 1 argument".into());
            } else if !types_compatible(&TypeAnno::Str, &arg_types[0]) {
                errors.push(format!("string.split expects string, got {:?}", arg_types[0]));
            }
            TypeAnno::Vec(Box::new(TypeAnno::Str))
        }
        _ => {
            errors.push(format!("string has no method '{}'", method));
            TypeAnno::None
        }
    }
}

fn infer_vector_method(
    method: &str,
    arg_types: &[TypeAnno],
    inner: &TypeAnno,
    errors: &mut Vec<String>,
) -> TypeAnno {
    match method {
        "push" => {
            if arg_types.len() != 1 {
                errors.push("vector.push expects 1 argument".into());
            } else if !types_compatible(inner, &arg_types[0]) {
                errors.push(format!("vector.push expects {:?}, got {:?}", inner, arg_types[0]));
            }
            TypeAnno::None
        }
        "extend" => {
            if arg_types.len() != 1 {
                errors.push("vector.extend expects 1 argument".into());
            } else if !types_compatible(&TypeAnno::Vec(Box::new(inner.clone())), &arg_types[0]) {
                errors.push(format!(
                    "vector.extend expects {:?}, got {:?}",
                    TypeAnno::Vec(Box::new(inner.clone())),
                    arg_types[0]
                ));
            }
            TypeAnno::None
        }
        "pop" => {
            if !arg_types.is_empty() {
                errors.push("vector.pop expects 0 arguments".into());
            }
            inner.clone()
        }
        "clear" => {
            if !arg_types.is_empty() {
                errors.push("vector.clear expects 0 arguments".into());
            }
            TypeAnno::None
        }
        "len" => {
            if !arg_types.is_empty() {
                errors.push("vector.len expects 0 arguments".into());
            }
            TypeAnno::Int
        }
        // Higher-order methods remain loose until builtin function types are richer.
        "map" | "fold" | "filter" | "zip" => TypeAnno::None,
        _ => {
            errors.push(format!("vector has no method '{}'", method));
            TypeAnno::None
        }
    }
}

fn check_stmt(stmt: &Stmt, env: &mut TypeEnv, errors: &mut Vec<String>, ctx: CheckCtx<'_>) {
    match stmt {
        Stmt::Let { name, type_anno, value, mutable } => {
            let declared = require_annotation(type_anno, &format!("let {}", name), env, errors);
            let inferred = infer_expr(value, env, errors);

            if !types_compatible(&declared, &inferred) {
                errors.push(format!(
                    "type mismatch: '{}' declared as {:?} but assigned {:?}",
                    name, declared, inferred
                ));
            }

            env.define(name.clone(), declared, *mutable);
        }

        Stmt::FnDef { name, params, tol_param, return_type, body } => {
            let param_types: Vec<TypeAnno> = params.iter()
                .map(|(pname, pty)| require_annotation(pty, &format!("fn {}({})", name, pname), env, errors))
                .collect();
            let resolved_return_type = fn_return_type(return_type, &format!("fn {} return", name), env, errors);

            let fn_type = TypeAnno::Fn {
                params: param_types.clone(),
                ret: Box::new(resolved_return_type.clone()),
            };
            env.define(name.clone(), fn_type, false);

            env.push_scope();
            for ((pname, _), pty) in params.iter().zip(param_types.iter()) {
                env.define(pname.clone(), pty.clone(), true);
            }
            if let Some(tol_name) = tol_param {
                env.define(tol_name.clone(), TypeAnno::Optional(Box::new(TypeAnno::Float)), false);
            }
            for s in body {
                check_stmt(s, env, errors, ctx.in_fn(name, &resolved_return_type));
            }
            env.pop_scope();
        }

        Stmt::StructDef { name, fields, methods } => {
            let resolved_fields: Vec<(String, TypeAnno)> = fields.iter()
                .map(|(fname, fty)| (
                    fname.clone(),
                    require_annotation(fty, &format!("struct {} field {}", name, fname), env, errors),
                ))
                .collect();

            let mut method_sigs = Vec::new();
            for method in methods {
                if let Stmt::FnDef { name: method_name, params, return_type, .. } = method {
                    let param_types: Vec<TypeAnno> = params.iter()
                        .map(|(pname, pty)| require_annotation(pty, &format!("fn {}.{}({})", name, method_name, pname), env, errors))
                        .collect();
                    let resolved_return_type = fn_return_type(return_type, &format!("fn {}.{} return", name, method_name), env, errors);
                    method_sigs.push((
                        method_name.clone(),
                        TypeAnno::Fn {
                            params: param_types,
                            ret: Box::new(resolved_return_type),
                        },
                    ));
                } else {
                    errors.push(format!("struct {} body may only contain methods", name));
                }
            }

            env.define_struct(StructInfo {
                name: name.clone(),
                fields: resolved_fields,
                methods: method_sigs,
            });

            for method in methods {
                if let Stmt::FnDef { name: method_name, params, tol_param, return_type, body } = method {
                    let param_types: Vec<TypeAnno> = params.iter()
                        .map(|(pname, pty)| require_annotation(pty, &format!("fn {}.{}({})", name, method_name, pname), env, errors))
                        .collect();
                    let resolved_return_type = fn_return_type(return_type, &format!("fn {}.{} return", name, method_name), env, errors);

                    if param_types.is_empty() {
                        errors.push(format!("struct {} method must declare self as first parameter", name));
                    } else {
                        let self_ty = TypeAnno::Named(name.clone());
                        if !types_compatible(&param_types[0], &self_ty) {
                            errors.push(format!(
                                "struct {} method self parameter must be {:?}, got {:?}",
                                name, self_ty, param_types[0]
                            ));
                        }
                    }

                    env.push_scope();
                    for ((pname, _), pty) in params.iter().zip(param_types.iter()) {
                        env.define(pname.clone(), pty.clone(), true);
                    }
                    if let Some(tol_name) = tol_param {
                        env.define(tol_name.clone(), TypeAnno::Optional(Box::new(TypeAnno::Float)), false);
                    }
                    for s in body {
                        check_stmt(s, env, errors, ctx.in_fn(method_name, &resolved_return_type));
                    }
                    env.pop_scope();
                }
            }
        }

        Stmt::Return { value } => {
            let actual = infer_expr(value, env, errors);
            match ctx.current_fn {
                Some((fn_name, expected)) => {
                    if !types_compatible(expected, &actual) {
                        errors.push(format!(
                            "fn {} return type mismatch: expected {:?}, got {:?}",
                            fn_name, expected, actual
                        ));
                    }
                }
                None => errors.push("return outside of function".into()),
            }
        }

        Stmt::If { condition, then_body, else_body } => {
            let cond_ty = infer_expr(condition, env, errors);
            if !is_bool_compatible(&cond_ty) {
                errors.push(format!("if condition must be bool, got {:?}", cond_ty));
            }

            env.push_scope();
            if let Some((name, narrowed_ty)) = truthy_narrowing(condition, env) {
                env.define(name, narrowed_ty, false);
            }
            for s in then_body {
                check_stmt(s, env, errors, ctx);
            }
            env.pop_scope();

            if let Some(eb) = else_body {
                env.push_scope();
                for s in eb {
                    check_stmt(s, env, errors, ctx);
                }
                env.pop_scope();
            }
        }

        Stmt::Assign { name, value } => {
            let Some(existing) = env.get(name).cloned() else {
                errors.push(format!("undefined variable: {}", name));
                return;
            };

            if !existing.mutable {
                errors.push(format!("cannot assign to const '{}'", name));
            }

            let actual = infer_expr(value, env, errors);
            if !types_compatible(&existing.ty, &actual) {
                errors.push(format!(
                    "type mismatch: '{}' is {:?} but assigned {:?}",
                    name, existing.ty, actual
                ));
            }
        }

        Stmt::Loop { body } => {
            env.push_scope();
            for s in body {
                check_stmt(s, env, errors, ctx.in_loop());
            }
            env.pop_scope();
        }

        Stmt::Break => {
            if ctx.loop_depth == 0 {
                errors.push("break outside of loop".into());
            }
        }

        Stmt::Import { .. } => {}

        Stmt::ExprStmt(expr) => {
            let _ = infer_expr(expr, env, errors);
        }
    }
}
