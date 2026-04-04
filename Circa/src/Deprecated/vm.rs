// vm.rs

use std::rc::Rc;

use crate::bytecode::{Chunk, Constant, Function, Op};
use crate::value::Value;

struct CallFrame {
    func: Function,
    ip: usize,
    stack_base: usize,
    upvalues: Vec<Value>,
    caller_tol: Option<f64>,
}

pub struct VM {
    stack: Vec<Value>,
    frames: Vec<CallFrame>,
    preloaded: Vec<Option<Value>>,
}

enum BinOpKind {
    Add,
    Sub,
    Mul,
    Div,
}

impl VM {
    pub fn new() -> Self {
        VM {
            stack: Vec::new(),
            frames: Vec::new(),
            preloaded: Vec::new(),
        }
    }

    /// Pre-load a native function into a specific local slot.
    pub fn preload_local(&mut self, slot: usize, val: Value) {
        while self.preloaded.len() <= slot {
            self.preloaded.push(None);
        }
        self.preloaded[slot] = Some(val);
    }

    pub fn run(&mut self, chunk: &Chunk) -> Result<(), String> {
        let main_func = Function {
            name: "<main>".into(),
            arity: 0,
            chunk: chunk.clone(),
            tol_param: false,
            upvalue_count: 0,
        };

        // Pre-allocate local slots on the stack
        let local_count = main_func.chunk.locals.len();
        for i in 0..local_count {
            if let Some(Some(val)) = self.preloaded.get(i) {
                self.stack.push(val.clone());
            } else {
                self.stack.push(Value::None);
            }
        }

        self.frames.push(CallFrame {
            func: main_func,
            ip: 0,
            stack_base: 0,
            upvalues: Vec::new(),
            caller_tol: None,
        });

        self.execute()
    }

    fn execute(&mut self) -> Result<(), String> {
        self.run_loop(0)
    }

    /// Core dispatch loop. Runs until frame count drops to `stop_depth`.
    fn run_loop(&mut self, stop_depth: usize) -> Result<(), String> {
        loop {
            if self.frames.len() == stop_depth {
                return Ok(());
            }
            let fi = self.frames.len() - 1;
            let ip = self.frames[fi].ip;
            if ip >= self.frames[fi].func.chunk.code.len() {
                if fi == 0 {
                    return Ok(());
                }
                return Err("function ended without return".into());
            }
            let op = self.frames[fi].func.chunk.code[ip];
            self.frames[fi].ip = ip + 1;

            match op {
                Op::LoadConst(idx) => {
                    let fi = self.frames.len() - 1;
                    let val = const_to_value(&self.frames[fi].func.chunk.constants[idx]);
                    self.stack.push(val);
                }
                Op::LoadLocal(slot) => {
                    let base = self.frames[fi].stack_base;
                    self.stack.push(self.stack[base + slot].clone());
                }
                Op::StoreLocal(slot) => {
                    let val = self.pop()?;
                    let base = self.frames[fi].stack_base;
                    self.stack[base + slot] = val;
                }
                Op::Pop => {
                    self.pop()?;
                }
                Op::Dup => {
                    let val = self.stack.last().ok_or("stack underflow")?.clone();
                    self.stack.push(val);
                }

                // Arithmetic
                Op::Add => self.binary_op(BinOpKind::Add)?,
                Op::Sub => self.binary_op(BinOpKind::Sub)?,
                Op::Mul => self.binary_op(BinOpKind::Mul)?,
                Op::Div => self.binary_op(BinOpKind::Div)?,
                Op::Neg => {
                    let val = self.pop()?;
                    match val {
                        Value::Number { val, tol } => {
                            self.stack.push(Value::Number { val: -val, tol });
                        }
                        Value::Integer(i) => self.stack.push(Value::Integer(-i)),
                        _ => return Err("cannot negate non-number".into()),
                    }
                }

                // Tolerance
                Op::WithTol => {
                    let tol = self.pop()?;
                    let val = self.pop()?;
                    let tol_f64 = tol.as_f64().ok_or("tolerance must be a number")?;
                    match val {
                        Value::Number { val, .. } => {
                            self.stack.push(Value::number_with_tol(val, tol_f64));
                        }
                        Value::Integer(i) => {
                            self.stack.push(Value::number_with_tol(i as f64, tol_f64));
                        }
                        _ => return Err("cannot apply tolerance to non-number".into()),
                    }
                }

                // Closures
                Op::LoadUpvalue(slot) => {
                    let val = self.frames[fi].upvalues[slot].clone();
                    self.stack.push(val);
                }
                Op::StoreUpvalue(slot) => {
                    let val = self.pop()?;
                    self.frames[fi].upvalues[slot] = val;
                }
                Op::MakeClosure(func_idx, upvalue_count) => {
                    let fi = self.frames.len() - 1;
                    let func = match &self.frames[fi].func.chunk.constants[func_idx] {
                        Constant::Func(f) => Rc::new(f.clone()),
                        _ => return Err("MakeClosure: expected function constant".into()),
                    };
                    let count = upvalue_count as usize;
                    let start = self.stack.len() - count;
                    let upvalues: Vec<Value> = self.stack.drain(start..).collect();
                    self.stack.push(Value::Closure {
                        func,
                        upvalues: Rc::new(upvalues),
                    });
                }

                // Comparisons — exact
                Op::Eq => {
                    let rhs = self.pop()?;
                    let lhs = self.pop()?;
                    let result =
                        lhs.exact_eq(&rhs).ok_or("cannot compare these types")?;
                    self.stack.push(Value::Bool(result));
                }
                Op::Neq => {
                    let rhs = self.pop()?;
                    let lhs = self.pop()?;
                    let result =
                        lhs.exact_eq(&rhs).ok_or("cannot compare these types")?;
                    self.stack.push(Value::Bool(!result));
                }

                // Comparisons — maybe
                Op::MaybeEq => {
                    let rhs = self.pop()?;
                    let lhs = self.pop()?;
                    let result =
                        lhs.maybe_eq(&rhs).ok_or("cannot compare these types")?;
                    self.stack.push(Value::Bool(result));
                }
                Op::MaybeNeq => {
                    let rhs = self.pop()?;
                    let lhs = self.pop()?;
                    let result =
                        lhs.maybe_eq(&rhs).ok_or("cannot compare these types")?;
                    self.stack.push(Value::Bool(!result));
                }

                // Comparisons — definite ordered
                Op::Lt | Op::Gt | Op::Lte | Op::Gte => {
                    let rhs = self.pop()?;
                    let lhs = self.pop()?;
                    let (a, ta) =
                        lhs.as_f64_tol().ok_or("left operand must be a number")?;
                    let (b, tb) =
                        rhs.as_f64_tol().ok_or("right operand must be a number")?;
                    let result = match op {
                        Op::Lt => (a + ta) < (b - tb),
                        Op::Gt => (a - ta) > (b + tb),
                        Op::Lte => (a + ta) <= (b - tb),
                        Op::Gte => (a - ta) >= (b + tb),
                        _ => unreachable!(),
                    };
                    self.stack.push(Value::Bool(result));
                }

                // Comparisons — maybe ordered
                Op::MaybeLt | Op::MaybeGt | Op::MaybeLte | Op::MaybeGte => {
                    let rhs = self.pop()?;
                    let lhs = self.pop()?;
                    let (a, ta) =
                        lhs.as_f64_tol().ok_or("left operand must be a number")?;
                    let (b, tb) =
                        rhs.as_f64_tol().ok_or("right operand must be a number")?;
                    let result = match op {
                        Op::MaybeLt => (a - ta) < (b + tb),
                        Op::MaybeGt => (a + ta) > (b - tb),
                        Op::MaybeLte => (a - ta) <= (b + tb),
                        Op::MaybeGte => (a + ta) >= (b - tb),
                        _ => unreachable!(),
                    };
                    self.stack.push(Value::Bool(result));
                }

                // Control flow
                Op::Jump(target) => {
                    self.frames[fi].ip = target;
                }
                Op::JumpIfFalse(target) => {
                    let val = self.pop()?;
                    let truthy = val.as_bool().ok_or("condition must be bool-like")?;
                    if !truthy {
                        self.frames[fi].ip = target;
                    }
                }

                // Function calls
                Op::Call(argc) => self.call_function(argc, None)?,
                Op::CallWithTol(argc) => {
                    let tol = self.pop()?;
                    let tol_f64 = tol.as_f64().ok_or("tolerance must be a number")?;
                    self.call_function(argc, Some(tol_f64))?;
                }

                Op::Return => {
                    let mut result = self.pop()?;
                    let frame = self.frames.pop().unwrap();
                    self.stack.truncate(frame.stack_base);

                    if self.frames.is_empty() {
                        return Ok(());
                    }

                    // Tag return value with caller tolerance if applicable
                    if frame.func.tol_param {
                        if let Some(t) = frame.caller_tol {
                            result = match result {
                                Value::Number { val, .. } => Value::number_with_tol(val, t),
                                other => other,
                            };
                        }
                    }

                    self.stack.push(result);
                }

                // Vectors
                Op::MakeVec(count) => {
                    let count = count as usize;
                    let start = self.stack.len() - count;
                    let elems: Vec<Value> = self.stack.drain(start..).collect();
                    self.stack.push(Value::Vector(elems));
                }

                Op::Index => {
                    let idx_val = self.pop()?;
                    let vec_val = self.pop()?;
                    let idx_f =
                        idx_val.as_f64().ok_or("index must be a number")?;
                    if idx_f < 0.0 {
                        return Err(format!("index {} is negative", idx_f));
                    }
                    let idx = idx_f as usize;

                    match vec_val {
                        Value::Vector(elems) => {
                            let len = elems.len();
                            let val = elems.into_iter().nth(idx).ok_or_else(|| {
                                format!("index {} out of bounds (len {})", idx, len)
                            })?;
                            self.stack.push(val);
                        }
                        Value::String(s) => {
                            let len = s.len();
                            let ch = s.as_bytes().get(idx).ok_or_else(|| {
                                format!("index {} out of bounds (len {})", idx, len)
                            })?;
                            self.stack
                                .push(Value::String((*ch as char).to_string()));
                        }
                        other => {
                            return Err(format!("cannot index into {}", other))
                        }
                    }
                }

                // Methods
                Op::CallMethod(name_idx, argc) => {
                    let argc = argc as usize;
                    let fi = self.frames.len() - 1;
                    let method_name = match &self.frames[fi].func.chunk.constants[name_idx] {
                        Constant::Str(s) => s.clone(),
                        _ => return Err("method name must be a string".into()),
                    };

                    let receiver_idx = self.stack.len() - argc - 1;
                    let receiver = self.stack[receiver_idx].clone();
                    let args: Vec<Value> =
                        self.stack[receiver_idx + 1..].to_vec();
                    self.stack.truncate(receiver_idx);

                    let result = match receiver {
                        Value::Vector(_) => {
                            self.eval_vector_method(receiver, &method_name, args)?
                        }
                        Value::String(_) => {
                            self.eval_string_method(receiver, &method_name, args)?
                        }
                        Value::StructInstance { struct_name, fields } => {
                            let def = self.find_struct_def(&struct_name)
                                .ok_or_else(|| format!("undefined struct: {}", struct_name))?;

                            let method_val = match &def {
                                Value::StructDef { methods, .. } => {
                                    methods.iter()
                                        .find(|(n, _)| n == &method_name)
                                        .map(|(_, v)| v.clone())
                                        .ok_or_else(|| format!("{} has no method '{}'", struct_name, method_name))?
                                }
                                _ => return Err(format!("{} is not a struct", struct_name)),
                            };

                            // Call the method with self as first arg
                            self.stack.push(method_val);
                            self.stack.push(Value::StructInstance {
                                struct_name,
                                fields,
                            });
                            for arg in args {
                                self.stack.push(arg);
                            }
                            self.call_function((argc + 1) as u8, None)?;
                            self.run_until_return()?
                        }
                        _ => {
                            return Err(format!("{} has no methods", receiver))
                        }
                    };

                    self.stack.push(result);
                }

                Op::NewStruct(method_count, field_count) => {
                    let _field_count = field_count as usize;
                    let method_count = method_count;

                    // Pop method functions
                    let mut method_funcs: Vec<Value> = Vec::new();
                    for _ in 0..method_count {
                        method_funcs.push(self.pop()?);
                    }
                    method_funcs.reverse();

                    // Pop methods_str, fields_str, name
                    let methods_str_val = self.pop()?;
                    let fields_str_val = self.pop()?;
                    let name_val = self.pop()?;

                    let name = match name_val {
                        Value::String(s) => s,
                        _ => return Err("NewStruct: expected string name".into()),
                    };
                    let fields_str = match fields_str_val {
                        Value::String(s) => s,
                        _ => return Err("NewStruct: expected string fields".into()),
                    };
                    let methods_str = match methods_str_val {
                        Value::String(s) => s,
                        _ => return Err("NewStruct: expected string methods".into()),
                    };

                    let fields: Vec<String> = if fields_str.is_empty() {
                        Vec::new()
                    } else {
                        fields_str.split(',').map(|s| s.to_string()).collect()
                    };

                    let method_names: Vec<String> = if methods_str.is_empty() {
                        Vec::new()
                    } else {
                        methods_str.split(',').map(|s| s.to_string()).collect()
                    };

                    let methods: Vec<(String, Value)> = method_names
                        .into_iter()
                        .zip(method_funcs)
                        .collect();

                    self.stack.push(Value::StructDef {
                        name: Rc::from(name.as_str()),
                        fields: Rc::from(fields),
                        methods: Rc::from(methods),
                    });
                }

                Op::MakeInstance(field_count) => {
                    let field_count = field_count as usize;

                    let mut init_fields: Vec<(String, Value)> = Vec::new();
                    for _ in 0..field_count {
                        let val = self.pop()?;
                        let name_val = self.pop()?;
                        let fname = match name_val {
                            Value::String(s) => s,
                            _ => return Err("MakeInstance: expected string field name".into()),
                        };
                        init_fields.push((fname, val));
                    }
                    init_fields.reverse();

                    let def = self.pop()?;
                    match def {
                        Value::StructDef { name, fields: def_fields, .. } => {
                            let mut field_values: Vec<(String, Value)> = def_fields
                                .iter()
                                .map(|f| (f.clone(), Value::None))
                                .collect();

                            for (fname, val) in init_fields {
                                let entry = field_values.iter_mut().find(|(k, _)| k == &fname);
                                match entry {
                                    Some(e) => e.1 = val,
                                    None => return Err(format!(
                                        "{}: unknown field '{}'", name, fname
                                    )),
                                }
                            }

                            self.stack.push(Value::StructInstance {
                                struct_name: name,
                                fields: field_values,
                            });
                        }
                        other => return Err(format!("{} is not a struct", other)),
                    }
                }

                Op::GetField(name_idx) => {
                    let fi = self.frames.len() - 1;
                    let field_name = match &self.frames[fi].func.chunk.constants[name_idx] {
                        Constant::Str(s) => s.clone(),
                        _ => return Err("field name must be a string".into()),
                    };

                    let val = self.pop()?;
                    match val {
                        Value::StructInstance { fields, .. } => {
                            let field_val = fields.iter()
                                .find(|(k, _)| k == &field_name)
                                .map(|(_, v)| v.clone())
                                .ok_or_else(|| format!("no field '{}' on struct", field_name))?;
                            self.stack.push(field_val);
                        }
                        other => return Err(format!("{} has no fields", other)),
                    }
                }

                _ => return Err(format!("unimplemented op: {:?}", op)),
            }
        }
    }

    fn call_function(
        &mut self,
        argc: u8,
        caller_tol: Option<f64>,
    ) -> Result<(), String> {
        let argc = argc as usize;
        let func_idx = self.stack.len() - argc - 1;
        let func_val = self.stack[func_idx].clone();

        match func_val {
            Value::NativeFunc {
                name,
                arity,
                func,
                ..
            } => {
                if argc != arity {
                    return Err(format!(
                        "{}: expected {} args, got {}",
                        name, arity, argc
                    ));
                }
                let args: Vec<Value> =
                    self.stack[func_idx + 1..].to_vec();
                self.stack.truncate(func_idx);
                let result = func(&args, caller_tol)?;
                self.stack.push(result);
                Ok(())
            }
            Value::CompiledFunc(func) => {
                if argc != func.arity as usize {
                    return Err(format!(
                        "{}: expected {} args, got {}",
                        func.name, func.arity, argc
                    ));
                }

                let stack_base = func_idx;
                // Move args down over the function value slot
                for i in 0..argc {
                    self.stack[stack_base + i] =
                        self.stack[stack_base + i + 1].clone();
                }
                self.stack.truncate(stack_base + argc);

                // Add tol_param slot if needed
                if func.tol_param {
                    let tol_val = match caller_tol {
                        Some(t) => Value::number(t),
                        None => Value::None,
                    };
                    self.stack.push(tol_val);
                }

                // Fill remaining locals with None
                let total_locals = func.chunk.locals.len();
                let current = self.stack.len() - stack_base;
                for _ in current..total_locals {
                    self.stack.push(Value::None);
                }

                self.frames.push(CallFrame {
                    func: (*func).clone(),
                    ip: 0,
                    stack_base,
                    upvalues: Vec::new(),
                    caller_tol,
                });

                Ok(())
            }
            Value::Closure { func, upvalues } => {
                if argc != func.arity as usize {
                    return Err(format!(
                        "{}: expected {} args, got {}",
                        func.name, func.arity, argc
                    ));
                }

                let stack_base = func_idx;
                for i in 0..argc {
                    self.stack[stack_base + i] =
                        self.stack[stack_base + i + 1].clone();
                }
                self.stack.truncate(stack_base + argc);

                if func.tol_param {
                    let tol_val = match caller_tol {
                        Some(t) => Value::number(t),
                        None => Value::None,
                    };
                    self.stack.push(tol_val);
                }

                let total_locals = func.chunk.locals.len();
                let current = self.stack.len() - stack_base;
                for _ in current..total_locals {
                    self.stack.push(Value::None);
                }

                self.frames.push(CallFrame {
                    func: (*func).clone(),
                    ip: 0,
                    stack_base,
                    upvalues: (*upvalues).clone(),
                    caller_tol,
                });

                Ok(())
            }
            other => Err(format!("{} is not callable", other)),
        }
    }

    /// Execute until the frame count drops back to target, returning the result.
    fn run_until_return(&mut self) -> Result<Value, String> {
        let target_depth = self.frames.len() - 1;

        // If call was to a native, result is already on the stack
        if self.frames.len() == target_depth {
            return self.pop();
        }

        self.run_loop(target_depth)?;
        self.pop()
    }

    fn pop(&mut self) -> Result<Value, String> {
        self.stack.pop().ok_or("stack underflow".into())
    }

    /// Search the main frame's locals for a StructDef with the given name.
    fn find_struct_def(&self, name: &str) -> Option<Value> {
        // The struct def lives as a local in the bottom-most (main) frame
        let base = self.frames.first().map(|f| f.stack_base).unwrap_or(0);
        let main_locals = &self.frames.first()?.func.chunk.locals;
        for (i, local_name) in main_locals.iter().enumerate() {
            if local_name == name {
                let val = &self.stack[base + i];
                if matches!(val, Value::StructDef { .. }) {
                    return Some(val.clone());
                }
            }
        }
        None
    }

    fn binary_op(&mut self, kind: BinOpKind) -> Result<(), String> {
        let rhs = self.pop()?;
        let lhs = self.pop()?;

        let result = match (&lhs, &rhs) {
            // String concatenation
            (Value::String(a), Value::String(b)) => match kind {
                BinOpKind::Add => Value::String(format!("{}{}", a, b)),
                _ => return Err("strings only support + and *".into()),
            },

            // String + non-string (or non-string + string) concatenation
            (Value::String(s), other) => match kind {
                BinOpKind::Add => Value::String(format!("{}{}", s, other)),
                _ => return Err("strings only support + and *".into()),
            },
            (other, Value::String(s)) => match kind {
                BinOpKind::Add => Value::String(format!("{}{}", other, s)),
                _ => return Err("strings only support + and *".into()),
            },

            // String repetition
            (Value::String(s), Value::Integer(n))
            | (Value::Integer(n), Value::String(s)) => match kind {
                BinOpKind::Mul => {
                    if *n < 0 {
                        return Err(
                            "cannot multiply string by negative".into()
                        );
                    }
                    Value::String(s.repeat(*n as usize))
                }
                _ => return Err("strings only support + and *".into()),
            },

            // Pure integer
            (Value::Integer(a), Value::Integer(b)) => match kind {
                BinOpKind::Add => Value::Integer(a + b),
                BinOpKind::Sub => Value::Integer(a - b),
                BinOpKind::Mul => Value::Integer(a * b),
                BinOpKind::Div => {
                    if *b == 0 {
                        return Err("division by zero".into());
                    }
                    Value::Integer(a / b)
                }
            },

            // Float or mixed
            _ => {
                let (a, ta) = numeric_parts(&lhs)?;
                let (b, tb) = numeric_parts(&rhs)?;

                let (result, result_tol) = match kind {
                    BinOpKind::Add => (a + b, ta + tb),
                    BinOpKind::Sub => (a - b, ta + tb),
                    BinOpKind::Mul => {
                        (a * b, a.abs() * tb + b.abs() * ta)
                    }
                    BinOpKind::Div => {
                        if b == 0.0 {
                            return Err("division by zero".into());
                        }
                        (
                            a / b,
                            (ta * b.abs() + a.abs() * tb) / (b * b),
                        )
                    }
                };

                let has_tol = matches!(
                    lhs,
                    Value::Number { tol: Some(_), .. }
                ) || matches!(
                    rhs,
                    Value::Number { tol: Some(_), .. }
                );

                if has_tol {
                    Value::number_with_tol(result, result_tol)
                } else {
                    Value::number(result)
                }
            }
        };

        self.stack.push(result);
        Ok(())
    }

    fn eval_vector_method(
        &mut self,
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
                Ok(Value::Vector(elems))
            }
            "extend" => {
                if args.len() != 1 {
                    return Err("extend: expected 1 argument".into());
                }
                let mut elems = match receiver {
                    Value::Vector(v) => v,
                    _ => unreachable!(),
                };
                let other = match args.into_iter().next().unwrap() {
                    Value::Vector(v) => v,
                    other => {
                        return Err(format!(
                            "extend: expected a vector, got {}",
                            other
                        ))
                    }
                };
                elems.extend(other);
                Ok(Value::Vector(elems))
            }
            "pop" => {
                if !args.is_empty() {
                    return Err("pop: expected 0 arguments".into());
                }
                let mut elems = match receiver {
                    Value::Vector(v) => v,
                    _ => unreachable!(),
                };
                let _val =
                    elems.pop().ok_or("pop: vector is empty".to_string())?;
                // Returns mutated vec; popped value is lost for now.
                // TODO: handle pop return value properly
                Ok(Value::Vector(elems))
            }
            "clear" => {
                if !args.is_empty() {
                    return Err("clear: expected 0 arguments".into());
                }
                Ok(Value::Vector(Vec::new()))
            }
            "len" => {
                if !args.is_empty() {
                    return Err("len: expected 0 arguments".into());
                }
                let elems = match &receiver {
                    Value::Vector(v) => v,
                    _ => unreachable!(),
                };
                Ok(Value::Integer(elems.len() as i64))
            }
            "map" => {
                if args.len() != 1 {
                    return Err(
                        "map: expected 1 argument (a function)".into()
                    );
                }
                let elems = match receiver {
                    Value::Vector(v) => v,
                    _ => unreachable!(),
                };
                let func = args.into_iter().next().unwrap();
                let mut result = Vec::with_capacity(elems.len());
                for elem in elems {
                    self.stack.push(func.clone());
                    self.stack.push(elem);
                    self.call_function(1, None)?;
                    let val = self.run_until_return()?;
                    result.push(val);
                }
                Ok(Value::Vector(result))
            }
            "filter" => {
                if args.len() != 1 {
                    return Err(
                        "filter: expected 1 argument (a function)".into()
                    );
                }
                let elems = match receiver {
                    Value::Vector(v) => v,
                    _ => unreachable!(),
                };
                let func = args.into_iter().next().unwrap();
                let mut result = Vec::new();
                for elem in elems {
                    self.stack.push(func.clone());
                    self.stack.push(elem.clone());
                    self.call_function(1, None)?;
                    let keep = self.run_until_return()?;
                    let keep_bool = keep
                        .as_bool()
                        .ok_or("filter: predicate must return bool-like")?;
                    if keep_bool {
                        result.push(elem);
                    }
                }
                Ok(Value::Vector(result))
            }
            "fold" => {
                if args.len() != 2 {
                    return Err(
                        "fold: expected 2 arguments (initial, function)"
                            .into(),
                    );
                }
                let elems = match receiver {
                    Value::Vector(v) => v,
                    _ => unreachable!(),
                };
                let mut args_iter = args.into_iter();
                let mut acc = args_iter.next().unwrap();
                let func = args_iter.next().unwrap();
                for elem in elems {
                    self.stack.push(func.clone());
                    self.stack.push(acc);
                    self.stack.push(elem);
                    self.call_function(2, None)?;
                    acc = self.run_until_return()?;
                }
                Ok(acc)
            }
            "zip" => {
                if args.len() != 1 {
                    return Err(
                        "zip: expected 1 argument (a vector)".into()
                    );
                }
                let elems = match receiver {
                    Value::Vector(v) => v,
                    _ => unreachable!(),
                };
                let other = match args.into_iter().next().unwrap() {
                    Value::Vector(v) => v,
                    other => {
                        return Err(format!(
                            "zip: expected a vector, got {}",
                            other
                        ))
                    }
                };
                let result = elems
                    .into_iter()
                    .zip(other)
                    .map(|(a, b)| Value::Vector(vec![a, b]))
                    .collect();
                Ok(Value::Vector(result))
            }
            other => Err(format!("vector has no method '{}'", other)),
        }
    }

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
                if !args.is_empty() {
                    return Err("len: expected 0 arguments".into());
                }
                Ok(Value::Integer(s.len() as i64))
            }
            "upper" => {
                if !args.is_empty() {
                    return Err("upper: expected 0 arguments".into());
                }
                Ok(Value::String(s.to_uppercase()))
            }
            "lower" => {
                if !args.is_empty() {
                    return Err("lower: expected 0 arguments".into());
                }
                Ok(Value::String(s.to_lowercase()))
            }
            "trim" => {
                if !args.is_empty() {
                    return Err("trim: expected 0 arguments".into());
                }
                Ok(Value::String(s.trim().to_string()))
            }
            "contains" => {
                if args.len() != 1 {
                    return Err("contains: expected 1 argument".into());
                }
                match &args[0] {
                    Value::String(sub) => {
                        Ok(Value::Bool(s.contains(sub.as_str())))
                    }
                    other => Err(format!(
                        "contains: expected a string, got {}",
                        other
                    )),
                }
            }
            "split" => {
                if args.len() != 1 {
                    return Err("split: expected 1 argument".into());
                }
                match &args[0] {
                    Value::String(delim) => {
                        let parts: Vec<Value> = s
                            .split(delim.as_str())
                            .map(|p| Value::String(p.to_string()))
                            .collect();
                        Ok(Value::Vector(parts))
                    }
                    other => Err(format!(
                        "split: expected a string, got {}",
                        other
                    )),
                }
            }
            other => Err(format!("string has no method '{}'", other)),
        }
    }
}

fn const_to_value(c: &Constant) -> Value {
    match c {
        Constant::Float(f) => Value::number(*f),
        Constant::Int(i) => Value::Integer(*i),
        Constant::Str(s) => Value::String(s.clone()),
        Constant::Bool(b) => Value::Bool(*b),
        Constant::None => Value::None,
        Constant::Func(f) => Value::CompiledFunc(Rc::new(f.clone())),
    }
}

fn numeric_parts(v: &Value) -> Result<(f64, f64), String> {
    match v {
        Value::Number { val, tol } => Ok((*val, tol.unwrap_or(0.0))),
        Value::Integer(i) => Ok((*i as f64, 0.0)),
        _ => Err("operand must be a number".into()),
    }
}