//! The Vortex lexer.
//!
//! Turns source text into a `Vec<Token>`. The lexical rules are specified in
//! `SPEC.md` section 5. Positions are 1-based, see `SPEC.md` section 4.

use crate::span::{render_snippet, Pos};
use crate::token::{Tok, Token};

/// A lex error, with the source it occurred in so the caller can render it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub message: String,
    pub pos: Pos,
    pub snippet: String,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error at {}: {}\n{}",
            self.pos, self.message, self.snippet
        )
    }
}

impl std::error::Error for LexError {}

/// The source encoded as a sequence of Unicode scalar values, which is what
/// lets columns count characters rather than bytes.
struct Chars {
    chars: Vec<char>,
    idx: usize,
    pos: Pos,
}

impl Chars {
    fn new(src: &str) -> Self {
        Chars {
            chars: src.chars().collect(),
            idx: 0,
            pos: Pos::START,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.idx).copied()
    }

    fn peek_at(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.idx + ahead).copied()
    }

    /// Consumes one character and advances the position.
    fn bump(&mut self) -> Option<char> {
        let c = self.chars.get(self.idx).copied()?;
        self.idx += 1;
        // Line numbers advance on a newline only, as `SPEC.md` section 4
        // requires. A carriage return is an ordinary character.
        if c == '\n' {
            self.pos = self.pos.next_line();
        } else {
            self.pos = self.pos.next_col();
        }
        Some(c)
    }

    fn at_end(&self) -> bool {
        self.idx >= self.chars.len()
    }
}

/// Turns `src` into tokens. Returns the first lex error, if any.
pub fn lex(src: &str) -> Result<Vec<Token>, LexError> {
    Lexer::new(src).run()
}

struct Lexer<'a> {
    src: &'a str,
    chars: Chars,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Lexer {
            src,
            chars: Chars::new(src),
        }
    }

    fn error<T>(&self, pos: Pos, expectation: &str) -> Result<T, LexError> {
        Err(self.make_error(pos, expectation))
    }

    fn make_error(&self, pos: Pos, expectation: &str) -> LexError {
        let message = format!("unexpected input; expected {}", expectation);
        let snippet = render_snippet(self.src, pos, &message);
        LexError {
            message,
            pos,
            snippet,
        }
    }

    fn run(mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_trivia()?;
            let start = self.chars.pos;
            if self.chars.at_end() {
                tokens.push(Token {
                    tok: Tok::Eof,
                    start,
                    end: start,
                });
                return Ok(tokens);
            }
            let tok = self.next_token()?;
            let end = self.chars.pos;
            tokens.push(Token { tok, start, end });
        }
    }

    /// Skips whitespace and comments. Comments are not tokens.
    fn skip_trivia(&mut self) -> Result<(), LexError> {
        loop {
            match self.chars.peek() {
                Some(' ') | Some('\t') | Some('\r') | Some('\n') => {
                    self.chars.bump();
                }
                Some('/') if self.chars.peek_at(1) == Some('/') => {
                    while let Some(c) = self.chars.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.chars.bump();
                    }
                }
                Some('/') if self.chars.peek_at(1) == Some('*') => self.skip_block_comment()?,
                _ => return Ok(()),
            }
        }
    }

    /// Skips a block comment. Block comments nest, as `SPEC.md` section 5.2
    /// requires.
    fn skip_block_comment(&mut self) -> Result<(), LexError> {
        let open = self.chars.pos;
        // Step past the opening `/*`.
        self.chars.bump();
        self.chars.bump();
        let mut depth = 1usize;
        while depth > 0 {
            match self.chars.peek() {
                None => return self.error(open, "a closing `*/` before end of file"),
                Some('*') if self.chars.peek_at(1) == Some('/') => {
                    self.chars.bump();
                    self.chars.bump();
                    depth -= 1;
                }
                Some('/') if self.chars.peek_at(1) == Some('*') => {
                    self.chars.bump();
                    self.chars.bump();
                    depth += 1;
                }
                _ => {
                    self.chars.bump();
                }
            }
        }
        Ok(())
    }

    fn next_token(&mut self) -> Result<Tok, LexError> {
        let c = self.chars.peek().expect("caller checked for end of input");
        match c {
            '0'..='9' => self.number(),
            '"' => {
                let open = self.chars.pos;
                self.chars.bump();
                self.string_body(open)
            }
            '\'' => {
                let open = self.chars.pos;
                self.chars.bump();
                self.char_body(open)
            }
            c if is_ident_start(c) => Ok(self.ident()),
            _ => self.punct(),
        }
    }

    fn ident(&mut self) -> Tok {
        let mut name = String::new();
        while let Some(c) = self.chars.peek() {
            if is_ident_continue(c) {
                name.push(c);
                self.chars.bump();
            } else {
                break;
            }
        }
        // Keywords are not distinguished here. The parser matches the name, so
        // growing the keyword list does not touch the lexer.
        Tok::Ident(leak_name(name))
    }

    fn number(&mut self) -> Result<Tok, LexError> {
        let start = self.chars.pos;

        // Radix prefixes.
        if self.chars.peek() == Some('0') {
            if let Some(prefix) = self.chars.peek_at(1) {
                let radix = match prefix {
                    'x' | 'X' => Some(16),
                    'o' | 'O' => Some(8),
                    'b' | 'B' => Some(2),
                    _ => None,
                };
                if let Some(radix) = radix {
                    self.chars.bump();
                    self.chars.bump();
                    return self.radix_number(radix);
                }
            }
        }

        self.digits(10)?;

        let mut is_float = false;

        // A `.` continues the number only when a digit follows it, so `1.foo()`
        // lexes as the integer 1 and then `.`, as `SPEC.md` section 5.5
        // requires.
        if self.chars.peek() == Some('.')
            && self.chars.peek_at(1).is_some_and(|c| c.is_ascii_digit())
        {
            is_float = true;
            self.chars.bump();
            self.digits(10)?;
        }

        // An exponent also makes the literal a float.
        if matches!(self.chars.peek(), Some('e') | Some('E')) {
            let next = self.chars.peek_at(1);
            let exponent_follows = matches!(next, Some('0'..='9'))
                || (matches!(next, Some('+') | Some('-'))
                    && self.chars.peek_at(2).is_some_and(|c| c.is_ascii_digit()));
            if exponent_follows {
                is_float = true;
                self.chars.bump();
                if matches!(self.chars.peek(), Some('+') | Some('-')) {
                    self.chars.bump();
                }
                self.digits(10)?;
            }
        }

        // Rebuild the numeric text to strip the `_` separators.
        let text: String = self.slice_from(start);
        let cleaned = strip_underscores(&text);

        if is_float {
            match cleaned.parse::<f64>() {
                Ok(v) => Ok(Tok::Float(v)),
                Err(_) => self.error(start, "a floating point literal"),
            }
        } else {
            match cleaned.parse::<u64>() {
                Ok(v) => Ok(Tok::Int(v)),
                Err(_) => self.error(start, "an integer literal that fits in 64 bits"),
            }
        }
    }

    /// Reads the digits of a radix prefixed literal, with `_` separators.
    fn radix_number(&mut self, radix: u32) -> Result<Tok, LexError> {
        let start = self.chars.pos;
        let digit_start = self.chars.idx;
        loop {
            match self.chars.peek() {
                Some('_') => {
                    self.chars.bump();
                }
                Some(c) if c.is_digit(radix) => {
                    self.chars.bump();
                }
                _ => break,
            }
        }
        let digits: String = self.chars.chars[digit_start..self.chars.idx]
            .iter()
            .filter(|c| **c != '_')
            .collect();
        if digits.is_empty() {
            return self.error(start, "at least one digit after the radix prefix");
        }
        match u64::from_str_radix(&digits, radix) {
            Ok(v) => Ok(Tok::Int(v)),
            Err(_) => self.error(start, "an integer literal that fits in 64 bits"),
        }
    }

    /// Consumes a run of decimal digits, allowing `_` separators.
    fn digits(&mut self, radix: u32) -> Result<(), LexError> {
        let start = self.chars.pos;
        let mut count = 0usize;
        loop {
            match self.chars.peek() {
                Some('_') => {
                    self.chars.bump();
                }
                Some(c) if c.is_digit(radix) => {
                    self.chars.bump();
                    count += 1;
                }
                _ => break,
            }
        }
        if count == 0 {
            return self.error(start, "at least one digit");
        }
        Ok(())
    }

    /// Reads a string body up to the closing quote. The opening quote is already
    /// consumed. `open` is where the opening quote was, and is where a diagnostic
    /// about the literal as a whole points. Escapes are decoded here.
    fn string_body(&mut self, open: Pos) -> Result<Tok, LexError> {
        let mut value = String::new();
        loop {
            match self.chars.peek() {
                None | Some('\n') => {
                    return self.error(open, "a closing `\"` before the end of the line")
                }
                Some('"') => {
                    self.chars.bump();
                    return Ok(Tok::Str(value));
                }
                Some('\\') => value.push(self.escape(open)?),
                Some(c) => {
                    self.chars.bump();
                    value.push(c);
                }
            }
        }
    }

    /// Reads a character literal body. The opening quote is already consumed.
    fn char_body(&mut self, open: Pos) -> Result<Tok, LexError> {
        let c = match self.chars.peek() {
            None | Some('\n') => {
                return self.error(open, "a closing `'` before the end of the line")
            }
            Some('\\') => self.escape(open)?,
            Some(c) => {
                self.chars.bump();
                c
            }
        };
        match self.chars.peek() {
            Some('\'') => {
                self.chars.bump();
                Ok(Tok::Char(c))
            }
            None | Some('\n') => self.error(open, "a closing `'` before the end of the line"),
            Some(_) => self.error(open, "exactly one character between the quotes"),
        }
    }

    /// Decodes one escape sequence. The caller has peeked a backslash but has not
    /// consumed it. `literal_start` is where the literal opened, and is used when
    /// the whole literal is at fault, such as an unterminated string.
    fn escape(&mut self, literal_start: Pos) -> Result<char, LexError> {
        // Step over the backslash. A diagnostic for a bad escape points at the
        // character after it, which is the thing that was unexpected.
        self.chars.bump();
        let esc_pos = self.chars.pos;
        let c = match self.chars.bump() {
            None | Some('\n') => return self.error(literal_start, "a complete escape sequence"),
            Some(c) => c,
        };
        Ok(match c {
            '0' => '\0',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            '\\' => '\\',
            '"' => '"',
            '\'' => '\'',
            'x' => {
                let hi = self.hex_digit(esc_pos, "two hexadecimal digits after `\\x`")?;
                let lo = self.hex_digit(esc_pos, "two hexadecimal digits after `\\x`")?;
                // A byte escape is a scalar value in the Latin-1 range, which
                // keeps the decoded String valid UTF-8.
                u8::try_from(hi * 16 + lo)
                    .map(char::from)
                    .unwrap_or('\u{FFFD}')
            }
            'u' => {
                if self.chars.peek() != Some('{') {
                    return self.error(self.chars.pos, "`{` after `\\u`");
                }
                self.chars.bump();
                let digits_start = self.chars.idx;
                let mut digits = String::new();
                loop {
                    match self.chars.peek() {
                        Some('}') => break,
                        Some(c) if c.is_ascii_hexdigit() && digits.len() < 6 => {
                            digits.push(c);
                            self.chars.bump();
                        }
                        None | Some('\n') => {
                            return self.error(esc_pos, "a closing `}` for the `\\u{...}` escape")
                        }
                        _ => return self.error(self.chars.pos, "a hexadecimal digit or `}`"),
                    }
                }
                let _ = digits_start;
                if digits.is_empty() {
                    return self.error(esc_pos, "at least one hexadecimal digit in `\\u{...}`");
                }
                if self.chars.peek() != Some('}') {
                    return self.error(self.chars.pos, "`}` after at most six hexadecimal digits");
                }
                self.chars.bump();
                let value = u32::from_str_radix(&digits, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .ok_or_else(|| {
                        self.make_error(esc_pos, "a Unicode scalar value in `\\u{...}`")
                    })?;
                value
            }
            other => {
                return self.error(
                    esc_pos,
                    &format!("a known escape sequence, but found `\\{}`", other),
                )
            }
        })
    }

    fn hex_digit(&mut self, esc_pos: Pos, expectation: &str) -> Result<u32, LexError> {
        match self.chars.peek() {
            Some(c) if c.is_ascii_hexdigit() => {
                self.chars.bump();
                Ok(c.to_digit(16).expect("checked as a hex digit"))
            }
            _ => self.error(esc_pos, expectation),
        }
    }

    /// Lexes punctuation, using maximal munch.
    fn punct(&mut self) -> Result<Tok, LexError> {
        let pos = self.chars.pos;
        let first = self.chars.peek().expect("caller checked for input");
        let second = self.chars.peek_at(1);
        let third = self.chars.peek_at(2);

        // Two character tokens first, so `==` wins over `=` and `..=` over `..`.
        if let (Some(a), Some(b)) = (Some(first), second) {
            let two = match (a, b) {
                ('+', '=') => Some(Tok::PlusAssign),
                ('-', '=') => Some(Tok::MinusAssign),
                ('*', '=') => Some(Tok::StarAssign),
                ('/', '=') => Some(Tok::SlashAssign),
                ('%', '=') => Some(Tok::PercentAssign),
                ('=', '=') => Some(Tok::EqEq),
                ('!', '=') => Some(Tok::NotEq),
                ('<', '=') => Some(Tok::LtEq),
                ('>', '=') => Some(Tok::GtEq),
                ('&', '&') => Some(Tok::AndAnd),
                ('|', '|') => Some(Tok::OrOr),
                ('-', '>') => Some(Tok::Arrow),
                ('=', '>') => Some(Tok::FatArrow),
                (':', ':') => Some(Tok::PathSep),
                ('.', '.') => {
                    if third == Some('=') {
                        Some(Tok::DotDotEq)
                    } else {
                        Some(Tok::DotDot)
                    }
                }
                _ => None,
            };
            if let Some(tok) = two {
                self.chars.bump();
                self.chars.bump();
                if tok == Tok::DotDotEq {
                    self.chars.bump();
                }
                return Ok(tok);
            }
        }

        let tok = match first {
            '+' => Tok::Plus,
            '-' => Tok::Minus,
            '*' => Tok::Star,
            '/' => Tok::Slash,
            '%' => Tok::Percent,
            '=' => Tok::Assign,
            '<' => Tok::Lt,
            '>' => Tok::Gt,
            '!' => Tok::Bang,
            '?' => Tok::Question,
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            '{' => Tok::LBrace,
            '}' => Tok::RBrace,
            '[' => Tok::LBracket,
            ']' => Tok::RBracket,
            '.' => Tok::Dot,
            ',' => Tok::Comma,
            ';' => Tok::Semicolon,
            ':' => Tok::Colon,
            '&' => Tok::Amp,
            '|' => Tok::Pipe,
            other => {
                return self.error(pos, &format!("a token, but found `{}`", other));
            }
        };
        self.chars.bump();
        Ok(tok)
    }

    /// The source text from a position to the current position.
    fn slice_from(&self, start: Pos) -> String {
        let start_idx = index_of(self.src, start).unwrap_or(0);
        let end_idx = index_of(self.src, self.chars.pos).unwrap_or(self.src.len());
        self.src[start_idx..end_idx].to_string()
    }
}

/// Converts a 1-based position to a byte index.
fn index_of(src: &str, pos: Pos) -> Option<usize> {
    let mut line = 1u32;
    let mut col = 1u32;
    for (idx, c) in src.char_indices() {
        if line == pos.line && col == pos.col {
            return Some(idx);
        }
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    if line == pos.line && col == pos.col {
        Some(src.len())
    } else {
        None
    }
}

fn strip_underscores(s: &str) -> String {
    s.chars().filter(|c| *c != '_').collect()
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Identifier tokens carry a `&'static str`, so the name is interned. A real
/// compiler interns through a symbol table; this keeps the token type simple
/// while the parser is still being written.
fn leak_name(name: String) -> &'static str {
    use std::collections::HashSet;
    use std::sync::Mutex;
    use std::sync::OnceLock;

    static TABLE: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let table = TABLE.get_or_init(|| Mutex::new(HashSet::new()));
    let mut table = table.lock().expect("intern table is not poisoned");
    if let Some(existing) = table.get(name.as_str()) {
        return existing;
    }
    let leaked: &'static str = Box::leak(name.into_boxed_str());
    table.insert(leaked);
    leaked
}
