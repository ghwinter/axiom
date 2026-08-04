//! SQL lexer — a pure `Func` that turns a string into a token stream.
//!
//! Implemented as an axiom `Func` because lexing is stateless and deterministic:
//! the same input string always produces the same token vector. There is no
//! reason for it to be a `Machine` — it carries no state across invocations.
//!
//! # Core pressure point
//! `Func::Input` must be `Send + 'static` and `Func::Output` must be
//! `Send + Sync + 'static`. `String` and `Vec<Token>` both satisfy this, but
//! note that the input is **moved by value** — there is no `&str` input path
//! in the `Func` trait, so every REPL line is heap-allocated and moved through
//! the port boundary. For a high-throughput pipeline this would be a
//! measurable copy tax.

use crate::ast::Value;
use axiom::func::{CostEstimate, Func, FuncRef};

// ════════════════════════════════════════════════════════════════════════════
// Token
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals & identifiers
    Ident(String),
    Int(i64),
    Str(String),

    // Punctuation
    Comma,
    LParen,
    RParen,
    Star,
    Semi,
    Eq,

    // Keywords (recognised case-insensitively, stored uppercase)
    Create,
    Table,
    Insert,
    Into,
    Values,
    Select,
    From,
    Drop,
    Where,
}

impl Token {
    /// Classify an identifier as a keyword, or leave it as `Ident`.
    fn classify_keyword(word: &str) -> Option<Token> {
        match word.to_ascii_uppercase().as_str() {
            "CREATE" => Some(Token::Create),
            "TABLE" => Some(Token::Table),
            "INSERT" => Some(Token::Insert),
            "INTO" => Some(Token::Into),
            "VALUES" => Some(Token::Values),
            "SELECT" => Some(Token::Select),
            "FROM" => Some(Token::From),
            "DROP" => Some(Token::Drop),
            "WHERE" => Some(Token::Where),
            _ => None,
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Lexer error
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub struct LexError {
    pub msg: String,
    pub pos: usize,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "lex error at {}: {}", self.pos, self.msg)
    }
}

impl std::error::Error for LexError {}

// ════════════════════════════════════════════════════════════════════════════
// Lexer (free function — the Func impl wraps it)
// ════════════════════════════════════════════════════════════════════════════

/// Lex a SQL string into tokens.
///
/// This is a free function so it can be unit-tested without going through the
/// `Func` trait. The `Func` impl is a thin wrapper that owns the error path.
pub fn lex(input: &str) -> Result<Vec<Token>, LexError> {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let c = bytes[i];

        // Whitespace
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Punctuation
        match c {
            b',' => { out.push(Token::Comma);  i += 1; continue; }
            b'(' => { out.push(Token::LParen); i += 1; continue; }
            b')' => { out.push(Token::RParen); i += 1; continue; }
            b'*' => { out.push(Token::Star);   i += 1; continue; }
            b';' => { out.push(Token::Semi);   i += 1; continue; }
            b'=' => { out.push(Token::Eq);     i += 1; continue; }
            _ => {}
        }

        // String literal: '...'
        if c == b'\'' {
            let start = i + 1;
            i += 1;
            while i < bytes.len() && bytes[i] != b'\'' {
                i += 1;
            }
            if i >= bytes.len() {
                return Err(LexError {
                    msg: "unterminated string literal".into(),
                    pos: start,
                });
            }
            // i points at the closing '
            let s = std::str::from_utf8(&bytes[start..i])
                .map_err(|e| LexError { msg: e.to_string(), pos: start })?
                .to_string();
            out.push(Token::Str(s));
            i += 1; // consume closing '
            continue;
        }

        // Number: [0-9]+
        if c.is_ascii_digit() || (c == b'-' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit()) {
            let start = i;
            if c == b'-' { i += 1; }
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let s = std::str::from_utf8(&bytes[start..i])
                .map_err(|e| LexError { msg: e.to_string(), pos: start })?;
            let n: i64 = s.parse().map_err(|e| LexError {
                msg: format!("bad integer: {}", e),
                pos: start,
            })?;
            out.push(Token::Int(n));
            continue;
        }

        // Identifier / keyword: [A-Za-z_][A-Za-z0-9_]*
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = std::str::from_utf8(&bytes[start..i])
                .map_err(|e| LexError { msg: e.to_string(), pos: start })?
                .to_string();
            match Token::classify_keyword(&word) {
                Some(kw) => out.push(kw),
                None => out.push(Token::Ident(word)),
            }
            continue;
        }

        // Unknown character
        return Err(LexError {
            msg: format!("unexpected character {:?}", c as char),
            pos: i,
        });
    }

    Ok(out)
}

// ════════════════════════════════════════════════════════════════════════════
// LexerFunc — the axiom Func wrapper.
// ════════════════════════════════════════════════════════════════════════════

/// `Func` wrapper around `lex()`.
///
/// The input is a `String` (owned) because `Func::Input: Send + 'static` —
/// `&str` cannot satisfy `'static` without a `'static` source. This is the
/// second core pressure point: there is no zero-copy `&str` input path in the
/// `Func` trait, so the REPL must allocate a `String` per line.
pub struct LexerFunc;

impl Func for LexerFunc {
    type Input = String;
    type Output = Result<Vec<Token>, LexError>;

    fn name() -> &'static str {
        "lexer"
    }

    fn call(input: String) -> Result<Vec<Token>, LexError> {
        lex(&input)
    }

    fn cost_estimate() -> CostEstimate {
        CostEstimate::Cheap
    }

    fn nondeterministic() -> bool {
        false
    }
}

/// Borrow-input path (A4): the lexer only reads its input, so a fused
/// pipeline can drive it with `call_ref(&str)` and skip the per-line
/// `String` copy that the owned `Func::call` path requires.
impl FuncRef for LexerFunc {
    fn call_ref(input: &String) -> Result<Vec<Token>, LexError> {
        lex(input)
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Convenience: lex a literal Value from a token (used by the parser).
// ════════════════════════════════════════════════════════════════════════════

/// Convert a literal token into a `Value`, for use by the parser.
pub fn token_to_value(tok: &Token) -> Option<Value> {
    match tok {
        Token::Int(n) => Some(Value::Int(*n)),
        Token::Str(s) => Some(Value::Text(s.clone())),
        _ => None,
    }
}
