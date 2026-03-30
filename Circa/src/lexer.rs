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
    #[token("print")]
    Print,
    #[token("tol")]
    Tol,

    // Literals
    #[regex(r"[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?", |lex| lex.slice().parse::<f64>().ok().map(F64))]
    Number(F64),

    // Identifiers
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", priority = 1, callback = |lex| lex.slice().to_string())]
    Ident(String),

    // Variable tolerance: `let x = expr ~= 0.1` or `(0.0 ~= tol)`
    #[token("~=")]
    TolAssign,

    // Function tolerance: `f(x) ~tol 0.01` or `fn f(x) ~tol { }`
    #[token("~tol")]
    TolCall,

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
    #[token(",")]
    Comma,

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
        let input = "let a = 0.1 ~= 0.5";
        let tokens: Vec<_> = Token::lexer(input)
            .map(|t| t.unwrap())
            .collect();

        assert_eq!(tokens, vec![
            Token::Let,
            Token::Ident("a".into()),
            Token::Assign,
            Token::Number(F64(0.1)),
            Token::TolAssign,
            Token::Number(F64(0.5)),
        ]);
    }

    #[test]
    fn test_function_def() {
        let input = "fn find_root(f, lo, hi)";
        let tokens: Vec<_> = Token::lexer(input)
            .map(|t| t.unwrap())
            .collect();

        assert_eq!(tokens, vec![
            Token::Fn,
            Token::Ident("find_root".into()),
            Token::LParen,
            Token::Ident("f".into()),
            Token::Comma,
            Token::Ident("lo".into()),
            Token::Comma,
            Token::Ident("hi".into()),
            Token::RParen,
        ]);
    }

    #[test]
    fn test_tol_keyword_in_comparison() {
        let input = "f(mid) == (0.0 ~= tol)";
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
            Token::TolAssign,
            Token::Tol,
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
}