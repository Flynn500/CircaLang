use crate::ast::*;

/// A type environment: maps variable names to their declared types.
struct TypeEnv {
    entries: Vec<(String, TypeAnno)>,
    scope_starts: Vec<usize>,
    /// Known struct names.
    structs: Vec<String>,
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

    fn define(&mut self, name: String, ty: TypeAnno) {
        self.entries.push((name, ty));
    }

    fn get(&self, name: &str) -> Option<&TypeAnno> {
        for (n, ty) in self.entries.iter().rev() {
            if n == name {
                return Some(ty);
            }
        }
        None
    }

    fn define_struct(&mut self, name: String) {
        self.structs.push(name);
    }

    fn struct_exists(&self, name: &str) -> bool {
        self.structs.iter().any(|s| s == name)
    }
}

/// Run type checking on a resolved program. Returns a list of errors.
pub fn typecheck(program: &Program) -> Vec<String> {
    let mut env = TypeEnv::new();
    let mut errors = Vec::new();
    for stmt in program {
        check_stmt(stmt, &mut env, &mut errors);
    }
    errors
}

/// Validate that a TypeAnno refers to known types.
fn validate_type(ty: &TypeAnno, env: &TypeEnv) -> Result<(), String> {
    match ty {
        TypeAnno::Int | TypeAnno::Float | TypeAnno::Bool | TypeAnno::Str | TypeAnno::None => Ok(()),
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

/// Infer the type of an expression from the AST (shallow — no deep inference).
fn infer_expr(expr: &Expr, env: &TypeEnv) -> TypeAnno {
    match expr {
        Expr::Number(_) => TypeAnno::Float,
        Expr::Integer(_) => TypeAnno::Int,
        Expr::StringLiteral(_) => TypeAnno::Str,
        Expr::Bool(_) => TypeAnno::Bool,
        Expr::None => TypeAnno::None,
        Expr::Ident(name) => {
            env.get(name).cloned().unwrap_or(TypeAnno::None)
        }
        Expr::WithTolerance { value, .. } => infer_expr(value, env),
        Expr::Unary { expr, .. } => infer_expr(expr, env),
        Expr::BinOp { left, op, .. } => {
            match op {
                BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Gt
                | BinOp::Lte | BinOp::Gte | BinOp::MaybeEq | BinOp::MaybeNeq
                | BinOp::MaybeLt | BinOp::MaybeGt | BinOp::MaybeLte | BinOp::MaybeGte => {
                    TypeAnno::Bool
                }
                _ => infer_expr(left, env),
            }
        }
        Expr::Call { func, .. } => {
            // If we know the function's return type, use it
            let func_ty = infer_expr(func, env);
            match func_ty {
                TypeAnno::Fn { ret, .. } => *ret,
                _ => TypeAnno::None,
            }
        }
        Expr::VecLiteral(elems) => {
            if let Some(first) = elems.first() {
                TypeAnno::Vec(Box::new(infer_expr(first, env)))
            } else {
                TypeAnno::Vec(Box::new(TypeAnno::None))
            }
        }
        Expr::Index { vec, .. } => {
            match infer_expr(vec, env) {
                TypeAnno::Vec(inner) => *inner,
                TypeAnno::Str => TypeAnno::Str,
                _ => TypeAnno::None,
            }
        }
        Expr::StructInit { name, .. } => TypeAnno::Named(name.clone()),
        Expr::FieldAccess { .. } => TypeAnno::None,
        Expr::MethodCall { .. } => TypeAnno::None,
        Expr::Lambda { params, tol_param: _, return_type, .. } => {
            let param_types: Vec<TypeAnno> = params.iter()
                .map(|(_, ty)| ty.clone().unwrap_or(TypeAnno::None))
                .collect();
            let ret = return_type.clone().unwrap_or(TypeAnno::None);
            TypeAnno::Fn {
                params: param_types,
                ret: Box::new(ret),
            }
        }
    }
}

/// Check whether two types are compatible.
/// None acts as a wildcard (untyped), compatible with anything.
fn types_compatible(declared: &TypeAnno, actual: &TypeAnno) -> bool {
    if matches!(declared, TypeAnno::None) || matches!(actual, TypeAnno::None) {
        return true;
    }
    // Int is compatible with Float (promotion)
    if matches!(declared, TypeAnno::Float) && matches!(actual, TypeAnno::Int) {
        return true;
    }
    match (declared, actual) {
        (TypeAnno::Int, TypeAnno::Int) => true,
        (TypeAnno::Float, TypeAnno::Float) => true,
        (TypeAnno::Bool, TypeAnno::Bool) => true,
        (TypeAnno::Str, TypeAnno::Str) => true,
        (TypeAnno::Named(a), TypeAnno::Named(b)) => a == b,
        (TypeAnno::Vec(a), TypeAnno::Vec(b)) => types_compatible(a, b),
        (TypeAnno::Fn { params: pa, ret: ra }, TypeAnno::Fn { params: pb, ret: rb }) => {
            pa.len() == pb.len()
                && pa.iter().zip(pb.iter()).all(|(a, b)| types_compatible(a, b))
                && types_compatible(ra, rb)
        }
        _ => false,
    }
}

fn check_stmt(stmt: &Stmt, env: &mut TypeEnv, errors: &mut Vec<String>) {
    match stmt {
        Stmt::Let { name, type_anno, value, .. } => {
            if let Some(ty) = type_anno {
                if let Err(e) = validate_type(ty, env) {
                    errors.push(format!("let {}: {}", name, e));
                    return;
                }

                let inferred = infer_expr(value, env);
                if !types_compatible(ty, &inferred) {
                    errors.push(format!(
                        "type mismatch: '{}' declared as {:?} but assigned {:?}",
                        name, ty, inferred
                    ));
                }

                env.define(name.clone(), ty.clone());
            } else {
                // No annotation — infer and record
                let inferred = infer_expr(value, env);
                env.define(name.clone(), inferred);
            }
        }

        Stmt::FnDef { name, params, tol_param: _, return_type, body } => {
            // Validate param types
            for (pname, pty) in params {
                if let Err(e) = validate_type(pty, env) {
                    errors.push(format!("fn {}({}): {}", name, pname, e));
                }
            }
            if let Err(e) = validate_type(return_type, env) {
                errors.push(format!("fn {} return: {}", name, e));
            }

            // Build the function's type and define it
            let fn_type = TypeAnno::Fn {
                params: params.iter().map(|(_, ty)| ty.clone()).collect(),
                ret: Box::new(return_type.clone()),
            };
            env.define(name.clone(), fn_type);

            // Check the body in a new scope with params defined
            env.push_scope();
            for (pname, pty) in params {
                env.define(pname.clone(), pty.clone());
            }
            for s in body {
                check_stmt(s, env, errors);
            }
            env.pop_scope();
        }

        Stmt::StructDef { name, fields, methods } => {
            env.define_struct(name.clone());

            // Validate field types
            for (fname, fty) in fields {
                if let Err(e) = validate_type(fty, env) {
                    errors.push(format!("struct {} field {}: {}", name, fname, e));
                }
            }

            // Check methods
            for m in methods {
                check_stmt(m, env, errors);
            }
        }

        Stmt::If { condition: _, then_body, else_body } => {
            env.push_scope();
            for s in then_body {
                check_stmt(s, env, errors);
            }
            env.pop_scope();

            if let Some(eb) = else_body {
                env.push_scope();
                for s in eb {
                    check_stmt(s, env, errors);
                }
                env.pop_scope();
            }
        }

        Stmt::Loop { body } => {
            env.push_scope();
            for s in body {
                check_stmt(s, env, errors);
            }
            env.pop_scope();
        }

        Stmt::Assign { .. }
        | Stmt::Return { .. }
        | Stmt::Break
        | Stmt::Import { .. }
        | Stmt::ExprStmt(_) => {}
    }
}