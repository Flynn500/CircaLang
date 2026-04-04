/// Pretty error reporting for Circa.
///
/// Provides coloured, source-annotated error messages for lex, parse, and runtime errors.

/// ANSI colour helpers — no external crate needed.
pub mod color {
    pub const RED: &str = "\x1b[31m";
    pub const RED_BOLD: &str = "\x1b[1;31m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const CYAN: &str = "\x1b[36m";
    pub const DIM: &str = "\x1b[2m";
    pub const BOLD: &str = "\x1b[1m";
    pub const RESET: &str = "\x1b[0m";
}

use color::*;

/// Locate a byte offset within source text, returning (line_number, column, line_text).
/// Line numbers are 1-based.
fn locate(src: &str, offset: usize) -> (usize, usize, &str) {
    let offset = offset.min(src.len());
    let mut line_start = 0;
    let mut line_no = 1;

    for (i, ch) in src.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line_no += 1;
            line_start = i + 1;
        }
    }

    let line_end = src[offset..]
        .find('\n')
        .map(|i| offset + i)
        .unwrap_or(src.len());

    let line_text = &src[line_start..line_end];
    let col = offset - line_start;
    (line_no, col, line_text)
}

/// Format a source-annotated error.
///
/// ```text
/// error: unexpected token ')'
///   --> test.ca:3:9
///    |
///  3 | print(z))
///    |         ^ unexpected token
/// ```
pub fn report_error(
    filename: &str,
    src: &str,
    offset: usize,
    len: usize,
    message: &str,
    label: &str,
) {
    let (line_no, col, line_text) = locate(src, offset);
    let gutter_width = line_no.to_string().len();

    eprintln!(
        "{RED_BOLD}error{RESET}{BOLD}: {message}{RESET}"
    );
    eprintln!(
        "{CYAN}{:>gw$}-->{RESET} {filename}:{line_no}:{col_1}",
        "",
        gw = gutter_width,
        col_1 = col + 1,
    );
    eprintln!(
        "{CYAN}{:>gw$} |{RESET}",
        "",
        gw = gutter_width,
    );
    eprintln!(
        "{CYAN}{line_no:>gw$} |{RESET} {line_text}",
        gw = gutter_width,
    );

    // Underline the offending span
    let underline_len = len.max(1);
    eprintln!(
        "{CYAN}{:>gw$} |{RESET} {:>col$}{RED_BOLD}{}{RESET} {RED}{label}{RESET}",
        "",
        "",
        "^".repeat(underline_len),
        gw = gutter_width,
        col = col,
    );

    eprintln!();
}

// ── Parse errors ────────────────────────────────────────────────────────────

use chumsky::error::Simple;
use crate::lexer::Token;

/// Pretty-print a chumsky parse error (operates on token spans = byte offsets).
pub fn report_parse_error(filename: &str, src: &str, err: &Simple<Token>) {
    let span = err.span();
    let offset = span.start;
    let len = span.end.saturating_sub(span.start).max(1);

    let message = match err.found() {
        Some(tok) => format!("unexpected token `{}`", token_name(tok)),
        None => "unexpected end of input".to_string(),
    };

    let label = if err.expected().len() > 0 {
        let mut names: Vec<String> = err
            .expected()
            .filter_map(|e| e.as_ref().map(|t| format!("`{}`", token_name(t))))
            .collect();
        names.sort();
        names.dedup();
        if names.len() <= 4 {
            format!("expected {}", names.join(", "))
        } else {
            // Too many — just say what went wrong
            "not expected here".to_string()
        }
    } else {
        "not expected here".to_string()
    };

    report_error(filename, src, offset, len, &message, &label);
}

/// Pretty-print a chumsky lex error (operates on char spans).
pub fn report_lex_error(filename: &str, src: &str, err: &Simple<char>) {
    let span = err.span();
    let offset = span.start;
    let len = span.end.saturating_sub(span.start).max(1);
    let message = match err.found() {
        Some(ch) => format!("unexpected character `{}`", ch),
        None => "unexpected end of input".to_string(),
    };
    report_error(filename, src, offset, len, &message, "");
}

// ── Runtime errors ──────────────────────────────────────────────────────────

/// Print a runtime error with colour and an optional call stack.
pub fn report_runtime_error(message: &str) {
    report_runtime_error_with_stack(message, &[]);
}

/// Print a runtime error with a call stack trace.
/// Stack entries are function names, outermost first.
pub fn report_runtime_error_with_stack(message: &str, call_stack: &[String]) {
    eprintln!("{RED_BOLD}error{RESET}{BOLD}: {message}{RESET}");

    if !call_stack.is_empty() {
        eprintln!();
        eprintln!("{DIM}call stack:{RESET}");
        for (i, frame) in call_stack.iter().rev().enumerate() {
            let marker = if i == 0 { "→" } else { " " };
            let color = if i == 0 { RED } else { DIM };
            eprintln!("  {color}{marker} {frame}(){RESET}");
        }
        eprintln!();
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Human-readable name for a token (used in error messages).
fn token_name(tok: &Token) -> &'static str {
    match tok {
        Token::Let => "let",
        Token::Const => "Const",
        Token::Colon => ":",
        Token::Arrow => "->",
        Token::Fn => "fn",
        Token::If => "if",
        Token::Else => "else",
        Token::Return => "return",
        Token::True => "true",
        Token::False => "false",
        Token::Loop => "loop",
        Token::Break => "break",
        Token::None => "None",
        Token::Import => "import",
        Token::New => "new",
        Token::Struct => "struct",
        Token::Number(_) => "number",
        Token::Integer(_) => "integer",
        Token::StringLit(_) => "string",
        Token::Ident(_) => "identifier",
        Token::Tilde => "~",
        Token::Eq => "==",
        Token::Neq => "!=",
        Token::Lte => "<=",
        Token::Gte => ">=",
        Token::Lt => "<",
        Token::Gt => ">",
        Token::MaybeEq => "?=",
        Token::MaybeNeq => "?!=",
        Token::MaybeNeq2 => "!?=",
        Token::MaybeGt => "?>",
        Token::MaybeLt => "?<",
        Token::MaybeGte => "?>=",
        Token::MaybeLte => "?<=",
        Token::Plus => "+",
        Token::Minus => "-",
        Token::Star => "*",
        Token::Slash => "/",
        Token::Assign => "=",
        Token::LParen => "(",
        Token::RParen => ")",
        Token::LBrace => "{",
        Token::RBrace => "}",
        Token::LBracket => "[",
        Token::RBracket => "]",
        Token::Comma => ",",
        Token::Dot => ".",
        Token::Newline => "newline",
        Token::Comment => "comment",
        Token::TyFloat => "Float",
        Token::TyInt => "Int",
        Token::TyString => "String",
        Token::TyBool => "Bool",
    }
}