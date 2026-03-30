use chumsky::prelude::*;
use crate::ast::*;
use crate::lexer::Token;

/// Span type used throughout the parser.
pub type Span = std::ops::Range<usize>;

/// A spanned token for chumsky input.
type Spanned = (Token, Span);

/// Convert a logos lexer output into the spanned token vec chumsky expects.
pub fn lex(src: &str) -> Result<Vec<Spanned>, Vec<Simple<char>>> {
    use logos::Logos;
    let mut tokens = Vec::new();
    let mut lex = Token::lexer(src);
    while let Some(tok) = lex.next() {
        match tok {
            Ok(t) => tokens.push((t, lex.span())),
            Err(()) => {
                return Err(vec![Simple::custom(
                    lex.span(),
                    "unexpected character",
                )]);
            }
        }
    }
    Ok(tokens)
}

/// Build the expression parser.
fn expr_parser() -> impl Parser<Token, Expr, Error = Simple<Token>> + Clone {
    recursive(|expr| {
        let number = select! { Token::Number(n) => Expr::Number(n.0) };
        let boolean = select! {
            Token::True => Expr::Bool(true),
            Token::False => Expr::Bool(false),
        };
        let ident = select! { Token::Ident(s) => Expr::Ident(s) };
        let tol = just(Token::Tol).to(Expr::Tol);

        // Parenthesised expr, also handles `(value ~= tolerance)`
        let paren_expr = expr
            .clone()
            .then(
                just(Token::TolAssign)
                    .ignore_then(expr.clone())
                    .or_not(),
            )
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .map(|(value, tol)| match tol {
                Some(t) => Expr::WithTolerance {
                    value: Box::new(value),
                    tolerance: Box::new(t),
                },
                None => value,
            });

        let atom = number
            .or(boolean)
            .or(tol)
            .or(ident)
            .or(paren_expr);

        // Unary negation
        let unary = just(Token::Minus)
            .repeated()
            .then(atom)
            .foldr(|_op, rhs| Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(rhs),
            });

        // Function calls: `expr(args)` with optional `~= tol`
        let args = expr
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .delimited_by(just(Token::LParen), just(Token::RParen));

        let call = unary
            .then(
                args.then(
                    just(Token::TolCall)
                        .ignore_then(expr.clone())
                        .or_not(),
                )
                .repeated(),
            )
            .foldl(|func, (args, tol)| Expr::Call {
                func: Box::new(func),
                args,
                tolerance: tol.map(Box::new),
            });

        // Mul / Div
        let op = just(Token::Star)
            .to(BinOp::Mul)
            .or(just(Token::Slash).to(BinOp::Div));

        let product = call
            .clone()
            .then(op.then(call).repeated())
            .foldl(|left, (op, right)| Expr::BinOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            });

        // Add / Sub
        let op = just(Token::Plus)
            .to(BinOp::Add)
            .or(just(Token::Minus).to(BinOp::Sub));

        let sum = product
            .clone()
            .then(op.then(product).repeated())
            .foldl(|left, (op, right)| Expr::BinOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            });

        // Comparison (single level, non-associative)
        let op = just(Token::Eq).to(BinOp::Eq)
            .or(just(Token::Neq).to(BinOp::Neq))
            .or(just(Token::Lte).to(BinOp::Lte))
            .or(just(Token::Gte).to(BinOp::Gte))
            .or(just(Token::Lt).to(BinOp::Lt))
            .or(just(Token::Gt).to(BinOp::Gt));

        sum.clone()
            .then(op.then(sum).or_not())
            .map(|(left, rhs)| match rhs {
                Some((op, right)) => Expr::BinOp {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
                None => left,
            })
    })
}

/// Build the full program parser (statements).
fn program_parser() -> impl Parser<Token, Program, Error = Simple<Token>> {
    let expr = expr_parser();
    let nl = just(Token::Newline).repeated();

    // let name = expr  OR  let name = expr ~= tol
    let let_stmt = just(Token::Let)
        .ignore_then(select! { Token::Ident(s) => s })
        .then_ignore(just(Token::Assign))
        .then(expr.clone())
        .then(
            just(Token::TolAssign)
                .ignore_then(expr.clone())
                .or_not(),
        )
        .map(|((name, value), tolerance)| Stmt::Let {
            name,
            value,
            tolerance,
        });

    // return expr
    let return_stmt = just(Token::Return)
        .ignore_then(expr.clone())
        .map(|value| Stmt::Return { value });

    // print(expr)
    let print_stmt = just(Token::Print)
        .ignore_then(
            expr.clone()
                .delimited_by(just(Token::LParen), just(Token::RParen)),
        )
        .map(Stmt::Print);

    // Block and statement are mutually recursive (if/fn contain blocks of stmts)
    let nl2 = nl.clone();
    recursive(move |stmt: Recursive<'_, Token, Stmt, Simple<Token>>| {
        let block = nl
            .clone()
            .ignore_then(stmt.clone())
            .then_ignore(nl.clone())
            .repeated()
            .delimited_by(just(Token::LBrace), just(Token::RBrace));

        // if cond { body } [else { body }]
        let if_stmt = just(Token::If)
            .ignore_then(expr.clone())
            .then(block.clone())
            .then(
                just(Token::Else)
                    .ignore_then(block.clone())
                    .or_not(),
            )
            .map(|((condition, then_body), else_body)| Stmt::If {
                condition,
                then_body,
                else_body,
            });

        // fn name(params) { body }  OR  fn name(params) ~tol { body }
        let fn_def = just(Token::Fn)
            .ignore_then(select! { Token::Ident(s) => s })
            .then(
                select! { Token::Ident(s) => s }
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .delimited_by(just(Token::LParen), just(Token::RParen)),
            )
            .then(just(Token::TolCall).or_not())
            .then(block)
            .map(|(((name, params), has_tol), body)| Stmt::FnDef {
                name,
                params,
                body,
                guarantees_tol: has_tol.is_some(),
            });

        let expr_stmt = expr
            .clone()
            .map(|value| Stmt::ExprStmt(value));

        let_stmt
            .clone()
            .or(return_stmt.clone())
            .or(print_stmt.clone())
            .or(if_stmt)
            .or(fn_def)
            .or(expr_stmt)
    })
    .then_ignore(nl2.clone())
    .repeated()
    .then_ignore(nl2)
    .then_ignore(end())
}

/// Parse a full program from tokens.
pub fn parse(tokens: Vec<Spanned>) -> Result<Program, Vec<Simple<Token>>> {
    let len = tokens.last().map(|(_, s)| s.end).unwrap_or(0);
    program_parser().parse(chumsky::Stream::from_iter(
        len..len + 1,
        tokens.into_iter(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(src: &str) -> Program {
        let tokens = lex(src).expect("lex failed");
        parse(tokens).expect("parse failed")
    }

    #[test]
    fn test_let_with_tolerance() {
        let prog = parse_str("let a = 0.1 ~= 0.5");
        assert_eq!(prog.len(), 1);
        match &prog[0] {
            Stmt::Let { name, tolerance, .. } => {
                assert_eq!(name, "a");
                assert!(tolerance.is_some());
            }
            other => panic!("expected Let, got {:?}", other),
        }
    }

    #[test]
    fn test_let_without_tolerance() {
        let prog = parse_str("let x = 42");
        match &prog[0] {
            Stmt::Let { name, tolerance, .. } => {
                assert_eq!(name, "x");
                assert!(tolerance.is_none());
            }
            other => panic!("expected Let, got {:?}", other),
        }
    }

    #[test]
    fn test_fn_def() {
        let prog = parse_str("fn add(a, b) {\n  return a + b\n}");
        assert_eq!(prog.len(), 1);
        match &prog[0] {
            Stmt::FnDef { name, params, body, .. } => {
                assert_eq!(name, "add");
                assert_eq!(params, &["a", "b"]);
                assert_eq!(body.len(), 1);
            }
            other => panic!("expected FnDef, got {:?}", other),
        }
    }

    #[test]
    fn test_print() {
        let prog = parse_str("print(42)");
        assert_eq!(prog.len(), 1);
        assert!(matches!(&prog[0], Stmt::Print(_)));
    }

    #[test]
    fn test_call_with_tolerance() {
        // `let r = f(1.0, 2.0) ~= 0.01`
        // The ~= is parsed on the let statement. The interpreter will:
        // 1. Pass tol=0.01 into f's scope
        // 2. Tag the resulting variable r with tolerance 0.01
        let prog = parse_str("let r = f(1.0, 2.0) ~= 0.01");
        match &prog[0] {
            Stmt::Let { name, value, tolerance } => {
                assert_eq!(name, "r");
                assert!(tolerance.is_some());
                assert!(matches!(value, Expr::Call { .. }));
            }
            other => panic!("expected Let, got {:?}", other),
        }
    }

    #[test]
    fn test_comparison_with_tolerance() {
        let prog = parse_str("let x = a == (0.0 ~= tol)");
        assert_eq!(prog.len(), 1);
    }

    #[test]
    fn test_multiline_program() {
        let src = "let a = 0.1 ~= 0.5\nlet b = 0.2 ~= 0.5\nprint(a == b)";
        let prog = parse_str(src);
        assert_eq!(prog.len(), 3);
    }

    #[test]
    fn test_if_else() {
        let src = "if x > 0 {\n  print(x)\n} else {\n  print(0)\n}";
        let prog = parse_str(src);
        assert_eq!(prog.len(), 1);
        match &prog[0] {
            Stmt::If { else_body, .. } => assert!(else_body.is_some()),
            other => panic!("expected If, got {:?}", other),
        }
    }

    #[test]
    fn test_arithmetic_precedence() {
        let prog = parse_str("1 + 2 * 3");
        match &prog[0] {
            Stmt::ExprStmt(Expr::BinOp { op: BinOp::Add, right, .. }) => {
                assert!(matches!(right.as_ref(), Expr::BinOp { op: BinOp::Mul, .. }));
            }
            other => panic!("expected Add(_, Mul(_, _)), got {:?}", other),
        }
    }
}