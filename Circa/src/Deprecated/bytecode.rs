// bytecode.rs

#[derive(Debug, Clone, Copy)]
pub enum Op {
    // Stack
    LoadConst(usize),
    LoadLocal(usize),
    StoreLocal(usize),
    Pop,
    Dup,

    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Neg,

    // Comparison
    Eq,
    Neq,
    Lt,
    Gt,
    Lte,
    Gte,
    MaybeEq,
    MaybeNeq,
    MaybeLt,
    MaybeGt,
    MaybeLte,
    MaybeGte,

    // Tolerance
    WithTol,

    // Control flow
    Jump(usize),
    JumpIfFalse(usize),

    // Functions
    Call(u8),
    CallWithTol(u8),
    Return,

    // Vectors
    MakeVec(u8),
    Index,

    // Methods
    CallMethod(usize, u8), // method name const idx, arg count

    // Closures
    LoadUpvalue(usize),
    StoreUpvalue(usize),
    MakeClosure(usize, u8), // func const idx, upvalue count

    // Structs
    GetField(usize),
    NewStruct(usize, u8),  // method_count, field_count
    MakeInstance(u8),      // field_count (pairs of name_const + value on stack)
}

/// A constant in the constant pool.
#[derive(Debug, Clone)]
pub enum Constant {
    Float(f64),
    Int(i64),
    Str(String),
    Bool(bool),
    None,
    Func(Function),
}

/// A compiled function.
#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub arity: u8,
    pub chunk: Chunk,
    pub tol_param: bool,
    pub upvalue_count: usize,
}

/// Compiled bytecode for one function or the top-level script.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub code: Vec<Op>,
    pub constants: Vec<Constant>,
    /// Maps local variable names to slot indices (compile-time only).
    pub locals: Vec<String>,
}

impl Chunk {
    pub fn new() -> Self {
        Chunk {
            code: Vec::new(),
            constants: Vec::new(),
            locals: Vec::new(),
        }
    }

    /// Add a constant, returning its index. Deduplicates (except functions).
    pub fn add_constant(&mut self, c: Constant) -> usize {
        for (i, existing) in self.constants.iter().enumerate() {
            if constant_eq(existing, &c) {
                return i;
            }
        }
        let idx = self.constants.len();
        self.constants.push(c);
        idx
    }

    /// Emit an instruction, returning its index.
    pub fn emit(&mut self, op: Op) -> usize {
        let idx = self.code.len();
        self.code.push(op);
        idx
    }

    /// Resolve a local variable name to a slot, creating one if new.
    pub fn resolve_local(&mut self, name: &str) -> usize {
        if let Some(i) = self.locals.iter().position(|n| n == name) {
            return i;
        }
        let idx = self.locals.len();
        self.locals.push(name.to_string());
        idx
    }
}

fn constant_eq(a: &Constant, b: &Constant) -> bool {
    match (a, b) {
        (Constant::Float(x), Constant::Float(y)) => x.to_bits() == y.to_bits(),
        (Constant::Int(x), Constant::Int(y)) => x == y,
        (Constant::Str(x), Constant::Str(y)) => x == y,
        (Constant::Bool(x), Constant::Bool(y)) => x == y,
        (Constant::None, Constant::None) => true,
        _ => false, // Functions are never deduplicated
    }
}