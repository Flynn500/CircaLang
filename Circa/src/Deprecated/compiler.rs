// compiler.rs

use crate::ast::{BinOp, Expr, Stmt, UnaryOp};
use crate::bytecode::{Chunk, Constant, Function, Op};

pub struct Compiler {
    pub chunk: Chunk,
    break_patches: Vec<usize>,
    /// Stack of ancestor local lists, innermost (direct parent) first.
    ancestor_locals: Vec<Vec<String>>,
    /// Upvalues captured from ancestor scopes. Each entry is
    /// (ancestor_depth, slot_in_that_ancestor). depth 0 = direct parent.
    upvalues: Vec<(usize, usize)>,
}

impl Compiler {
    pub fn new() -> Self {
        Compiler {
            chunk: Chunk::new(),
            break_patches: Vec::new(),
            ancestor_locals: Vec::new(),
            upvalues: Vec::new(),
        }
    }

    fn new_with_ancestors(ancestors: Vec<Vec<String>>) -> Self {
        Compiler {
            chunk: Chunk::new(),
            break_patches: Vec::new(),
            ancestor_locals: ancestors,
            upvalues: Vec::new(),
        }
    }

    /// Build the ancestor chain for a child compiler: our own locals
    /// become the first entry, followed by our ancestors.
    fn child_ancestors(&self) -> Vec<Vec<String>> {
        let mut ancestors = vec![self.chunk.locals.clone()];
        ancestors.extend(self.ancestor_locals.clone());
        ancestors
    }

    /// Ensure we have an upvalue for a given (depth, slot) in our own
    /// ancestor chain. Returns the upvalue index in our own upvalues list.
    fn ensure_upvalue(&mut self, depth: usize, slot: usize) -> usize {
        // Check if we already have this upvalue
        for (i, &(d, s)) in self.upvalues.iter().enumerate() {
            if d == depth && s == slot {
                return i;
            }
        }
        let idx = self.upvalues.len();
        self.upvalues.push((depth, slot));
        idx
    }

    /// Try to resolve a name as a local. If not found, walk ancestor
    /// scopes and register as an upvalue. Returns the Op to emit.
    fn resolve_variable(&mut self, name: &str) -> Op {
        // Check own locals first
        if let Some(idx) = self.chunk.locals.iter().position(|n| n == name) {
            return Op::LoadLocal(idx);
        }
        // Check if already captured as upvalue
        for (uv_idx, &(depth, slot)) in self.upvalues.iter().enumerate() {
            if self.ancestor_locals[depth][slot] == name {
                return Op::LoadUpvalue(uv_idx);
            }
        }
        // Walk ancestors to find and capture
        for (depth, ancestor) in self.ancestor_locals.iter().enumerate() {
            if let Some(slot) = ancestor.iter().position(|n| n == name) {
                let uv_idx = self.upvalues.len();
                self.upvalues.push((depth, slot));
                return Op::LoadUpvalue(uv_idx);
            }
        }
        panic!("undefined variable: {}", name);
    }

    /// Register a builtin name so it gets a local slot.
    /// Call this before compiling any statements.
    pub fn register_builtin(&mut self, name: &str) {
        self.chunk.resolve_local(name);
    }

    /// Pre-register all top-level function (and struct) names so they can be
    /// referenced before their definition. Call before compiling statements.
    pub fn register_toplevel_names(&mut self, program: &[Stmt]) {
        for stmt in program {
            match stmt {
                Stmt::FnDef { name, .. } => {
                    self.chunk.resolve_local(name);
                }
                Stmt::StructDef { name, .. } => {
                    self.chunk.resolve_local(name);
                }
                _ => {}
            }
        }
    }

    /// Emit a jump with a placeholder offset. Returns the index to patch later.
    fn emit_jump(&mut self, op: Op) -> usize {
        self.chunk.emit(op)
    }

    /// Patch a previously emitted jump to point to the current instruction.
    fn patch_jump(&mut self, idx: usize) {
        let target = self.chunk.code.len();
        match &mut self.chunk.code[idx] {
            Op::Jump(ref mut dest) | Op::JumpIfFalse(ref mut dest) => {
                *dest = target;
            }
            _ => panic!("tried to patch non-jump instruction"),
        }
    }

    pub fn compile_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, value, tolerance } => {
                self.compile_expr(value);
                if let Some(tol_expr) = tolerance {
                    self.compile_expr(tol_expr);
                    self.chunk.emit(Op::WithTol);
                }
                let slot = self.chunk.resolve_local(name);
                self.chunk.emit(Op::StoreLocal(slot));
            }

            Stmt::Assign { name, value } => {
                self.compile_expr(value);
                let slot = self.chunk.locals.iter().position(|n| n == name);
                match slot {
                    Some(idx) => {
                        self.chunk.emit(Op::StoreLocal(idx));
                    }
                    None => panic!("undefined variable: {}", name),
                }
            }

            Stmt::ExprStmt(expr) => {
                self.compile_expr(expr);
                self.chunk.emit(Op::Pop);
            }

            Stmt::If { condition, then_body, else_body } => {
                self.compile_expr(condition);
                let jump_to_else = self.emit_jump(Op::JumpIfFalse(0));

                for s in then_body {
                    self.compile_stmt(s);
                }

                if let Some(else_stmts) = else_body {
                    let jump_over_else = self.emit_jump(Op::Jump(0));
                    self.patch_jump(jump_to_else);
                    for s in else_stmts {
                        self.compile_stmt(s);
                    }
                    self.patch_jump(jump_over_else);
                } else {
                    self.patch_jump(jump_to_else);
                }
            }

            Stmt::Loop { body } => {
                let loop_start = self.chunk.code.len();
                let prev_break_patches = std::mem::take(&mut self.break_patches);

                for s in body {
                    self.compile_stmt(s);
                }

                self.chunk.emit(Op::Jump(loop_start));

                let loop_end = self.chunk.code.len();
                for idx in &self.break_patches {
                    if let Op::Jump(ref mut dest) = self.chunk.code[*idx] {
                        *dest = loop_end;
                    }
                }
                self.break_patches = prev_break_patches;
            }

            Stmt::Break => {
                let idx = self.emit_jump(Op::Jump(0));
                self.break_patches.push(idx);
            }

            Stmt::Return { value } => {
                self.compile_expr(value);
                self.chunk.emit(Op::Return);
            }

            Stmt::FnDef { name, params, body, tol_param } => {
                let mut fn_compiler = Compiler::new_with_ancestors(
                    self.child_ancestors(),
                );

                // Define parameter slots in the function's chunk
                for p in params {
                    fn_compiler.chunk.resolve_local(p);
                }

                // If tol_param, add it as a local too
                if let Some(tp) = tol_param {
                    fn_compiler.chunk.resolve_local(tp);
                }

                for s in body {
                    fn_compiler.compile_stmt(s);
                }

                // Implicit return None if body doesn't end with return
                let needs_return =
                    !matches!(fn_compiler.chunk.code.last(), Some(Op::Return));
                if needs_return {
                    let idx = fn_compiler.chunk.add_constant(Constant::None);
                    fn_compiler.chunk.emit(Op::LoadConst(idx));
                    fn_compiler.chunk.emit(Op::Return);
                }

                let upvalue_count = fn_compiler.upvalues.len();
                let captured_slots = fn_compiler.upvalues.clone();

                let func = Function {
                    name: name.clone(),
                    arity: params.len() as u8,
                    chunk: fn_compiler.chunk,
                    tol_param: tol_param.is_some(),
                    upvalue_count,
                };

                let func_idx = self.chunk.constants.len();
                self.chunk.constants.push(Constant::Func(func));

                if upvalue_count > 0 {
                    for &(depth, slot) in &captured_slots {
                        if depth == 0 {
                            // Captured from our own locals
                            self.chunk.emit(Op::LoadLocal(slot));
                        } else {
                            // Captured from a grandparent — we need to
                            // capture it ourselves first as an upvalue
                            let our_uv = self.ensure_upvalue(depth - 1, slot);
                            self.chunk.emit(Op::LoadUpvalue(our_uv));
                        }
                    }
                    self.chunk.emit(Op::MakeClosure(func_idx, upvalue_count as u8));
                } else {
                    self.chunk.emit(Op::LoadConst(func_idx));
                }

                let slot = self.chunk.resolve_local(name);
                self.chunk.emit(Op::StoreLocal(slot));
            }

            Stmt::StructDef { name, fields, methods } => {
                // Compile each method as a Function constant
                let mut method_entries: Vec<(String, usize)> = Vec::new();
                for m in methods {
                    match m {
                        Stmt::FnDef { name: mname, params, body, tol_param } => {
                            let mut fn_compiler = Compiler::new_with_ancestors(
                                self.child_ancestors(),
                            );

                            for p in params {
                                fn_compiler.chunk.resolve_local(p);
                            }
                            if let Some(tp) = tol_param {
                                fn_compiler.chunk.resolve_local(tp);
                            }

                            for s in body {
                                fn_compiler.compile_stmt(s);
                            }

                            let needs_return =
                                !matches!(fn_compiler.chunk.code.last(), Some(Op::Return));
                            if needs_return {
                                let idx = fn_compiler.chunk.add_constant(Constant::None);
                                fn_compiler.chunk.emit(Op::LoadConst(idx));
                                fn_compiler.chunk.emit(Op::Return);
                            }

                            let func = Function {
                                name: mname.clone(),
                                arity: params.len() as u8,
                                chunk: fn_compiler.chunk,
                                tol_param: tol_param.is_some(),
                                upvalue_count: 0,
                            };

                            let func_idx = self.chunk.add_constant(Constant::Func(func));
                            method_entries.push((mname.clone(), func_idx));
                        }
                        _ => {}
                    }
                }

                // Build the StructDef value at runtime:
                // Push field names as a constant, method funcs, then emit NewStruct
                let name_idx = self.chunk.add_constant(Constant::Str(name.clone()));

                // Store field names as a comma-separated string constant
                let fields_str = fields.join(",");
                let fields_idx = self.chunk.add_constant(Constant::Str(fields_str));

                // Store method names+func_idx pairs as string constant "name1,name2,..."
                // and push the compiled functions onto the stack
                let method_names: Vec<String> = method_entries.iter().map(|(n, _)| n.clone()).collect();
                let methods_str = method_names.join(",");
                let methods_idx = self.chunk.add_constant(Constant::Str(methods_str));

                // Emit: LoadConst(name), LoadConst(fields), LoadConst(methods_str)
                // Then load each method function
                self.chunk.emit(Op::LoadConst(name_idx));
                self.chunk.emit(Op::LoadConst(fields_idx));
                self.chunk.emit(Op::LoadConst(methods_idx));

                for &(_, func_idx) in &method_entries {
                    self.chunk.emit(Op::LoadConst(func_idx));
                }

                self.chunk.emit(Op::NewStruct(method_entries.len() as usize, fields.len() as u8));

                let slot = self.chunk.resolve_local(name);
                self.chunk.emit(Op::StoreLocal(slot));
            }

            Stmt::Import { .. } => unreachable!("unresolved import"),

            _ => todo!("compile_stmt: {:?}", stmt),
        }
    }

    pub fn compile_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Number(n) => {
                let idx = self.chunk.add_constant(Constant::Float(*n));
                self.chunk.emit(Op::LoadConst(idx));
            }
            Expr::Integer(i) => {
                let idx = self.chunk.add_constant(Constant::Int(*i));
                self.chunk.emit(Op::LoadConst(idx));
            }
            Expr::StringLiteral(s) => {
                let idx = self.chunk.add_constant(Constant::Str(s.clone()));
                self.chunk.emit(Op::LoadConst(idx));
            }
            Expr::Bool(b) => {
                let idx = self.chunk.add_constant(Constant::Bool(*b));
                self.chunk.emit(Op::LoadConst(idx));
            }
            Expr::None => {
                let idx = self.chunk.add_constant(Constant::None);
                self.chunk.emit(Op::LoadConst(idx));
            }
            Expr::Ident(name) => {
                let op = self.resolve_variable(name);
                self.chunk.emit(op);
            }
            Expr::Unary { op: UnaryOp::Neg, expr } => {
                self.compile_expr(expr);
                self.chunk.emit(Op::Neg);
            }
            Expr::BinOp { left, op, right } => {
                self.compile_expr(left);
                self.compile_expr(right);
                let instruction = match op {
                    BinOp::Add => Op::Add,
                    BinOp::Sub => Op::Sub,
                    BinOp::Mul => Op::Mul,
                    BinOp::Div => Op::Div,
                    BinOp::Eq => Op::Eq,
                    BinOp::Neq => Op::Neq,
                    BinOp::Lt => Op::Lt,
                    BinOp::Gt => Op::Gt,
                    BinOp::Lte => Op::Lte,
                    BinOp::Gte => Op::Gte,
                    BinOp::MaybeEq => Op::MaybeEq,
                    BinOp::MaybeNeq => Op::MaybeNeq,
                    BinOp::MaybeLt => Op::MaybeLt,
                    BinOp::MaybeGt => Op::MaybeGt,
                    BinOp::MaybeLte => Op::MaybeLte,
                    BinOp::MaybeGte => Op::MaybeGte,
                };
                self.chunk.emit(instruction);
            }
            Expr::Call { func, args, tolerance } => {
                self.compile_expr(func);
                for arg in args {
                    self.compile_expr(arg);
                }
                if let Some(tol_expr) = tolerance {
                    self.compile_expr(tol_expr);
                    self.chunk.emit(Op::CallWithTol(args.len() as u8));
                } else {
                    self.chunk.emit(Op::Call(args.len() as u8));
                }
            }
            Expr::WithTolerance { value, tolerance } => {
                self.compile_expr(value);
                self.compile_expr(tolerance);
                self.chunk.emit(Op::WithTol);
            }
            Expr::Lambda { params, body, tol_param } => {
                let mut fn_compiler = Compiler::new_with_ancestors(
                    self.child_ancestors(),
                );

                for p in params {
                    fn_compiler.chunk.resolve_local(p);
                }
                if let Some(tp) = tol_param {
                    fn_compiler.chunk.resolve_local(tp);
                }

                for s in body {
                    fn_compiler.compile_stmt(s);
                }

                let needs_return =
                    !matches!(fn_compiler.chunk.code.last(), Some(Op::Return));
                if needs_return {
                    let idx = fn_compiler.chunk.add_constant(Constant::None);
                    fn_compiler.chunk.emit(Op::LoadConst(idx));
                    fn_compiler.chunk.emit(Op::Return);
                }

                let upvalue_count = fn_compiler.upvalues.len();
                let captured_slots = fn_compiler.upvalues.clone();

                let func = Function {
                    name: "<lambda>".to_string(),
                    arity: params.len() as u8,
                    chunk: fn_compiler.chunk,
                    tol_param: tol_param.is_some(),
                    upvalue_count,
                };

                let func_idx = self.chunk.constants.len();
                self.chunk.constants.push(Constant::Func(func));

                if upvalue_count > 0 {
                    for &(depth, slot) in &captured_slots {
                        if depth == 0 {
                            self.chunk.emit(Op::LoadLocal(slot));
                        } else {
                            let our_uv = self.ensure_upvalue(depth - 1, slot);
                            self.chunk.emit(Op::LoadUpvalue(our_uv));
                        }
                    }
                    self.chunk.emit(Op::MakeClosure(func_idx, upvalue_count as u8));
                } else {
                    self.chunk.emit(Op::LoadConst(func_idx));
                }
            }
            Expr::VecLiteral(elements) => {
                for elem in elements {
                    self.compile_expr(elem);
                }
                self.chunk.emit(Op::MakeVec(elements.len() as u8));
            }
            Expr::Index { vec, index } => {
                self.compile_expr(vec);
                self.compile_expr(index);
                self.chunk.emit(Op::Index);
            }
            Expr::MethodCall { receiver, method, args } => {
                self.compile_expr(receiver);
                for arg in args {
                    self.compile_expr(arg);
                }
                let name_idx = self.chunk.add_constant(Constant::Str(method.clone()));
                let is_mutating =
                    matches!(method.as_str(), "push" | "pop" | "extend" | "clear");
                self.chunk.emit(Op::CallMethod(name_idx, args.len() as u8));

                // Mutating methods return the modified collection.
                // Write it back to the receiver's local slot.
                if is_mutating {
                    if let Expr::Ident(name) = receiver.as_ref() {
                        let slot = self
                            .chunk
                            .locals
                            .iter()
                            .position(|n| n == name)
                            .expect("undefined variable for mutating method");
                        self.chunk.emit(Op::StoreLocal(slot));
                    }
                }
            }
            Expr::StructInit { name, fields: init_fields } => {
                // Load the struct def
                let op = self.resolve_variable(name);
                self.chunk.emit(op);

                // Push each field name and value
                for (fname, fexpr) in init_fields {
                    let name_idx = self.chunk.add_constant(Constant::Str(fname.clone()));
                    self.chunk.emit(Op::LoadConst(name_idx));
                    self.compile_expr(fexpr);
                }

                // Emit MakeInstance with field count
                self.chunk.emit(Op::MakeInstance(init_fields.len() as u8));
            }
            Expr::FieldAccess { object, field } => {
                self.compile_expr(object);
                let name_idx = self.chunk.add_constant(Constant::Str(field.clone()));
                self.chunk.emit(Op::GetField(name_idx));
            }
            _ => todo!("compile_expr: {:?}", expr),
        }
    }
}