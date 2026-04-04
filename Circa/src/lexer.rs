use logos::Logos;
use std::hash::{Hash, Hasher};

/// Wrapper around f64 that implements Eq and Hash (via bit representation).
/// This is safe for our use case — tokens are compared for identity, not arithmetic.
#[derive(Debug, Clone, Copy)]
pub struct F64(pub f64);

impl PartialEq for F64 {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}
impl Eq for F64 {}

impl Hash for F64 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

#[derive(Logos, Debug, PartialEq, Eq, Hash, Clone)]
#[logos(skip r"[ \t\r]+")]
pub enum Token {
    // Keywords
    #[token("let")]
    Let,
    #[token("const")]
    Const,
    #[token("fn")]
    Fn,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("return")]
    Return,
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("loop")]
    Loop,
    #[token("break")]
    Break,
    #[token("None")]
    None,

    #[token("import")]
    Import,

    #[token("new")]
    New,
    #[token("struct")]
    Struct,

    // Type keywords
    #[token("int")]
    TyInt,
    #[token("float")]
    TyFloat,
    #[token("bool")]
    TyBool,
    #[token("string")]
    TyString,

    // Literals
    #[regex(r"[0-9]+[eE][+-]?[0-9]+", |lex| lex.slice().parse::<f64>().ok().map(F64), priority = 3)]
    #[regex(r"[0-9]+\.[0-9]+([eE][+-]?[0-9]+)?", |lex| lex.slice().parse::<f64>().ok().map(F64), priority = 3)]
    Number(F64),

    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok(), priority = 2)]
    Integer(i64),

    #[regex(r#""[^"]*""#, |lex| { let s = lex.slice(); Some(s[1..s.len()-1].to_string()) })]
    StringLit(String),

    // Identifiers
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", priority = 1, callback = |lex| lex.slice().to_string())]
    Ident(String),

    // Tolerance operator: `let x = expr ~ 0.1`, `f(x) ~ 0.01`, `fn f(x) ~t { }`
    #[token("~")]
    Tilde,

    // Comparison operators
    #[token("==")]
    Eq,
    #[token("!=")]
    Neq,
    #[token("<=")]
    Lte,
    #[token(">=")]
    Gte,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,

    // Possible comparison operators
    #[token("?=")]
    MaybeEq,
    #[token("?!=")]
    MaybeNeq,
    #[token("!?=")]
    MaybeNeq2,
    #[token("?>")]
    MaybeGt,
    #[token("?<")]
    MaybeLt,
    #[token("?>=")]
    MaybeGte,
    #[token("?<=")]
    MaybeLte,

    // Arithmetic
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,

    // Assignment & delimiters
    #[token("=")]
    Assign,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(",")]
    Comma,
    #[token(".")]
    Dot,
    #[token(":")]
    Colon,
    #[token("->")]
    Arrow,

    // Newlines are significant (statement terminators)
    #[token("\n")]
    Newline,

    // Comments
    #[regex(r"//[^\n]*", logos::skip)]
    Comment,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tolerance_binding() {
        let input = "let a = 0.1 ~ 0.5";
        let tokens: Vec<_> = Token::lexer(input)
            .map(|t| t.unwrap())
            .collect();

        assert_eq!(tokens, vec![
            Token::Let,
            Token::Ident("a".into()),
            Token::Assign,
            Token::Number(F64(0.1)),
            Token::Tilde,
            Token::Number(F64(0.5)),
        ]);
    }

    #[test]
    fn test_function_def() {
        let input = "fn find_root(f: float, lo: float) -> float";
        let tokens: Vec<_> = Token::lexer(input)
            .map(|t| t.unwrap())
            .collect();

        assert_eq!(tokens, vec![
            Token::Fn,
            Token::Ident("find_root".into()),
            Token::LParen,
            Token::Ident("f".into()),
            Token::Colon,
            Token::TyFloat,
            Token::Comma,
            Token::Ident("lo".into()),
            Token::Colon,
            Token::TyFloat,
            Token::RParen,
            Token::Arrow,
            Token::TyFloat,
        ]);
    }

    #[test]
    fn test_tol_keyword_in_comparison() {
        let input = "f(mid) == (0.0 ~ tol)";
        let tokens: Vec<_> = Token::lexer(input)
            .map(|t| t.unwrap())
            .collect();

        assert_eq!(tokens, vec![
            Token::Ident("f".into()),
            Token::LParen,
            Token::Ident("mid".into()),
            Token::RParen,
            Token::Eq,
            Token::LParen,
            Token::Number(F64(0.0)),
            Token::Tilde,
            Token::Ident("tol".into()),
            Token::RParen,
        ]);
    }

    #[test]
    fn test_scientific_notation() {
        let input = "1E-10";
        let tokens: Vec<_> = Token::lexer(input)
            .map(|t| t.unwrap())
            .collect();

        assert_eq!(tokens, vec![Token::Number(F64(1e-10))]);
    }

    #[test]
    fn test_integer_literal() {
        let input = "42";
        let tokens: Vec<_> = Token::lexer(input)
            .map(|t| t.unwrap())
            .collect();

        assert_eq!(tokens, vec![Token::Integer(42)]);
    }

    #[test]
    fn test_string_literal() {
        let input = r#""hello world""#;
        let tokens: Vec<_> = Token::lexer(input)
            .map(|t| t.unwrap())
            .collect();

        assert_eq!(tokens, vec![Token::StringLit("hello world".into())]);
    }

    #[test]
    fn test_const_binding() {
        let input = "const a: float = 1.0 ~ 0.1";
        let tokens: Vec<_> = Token::lexer(input)
            .map(|t| t.unwrap())
            .collect();

        assert_eq!(tokens, vec![
            Token::Const,
            Token::Ident("a".into()),
            Token::Colon,
            Token::TyFloat,
            Token::Assign,
            Token::Number(F64(1.0)),
            Token::Tilde,
            Token::Number(F64(0.1)),
        ]);
    }

    #[test]
    fn test_callable_type() {
        let input = "fn diff(f: fn(float) -> float, x: float) -> float";
        let tokens: Vec<_> = Token::lexer(input)
            .map(|t| t.unwrap())
            .collect();

        assert_eq!(tokens, vec![
            Token::Fn,
            Token::Ident("diff".into()),
            Token::LParen,
            Token::Ident("f".into()),
            Token::Colon,
            Token::Fn,
            Token::LParen,
            Token::TyFloat,
            Token::RParen,
            Token::Arrow,
            Token::TyFloat,
            Token::Comma,
            Token::Ident("x".into()),
            Token::Colon,
            Token::TyFloat,
            Token::RParen,
            Token::Arrow,
            Token::TyFloat,
        ]);
    }
}