/// A complete Circa program is a list of statements.
pub type Program = Vec<Stmt>;

/// Statements
#[derive(Debug, Clone)]
pub enum Stmt {
    /// `let x = expr` or `let x = expr ~= tol_expr`
    Let {
        name: String,
        value: Expr,
        tolerance: Option<Expr>,
    },

    /// `fn name(params) { body }` or `fn name(params) ~= tol { body }`
    /// When `guarantees_tol` is true, the caller's tol is applied to the return value.
    FnDef {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
        guarantees_tol: bool,
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

    /// `print(expr)`
    Print(Expr),

    /// An expression used as a statement (e.g. a bare function call)
    ExprStmt(Expr),
}

/// Expressions
#[derive(Debug, Clone)]
pub enum Expr {
    /// Numeric literal: `0.1`, `1E-10`
    Number(f64),

    /// Boolean literal
    Bool(bool),

    /// Variable reference
    Ident(String),

    /// The `tol` keyword — resolves to the caller-provided tolerance
    Tol,

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

    /// Function call: `f(args)` or `f(args) ~= tol_expr`
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
}

#[derive(Debug, Clone)]
pub enum UnaryOp {
    Neg,
}