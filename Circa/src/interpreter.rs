use std::rc::Rc;

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
    pub env: Env,
    /// Lightweight call stack for error reporting — just function names.
    pub call_stack: Vec<String>,
}

impl Interpreter {
    /// Create an interpreter with no builtins registered.
    /// Use this when the resolver handles builtin registration.
    pub fn new() -> Self {
        Interpreter {
            env: Env::new(),
            call_stack: Vec::new(),
        }
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
            Stmt::Let { name, value, .. } => {
                let result = self.eval_expr(value)?;
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

            Stmt::FnDef { name, params, body, tol_param, .. } => {
                let param_names: Vec<String> = params.iter().map(|(n, _)| n.clone()).collect();
                let func = Value::Func {
                    name: Rc::from(name.as_str()),
                    params: Rc::from(param_names.as_slice()),
                    body: Rc::from(body.as_slice()),
                    tol_param: tol_param.as_deref().map(Rc::from),
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

            Stmt::StructDef { name, fields, methods } => {
                let mut method_values = Vec::new();
                for m in methods {
                    match m {
                        Stmt::FnDef { name, params, body, tol_param, .. } => {
                            let param_names: Vec<String> = params.iter().map(|(n, _)| n.clone()).collect();
                            let func = Value::Func {
                                name: Rc::from(name.as_str()),
                                params: Rc::from(param_names.as_slice()),
                                body: Rc::from(body.as_slice()),
                                tol_param: tol_param.as_deref().map(Rc::from),
                            };
                            method_values.push((name.clone(), func));
                        }
                        _ => return Err("struct body must contain only field declarations and methods".into()),
                    }
                }
                let field_names: Vec<String> = fields.iter().map(|(n, _)| n.clone()).collect();
                let def = Value::StructDef {
                    name: Rc::from(name.as_str()),
                    fields: Rc::from(field_names.as_slice()),
                    methods: Rc::from(method_values),
                };
                self.env.define(name.clone(), def);
                Ok(None)
            }

            Stmt::Break => Ok(Some(Signal::Break)),

            // Imports are resolved before interpretation; none should remain.
            Stmt::Import { .. } => unreachable!("unresolved import"),

            Stmt::ExprStmt(expr) => {
                self.eval_expr(expr)?;
                Ok(None)
            }
        }
    }

    /// Evaluate an expression to a Value.
    fn eval_expr(&mut self, expr: &Expr) -> Result<Value, String> {
        match expr {
            Expr::Number(n) => Ok(Value::number(*n)),

            Expr::Integer(i) => Ok(Value::Integer(*i)),

            Expr::StringLiteral(s) => Ok(Value::String(s.clone())),

            Expr::Bool(b) => Ok(Value::Bool(*b)),

            Expr::None => Ok(Value::None),

            Expr::Ident(name) => {
                self.env
                    .get(name)
                    .cloned()
                    .ok_or(format!("undefined variable: {}", name))
            }

            Expr::Unary { expr: inner, .. } => {
                let val = self.eval_expr(inner)?;
                match val {
                    Value::Number { val, tol } => {
                        Ok(Value::Number { val: -val, tol })
                    }
                    Value::Integer(i) => Ok(Value::Integer(-i)),
                    _ => Err("cannot negate non-number".into()),
                }
            }

            Expr::BinOp { left, op, right } => {
                let lhs = self.eval_expr(left)?;
                let rhs = self.eval_expr(right)?;
                self.eval_binop(&lhs, op, &rhs)
            }

            Expr::StructInit { name, fields: init_fields } => {
                let def = self.env.get(name)
                    .cloned()
                    .ok_or(format!("undefined struct: {}", name))?;

                match def {
                    Value::StructDef { fields: def_fields, name: sname, .. } => {
                        let mut field_values: Vec<(String, Value)> = Vec::new();

                        // Start with all fields defaulting to None
                        for f in def_fields.iter() {
                            field_values.push((f.clone(), Value::None));
                        }

                        // Apply provided values
                        for (fname, fexpr) in init_fields {
                            let val = self.eval_expr(fexpr)?;
                            let entry = field_values.iter_mut().find(|(k, _)| k == fname);
                            match entry {
                                Some(e) => e.1 = val,
                                None => return Err(format!(
                                    "{}: unknown field '{}'", sname, fname
                                )),
                            }
                        }

                        Ok(Value::StructInstance {
                            struct_name: sname,
                            fields: field_values,
                        })
                    }
                    other => Err(format!("{} is not a struct", other)),
                }
            }

            Expr::FieldAccess { object, field } => {
                let val = self.eval_expr(object)?;
                match val {
                    Value::StructInstance { fields, .. } => {
                        fields.iter()
                            .find(|(k, _)| k == field)
                            .map(|(_, v)| v.clone())
                            .ok_or(format!("no field '{}' on struct", field))
                    }
                    other => Err(format!("{} has no fields", other)),
                }
            }

            Expr::Call { func, args, tolerance } => {
                let caller_tol = match tolerance {
                    Some(tol_expr) => {
                        let tol_val = self.eval_expr(tol_expr)?;
                        match tol_val {
                            Value::None => None,
                            other => Some(other.as_f64().ok_or(
                                "tolerance must be a number or None".to_string(),
                            )?),
                        }
                    }
                    None => None,
                };
                self.eval_call(func, args, caller_tol)
            }

            Expr::Lambda { params, body, tol_param, .. } => {
                let param_names: Vec<String> = params.iter().map(|(n, _)| n.clone()).collect();
                Ok(Value::Func {
                    name: Rc::from("<lambda>"),
                    params: Rc::from(param_names.as_slice()),
                    body: Rc::from(body.as_slice()),
                    tol_param: tol_param.as_deref().map(Rc::from),
                })
            }

            Expr::WithTolerance { value, tolerance } => {
                let val = self.eval_expr(value)?;
                let tol = self.eval_expr(tolerance)?;
                match tol {
                    Value::None => Ok(val),
                    _ => {
                        let tol_f64 = tol
                            .as_f64()
                            .ok_or("tolerance must be a number or None".to_string())?;
                        match val {
                            Value::Number { val, .. } => Ok(Value::number_with_tol(val, tol_f64)),
                            Value::Integer(i) => Ok(Value::number_with_tol(i as f64, tol_f64)),
                            _ => Err("cannot apply tolerance to non-number".into()),
                        }
                    }
                }
            }
            Expr::VecLiteral(elements) => {
                let values = elements
                    .iter()
                    .map(|e| self.eval_expr(e))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Value::Vector(values))
            }

            Expr::MethodCall { receiver, method, args } => {
                self.eval_method_call(receiver, method, args)
            }

            Expr::Index { vec, index } => {
                let vec_val = self.eval_expr(vec)?;
                let idx_val = self.eval_expr(index)?;

                let idx_f = idx_val
                    .as_f64()
                    .ok_or_else(|| "index must be a number".to_string())?;

                if idx_f < 0.0 {
                    return Err(format!("index {} is negative", idx_f));
                }
                let idx = idx_f as usize;

                match vec_val {
                    Value::Vector(elems) => {
                        let len = elems.len();
                        elems.get(idx)
                            .cloned()
                            .ok_or_else(|| format!("index {} out of bounds (len {})", idx, len))
                    }
                    Value::String(s) => {
                        let len = s.len();
                        s.as_bytes().get(idx)
                            .map(|&b| Value::String((b as char).to_string()))
                            .ok_or_else(|| format!("index {} out of bounds (len {})", idx, len))
                    }
                    other => Err(format!("cannot index into {}", other)),
                }
            }
        }
    }

    /// Evaluate a function call, optionally injecting a tol value.
    fn eval_call(
        &mut self,
        func_expr: &Expr,
        arg_exprs: &[Expr],
        caller_tol: Option<f64>,
    ) -> Result<Value, String> {
        let func = self.eval_expr(func_expr)?;
        let args: Vec<Value> = arg_exprs
            .iter()
            .map(|a| self.eval_expr(a))
            .collect::<Result<_, _>>()?;

        self.call_value(func, args, caller_tol)
    }

    /// Call a function value with pre-evaluated arguments.
    fn call_value(
        &mut self,
        func: Value,
        args: Vec<Value>,
        caller_tol: Option<f64>,
    ) -> Result<Value, String> {
        match func {
            Value::Func { params, body, name, tol_param } => {
                if args.len() != params.len() {
                    return Err(format!(
                        "{}: expected {} args, got {}",
                        name, params.len(), args.len()
                    ));
                }

                if caller_tol.is_some() && tol_param.is_none() {
                    return Err(format!(
                        "{}: does not accept a tolerance argument (~ not in signature)",
                        name
                    ));
                }

                // Track call stack for error reporting
                self.call_stack.push(name.to_string());

                self.env.push_scope();

                // Bind parameters
                for (param, arg) in params.iter().zip(args) {
                    self.env.define(param.clone(), arg);
                }

                // Inject tolerance under the declared param name
                if let Some(ref param_name) = tol_param {
                    let tol_val = match caller_tol {
                        Some(t) => Value::number(t),
                        None => Value::None,
                    };
                    self.env.define(param_name.as_ref().to_string(), tol_val);
                }

                // Execute body
                let mut result = Value::Bool(false); // default return
                for s in body.iter() {
                    match self.exec_stmt(s)? {
                        Some(Signal::Return(val)) => {
                            result = val;
                            break;
                        }
                        Some(Signal::Break) => {
                            self.env.pop_scope();
                            self.call_stack.pop();
                            return Err("break outside of loop".into());
                        }
                        None => {}
                    }
                }

                self.env.pop_scope();
                self.call_stack.pop();

                // Tag the return value with the caller's tolerance.
                if tol_param.is_some() {
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
                        "{}: does not accept a tolerance argument (~ not in signature)",
                        name
                    ));
                }

                // Track native calls too
                self.call_stack.push(name.to_string());

                let mut result = func(&args, caller_tol)?;

                if guarantees_tol {
                    if let Some(t) = caller_tol {
                        result = match result {
                            Value::Number { val, .. } => Value::number_with_tol(val, t),
                            other => other,
                        };
                    }
                }

                self.call_stack.pop();

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
                // String concatenation: string + string
                if let (Value::String(a), Value::String(b)) = (lhs, rhs) {
                    return match op {
                        BinOp::Add => Ok(Value::String(format!("{}{}", a, b))),
                        _ => Err("strings only support + and *".into()),
                    };
                }

                // String repetition: string * int or int * string
                if matches!(op, BinOp::Mul) {
                    match (lhs, rhs) {
                        (Value::String(s), Value::Integer(n)) |
                        (Value::Integer(n), Value::String(s)) => {
                            if *n < 0 {
                                return Err("cannot multiply string by negative".into());
                            }
                            return Ok(Value::String(s.repeat(*n as usize)));
                        }
                        _ => {}
                    }
                }

                // Pure integer arithmetic: int op int
                if let (Value::Integer(a), Value::Integer(b)) = (lhs, rhs) {
                    return match op {
                        BinOp::Add => Ok(Value::Integer(a + b)),
                        BinOp::Sub => Ok(Value::Integer(a - b)),
                        BinOp::Mul => Ok(Value::Integer(a * b)),
                        BinOp::Div => {
                            if *b == 0 { return Err("division by zero".into()); }
                            Ok(Value::Integer(a / b))
                        }
                        _ => unreachable!(),
                    };
                }

                // Float or mixed int/float — promote to f64 with tolerance
                let (a, tol_a) = match lhs {
                    Value::Number { val, tol } => (*val, tol.unwrap_or(0.0)),
                    Value::Integer(i) => (*i as f64, 0.0),
                    _ => return Err("left operand must be a number".into()),
                };
                let (b, tol_b) = match rhs {
                    Value::Number { val, tol } => (*val, tol.unwrap_or(0.0)),
                    Value::Integer(i) => (*i as f64, 0.0),
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

            // Exact equality — values must match, treat ~0 as exact
            BinOp::Eq => {
                let result = lhs.exact_eq(rhs).ok_or(
                    "cannot compare these types".to_string(),
                )?;
                Ok(Value::Bool(result))
            }
            BinOp::Neq => {
                let result = lhs.exact_eq(rhs).ok_or(
                    "cannot compare these types".to_string(),
                )?;
                Ok(Value::Bool(!result))
            }

            // Possible equality — do tolerance ranges overlap?
            BinOp::MaybeEq => {
                let result = lhs.maybe_eq(rhs).ok_or(
                    "cannot compare these types".to_string(),
                )?;
                Ok(Value::Bool(result))
            }
            BinOp::MaybeNeq => {
                let result = lhs.maybe_eq(rhs).ok_or(
                    "cannot compare these types".to_string(),
                )?;
                Ok(Value::Bool(!result))
            }

            // Definite ordered comparisons — must hold even in worst case
            BinOp::Lt | BinOp::Gt | BinOp::Lte | BinOp::Gte => {
                let (a, tol_a) = lhs.as_f64_tol().ok_or("left operand must be a number")?;
                let (b, tol_b) = rhs.as_f64_tol().ok_or("right operand must be a number")?;
                let result = match op {
                    BinOp::Lt  => (a + tol_a) < (b - tol_b),
                    BinOp::Gt  => (a - tol_a) > (b + tol_b),
                    BinOp::Lte => (a + tol_a) <= (b - tol_b),
                    BinOp::Gte => (a - tol_a) >= (b + tol_b),
                    _ => unreachable!(),
                };
                Ok(Value::Bool(result))
            }

            // Possible ordered comparisons — could be true for some value in range
            BinOp::MaybeLt | BinOp::MaybeGt | BinOp::MaybeLte | BinOp::MaybeGte => {
                let (a, tol_a) = lhs.as_f64_tol().ok_or("left operand must be a number")?;
                let (b, tol_b) = rhs.as_f64_tol().ok_or("right operand must be a number")?;
                let result = match op {
                    BinOp::MaybeLt  => (a - tol_a) < (b + tol_b),
                    BinOp::MaybeGt  => (a + tol_a) > (b - tol_b),
                    BinOp::MaybeLte => (a - tol_a) <= (b + tol_b),
                    BinOp::MaybeGte => (a + tol_a) >= (b - tol_b),
                    _ => unreachable!(),
                };
                Ok(Value::Bool(result))
            }
        }
    }

    /// Evaluate a method call on a receiver value.
    fn eval_method_call(
        &mut self,
        receiver_expr: &Expr,
        method: &str,
        arg_exprs: &[Expr],
    ) -> Result<Value, String> {
        let args: Vec<Value> = arg_exprs
            .iter()
            .map(|a| self.eval_expr(a))
            .collect::<Result<_, _>>()?;
        let receiver = self.eval_expr(receiver_expr)?;

        match &receiver {
            Value::Vector(_) => self.eval_vector_method(receiver_expr, receiver, method, args),
            Value::String(_) => self.eval_string_method(receiver, method, args),
            Value::StructInstance { struct_name, fields } => {
                // Look up the struct def to find the method
                let def = self.env.get(struct_name.as_ref())
                    .cloned()
                    .ok_or(format!("undefined struct: {}", struct_name))?;

                match def {
                    Value::StructDef { methods, .. } => {
                        let method_val = methods.iter()
                            .find(|(n, _)| n == method)
                            .map(|(_, v)| v.clone())
                            .ok_or(format!("{} has no method '{}'", struct_name, method))?;

                        // Call the method with `self` (the instance) as the first arg
                        match method_val {
                            Value::Func { params, body, name, tol_param } => {
                                if args.len() + 1 != params.len() {
                                    return Err(format!(
                                        "{}.{}: expected {} args (besides self), got {}",
                                        struct_name, method, params.len() - 1, args.len()
                                    ));
                                }

                                // Track method call
                                self.call_stack.push(format!("{}.{}", struct_name, method));

                                self.env.push_scope();

                                // Bind self as first param
                                self.env.define(params[0].clone(), receiver);

                                // Bind remaining params
                                for (param, arg) in params[1..].iter().zip(args) {
                                    self.env.define(param.clone(), arg);
                                }

                                if let Some(ref pname) = tol_param {
                                    self.env.define(pname.as_ref().to_string(), Value::None);
                                }

                                let mut result = Value::Bool(false);
                                for s in body.iter() {
                                    match self.exec_stmt(s)? {
                                        Some(Signal::Return(val)) => {
                                            result = val;
                                            break;
                                        }
                                        Some(Signal::Break) => {
                                            self.env.pop_scope();
                                            self.call_stack.pop();
                                            return Err("break outside of loop".into());
                                        }
                                        None => {}
                                    }
                                }

                                self.env.pop_scope();
                                self.call_stack.pop();
                                Ok(result)
                            }
                            _ => Err(format!("{}.{} is not a function", struct_name, method)),
                        }
                    }
                    _ => Err(format!("{} is not a struct", struct_name)),
                }
            }
            other => Err(format!("{} has no methods", other)),
        }
    }

    /// Handle methods on vectors: push, pop, append, clear, len.
    fn eval_vector_method(
        &mut self,
        receiver_expr: &Expr,
        receiver: Value,
        method: &str,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        match method {
            "push" => {
                if args.len() != 1 {
                    return Err("push: expected 1 argument".into());
                }
                let mut elems = match receiver {
                    Value::Vector(v) => v,
                    _ => unreachable!(),
                };
                elems.push(args.into_iter().next().unwrap());
                self.reassign_receiver(receiver_expr, Value::Vector(elems))?;
                Ok(Value::Bool(false))
            }
            "extend" => {
                if args.len() != 1 {
                    return Err("append: expected 1 argument".into());
                }
                let mut elems = match receiver {
                    Value::Vector(v) => v,
                    _ => unreachable!(),
                };
                let other = match args.into_iter().next().unwrap() {
                    Value::Vector(v) => v,
                    other => return Err(format!("append: expected a vector, got {}", other)),
                };
                elems.extend(other);
                self.reassign_receiver(receiver_expr, Value::Vector(elems))?;
                Ok(Value::Bool(false))
            }
            "pop" => {
                if !args.is_empty() {
                    return Err("pop: expected 0 arguments".into());
                }
                let mut elems = match receiver {
                    Value::Vector(v) => v,
                    _ => unreachable!(),
                };
                let val = elems.pop().ok_or("pop: vector is empty".to_string())?;
                self.reassign_receiver(receiver_expr, Value::Vector(elems))?;
                Ok(val)
            }
            "clear" => {
                if !args.is_empty() {
                    return Err("clear: expected 0 arguments".into());
                }
                self.reassign_receiver(receiver_expr, Value::Vector(Vec::new()))?;
                Ok(Value::Bool(false))
            }
            "len" => {
                if !args.is_empty() {
                    return Err("len: expected 0 arguments".into());
                }
                let elems = match &receiver {
                    Value::Vector(v) => v,
                    _ => unreachable!(),
                };
                Ok(Value::number(elems.len()as f64))
            }
            "map" => {
                if args.len() != 1 {
                    return Err("map: expected 1 argument (a function)".into());
                }
                let elems = match receiver {
                    Value::Vector(v) => v,
                    _ => unreachable!(),
                };
                let func = args.into_iter().next().unwrap();
                let mut result = Vec::with_capacity(elems.len());
                for elem in elems {
                    let mapped = self.call_value(func.clone(), vec![elem], None)?;
                    result.push(mapped);
                }
                Ok(Value::Vector(result))
            }
            "fold" => {
                if args.len() != 2 {
                    return Err("fold: expected 2 arguments (initial value, function)".into());
                }
                let elems = match receiver {
                    Value::Vector(v) => v,
                    _ => unreachable!(),
                };
                let mut args_iter = args.into_iter();
                let mut acc = args_iter.next().unwrap();
                let func = args_iter.next().unwrap();
                for elem in elems {
                    acc = self.call_value(func.clone(), vec![acc, elem], None)?;
                }
                Ok(acc)
            }
            "filter" => {
                if args.len() != 1 {
                    return Err("filter: expected 1 argument (a function)".into());
                }
                let elems = match receiver {
                    Value::Vector(v) => v,
                    _ => unreachable!(),
                };
                let func = args.into_iter().next().unwrap();
                let mut result = Vec::new();
                for elem in elems {
                    let keep = self.call_value(func.clone(), vec![elem.clone()], None)?;
                    let keep_bool = keep.as_bool().ok_or(
                        "filter: predicate must return a bool-like value".to_string(),
                    )?;
                    if keep_bool {
                        result.push(elem);
                    }
                }
                Ok(Value::Vector(result))
            }
            "zip" => {
                if args.len() != 1 {
                    return Err("zip: expected 1 argument (a vector)".into());
                }
                let elems = match receiver {
                    Value::Vector(v) => v,
                    _ => unreachable!(),
                };
                let other = match args.into_iter().next().unwrap() {
                    Value::Vector(v) => v,
                    other => return Err(format!("zip: expected a vector, got {}", other)),
                };
                let result = elems.into_iter()
                    .zip(other)
                    .map(|(a, b)| Value::Vector(vec![a, b]))
                    .collect();
                Ok(Value::Vector(result))
            }
            other => Err(format!("vector has no method '{}'", other)),
        }
    }

    /// Write a mutated value back to the receiver variable.
    fn reassign_receiver(&mut self, receiver_expr: &Expr, new_val: Value) -> Result<(), String> {
        match receiver_expr {
            Expr::Ident(name) => {
                if !self.env.assign(name, new_val) {
                    Err(format!("undefined variable: {}", name))
                } else {
                    Ok(())
                }
            }
            _ => Err("cannot mutate a temporary value; assign to a variable first".into()),
        }
    }

    /// Handle methods on strings: len, slice, upper, lower, trim, contains, split.
    fn eval_string_method(
        &self,
        receiver: Value,
        method: &str,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        let s = match &receiver {
            Value::String(s) => s,
            _ => unreachable!(),
        };

        match method {
            "len" => {
                if !args.is_empty() { return Err("len: expected 0 arguments".into()); }
                Ok(Value::Integer(s.len() as i64))
            }
            "upper" => {
                if !args.is_empty() { return Err("upper: expected 0 arguments".into()); }
                Ok(Value::String(s.to_uppercase()))
            }
            "lower" => {
                if !args.is_empty() { return Err("lower: expected 0 arguments".into()); }
                Ok(Value::String(s.to_lowercase()))
            }
            "trim" => {
                if !args.is_empty() { return Err("trim: expected 0 arguments".into()); }
                Ok(Value::String(s.trim().to_string()))
            }
            "contains" => {
                if args.len() != 1 { return Err("contains: expected 1 argument".into()); }
                match &args[0] {
                    Value::String(sub) => Ok(Value::Bool(s.contains(sub.as_str()))),
                    other => Err(format!("contains: expected a string, got {}", other)),
                }
            }
            "split" => {
                if args.len() != 1 { return Err("split: expected 1 argument".into()); }
                match &args[0] {
                    Value::String(delim) => {
                        let parts: Vec<Value> = s.split(delim.as_str())
                            .map(|p| Value::String(p.to_string()))
                            .collect();
                        Ok(Value::Vector(parts))
                    }
                    other => Err(format!("split: expected a string, got {}", other)),
                }
            }
            other => Err(format!("string has no method '{}'", other)),
        }
    }
}