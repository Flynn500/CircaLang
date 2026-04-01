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

/// Build the full program parser (statements + expressions with mutual recursion).
///
/// expr and stmt are mutually recursive because lambda expressions (`fn(x) { body }`)
/// appear in expression position but contain statement blocks.
/// We use `Recursive::declare()` to break the cycle: declare stmt first, build block
/// and expr from it, then define stmt using the completed expr.
fn program_parser() -> impl Parser<Token, Program, Error = Simple<Token>> {
    let nl = just(Token::Newline).repeated();

    // Step 1: declare stmt so block can reference it before stmt is defined.
    let mut stmt_rec: Recursive<Token, Stmt, Simple<Token>> = Recursive::declare();

    // Step 2: build block from the declared stmt.
    let block = {
        let nl_b = nl.clone();
        let nl_b2 = nl.clone();
        nl_b.ignore_then(stmt_rec.clone())
            .then_ignore(nl_b2)
            .repeated()
            .delimited_by(just(Token::LBrace), just(Token::RBrace))
    };

    // Step 3: build expr WITH lambda support (lambda uses block).
    let block_for_expr = block.clone();

    // Used in the postfix fold to distinguish call vs index suffixes.
    enum PostfixOp {
        Call(Vec<Expr>, Option<Expr>),
        Index(Expr),
        MethodCall(String, Vec<Expr>),
    }

    let expr = recursive(move |expr| {
        let number = select! { Token::Number(n) => Expr::Number(n.0) };
        let boolean = select! {
            Token::True => Expr::Bool(true),
            Token::False => Expr::Bool(false),
        };
        let none = just(Token::None).map(|_| Expr::None);
        let ident = select! { Token::Ident(s) => Expr::Ident(s) };

        // Lambda: `fn(params) { body }` or `fn(params) ~ident { body }`
        let lambda = just(Token::Fn)
            .ignore_then(
                select! { Token::Ident(s) => s }
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .delimited_by(just(Token::LParen), just(Token::RParen)),
            )
            .then(
                just(Token::Tilde)
                    .ignore_then(select! { Token::Ident(s) => s })
                    .or_not(),
            )
            .then(block_for_expr.clone())
            .map(|((params, tol_param), body)| Expr::Lambda {
                params,
                body,
                tol_param,
            });

        // Parenthesised expr, also handles `(value ~ tolerance)`
        let paren_expr = expr
            .clone()
            .then(
                just(Token::Tilde)
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

        // expr optionally annotated with `~ tol` — reused in vec literals, call args, and method args
        let tol_expr = expr
            .clone()
            .then(
                just(Token::Tilde)
                    .ignore_then(expr.clone())
                    .or_not(),
            )
            .map(|(value, tol)| match tol {
                Some(t) => Expr::WithTolerance {
                    value: Box::new(value),
                    tolerance: Box::new(t),
                },
                None => value,
            });

        let vec_literal = tol_expr
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map(Expr::VecLiteral);

        let atom = number
            .or(boolean)
            .or(none)
            .or(lambda)
            .or(ident)
            .or(paren_expr)
            .or(vec_literal);

        // Unary negation
        let unary = just(Token::Minus)
            .repeated()
            .then(atom)
            .foldr(|_op, rhs| Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(rhs),
            });

        // Function calls: `expr(args)` with optional `~ expr`
        let args = tol_expr
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .delimited_by(just(Token::LParen), just(Token::RParen));

        let call_suffix = args
            .then(
                just(Token::Tilde)
                    .ignore_then(expr.clone())
                    .or_not(),
            )
            .map(|(a, tol)| PostfixOp::Call(a, tol));

        let index_suffix = expr
            .clone()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map(PostfixOp::Index);

        let method_suffix = just(Token::Dot)
            .ignore_then(select! { Token::Ident(s) => s })
            .then(
                tol_expr
                    .clone()
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .delimited_by(just(Token::LParen), just(Token::RParen)),
            )
            .map(|(name, args)| PostfixOp::MethodCall(name, args));

        let postfixed = unary
            .then(call_suffix.or(index_suffix).or(method_suffix).repeated())
            .foldl(|base, op| match op {
                PostfixOp::Call(args, tol) => Expr::Call {
                    func: Box::new(base),
                    args,
                    tolerance: tol.map(Box::new),
                },
                PostfixOp::Index(idx) => Expr::Index {
                    vec: Box::new(base),
                    index: Box::new(idx),
                },
                PostfixOp::MethodCall(method, args) => Expr::MethodCall {
                    receiver: Box::new(base),
                    method,
                    args,
                },
            });

        // Mul / Div
        let op = just(Token::Star)
            .to(BinOp::Mul)
            .or(just(Token::Slash).to(BinOp::Div));

        let product = postfixed
            .clone()
            .then(op.then(postfixed).repeated())
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
    });

    // Step 4: build all statement parsers using expr and block, then define stmt_rec.

    // let name = expr  OR  let name = expr ~ tol
    let let_stmt = just(Token::Let)
        .ignore_then(select! { Token::Ident(s) => s })
        .then_ignore(just(Token::Assign))
        .then(expr.clone())
        .then(
            just(Token::Tilde)
                .ignore_then(expr.clone())
                .or_not(),
        )
        .map(|((name, value), tolerance)| Stmt::Let { name, value, tolerance });

    // return expr
    let return_stmt = just(Token::Return)
        .ignore_then(expr.clone())
        .map(|value| Stmt::Return { value });

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

    // fn name(params) { body }  OR  fn name(params) ~ident { body }
    let fn_def = just(Token::Fn)
        .ignore_then(select! { Token::Ident(s) => s })
        .then(
            select! { Token::Ident(s) => s }
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .delimited_by(just(Token::LParen), just(Token::RParen)),
        )
        .then(
            just(Token::Tilde)
                .ignore_then(select! { Token::Ident(s) => s })
                .or_not(),
        )
        .then(block.clone())
        .map(|(((name, params), tol_param), body)| Stmt::FnDef {
            name,
            params,
            body,
            tol_param,
        });

    // loop { body }
    let loop_stmt = just(Token::Loop)
        .ignore_then(block)
        .map(|body| Stmt::Loop { body });

    // break
    let break_stmt = just(Token::Break).map(|_| Stmt::Break);

    // name = expr  (reassignment, no `let`)
    let assign_stmt = select! { Token::Ident(s) => s }
        .then_ignore(just(Token::Assign))
        .then(expr.clone())
        .map(|(name, value)| Stmt::Assign { name, value });

    let expr_stmt = expr.clone().map(Stmt::ExprStmt);

    stmt_rec.define(
        let_stmt
            .or(return_stmt)
            .or(if_stmt)
            .or(fn_def)
            .or(loop_stmt)
            .or(break_stmt)
            .or(assign_stmt)
            .or(expr_stmt),
    );

    // Step 5: full program = newline-separated statements.
    let nl2 = nl.clone();
    nl.ignore_then(
        stmt_rec
            .then_ignore(nl2.clone())
            .repeated()
    )
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
        let prog = parse_str("let a = 0.1 ~ 0.5");
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
        // print is now a builtin function, not a statement keyword
        let prog = parse_str("print(42)");
        assert_eq!(prog.len(), 1);
        assert!(matches!(&prog[0], Stmt::ExprStmt(Expr::Call { .. })));
    }

    #[test]
    fn test_call_with_tolerance() {
        // `let r = f(1.0, 2.0) ~ 0.01`
        // The ~ is consumed by call_suffix, so the Call holds the tolerance.
        let prog = parse_str("let r = f(1.0, 2.0) ~ 0.01");
        match &prog[0] {
            Stmt::Let { name, value, tolerance } => {
                assert_eq!(name, "r");
                assert!(tolerance.is_none());
                assert!(matches!(value, Expr::Call { tolerance: Some(_), .. }));
            }
            other => panic!("expected Let, got {:?}", other),
        }
    }

    #[test]
    fn test_comparison_with_tolerance() {
        let prog = parse_str("let x = a == (0.0 ~ tol)");
        assert_eq!(prog.len(), 1);
    }

    #[test]
    fn test_multiline_program() {
        let src = "let a = 0.1 ~ 0.5\nlet b = 0.2 ~ 0.5\nprint(a == b)";
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