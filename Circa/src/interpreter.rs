use crate::ast::*;
use crate::builtins;
use crate::env::Env;
use crate::value::Value;

/// Signals that can interrupt normal statement flow.
enum Signal {
    /// A return statement was hit.
    Return(Value),
    /// A break statement was hit.
    Break,
}

pub struct Interpreter {
    env: Env,
}

impl Interpreter {
    pub fn new() -> Self {
        let mut env = Env::new();
        builtins::register_builtins(&mut env);
        Interpreter { env }
    }

    /// Run a full program.
    pub fn run(&mut self, program: &Program) -> Result<(), String> {
        for stmt in program {
            match self.exec_stmt(stmt)? {
                Some(Signal::Return(_)) => return Err("return outside of function".into()),
                Some(Signal::Break) => return Err("break outside of loop".into()),
                None => {}
            }
        }
        Ok(())
    }

    /// Execute a statement. Returns Some(Signal) if flow was interrupted.
    fn exec_stmt(&mut self, stmt: &Stmt) -> Result<Option<Signal>, String> {
        match stmt {
            Stmt::Let { name, value, tolerance } => {
                // If value is a Call and we have a tolerance, pass tol into the call
                // Evaluate the value expression (calls handle their own ~tol)
                let result = self.eval_expr(value)?;

                // Apply ~= tolerance to the stored variable
                let result = if let Some(tol_expr) = tolerance {
                    let tol_val = self.eval_expr(tol_expr)?;
                    let tol_f32 = tol_val
                        .as_f32()
                        .ok_or("tolerance must be a number".to_string())?;
                    match result {
                        Value::Number { val, .. } => Value::number_with_tol(val, tol_f32),
                        other => return Err(format!(
                            "cannot apply tolerance to {}", other
                        )),
                    }
                } else {
                    result
                };

                self.env.define(name.clone(), result);
                Ok(None)
            }

            Stmt::Assign { name, value } => {
                let result = self.eval_expr(value)?;
                if !self.env.assign(name, result) {
                    return Err(format!("undefined variable: {}", name));
                }
                Ok(None)
            }

            Stmt::FnDef { name, params, body, guarantees_tol } => {
                let func = Value::Func {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                    guarantees_tol: *guarantees_tol,
                };
                self.env.define(name.clone(), func);
                Ok(None)
            }

            Stmt::Return { value } => {
                let result = self.eval_expr(value)?;
                Ok(Some(Signal::Return(result)))
            }

            Stmt::If { condition, then_body, else_body } => {
                let cond = self.eval_expr(condition)?;
                let truthy = cond
                    .as_bool()
                    .ok_or("if condition must be bool-like".to_string())?;

                let body = if truthy { then_body } else {
                    match else_body {
                        Some(b) => b,
                        None => return Ok(None),
                    }
                };

                self.env.push_scope();
                for s in body {
                    if let Some(signal) = self.exec_stmt(s)? {
                        self.env.pop_scope();
                        return Ok(Some(signal));
                    }
                }
                self.env.pop_scope();
                Ok(None)
            }

            Stmt::Loop { body } => {
                loop {
                    self.env.push_scope();
                    let mut broke = false;
                    for s in body {
                        match self.exec_stmt(s)? {
                            Some(Signal::Break) => {
                                broke = true;
                                break;
                            }
                            Some(sig) => {
                                self.env.pop_scope();
                                return Ok(Some(sig));
                            }
                            None => {}
                        }
                    }
                    self.env.pop_scope();
                    if broke { break; }
                }
                Ok(None)
            }

            Stmt::Break => Ok(Some(Signal::Break)),

            Stmt::ExprStmt(expr) => {
                self.eval_expr(expr)?;
                Ok(None)
            }
        }
    }

    /// Evaluate an expression to a Value.
    fn eval_expr(&mut self, expr: &Expr) -> Result<Value, String> {
        match expr {
            Expr::Number(n) => Ok(Value::number(*n as f32)),

            Expr::Bool(b) => Ok(Value::Bool(*b)),

            Expr::Ident(name) => {
                self.env
                    .get(name)
                    .cloned()
                    .ok_or(format!("undefined variable: {}", name))
            }

            Expr::Tol => {
                self.env
                    .get("tol")
                    .cloned()
                    .ok_or("tol is not defined in this scope".into())
            }

            Expr::Unary { expr: inner, .. } => {
                let val = self.eval_expr(inner)?;
                match val {
                    Value::Number { val, tol } => {
                        Ok(Value::Number { val: -val, tol })
                    }
                    _ => Err("cannot negate non-number".into()),
                }
            }

            Expr::BinOp { left, op, right } => {
                let lhs = self.eval_expr(left)?;
                let rhs = self.eval_expr(right)?;
                self.eval_binop(&lhs, op, &rhs)
            }

            Expr::Call { func, args, tolerance } => {
                let caller_tol = match tolerance {
                    Some(tol_expr) => {
                        let tol_val = self.eval_expr(tol_expr)?;
                        Some(tol_val.as_f32().ok_or(
                            "tolerance must be a number".to_string(),
                        )?)
                    }
                    None => None,
                };
                self.eval_call(func, args, caller_tol)
            }

            Expr::Lambda { params, body, guarantees_tol } => {
                Ok(Value::Func {
                    name: "<lambda>".to_string(),
                    params: params.clone(),
                    body: body.clone(),
                    guarantees_tol: *guarantees_tol,
                })
            }

            Expr::WithTolerance { value, tolerance } => {
                let val = self.eval_expr(value)?;
                let tol = self.eval_expr(tolerance)?;
                let tol_f32 = tol
                    .as_f32()
                    .ok_or("tolerance must be a number".to_string())?;
                match val {
                    Value::Number { val, .. } => Ok(Value::number_with_tol(val, tol_f32)),
                    _ => Err("cannot apply tolerance to non-number".into()),
                }
            }
        }
    }

    /// Evaluate a function call, optionally injecting a tol value.
    fn eval_call(
        &mut self,
        func_expr: &Expr,
        arg_exprs: &[Expr],
        caller_tol: Option<f32>,
    ) -> Result<Value, String> {
        let func = self.eval_expr(func_expr)?;
        let args: Vec<Value> = arg_exprs
            .iter()
            .map(|a| self.eval_expr(a))
            .collect::<Result<_, _>>()?;

        match func {
            Value::Func { params, body, name, guarantees_tol } => {
                if args.len() != params.len() {
                    return Err(format!(
                        "{}: expected {} args, got {}",
                        name, params.len(), args.len()
                    ));
                }

                if caller_tol.is_some() && !guarantees_tol {
                    return Err(format!(
                        "{}: does not accept a tolerance argument (~tol not in signature)",
                        name
                    ));
                }

                self.env.push_scope();

                // Bind parameters
                for (param, arg) in params.iter().zip(args) {
                    self.env.define(param.clone(), arg);
                }

                // Inject tol only for functions that declare ~tol
                if guarantees_tol {
                    let tol_f32 = caller_tol.unwrap_or(0.0);
                    self.env.define("tol".into(), Value::number(tol_f32));
                }

                // Execute body
                let mut result = Value::Bool(false); // default return
                for s in &body {
                    match self.exec_stmt(s)? {
                        Some(Signal::Return(val)) => {
                            result = val;
                            break;
                        }
                        Some(Signal::Break) => {
                            return Err("break outside of loop".into());
                        }
                        None => {}
                    }
                }

                self.env.pop_scope();

                // If the function guarantees tol (~= tol signature),
                // tag the return value with the caller's tolerance.
                if guarantees_tol {
                    if let Some(t) = caller_tol {
                        result = match result {
                            Value::Number { val, .. } => Value::number_with_tol(val, t),
                            other => other,
                        };
                    }
                }

                Ok(result)
            }
            Value::NativeFunc { name, arity, func, guarantees_tol } => {
                if args.len() != arity {
                    return Err(format!(
                        "{}: expected {} args, got {}",
                        name, arity, args.len()
                    ));
                }

                if caller_tol.is_some() && !guarantees_tol {
                    return Err(format!(
                        "{}: does not accept a tolerance argument (~tol not in signature)",
                        name
                    ));
                }

                let mut result = func(&args, caller_tol)?;

                if guarantees_tol {
                    if let Some(t) = caller_tol {
                        result = match result {
                            Value::Number { val, .. } => Value::number_with_tol(val, t),
                            other => other,
                        };
                    }
                }

                Ok(result)
            }
            other => Err(format!("{} is not callable", other)),
        }
    }

    /// Evaluate a binary operation.
    fn eval_binop(
        &self,
        lhs: &Value,
        op: &BinOp,
        rhs: &Value,
    ) -> Result<Value, String> {
        match op {
            // Arithmetic — with tolerance propagation
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                let (a, tol_a) = match lhs {
                    Value::Number { val, tol } => (*val, tol.unwrap_or(0.0)),
                    _ => return Err("left operand must be a number".into()),
                };
                let (b, tol_b) = match rhs {
                    Value::Number { val, tol } => (*val, tol.unwrap_or(0.0)),
                    _ => return Err("right operand must be a number".into()),
                };

                let (result, result_tol) = match op {
                    // add/sub: tolerances add
                    BinOp::Add => (a + b, tol_a + tol_b),
                    BinOp::Sub => (a - b, tol_a + tol_b),
                    // mul: first-order |a|*tol_b + |b|*tol_a
                    BinOp::Mul => (a * b, a.abs() * tol_b + b.abs() * tol_a),
                    // div: first-order (|a|*tol_b + |b|*tol_a) / b^2
                    BinOp::Div => {
                        if b == 0.0 {
                            return Err("division by zero".into());
                        }
                        (a / b, (tol_a * b.abs() + a.abs() * tol_b) / (b * b))
                    }
                    _ => unreachable!(),
                };

                // Only carry tolerance if at least one operand had it
                let has_tol = matches!(lhs, Value::Number { tol: Some(_), .. })
                    || matches!(rhs, Value::Number { tol: Some(_), .. });

                if has_tol {
                    Ok(Value::number_with_tol(result, result_tol))
                } else {
                    Ok(Value::number(result))
                }
            }

            // Comparisons — tolerance-aware via approx_eq
            BinOp::Eq => {
                let result = lhs.approx_eq(rhs).ok_or(
                    "cannot compare these types".to_string(),
                )?;
                Ok(Value::Bool(result))
            }
            BinOp::Neq => {
                let result = lhs.approx_eq(rhs).ok_or(
                    "cannot compare these types".to_string(),
                )?;
                Ok(Value::Bool(!result))
            }

            // Ordered comparisons (exact, no tolerance)
            BinOp::Lt | BinOp::Gt | BinOp::Lte | BinOp::Gte => {
                let a = lhs.as_f32().ok_or("left operand must be a number")?;
                let b = rhs.as_f32().ok_or("right operand must be a number")?;
                let result = match op {
                    BinOp::Lt => a < b,
                    BinOp::Gt => a > b,
                    BinOp::Lte => a <= b,
                    BinOp::Gte => a >= b,
                    _ => unreachable!(),
                };
                Ok(Value::Bool(result))
            }
        }
    }
}