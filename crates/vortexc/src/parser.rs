//! The recursive descent parser.
//!
//! Turns the token stream from the stage 1 lexer into an [`crate::ast::Program`].
//! Diagnostics name the line, the column and what was expected, in the same
//! shape as the lexer errors, so a reader sees one style across the project.
//!
//! One ambiguity is resolved the way Rust resolves it. `Point { x: 1 }` and
//! `if c { .. }` both put a `{` after an expression. A struct literal is only
//! recognised where a statement cannot start, so the condition of an `if`,
//! `while` or `match` is parsed with struct literals disallowed and the `{`
//! belongs to the block.

use crate::ast::*;
use crate::span::Pos;
use crate::token::{Tok, Token};

/// A parse error, with the source so the caller can render a caret.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub pos: Pos,
    pub snippet: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error at {}: {}\n{}",
            self.pos, self.message, self.snippet
        )
    }
}

impl std::error::Error for ParseError {}

/// Parses `src` into a program.
pub fn parse(src: &str) -> Result<Program, ParseError> {
    // A lex error already carries a line, a column and an expectation, so it
    // becomes a parse error unchanged rather than being reworded.
    let tokens = crate::lexer::lex(src).map_err(|e| ParseError {
        message: e.message,
        pos: e.pos,
        snippet: e.snippet,
    })?;
    let mut p = Parser::new(src, tokens);
    p.program()
}

struct Parser<'a> {
    src: &'a str,
    toks: Vec<Token>,
    idx: usize,
    /// False while parsing a condition, where `{` opens a block rather than a
    /// struct literal.
    struct_literal_ok: bool,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str, toks: Vec<Token>) -> Self {
        Parser {
            src,
            toks,
            idx: 0,
            struct_literal_ok: true,
        }
    }

    // --- token helpers ---------------------------------------------------

    fn peek(&self) -> &Tok {
        &self.toks[self.idx].tok
    }

    fn pos(&self) -> Pos {
        self.toks[self.idx].start
    }

    fn advance(&mut self) -> Token {
        let t = self.toks[self.idx].clone();
        if self.idx + 1 < self.toks.len() {
            self.idx += 1;
        }
        t
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.peek() == tok {
            self.advance();
            true
        } else {
            false
        }
    }

    fn at(&self, tok: &Tok) -> bool {
        self.peek() == tok
    }

    /// Consumes `tok` or fails, naming what was expected and what was found.
    fn expect(&mut self, tok: Tok) -> Result<Token, ParseError> {
        if self.peek() == &tok {
            return Ok(self.advance());
        }
        let found = self.peek().describe();
        let pos = self.pos();
        self.fail(pos, &format!("`{}`", tok_spelling(&tok)), &found)
    }

    fn expect_ident(&mut self, expectation: &str) -> Result<String, ParseError> {
        match self.peek().clone() {
            Tok::Ident(name) if !is_keyword(name) => {
                self.advance();
                Ok(name.to_string())
            }
            found => {
                let pos = self.pos();
                self.fail(pos, expectation, &found.describe())
            }
        }
    }

    /// Builds an error that says what was expected and what was found.
    fn fail<T>(&self, pos: Pos, expectation: &str, found: &str) -> Result<T, ParseError> {
        let message = format!("expected {}, found {}", expectation, found);
        let snippet = crate::span::render_snippet(self.src, pos, &message);
        Err(ParseError {
            message,
            pos,
            snippet,
        })
    }

    fn at_keyword(&self, kw: &str) -> bool {
        matches!(self.peek(), Tok::Ident(n) if *n == kw)
    }

    /// Parses an expression with struct literals disallowed, for conditions.
    fn condition(&mut self) -> Result<Expr, ParseError> {
        let saved = self.struct_literal_ok;
        self.struct_literal_ok = false;
        let result = self.expr();
        self.struct_literal_ok = saved;
        result
    }

    /// Consumes `;`, wrapping the message so a missing semicolon reads well.
    fn expect_semicolon(&mut self) -> Result<(), ParseError> {
        if self.eat(&Tok::Semicolon) {
            return Ok(());
        }
        let found = self.peek().describe();
        let pos = self.pos();
        let message = format!("expected `;` to end the statement, found {}", found);
        let snippet = crate::span::render_snippet(self.src, pos, &message);
        Err(ParseError {
            message,
            pos,
            snippet,
        })
    }

    // --- top level -------------------------------------------------------

    fn program(&mut self) -> Result<Program, ParseError> {
        let mut items = Vec::new();
        while !self.at(&Tok::Eof) {
            items.push(self.item()?);
        }
        Ok(Program { items })
    }

    fn item(&mut self) -> Result<Item, ParseError> {
        let pos = self.pos();
        if self.at_keyword("fn") {
            return Ok(Item::Function(Spanned::new(self.fn_decl()?, pos)));
        }
        if self.at_keyword("struct") {
            return Ok(Item::Struct(Spanned::new(self.struct_decl()?, pos)));
        }
        if self.at_keyword("enum") {
            return Ok(Item::Enum(Spanned::new(self.enum_decl()?, pos)));
        }
        let found = self.peek().describe();
        self.fail(pos, "an item: `fn`, `struct` or `enum`", &found)
    }

    fn fn_decl(&mut self) -> Result<FnDecl, ParseError> {
        self.advance();
        let name = self.expect_ident("a function name")?;
        self.expect(Tok::LParen)?;

        let mut params = Vec::new();
        if !self.eat(&Tok::RParen) {
            loop {
                // `var` before the parameter name makes it mutable. See
                // `SPEC.md` section 8.
                let mutable = self.eat(&Tok::Ident("var"));
                let pname = self.expect_ident("a parameter name")?;
                self.expect(Tok::Colon)?;
                let ty = self.type_expr()?;
                params.push(Param {
                    name: pname,
                    ty,
                    mutable,
                });
                if self.eat(&Tok::Comma) {
                    if self.at(&Tok::RParen) {
                        break;
                    }
                } else {
                    break;
                }
            }
            self.expect(Tok::RParen)?;
        }

        let ret = if self.eat(&Tok::Arrow) {
            Some(self.type_expr()?)
        } else {
            None
        };

        let body = self.block()?;
        Ok(FnDecl {
            name,
            params,
            ret,
            body,
        })
    }

    fn type_expr(&mut self) -> Result<TypeExpr, ParseError> {
        let mut name = self.expect_ident("a type name")?;
        // `SPEC.md` section 6.2 writes a variant path with a dot, and section
        // 5.8 lists `::` as a token, so both spellings are accepted.
        if self.eat(&Tok::PathSep) || self.eat(&Tok::Dot) {
            let variant = self.expect_ident("a variant name in the type path")?;
            name.push('.');
            name.push_str(&variant);
        }
        Ok(TypeExpr::Named(name))
    }

    fn struct_decl(&mut self) -> Result<StructDecl, ParseError> {
        self.advance();
        let name = self.expect_ident("a struct name")?;
        self.expect(Tok::LBrace)?;
        let mut fields = Vec::new();
        if !self.eat(&Tok::RBrace) {
            loop {
                let fname = self.expect_ident("a field name")?;
                self.expect(Tok::Colon)?;
                let ty = self.type_expr()?;
                fields.push(Field { name: fname, ty });
                if self.eat(&Tok::Comma) {
                    if self.at(&Tok::RBrace) {
                        break;
                    }
                } else {
                    break;
                }
            }
            self.expect(Tok::RBrace)?;
        }
        Ok(StructDecl { name, fields })
    }

    fn enum_decl(&mut self) -> Result<EnumDecl, ParseError> {
        self.advance();
        let name = self.expect_ident("an enum name")?;
        self.expect(Tok::LBrace)?;
        let mut variants = Vec::new();
        if !self.eat(&Tok::RBrace) {
            loop {
                variants.push(self.variant()?);
                if self.eat(&Tok::Comma) {
                    if self.at(&Tok::RBrace) {
                        break;
                    }
                } else {
                    break;
                }
            }
            self.expect(Tok::RBrace)?;
        }
        Ok(EnumDecl { name, variants })
    }

    /// A variant, which may carry positional payloads, named fields, or
    /// neither.
    fn variant(&mut self) -> Result<Variant, ParseError> {
        let pos = self.pos();
        let vname = self.expect_ident("a variant name")?;
        let mut payloads = Vec::new();
        let mut named = Vec::new();

        if self.eat(&Tok::LParen) {
            if !self.eat(&Tok::RParen) {
                loop {
                    payloads.push(self.type_expr()?);
                    if self.eat(&Tok::Comma) {
                        if self.at(&Tok::RParen) {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                self.expect(Tok::RParen)?;
            }
        } else if self.eat(&Tok::LBrace) {
            if !self.eat(&Tok::RBrace) {
                loop {
                    let fname = self.expect_ident("a field name")?;
                    self.expect(Tok::Colon)?;
                    let ty = self.type_expr()?;
                    named.push(Field { name: fname, ty });
                    if self.eat(&Tok::Comma) {
                        if self.at(&Tok::RBrace) {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                self.expect(Tok::RBrace)?;
            }
        }

        Ok(Variant {
            name: vname,
            payloads,
            named,
            pos,
        })
    }

    // --- statements ------------------------------------------------------

    fn block(&mut self) -> Result<Block, ParseError> {
        self.expect(Tok::LBrace)?;
        let saved = self.struct_literal_ok;
        self.struct_literal_ok = true;
        let result = self.block_body();
        self.struct_literal_ok = saved;
        result
    }

    fn block_body(&mut self) -> Result<Block, ParseError> {
        let pos = self.pos();
        let mut stmts = Vec::new();
        loop {
            if self.at(&Tok::RBrace) {
                self.advance();
                return Ok(Block {
                    pos,
                    stmts,
                    tail: None,
                });
            }
            if self.at(&Tok::Eof) {
                return self.fail(self.pos(), "a `}` to close the block", "end of file");
            }

            // A nested block is a statement when a `;` follows it and the
            // value of this block when the `}` follows it directly.
            if self.at(&Tok::LBrace) {
                let inner = self.block()?;
                if self.eat(&Tok::Semicolon) {
                    stmts.push(Stmt {
                        pos: inner.pos,
                        kind: StmtKind::Block(inner),
                    });
                    continue;
                }
                self.expect(Tok::RBrace)?;
                let pos = inner.pos;
                return Ok(Block {
                    pos,
                    stmts,
                    tail: Some(Box::new(Expr::new(pos, ExprKind::Block(inner)))),
                });
            }

            if self.starts_statement() {
                stmts.push(self.stmt()?);
                continue;
            }

            // Otherwise this is an expression. A `;` makes it a statement; no
            // `;` before the `}` makes it the value of the block, which
            // `SPEC.md` section 8 allows.
            let expr = self.expr()?;
            if self.eat(&Tok::Semicolon) {
                stmts.push(Stmt {
                    pos: expr.pos,
                    kind: StmtKind::Expr(expr),
                });
                continue;
            }
            // `if` and `match` produce a value but read as statements, so they
            // may stand alone without a `;`.
            if matches!(expr.kind, ExprKind::If { .. } | ExprKind::Match { .. }) {
                stmts.push(Stmt {
                    pos: expr.pos,
                    kind: StmtKind::Expr(expr),
                });
                continue;
            }
            self.expect(Tok::RBrace)?;
            return Ok(Block {
                pos,
                stmts,
                tail: Some(Box::new(expr)),
            });
        }
    }

    fn starts_statement(&self) -> bool {
        matches!(
            self.peek(),
            Tok::Ident("let")
                | Tok::Ident("var")
                | Tok::Ident("return")
                | Tok::Ident("while")
                | Tok::Ident("for")
                | Tok::Ident("break")
                | Tok::Ident("continue")
        )
    }

    fn stmt(&mut self) -> Result<Stmt, ParseError> {
        let pos = self.pos();
        let kind = if self.at_keyword("let") || self.at_keyword("var") {
            let mutable = self.at_keyword("var");
            let keyword = if mutable { "var" } else { "let" };
            self.advance();
            let name = self.expect_ident(&format!("a name after `{}`", keyword))?;
            self.expect(Tok::Assign)?;
            let init = self.expr()?;
            self.expect_semicolon()?;
            StmtKind::Let {
                name,
                mutable,
                init,
            }
        } else if self.at_keyword("return") {
            self.advance();
            if self.eat(&Tok::Semicolon) {
                StmtKind::Return(Expr::new(pos, ExprKind::Int(0)))
            } else {
                let value = self.expr()?;
                self.expect_semicolon()?;
                StmtKind::Return(value)
            }
        } else if self.at_keyword("break") {
            self.advance();
            self.expect_semicolon()?;
            StmtKind::Break
        } else if self.at_keyword("continue") {
            self.advance();
            self.expect_semicolon()?;
            StmtKind::Continue
        } else if self.at_keyword("while") {
            self.advance();
            let cond = self.condition()?;
            let body = self.block()?;
            StmtKind::While { cond, body }
        } else if self.at_keyword("for") {
            self.advance();
            let var = self.expect_ident("a loop variable after `for`")?;
            if !self.eat(&Tok::Ident("in")) {
                // The diagnostic belongs on the token that was found, not at
                // the start of the statement.
                let found_pos = self.pos();
                let found = self.peek().describe();
                return self.fail(found_pos, "`in`", &found);
            }
            let start = self.expr()?;
            let (end, inclusive) = if self.eat(&Tok::DotDotEq) {
                (Some(self.expr()?), true)
            } else if self.eat(&Tok::DotDot) {
                (Some(self.expr()?), false)
            } else {
                // No range operator, so this is `for x in e`, which walks a
                // list or a string.
                (None, false)
            };
            let body = self.block()?;
            StmtKind::For {
                var,
                start,
                end,
                inclusive,
                body,
            }
        } else if self.at(&Tok::LBrace) {
            StmtKind::Block(self.block()?)
        } else {
            let e = self.expr()?;
            self.expect_semicolon()?;
            StmtKind::Expr(e)
        };
        Ok(Stmt { pos, kind })
    }

    // --- expressions -----------------------------------------------------
    //
    // Precedence, loosest first:
    //   ||  1        == !=  3        + -      5
    //   &&  2        < <= > >=  4     * / %    6
    //   unary - !, then postfix call, index, field and `?`

    fn expr(&mut self) -> Result<Expr, ParseError> {
        let lhs = self.binary(1)?;
        // An assignment is not a binary operator: the left side is a name and
        // not a value, so it is recognised here, after a full expression.
        if self.at(&Tok::Assign) {
            let pos = self.pos();
            self.advance();
            let value = self.expr()?;
            return match lhs.kind {
                ExprKind::Ident(name) => Ok(Expr::new(
                    pos,
                    ExprKind::Assign {
                        name,
                        value: Box::new(value),
                    },
                )),
                other => {
                    let found = describe_expr(&other);
                    self.fail(lhs.pos, "a name on the left of `=`", &found)
                }
            };
        }
        Ok(lhs)
    }

    fn binary(&mut self, min_prec: u8) -> Result<Expr, ParseError> {
        let mut lhs = self.binary_operand()?;
        loop {
            let pos = self.pos();
            let (op, prec) = match self.peek() {
                Tok::OrOr => (BinOp::Or, 1),
                Tok::AndAnd => (BinOp::And, 2),
                Tok::EqEq => (BinOp::Eq, 3),
                Tok::NotEq => (BinOp::Ne, 3),
                Tok::Lt => (BinOp::Lt, 4),
                Tok::LtEq => (BinOp::Le, 4),
                Tok::Gt => (BinOp::Gt, 4),
                Tok::GtEq => (BinOp::Ge, 4),
                Tok::Plus => (BinOp::Add, 5),
                Tok::Minus => (BinOp::Sub, 5),
                Tok::Star => (BinOp::Mul, 6),
                Tok::Slash => (BinOp::Div, 6),
                Tok::Percent => (BinOp::Rem, 6),
                _ => break,
            };
            if prec < min_prec {
                break;
            }
            self.advance();
            // Left associative: the right side binds one level tighter.
            let rhs = self.binary(prec + 1)?;
            lhs = Expr::new(
                pos,
                ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
            );
        }
        Ok(lhs)
    }

    /// The operand form of an expression, which cannot be a bare assignment.
    fn binary_operand(&mut self) -> Result<Expr, ParseError> {
        self.unary()
    }

    fn unary(&mut self) -> Result<Expr, ParseError> {
        let pos = self.pos();
        if self.eat(&Tok::Minus) {
            let operand = self.unary()?;
            return Ok(Expr::new(pos, ExprKind::Neg(Box::new(operand))));
        }
        if self.eat(&Tok::Bang) {
            let operand = self.unary()?;
            return Ok(Expr::new(pos, ExprKind::Not(Box::new(operand))));
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, ParseError> {
        let mut e = self.primary()?;
        loop {
            if self.at(&Tok::LParen) {
                let pos = self.pos();
                let name = match &e.kind {
                    ExprKind::Ident(n) => n.clone(),
                    other => {
                        let found = describe_expr(other);
                        return self.fail(pos, "a function name before `(`", &found);
                    }
                };
                self.advance();
                let args = self.call_args()?;
                e = Expr::new(pos, ExprKind::Call { callee: name, args });
                continue;
            }

            if self.at(&Tok::LBracket) {
                let pos = self.pos();
                self.advance();
                let index = self.expr()?;
                self.expect(Tok::RBracket)?;
                e = Expr::new(pos, ExprKind::Index(Box::new(e), Box::new(index)));
                continue;
            }

            if self.at(&Tok::Dot) {
                let pos = self.pos();
                self.advance();
                // `t.0` reads the first element of a tuple, so a number is a
                // valid name after a dot as well as an identifier.
                let name = match self.peek().clone() {
                    Tok::Int(v) => {
                        self.advance();
                        v.to_string()
                    }
                    _ => self.expect_ident("a field, index or variant name after `.`")?,
                };
                e = self.after_dot(pos, e, name)?;
                continue;
            }

            if self.at(&Tok::Question) {
                let pos = self.pos();
                self.advance();
                e = Expr::new(pos, ExprKind::Try(Box::new(e)));
                continue;
            }

            break;
        }
        Ok(e)
    }

    fn after_dot(&mut self, pos: Pos, base: Expr, name: String) -> Result<Expr, ParseError> {
        // `Type.variant(..)` needs a type name in front of the dot.
        let ty = match &base.kind {
            ExprKind::Ident(n) if !is_keyword(n) => Some(n.clone()),
            _ => None,
        };

        if self.at(&Tok::LParen) {
            let ty = match ty {
                Some(t) => t,
                None => {
                    return self.fail(pos, "a type name before a variant call", "this expression")
                }
            };
            self.advance();
            let args = self.call_args()?;
            return Ok(Expr::new(
                pos,
                ExprKind::VariantCall {
                    ty,
                    variant: name,
                    args,
                },
            ));
        }

        if self.at(&Tok::LBrace) && self.struct_literal_ok {
            let ty = match ty {
                Some(t) => t,
                None => return self.fail(pos, "a type name before `{`", "this expression"),
            };
            let fields = self.record_body()?;
            return Ok(Expr::new(
                pos,
                ExprKind::VariantRecord {
                    ty,
                    variant: name,
                    fields,
                },
            ));
        }

        Ok(Expr::new(pos, ExprKind::Field(Box::new(base), name)))
    }

    fn call_args(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();
        if self.eat(&Tok::RParen) {
            return Ok(args);
        }
        loop {
            args.push(self.expr()?);
            if self.eat(&Tok::Comma) {
                if self.at(&Tok::RParen) {
                    break;
                }
            } else {
                break;
            }
        }
        self.expect(Tok::RParen)?;
        Ok(args)
    }

    /// Reads `{ field: value, .. }` for a struct or variant record.
    fn record_body(&mut self) -> Result<Vec<(String, Expr)>, ParseError> {
        self.expect(Tok::LBrace)?;
        let mut fields = Vec::new();
        if self.eat(&Tok::RBrace) {
            return Ok(fields);
        }
        loop {
            let name = self.expect_ident("a field name")?;
            self.expect(Tok::Colon)?;
            let value = self.expr()?;
            fields.push((name, value));
            if self.eat(&Tok::Comma) {
                if self.at(&Tok::RBrace) {
                    break;
                }
            } else {
                break;
            }
        }
        self.expect(Tok::RBrace)?;
        Ok(fields)
    }

    fn primary(&mut self) -> Result<Expr, ParseError> {
        let pos = self.pos();
        let kind = match self.peek().clone() {
            Tok::Int(v) => {
                self.advance();
                ExprKind::Int(v as i64)
            }
            Tok::Float(v) => {
                self.advance();
                ExprKind::Float(v)
            }
            Tok::Str(s) => {
                self.advance();
                ExprKind::Str(s)
            }
            Tok::Char(c) => {
                self.advance();
                ExprKind::Char(c)
            }
            Tok::Ident(name) => match name {
                "true" => {
                    self.advance();
                    ExprKind::Bool(true)
                }
                "false" => {
                    self.advance();
                    ExprKind::Bool(false)
                }
                "if" => {
                    self.advance();
                    let cond = self.condition()?;
                    let then = self.block()?;
                    let otherwise = if self.eat(&Tok::Ident("else")) {
                        if self.at_keyword("if") {
                            let else_pos = self.pos();
                            let nested = self.primary()?;
                            Some(Box::new(Else::If(Spanned::new(nested, else_pos))))
                        } else {
                            Some(Box::new(Else::Block(self.block()?)))
                        }
                    } else {
                        None
                    };
                    return Ok(Expr::new(
                        pos,
                        ExprKind::If {
                            cond: Box::new(cond),
                            then,
                            otherwise,
                        },
                    ));
                }
                "match" => {
                    self.advance();
                    let scrutinee = self.condition()?;
                    self.expect(Tok::LBrace)?;
                    let mut arms = Vec::new();
                    while !self.at(&Tok::RBrace) {
                        if self.at(&Tok::Eof) {
                            return self.fail(
                                self.pos(),
                                "a `}` to close the `match`",
                                "end of file",
                            );
                        }
                        arms.push(self.arm()?);
                    }
                    self.expect(Tok::RBrace)?;
                    ExprKind::Match {
                        scrutinee: Box::new(scrutinee),
                        arms,
                    }
                }
                _ => {
                    if is_keyword(name) {
                        let found = format!("the keyword `{}`", name);
                        return self.fail(pos, "an expression", &found);
                    }
                    self.advance();
                    // `Point { x: 1 }` is a struct literal where a statement
                    // cannot start.
                    if self.at(&Tok::LBrace) && self.struct_literal_ok {
                        let fields = self.record_body()?;
                        ExprKind::Record {
                            ty: name.to_string(),
                            fields,
                        }
                    } else {
                        ExprKind::Ident(name.to_string())
                    }
                }
            },
            Tok::LParen => {
                self.advance();
                if self.eat(&Tok::RParen) {
                    ExprKind::Tuple(Vec::new())
                } else {
                    let first = self.expr()?;
                    if self.eat(&Tok::Comma) {
                        let mut items = vec![first];
                        if !self.at(&Tok::RParen) {
                            loop {
                                items.push(self.expr()?);
                                if self.eat(&Tok::Comma) {
                                    if self.at(&Tok::RParen) {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                        }
                        self.expect(Tok::RParen)?;
                        ExprKind::Tuple(items)
                    } else {
                        self.expect(Tok::RParen)?;
                        ExprKind::Paren(Box::new(first))
                    }
                }
            }
            Tok::LBracket => {
                self.advance();
                let mut items = Vec::new();
                if !self.eat(&Tok::RBracket) {
                    loop {
                        items.push(self.expr()?);
                        if self.eat(&Tok::Comma) {
                            if self.at(&Tok::RBracket) {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    self.expect(Tok::RBracket)?;
                }
                ExprKind::Array(items)
            }
            other => return self.fail(pos, "an expression", &other.describe()),
        };
        Ok(Expr::new(pos, kind))
    }

    fn arm(&mut self) -> Result<Arm, ParseError> {
        let pos = self.pos();
        let mut patterns = vec![self.pattern()?];
        // Further alternatives make one arm. A comma separates arms and a
        // `|` separates patterns inside one arm.
        loop {
            if self.at(&Tok::Pipe) {
                self.advance();
                patterns.push(self.pattern()?);
                continue;
            }
            if self.at(&Tok::Comma) && !self.at_one_of(&[Tok::FatArrow, Tok::RBrace]) {
                self.advance();
                patterns.push(self.pattern()?);
                continue;
            }
            break;
        }
        self.expect(Tok::FatArrow)?;
        let body = self.expr()?;
        if !self.eat(&Tok::Comma) && !self.at(&Tok::RBrace) {
            let found = self.peek().describe();
            return self.fail(self.pos(), "a `,` or `}` after the arm", &found);
        }
        Ok(Arm {
            pos,
            patterns,
            body,
        })
    }

    fn at_one_of(&self, toks: &[Tok]) -> bool {
        toks.iter().any(|t| self.peek() == t)
    }

    // --- patterns --------------------------------------------------------

    fn pattern(&mut self) -> Result<Pattern, ParseError> {
        let pos = self.pos();
        let kind = match self.peek().clone() {
            // `_` is a legal identifier, so it is matched by name.
            Tok::Ident("_") => {
                self.advance();
                PatternKind::Wildcard
            }
            Tok::LParen => {
                self.advance();
                let mut items = Vec::new();
                if !self.eat(&Tok::RParen) {
                    loop {
                        items.push(self.pattern()?);
                        if self.eat(&Tok::Comma) {
                            if self.at(&Tok::RParen) {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    self.expect(Tok::RParen)?;
                }
                PatternKind::Tuple(items)
            }
            Tok::Int(v) => {
                self.advance();
                PatternKind::Literal(Expr::new(pos, ExprKind::Int(v as i64)))
            }
            Tok::Float(v) => {
                self.advance();
                PatternKind::Literal(Expr::new(pos, ExprKind::Float(v)))
            }
            Tok::Str(s) => {
                self.advance();
                PatternKind::Literal(Expr::new(pos, ExprKind::Str(s)))
            }
            Tok::Char(c) => {
                self.advance();
                PatternKind::Literal(Expr::new(pos, ExprKind::Char(c)))
            }
            Tok::Ident("true") => {
                self.advance();
                PatternKind::Literal(Expr::new(pos, ExprKind::Bool(true)))
            }
            Tok::Ident("false") => {
                self.advance();
                PatternKind::Literal(Expr::new(pos, ExprKind::Bool(false)))
            }
            Tok::Ident(name) => {
                self.advance();
                if self.eat(&Tok::PathSep) {
                    let variant = self.expect_ident("a variant name after `::`")?;
                    PatternKind::Variant {
                        ty: name.to_string(),
                        variant,
                        bindings: Vec::new(),
                    }
                } else if self.eat(&Tok::Dot) {
                    let variant = self.expect_ident("a variant name after `.`")?;
                    let mut bindings = Vec::new();
                    if self.eat(&Tok::LParen) {
                        if !self.eat(&Tok::RParen) {
                            loop {
                                bindings.push(self.binding_name()?);
                                if self.eat(&Tok::Comma) {
                                    if self.at(&Tok::RParen) {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                            self.expect(Tok::RParen)?;
                        }
                    } else if self.at(&Tok::LBrace) {
                        // `Shape.rect { w, h }` binds each named field in order.
                        self.advance();
                        if !self.eat(&Tok::RBrace) {
                            loop {
                                bindings.push(self.binding_name()?);
                                if self.eat(&Tok::Comma) {
                                    if self.at(&Tok::RBrace) {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                            self.expect(Tok::RBrace)?;
                        }
                    }
                    PatternKind::Variant {
                        ty: name.to_string(),
                        variant,
                        bindings,
                    }
                } else {
                    PatternKind::Binding(name.to_string())
                }
            }
            other => return self.fail(pos, "a pattern", &other.describe()),
        };
        Ok(Pattern::new(pos, kind))
    }

    /// Reads a name that binds the matched value.
    fn binding_name(&mut self) -> Result<String, ParseError> {
        match self.peek().clone() {
            Tok::Ident(name) if !is_keyword(name) => {
                self.advance();
                Ok(name.to_string())
            }
            Tok::Ident("_") => {
                self.advance();
                Ok("_".to_string())
            }
            found => {
                let pos = self.pos();
                self.fail(pos, "a name to bind", &found.describe())
            }
        }
    }
}

fn tok_spelling(tok: &Tok) -> String {
    match tok {
        Tok::LParen => "(".into(),
        Tok::RParen => ")".into(),
        Tok::LBrace => "{".into(),
        Tok::RBrace => "}".into(),
        Tok::LBracket => "[".into(),
        Tok::RBracket => "]".into(),
        Tok::Comma => ",".into(),
        Tok::Semicolon => ";".into(),
        Tok::Colon => ":".into(),
        Tok::Assign => "=".into(),
        Tok::Arrow => "->".into(),
        Tok::FatArrow => "=>".into(),
        Tok::Ident(n) => format!("`{}`", n),
        other => other.describe(),
    }
}

fn describe_expr(e: &ExprKind) -> String {
    match e {
        ExprKind::Ident(n) => format!("the name `{}`", n),
        ExprKind::Paren(inner) => describe_expr(&inner.kind),
        ExprKind::Int(_) => "an integer".into(),
        ExprKind::Str(_) => "a string".into(),
        ExprKind::Call { .. } => "a call".into(),
        ExprKind::Index(..) => "an index".into(),
        ExprKind::Field(..) => "a field read".into(),
        ExprKind::Binary { .. } => "an operator".into(),
        _ => "this expression".into(),
    }
}
