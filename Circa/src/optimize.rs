// optimize.rs

use crate::ast::*;

/// Run all optimization passes on a program.
pub fn optimize(program: Program) -> Program {
    program.into_iter().map(optimize_stmt).collect()
}

fn optimize_stmt(stmt: Stmt) -> Stmt {
    match stmt {
        Stmt::Let { name, type_anno, value, mutable } => Stmt::Let {
            name,
            type_anno,
            value: optimize_expr(value),
            mutable,
        },
        Stmt::Assign { name, value } => Stmt::Assign {
            name,
            value: optimize_expr(value),
        },
        Stmt::Return { value } => Stmt::Return {
            value: optimize_expr(value),
        },
        Stmt::If { condition, then_body, else_body } => Stmt::If {
            condition: optimize_expr(condition),
            then_body: eliminate_dead_code(then_body.into_iter().map(optimize_stmt).collect()),
            else_body: else_body.map(|eb| eliminate_dead_code(eb.into_iter().map(optimize_stmt).collect()))
        },
        Stmt::FnDef { name, params, body, tol_param, return_type } => Stmt::FnDef {
            name,
            params,
            body: eliminate_dead_code(body.into_iter().map(optimize_stmt).collect()),
            tol_param,
            return_type,
        },
        Stmt::StructDef { name, fields, methods } => Stmt::StructDef {
            name,
            fields,
            methods: methods.into_iter().map(optimize_stmt).collect(),
        },
        Stmt::Loop { body } => Stmt::Loop {
            body: eliminate_dead_code(body.into_iter().map(optimize_stmt).collect()),
        },
        Stmt::ExprStmt(expr) => Stmt::ExprStmt(optimize_expr(expr)),
        Stmt::Break => Stmt::Break,
        Stmt::Import { .. } => unreachable!("unresolved import"),
    }
}

fn optimize_expr(expr: Expr) -> Expr {
    // Recurse first (bottom-up), then apply optimizations
    let expr = recurse_expr(expr);
    
    // --- Pass 1: Constant folding ---
    fold_constants(expr)
}

/// Recursively optimize sub-expressions.
fn recurse_expr(expr: Expr) -> Expr {
    match expr {
        Expr::BinOp { left, op, right } => Expr::BinOp {
            left: Box::new(optimize_expr(*left)),
            op,
            right: Box::new(optimize_expr(*right)),
        },
        Expr::Unary { op, expr } => Expr::Unary {
            op,
            expr: Box::new(optimize_expr(*expr)),
        },
        Expr::Call { func, args, tolerance } => Expr::Call {
            func: Box::new(optimize_expr(*func)),
            args: args.into_iter().map(optimize_expr).collect(),
            tolerance: tolerance.map(|t| Box::new(optimize_expr(*t))),
        },
        Expr::WithTolerance { value, tolerance } => Expr::WithTolerance {
            value: Box::new(optimize_expr(*value)),
            tolerance: Box::new(optimize_expr(*tolerance)),
        },
        Expr::Lambda { params, body, tol_param, return_type } => Expr::Lambda {
            params,
            body: eliminate_dead_code(body.into_iter().map(optimize_stmt).collect()),
            tol_param,
            return_type,
        },
        Expr::StructInit { name, fields } => Expr::StructInit {
            name,
            fields: fields.into_iter()
                .map(|(k, v)| (k, optimize_expr(v)))
                .collect(),
        },
        Expr::FieldAccess { object, field } => Expr::FieldAccess {
            object: Box::new(optimize_expr(*object)),
            field,
        },
        Expr::VecLiteral(elems) => Expr::VecLiteral(
            elems.into_iter().map(optimize_expr).collect(),
        ),
        Expr::Index { vec, index } => Expr::Index {
            vec: Box::new(optimize_expr(*vec)),
            index: Box::new(optimize_expr(*index)),
        },
        Expr::MethodCall { receiver, method, args } => Expr::MethodCall {
            receiver: Box::new(optimize_expr(*receiver)),
            method,
            args: args.into_iter().map(optimize_expr).collect(),
        },
        // Leaves — nothing to recurse into
        other => other,
    }
}

/// Fold constant arithmetic: BinOp(Number, op, Number) -> Number
fn fold_constants(expr: Expr) -> Expr {
    match expr {
        Expr::BinOp { left, op, right } => {
            match (*left, *right) {
                (Expr::Number(a), Expr::Number(b)) => {
                    match try_fold_float(a, &op, b) {
                        Some(result) => result,
                        None => Expr::BinOp {
                            left: Box::new(Expr::Number(a)),
                            op,
                            right: Box::new(Expr::Number(b)),
                        },
                    }
                }
                (Expr::Integer(a), Expr::Integer(b)) => {
                    match try_fold_int(a, &op, b) {
                        Some(result) => result,
                        None => Expr::BinOp {
                            left: Box::new(Expr::Integer(a)),
                            op,
                            right: Box::new(Expr::Integer(b)),
                        },
                    }
                }
                // String concat: "a" + "b"
                (Expr::StringLiteral(a), Expr::StringLiteral(b)) => {
                    match op {
                        BinOp::Add => Expr::StringLiteral(format!("{}{}", a, b)),
                        _ => Expr::BinOp {
                            left: Box::new(Expr::StringLiteral(a)),
                            op,
                            right: Box::new(Expr::StringLiteral(b)),
                        },
                    }
                }
                // String repeat: "a" * 3 or 3 * "a"
                (Expr::StringLiteral(s), Expr::Integer(n)) => {
                    match op {
                        BinOp::Mul if n >= 0 => Expr::StringLiteral(s.repeat(n as usize)),
                        _ => Expr::BinOp {
                            left: Box::new(Expr::StringLiteral(s)),
                            op,
                            right: Box::new(Expr::Integer(n)),
                        },
                    }
                }
                (Expr::Integer(n), Expr::StringLiteral(s)) => {
                    match op {
                        BinOp::Mul if n >= 0 => Expr::StringLiteral(s.repeat(n as usize)),
                        _ => Expr::BinOp {
                            left: Box::new(Expr::Integer(n)),
                            op,
                            right: Box::new(Expr::StringLiteral(s)),
                        },
                    }
                }
                (left, right) => Expr::BinOp {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
            }
        }
        Expr::Unary { op: UnaryOp::Neg, expr } => {
            match *expr {
                Expr::Number(n) => Expr::Number(-n),
                Expr::Integer(n) => Expr::Integer(-n),
                other => Expr::Unary { op: UnaryOp::Neg, expr: Box::new(other) },
            }
        }
        other => other,
    }
}

fn try_fold_float(a: f64, op: &BinOp, b: f64) -> Option<Expr> {
    match op {
        BinOp::Add => Some(Expr::Number(a + b)),
        BinOp::Sub => Some(Expr::Number(a - b)),
        BinOp::Mul => Some(Expr::Number(a * b)),
        BinOp::Div => {
            if b == 0.0 { None }
            else { Some(Expr::Number(a / b)) }
        }
        BinOp::Eq  => Some(Expr::Bool(a == b)),
        BinOp::Neq => Some(Expr::Bool(a != b)),
        BinOp::Lt  => Some(Expr::Bool(a < b)),
        BinOp::Gt  => Some(Expr::Bool(a > b)),
        BinOp::Lte => Some(Expr::Bool(a <= b)),
        BinOp::Gte => Some(Expr::Bool(a >= b)),
        BinOp::MaybeEq  => Some(Expr::Bool(a == b)),
        BinOp::MaybeNeq => Some(Expr::Bool(a != b)),
        BinOp::MaybeLt  => Some(Expr::Bool(a < b)),
        BinOp::MaybeGt  => Some(Expr::Bool(a > b)),
        BinOp::MaybeLte => Some(Expr::Bool(a <= b)),
        BinOp::MaybeGte => Some(Expr::Bool(a >= b)),
    }
}

fn try_fold_int(a: i64, op: &BinOp, b: i64) -> Option<Expr> {
    match op {
        BinOp::Add => Some(Expr::Integer(a + b)),
        BinOp::Sub => Some(Expr::Integer(a - b)),
        BinOp::Mul => Some(Expr::Integer(a * b)),
        BinOp::Div => {
            if b == 0 { None }
            else { Some(Expr::Integer(a / b)) }
        }
        BinOp::Eq  => Some(Expr::Bool(a == b)),
        BinOp::Neq => Some(Expr::Bool(a != b)),
        BinOp::Lt  => Some(Expr::Bool(a < b)),
        BinOp::Gt  => Some(Expr::Bool(a > b)),
        BinOp::Lte => Some(Expr::Bool(a <= b)),
        BinOp::Gte => Some(Expr::Bool(a >= b)),
        BinOp::MaybeEq  => Some(Expr::Bool(a == b)),
        BinOp::MaybeNeq => Some(Expr::Bool(a != b)),
        BinOp::MaybeLt  => Some(Expr::Bool(a < b)),
        BinOp::MaybeGt  => Some(Expr::Bool(a > b)),
        BinOp::MaybeLte => Some(Expr::Bool(a <= b)),
        BinOp::MaybeGte => Some(Expr::Bool(a >= b)),
    }
}

/// Remove statements after an unconditional return or break.
fn eliminate_dead_code(body: Vec<Stmt>) -> Vec<Stmt> {
    let mut result = Vec::new();
    for stmt in body {
        let is_terminal = matches!(&stmt, Stmt::Return { .. } | Stmt::Break);
        result.push(stmt);
        if is_terminal {
            break;
        }
    }
    result
}