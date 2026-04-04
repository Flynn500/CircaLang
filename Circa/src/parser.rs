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

    // Type annotation parser (needed inside expr for lambdas)
    let type_anno_for_expr = recursive(|type_anno| {
        let base = just(Token::TyFloat).to(TypeAnno::Float)
            .or(just(Token::TyInt).to(TypeAnno::Int))
            .or(just(Token::TyBool).to(TypeAnno::Bool))
            .or(just(Token::TyString).to(TypeAnno::Str))
            .or(just(Token::None).to(TypeAnno::None))
            .or(select! { Token::Ident(s) => TypeAnno::Named(s) });

        let vec_type = type_anno.clone()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map(|inner| TypeAnno::Vec(Box::new(inner)));

        let fn_type = just(Token::Fn)
            .ignore_then(
                type_anno.clone()
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .delimited_by(just(Token::LParen), just(Token::RParen)),
            )
            .then_ignore(just(Token::Arrow))
            .then(type_anno.clone())
            .map(|(params, ret)| TypeAnno::Fn {
                params,
                ret: Box::new(ret),
            });

        fn_type.or(vec_type).or(base)
    });

    // Used in the postfix fold to distinguish call vs index suffixes.
    enum PostfixOp {
        Call(Vec<Expr>, Option<Expr>),
        Index(Expr),
        MethodCall(String, Vec<Expr>),
        FieldAccess(String),
    }

        let expr = recursive(move |expr| {
        let number = select! { Token::Number(n) => Expr::Number(n.0) };
        let integer = select! { Token::Integer(i) => Expr::Integer(i) };
        let string_lit = select! { Token::StringLit(s) => Expr::StringLiteral(s) };
        let boolean = select! {
            Token::True => Expr::Bool(true),
            Token::False => Expr::Bool(false),
        };
        let none = just(Token::None).map(|_| Expr::None);
    
        let ident = select! { Token::Ident(s) => Expr::Ident(s) };

        // Lambda typed param: `name: type` or just `name` (type is None)
        let lambda_param = select! { Token::Ident(s) => s }
            .then(
                just(Token::Colon)
                    .ignore_then(type_anno_for_expr.clone())
                    .or_not(),
            );

        // Lambda: `fn(params) { body }` or `fn(params) ~ident -> RetType { body }`
        let lambda = just(Token::Fn)
            .ignore_then(
                lambda_param
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .delimited_by(just(Token::LParen), just(Token::RParen)),
            )
            .then(
                just(Token::Tilde)
                    .ignore_then(select! { Token::Ident(s) => s })
                    .or_not(),
            )
            .then(
                just(Token::Arrow)
                    .ignore_then(type_anno_for_expr.clone())
                    .or_not(),
            )
            .then(block_for_expr.clone())
            .map(|(((params, tol_param), return_type), body)| Expr::Lambda {
                params,
                body,
                tol_param,
                return_type,
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

        let struct_init = just(Token::New)
            .ignore_then(select! { Token::Ident(s) => s })
            .then(
                select! { Token::Ident(s) => s }
                    .then_ignore(just(Token::Assign))
                    .then(tol_expr.clone())
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .delimited_by(just(Token::LBrace), just(Token::RBrace)),
            )
            .map(|(name, fields)| Expr::StructInit { name, fields });

        let vec_literal = tol_expr
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map(Expr::VecLiteral);

        let atom = number
            .or(integer)
            .or(string_lit)
            .or(boolean)
            .or(none)
            .or(lambda)
            .or(struct_init)
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

        let dot_suffix = just(Token::Dot)
            .ignore_then(select! { Token::Ident(s) => s })
            .then(
                tol_expr
                    .clone()
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .delimited_by(just(Token::LParen), just(Token::RParen))
                    .or_not(),
            )
            .map(|(name, maybe_args)| match maybe_args {
                Some(args) => PostfixOp::MethodCall(name, args),
                None => PostfixOp::FieldAccess(name),
            });

        let postfixed = unary
            .then(call_suffix.or(index_suffix).or(dot_suffix).repeated())
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
                PostfixOp::FieldAccess(field) => Expr::FieldAccess {
                    object: Box::new(base),
                    field,
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
            .or(just(Token::Gt).to(BinOp::Gt))
            .or(just(Token::MaybeEq).to(BinOp::MaybeEq))
            .or(just(Token::MaybeNeq).to(BinOp::MaybeNeq))
            .or(just(Token::MaybeNeq2).to(BinOp::MaybeNeq))
            .or(just(Token::MaybeGte).to(BinOp::MaybeGte))
            .or(just(Token::MaybeLte).to(BinOp::MaybeLte))
            .or(just(Token::MaybeGt).to(BinOp::MaybeGt))
            .or(just(Token::MaybeLt).to(BinOp::MaybeLt));

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

    // Type annotation parser: float, int, bool, string, [float], fn(float) -> float, StructName
    let type_anno = recursive(|type_anno| {
        let base = just(Token::TyFloat).to(TypeAnno::Float)
            .or(just(Token::TyInt).to(TypeAnno::Int))
            .or(just(Token::TyBool).to(TypeAnno::Bool))
            .or(just(Token::TyString).to(TypeAnno::Str))
            .or(just(Token::None).to(TypeAnno::None))
            .or(select! { Token::Ident(s) => TypeAnno::Named(s) });

        let vec_type = type_anno.clone()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map(|inner| TypeAnno::Vec(Box::new(inner)));

        let fn_type = just(Token::Fn)
            .ignore_then(
                type_anno.clone()
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .delimited_by(just(Token::LParen), just(Token::RParen)),
            )
            .then_ignore(just(Token::Arrow))
            .then(type_anno.clone())
            .map(|(params, ret)| TypeAnno::Fn {
                params,
                ret: Box::new(ret),
            });

        fn_type.or(vec_type).or(base)
    });

    // let name = expr ~ tol
    // let name: type = expr ~ tol
    // const name: type = expr ~ tol
    let let_stmt = just(Token::Let).to(true)
        .or(just(Token::Const).to(false))
        .then(select! { Token::Ident(s) => s })
        .then(
            just(Token::Colon)
                .ignore_then(type_anno.clone())
                .or_not(),
        )
        .then_ignore(just(Token::Assign))
        .then(
            expr.clone()
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
                }),
        )
        .map(|(((mutable, name), type_anno), value)| Stmt::Let {
            name,
            type_anno,
            value,
            mutable,
        });

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

    // Typed param: `name: type` or just `name` (rejected later if omitted)
    let typed_param = select! { Token::Ident(s) => s }
        .then(
            just(Token::Colon)
                .ignore_then(type_anno.clone())
                .or_not(),
        )
        .map(|(name, ty)| (name, ty));

    // fn name(params) ~ident -> RetType { body }
    let fn_def = just(Token::Fn)
        .ignore_then(select! { Token::Ident(s) => s })
        .then(
            typed_param.clone()
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .delimited_by(just(Token::LParen), just(Token::RParen)),
        )
        .then(
            just(Token::Tilde)
                .ignore_then(select! { Token::Ident(s) => s })
                .or_not(),
        )
        .then(
            just(Token::Arrow)
                .ignore_then(type_anno.clone())
                .or_not(),
        )
        .then(block.clone())
        .map(|((((name, params), tol_param), return_type), body)| Stmt::FnDef {
            name,
            params,
            body,
            tol_param,
            return_type: return_type.unwrap_or(TypeAnno::None),
        });
    
            // Field declaration inside struct: `let name` or `let name: type`
        let struct_field = just(Token::Let)
            .ignore_then(select! { Token::Ident(s) => s })
            .then(
                just(Token::Colon)
                    .ignore_then(type_anno.clone())
                    .or_not(),
            )
            .map(|(name, ty)| (name, ty));

        // Method inside struct: reuse fn_def parser shape
        let struct_method = just(Token::Fn)
            .ignore_then(select! { Token::Ident(s) => s })
            .then(
                typed_param.clone()
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .delimited_by(just(Token::LParen), just(Token::RParen)),
            )
            .then(
                just(Token::Tilde)
                    .ignore_then(select! { Token::Ident(s) => s })
                    .or_not(),
            )
            .then(
                just(Token::Arrow)
                    .ignore_then(type_anno.clone())
                    .or_not(),
            )
            .then(block.clone())
            .map(|((((name, params), tol_param), return_type), body)| Stmt::FnDef {
                name,
                params,
                body,
                tol_param,
                return_type: return_type.unwrap_or(TypeAnno::None),
            });

        // A struct member is either a field or a method
        enum StructMember {
            Field((String, Option<TypeAnno>)),
            Method(Stmt),
        }

        let struct_member = struct_field.map(StructMember::Field)
            .or(struct_method.map(StructMember::Method));

        let nl_s = nl.clone();
        let nl_s2 = nl.clone();

        let struct_def = just(Token::Struct)
            .ignore_then(select! { Token::Ident(s) => s })
            .then(
                nl_s.ignore_then(struct_member)
                    .then_ignore(nl_s2)
                    .repeated()
                    .delimited_by(just(Token::LBrace), just(Token::RBrace)),
            )
            .map(|(name, members)| {
                let mut fields = Vec::new();
                let mut methods = Vec::new();
                for m in members {
                    match m {
                        StructMember::Field(f) => fields.push(f),
                        StructMember::Method(s) => methods.push(s),
                    }
                }
                Stmt::StructDef { name, fields, methods }
            });

    // loop { body }
    let loop_stmt = just(Token::Loop)
        .ignore_then(block)
        .map(|body| Stmt::Loop { body });

    // break
    let break_stmt = just(Token::Break).map(|_| Stmt::Break);

    // import name
    let import_stmt = just(Token::Import)
        .ignore_then(select! { Token::Ident(s) => s })
        .map(|name| Stmt::Import { name });

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
            .or(struct_def)
            .or(loop_stmt)
            .or(break_stmt)
            .or(import_stmt)
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
