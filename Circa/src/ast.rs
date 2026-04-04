/// A complete Circa program is a list of statements.
pub type Program = Vec<Stmt>;

/// Type annotations in Circa.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeAnno {
    Int,
    Float,
    Bool,
    Str,
    None,
    /// Internal-only helper for builtins that accept any vector shape.
    /// Not produced by the parser.
    AnyVec,
    /// Internal-only optional type, primarily for tolerance bindings.
    Optional(Box<TypeAnno>),
    /// Homogeneous vector: `[float]`, `[int]`, etc.
    Vec(Box<TypeAnno>),
    /// Callable: `fn(float, float) -> float`
    Fn {
        params: Vec<TypeAnno>,
        ret: Box<TypeAnno>,
    },
    /// A user-defined struct type, referenced by name.
    Named(String),
}

/// Statements
#[derive(Debug, Clone)]
pub enum Stmt {
    /// `let x = expr` or `const x: float = expr`
    /// `mutable`: false for `const`, true for `let`
    Let {
        name: String,
        type_anno: Option<TypeAnno>,
        value: Expr,
        mutable: bool,
    },

    /// `fn name(x: float, y: float) ~tol -> float { body }`
    /// Params require explicit annotations during typecheck/interp construction.
    /// Return type defaults to None if omitted.
    FnDef {
        name: String,
        params: Vec<(String, Option<TypeAnno>)>,
        body: Vec<Stmt>,
        tol_param: Option<String>,
        return_type: TypeAnno,
    },

    StructDef {
        name: String,
        fields: Vec<(String, Option<TypeAnno>)>,
        methods: Vec<Stmt>,
    },

    /// `return expr`
    Return {
        value: Expr,
    },

    /// `if cond { body } else { else_body }`
    If {
        condition: Expr,
        then_body: Vec<Stmt>,
        else_body: Option<Vec<Stmt>>,
    },

    /// `a = expr` — reassign an existing variable
    Assign {
        name: String,
        value: Expr,
    },

    /// `loop { body }` — runs forever until a `break` is hit
    Loop { body: Vec<Stmt> },

    /// `break` — exits the innermost loop
    Break,

    /// `import name` — pull in a module (stdlib or local file)
    Import { name: String },

    /// An expression used as a statement (e.g. a bare function call)
    ExprStmt(Expr),
}

/// Expressions
#[derive(Debug, Clone)]
pub enum Expr {
    /// Numeric literal: `0.1`, `1E-10`
    Number(f64),

    /// Integer literal: `42`, `0`
    Integer(i64),

    /// String literal: `"hello"`
    StringLiteral(String),

    /// Boolean literal
    Bool(bool),

    /// None literal
    None,

    /// Variable reference
    Ident(String),

    /// Binary operation: `a + b`, `a == b`, etc.
    BinOp {
        left: Box<Expr>,
        op: BinOp,
        right: Box<Expr>,
    },

    /// Unary negation: `-x`
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },

    /// Function call: `f(args)` or `f(args) ~= tol`
    Call {
        func: Box<Expr>,
        args: Vec<Expr>,
        tolerance: Option<Box<Expr>>,
    },

    /// Tolerance-annotated expression used inline: `(0.0 ~= tol)`
    /// This pairs a value with its tolerance for comparisons.
    WithTolerance {
        value: Box<Expr>,
        tolerance: Box<Expr>,
    },

    /// Anonymous function: `fn(x: float) -> float { body }`
    /// Param types may be absent in the AST but are rejected during checking.
    Lambda {
        params: Vec<(String, Option<TypeAnno>)>,
        body: Vec<Stmt>,
        tol_param: Option<String>,
        return_type: Option<TypeAnno>,
    },

    /// Vector literal: `[e1, e2, e3]`
    /// Each element may be a `WithTolerance` node if written as `e ~= tol`.
    VecLiteral(Vec<Expr>),

    /// Index into a vector: `v[i]`
    Index {
        vec: Box<Expr>,
        index: Box<Expr>,
    },

    /// Method call: `receiver.method(args)`
    MethodCall {
        receiver: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },

    /// Struct instantiation: `Foo { a = 1.0, b = 2.0 }`
    StructInit {
        name: String,
        fields: Vec<(String, Expr)>,
    },

    /// Field access: `instance.field`
    FieldAccess {
        object: Box<Expr>,
        field: String,
    },
}

#[derive(Debug, Clone)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Neq,
    Lt,
    Gt,
    Lte,
    Gte,
    // "Possible" comparisons — true if tolerance ranges allow it
    MaybeEq,
    MaybeNeq,
    MaybeLt,
    MaybeGt,
    MaybeLte,
    MaybeGte,
}

#[derive(Debug, Clone)]
pub enum UnaryOp {
    Neg,
}
