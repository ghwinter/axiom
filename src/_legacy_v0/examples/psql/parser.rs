//! SQL parser — a pure `Func` that turns a token stream into an AST.
//!
//! Implemented as a recursive-descent parser. Like the lexer, it is stateless
//! across invocations and therefore a `Func`, not a `Machine`.

use crate::ast::{ColumnDef, SelectCols, SqlType, Statement};
use crate::lexer::{token_to_value, Token};
use axiom::func::{CostEstimate, Func, FuncRef};

// ════════════════════════════════════════════════════════════════════════════
// Parse error
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub msg: String,
    pub at: Option<Token>,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.at {
            Some(t) => write!(f, "parse error near {:?}: {}", t, self.msg),
            None => write!(f, "parse error: {}", self.msg),
        }
    }
}

impl std::error::Error for ParseError {}

// ════════════════════════════════════════════════════════════════════════════
// Parser — recursive descent over a token slice.
// ════════════════════════════════════════════════════════════════════════════

struct Parser<'a> {
    toks: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(toks: &'a [Token]) -> Self {
        Self { toks, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.toks.get(self.pos)
    }

    fn bump(&mut self) -> Option<&Token> {
        let t = self.toks.get(self.pos);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, expected: &Token) -> Result<(), ParseError> {
        match self.peek() {
            Some(t) if t == expected => {
                self.pos += 1;
                Ok(())
            }
            other => Err(ParseError {
                msg: format!("expected {:?}", expected),
                at: other.cloned(),
            }),
        }
    }

    fn eat_ident(&mut self) -> Result<String, ParseError> {
        match self.peek() {
            Some(Token::Ident(s)) => {
                let v = s.clone();
                self.pos += 1;
                Ok(v)
            }
            other => Err(ParseError {
                msg: "expected identifier".into(),
                at: other.cloned(),
            }),
        }
    }

    /// Parse a top-level statement. Trailing `;` is optional.
    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        let first = self.peek().ok_or_else(|| ParseError {
            msg: "empty input".into(),
            at: None,
        })?;

        match first {
            Token::Create => self.parse_create(),
            Token::Insert => self.parse_insert(),
            Token::Select => self.parse_select(),
            Token::Drop => self.parse_drop(),
            other => Err(ParseError {
                msg: format!("unexpected leading token {:?}", other),
                at: Some(other.clone()),
            }),
        }
    }

    // ── CREATE TABLE name ( col TYPE, ... ) ──────────────────────

    fn parse_create(&mut self) -> Result<Statement, ParseError> {
        self.eat(&Token::Create)?;
        self.eat(&Token::Table)?;
        let name = self.eat_ident()?;
        self.eat(&Token::LParen)?;

        let mut columns = Vec::new();
        loop {
            let col_name = self.eat_ident()?;
            let ty_name = self.eat_ident()?;
            let ty = SqlType::from_keyword(&ty_name).ok_or_else(|| ParseError {
                msg: format!("unknown type: {}", ty_name),
                at: Some(Token::Ident(ty_name)),
            })?;
            columns.push(ColumnDef { name: col_name, ty });

            match self.peek() {
                Some(Token::Comma) => { self.pos += 1; continue; }
                Some(Token::RParen) => { self.pos += 1; break; }
                other => return Err(ParseError {
                    msg: "expected , or ) in column list".into(),
                    at: other.cloned(),
                }),
            }
        }
        // optional trailing semicolon
        if matches!(self.peek(), Some(Token::Semi)) {
            self.pos += 1;
        }

        Ok(Statement::CreateTable { name, columns })
    }

    // ── INSERT INTO name [ ( col, ... ) ] VALUES ( v, ... ) ──────

    fn parse_insert(&mut self) -> Result<Statement, ParseError> {
        self.eat(&Token::Insert)?;
        self.eat(&Token::Into)?;
        let table = self.eat_ident()?;

        // optional column list
        let columns = if matches!(self.peek(), Some(Token::LParen)) {
            self.pos += 1;
            let mut cols = Vec::new();
            loop {
                cols.push(self.eat_ident()?);
                match self.peek() {
                    Some(Token::Comma) => { self.pos += 1; continue; }
                    Some(Token::RParen) => { self.pos += 1; break; }
                    other => return Err(ParseError {
                        msg: "expected , or ) in column list".into(),
                        at: other.cloned(),
                    }),
                }
            }
            Some(cols)
        } else {
            None
        };

        self.eat(&Token::Values)?;
        self.eat(&Token::LParen)?;

        let mut values = Vec::new();
        loop {
            let t = self.peek().ok_or_else(|| ParseError {
                msg: "unexpected end of input in VALUES".into(),
                at: None,
            })?;
            let v = token_to_value(t).ok_or_else(|| ParseError {
                msg: "expected a literal in VALUES".into(),
                at: Some(t.clone()),
            })?;
            self.pos += 1;
            values.push(v);

            match self.peek() {
                Some(Token::Comma) => { self.pos += 1; continue; }
                Some(Token::RParen) => { self.pos += 1; break; }
                other => return Err(ParseError {
                    msg: "expected , or ) in VALUES".into(),
                    at: other.cloned(),
                }),
            }
        }

        if matches!(self.peek(), Some(Token::Semi)) {
            self.pos += 1;
        }

        Ok(Statement::Insert { table, columns, values })
    }

    // ── SELECT * | col, ... FROM name ────────────────────────────

    fn parse_select(&mut self) -> Result<Statement, ParseError> {
        self.eat(&Token::Select)?;

        let columns = match self.peek() {
            Some(Token::Star) => {
                self.pos += 1;
                SelectCols::Star
            }
            Some(Token::Ident(_)) => {
                let mut cols = Vec::new();
                loop {
                    cols.push(self.eat_ident()?);
                    match self.peek() {
                        Some(Token::Comma) => { self.pos += 1; continue; }
                        _ => break,
                    }
                }
                SelectCols::Cols(cols)
            }
            other => return Err(ParseError {
                msg: "expected * or column list after SELECT".into(),
                at: other.cloned(),
            }),
        };

        self.eat(&Token::From)?;
        let table = self.eat_ident()?;

        if matches!(self.peek(), Some(Token::Semi)) {
            self.pos += 1;
        }

        Ok(Statement::Select { columns, table })
    }

    // ── DROP TABLE name ──────────────────────────────────────────
    //
    // Not in the documented minimal subset, but trivial to add and useful for
    // the REPL. We accept it but the executor will reject it (returns an error)
    // — kept here to demonstrate grammar extensibility.

    fn parse_drop(&mut self) -> Result<Statement, ParseError> {
        self.eat(&Token::Drop)?;
        self.eat(&Token::Table)?;
        let _name = self.eat_ident()?;
        if matches!(self.peek(), Some(Token::Semi)) {
            self.pos += 1;
        }
        // We don't model DROP in Statement yet — surface a parse error so the
        // REPL reports it cleanly instead of silently accepting.
        Err(ParseError {
            msg: "DROP TABLE is recognised but not yet supported".into(),
            at: None,
        })
    }
}

// ════════════════════════════════════════════════════════════════════════════
// parse — the free-function entry point.
// ════════════════════════════════════════════════════════════════════════════

pub fn parse(tokens: &[Token]) -> Result<Statement, ParseError> {
    let mut p = Parser::new(tokens);
    let stmt = p.parse_statement()?;
    // Reject trailing junk after a complete statement.
    if p.pos != p.toks.len() {
        return Err(ParseError {
            msg: format!("trailing tokens after statement ({} unparsed)", p.toks.len() - p.pos),
            at: p.peek().cloned(),
        });
    }
    Ok(stmt)
}

// ════════════════════════════════════════════════════════════════════════════
// ParserFunc — the axiom Func wrapper.
// ════════════════════════════════════════════════════════════════════════════

pub struct ParserFunc;

impl Func for ParserFunc {
    type Input = Vec<Token>;
    type Output = Result<Statement, ParseError>;

    fn name() -> &'static str {
        "parser"
    }

    fn call(input: Vec<Token>) -> Result<Statement, ParseError> {
        parse(&input)
    }

    fn cost_estimate() -> CostEstimate {
        CostEstimate::Cheap
    }

    fn nondeterministic() -> bool {
        false
    }
}

/// Borrow-input path (A4): the parser only reads the token slice, so a
/// fused pipeline can drive it with `call_ref(&[Token])` without moving
/// (or re-owning) the token vector.
impl FuncRef for ParserFunc {
    fn call_ref(input: &Vec<Token>) -> Result<Statement, ParseError> {
        parse(input)
    }
}
