// optimize.rs

use crate::ast::*;

/// Run all optimization passes on a program.
pub fn optimize(program: Program) -> Program {
    program.into_iter().map(optimize_stmt).collect()
}

fn optimize_stmt(stmt: Stmt) -> Stmt {
    match stmt {
        Stmt::Let { name, value, tolerance } => Stmt::Let {
            name,
            value: optimize_expr(value),
            tolerance: tolerance.map(optimize_expr),
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
        Stmt::FnDef { name, params, body, tol_param } => Stmt::FnDef {
            name,
            params,
            body: eliminate_dead_code(body.into_iter().map(optimize_stmt).collect()),
            tol_param,
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
        Expr::Lambda { params, body, tol_param } => Expr::Lambda {
            params,
            body: eliminate_dead_code(body.into_iter().map(optimize_stmt).collect()),
            tol_param,
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
                    match try_fold(a, &op, b) {
                        Some(result) => result,
                        None => Expr::BinOp {
                            left: Box::new(Expr::Number(a)),
                            op,
                            right: Box::new(Expr::Number(b)),
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
                other => Expr::Unary { op: UnaryOp::Neg, expr: Box::new(other) },
            }
        }
        other => other,
    }
}

fn try_fold(a: f64, op: &BinOp, b: f64) -> Option<Expr> {
    match op {
        BinOp::Add => Some(Expr::Number(a + b)),
        BinOp::Sub => Some(Expr::Number(a - b)),
        BinOp::Mul => Some(Expr::Number(a * b)),
        BinOp::Div => {
            if b == 0.0 { None } // don't fold div by zero, let runtime catch it
            else { Some(Expr::Number(a / b)) }
        }
        BinOp::Eq  => Some(Expr::Bool(a == b)),
        BinOp::Neq => Some(Expr::Bool(a != b)),
        BinOp::Lt  => Some(Expr::Bool(a < b)),
        BinOp::Gt  => Some(Expr::Bool(a > b)),
        BinOp::Lte => Some(Expr::Bool(a <= b)),
        BinOp::Gte => Some(Expr::Bool(a >= b)),
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