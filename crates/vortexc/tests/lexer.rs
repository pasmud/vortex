//! Lexer tests.
//!
//! Coverage follows the acceptance criteria in `ROADMAP.md` stage 1: ordinary
//! input, string and character escapes, comments, and negative cases whose
//! diagnostics name the line, the column and what was expected.

use vortexc::lexer::lex;
use vortexc::token::Tok;

/// The kinds of every token except the trailing end of file token.
fn kinds(src: &str) -> Vec<Tok> {
    let tokens = lex(src).expect("source should lex");
    let mut kinds: Vec<Tok> = tokens.into_iter().map(|t| t.tok).collect();
    assert_eq!(
        kinds.pop(),
        Some(Tok::Eof),
        "the last token must be end of file"
    );
    kinds
}

/// Asserts the failure message of a source that must not lex.
fn expect_error(src: &str) -> vortexc::lexer::LexError {
    match lex(src) {
        Ok(tokens) => panic!(
            "expected a lex error, but lexed {} tokens from {:?}",
            tokens.len(),
            src
        ),
        Err(err) => err,
    }
}

// ---------------------------------------------------------------- ordinary input

#[test]
fn lexes_a_simple_let_binding() {
    assert_eq!(
        kinds("let x = 1"),
        vec![Tok::Ident("let"), Tok::Ident("x"), Tok::Assign, Tok::Int(1)]
    );
}

#[test]
fn lexes_an_integer_and_a_float() {
    assert_eq!(kinds("42 3.5"), vec![Tok::Int(42), Tok::Float(3.5)]);
}

#[test]
fn lexes_nested_blocks_and_punctuation() {
    let got = kinds("{ x[1] = f(2, 3); }");
    assert_eq!(
        got,
        vec![
            Tok::LBrace,
            Tok::Ident("x"),
            Tok::LBracket,
            Tok::Int(1),
            Tok::RBracket,
            Tok::Assign,
            Tok::Ident("f"),
            Tok::LParen,
            Tok::Int(2),
            Tok::Comma,
            Tok::Int(3),
            Tok::RParen,
            Tok::Semicolon,
            Tok::RBrace,
        ]
    );
}

#[test]
fn lexes_every_keyword_spelled_in_the_spec() {
    let keywords = [
        "as", "break", "continue", "else", "enum", "false", "fn", "for", "if", "impl", "import",
        "in", "let", "match", "return", "self", "struct", "trait", "true", "var", "while",
    ];
    for kw in keywords {
        let got = kinds(kw);
        assert_eq!(
            got,
            vec![Tok::Ident(kw)],
            "keyword {} lexed incorrectly",
            kw
        );
    }
}

#[test]
fn treats_identifiers_as_case_sensitive() {
    assert_eq!(
        kinds("x X x1 _x"),
        vec![
            Tok::Ident("x"),
            Tok::Ident("X"),
            Tok::Ident("x1"),
            Tok::Ident("_x")
        ]
    );
}

#[test]
fn accepts_both_slash_forms_of_comments_around_tokens() {
    assert_eq!(kinds("1 // trailing\n2"), vec![Tok::Int(1), Tok::Int(2)]);
}

#[test]
fn reports_end_of_file_position_on_the_last_line() {
    let tokens = lex("a\nbb").expect("should lex");
    let eof = tokens.last().expect("a token");
    assert_eq!(eof.tok, Tok::Eof);
    // Just past the last character of "bb", which starts at column 1.
    assert_eq!(eof.start, vortexc::Pos { line: 2, col: 3 });
}

#[test]
fn empty_input_is_only_end_of_file() {
    let tokens = lex("").expect("empty source should lex");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].tok, Tok::Eof);
    assert_eq!(tokens[0].start, vortexc::Pos::START);
}

#[test]
fn a_trailing_newline_does_not_add_an_empty_line() {
    // "a\n" ends with line 2 column 1, not line 3.
    let tokens = lex("a\n").expect("should lex");
    let eof = tokens.last().expect("a token");
    assert_eq!(eof.start, vortexc::Pos { line: 2, col: 1 });
}

// ---------------------------------------------------------------- integer literals

#[test]
fn lexes_radix_prefixed_integers() {
    assert_eq!(
        kinds("0xFF 0o755 0b1010"),
        vec![Tok::Int(255), Tok::Int(493), Tok::Int(10)]
    );
}

#[test]
fn lexes_underscore_separators() {
    assert_eq!(
        kinds("1_000_000 0xFF_FF 0b1010_1010"),
        vec![Tok::Int(1_000_000), Tok::Int(65535), Tok::Int(170)]
    );
}

#[test]
fn lexes_a_zero_by_itself() {
    assert_eq!(kinds("0"), vec![Tok::Int(0)]);
}

// ---------------------------------------------------------------- float literals

#[test]
fn lexes_floats_with_a_point_and_an_exponent() {
    assert_eq!(
        kinds("1.0 3.14159 1.5e10 2e-9"),
        vec![
            Tok::Float(1.0),
            Tok::Float(3.14159),
            Tok::Float(1.5e10),
            Tok::Float(2e-9)
        ]
    );
}

#[test]
fn a_point_not_followed_by_a_digit_ends_the_number() {
    // `SPEC.md` section 5.5: `1.foo` is the integer 1 then `.` then an
    // identifier, so a method call on a literal needs parentheses.
    assert_eq!(
        kinds("1.foo"),
        vec![Tok::Int(1), Tok::Dot, Tok::Ident("foo")]
    );
}

#[test]
fn a_trailing_dot_is_range_syntax_not_a_float() {
    assert_eq!(
        kinds("1..=10"),
        vec![Tok::Int(1), Tok::DotDotEq, Tok::Int(10)]
    );
}

// ---------------------------------------------------------------- strings

#[test]
fn lexes_an_empty_string() {
    assert_eq!(kinds("\"\""), vec![Tok::Str(String::new())]);
}

#[test]
fn lexes_a_string_with_ordinary_characters() {
    assert_eq!(
        kinds("\"hello world\""),
        vec![Tok::Str("hello world".to_string())]
    );
}

#[test]
fn decodes_string_escapes() {
    // The Vortex source is `"a\nb\tc\\d\"e\0f"`. `r##"..."##` ends at `"##`,
    // so the `\"` inside stays a backslash and a quote.
    assert_eq!(
        kinds(r##""a\nb\tc\\d\"e\0f""##),
        vec![Tok::Str("a\nb\tc\\d\"e\0f".to_string())]
    );
}

#[test]
fn decodes_carriage_return_escape() {
    assert_eq!(kinds("\"\\r\""), vec![Tok::Str("\r".to_string())]);
}

#[test]
fn decodes_hex_and_unicode_escapes() {
    // `r#"..."#` ends at `"#`, so a `\"` inside stays two characters.
    assert_eq!(kinds(r#""\x41""#), vec![Tok::Str("A".to_string())]);
    assert_eq!(
        kinds(r#""\u{1F600}""#),
        vec![Tok::Str("\u{1F600}".to_string())]
    );
}

// ---------------------------------------------------------------- characters

#[test]
fn lexes_character_literals() {
    let src = concat!("'a' '\\n' '\\u{1F600}' ", "'\\", "''");
    assert_eq!(
        kinds(src),
        vec![
            Tok::Char('a'),
            Tok::Char('\n'),
            Tok::Char('\u{1F600}'),
            Tok::Char('\''),
        ]
    );
}

#[test]
fn rejects_a_character_literal_with_two_characters() {
    let err = expect_error("'ab'");
    // The literal as a whole is at fault, so the diagnostic points at the
    // opening quote, as `SPEC.md` section 5.7 requires for a bad literal.
    assert_eq!(err.pos, vortexc::Pos { line: 1, col: 1 });
    assert!(
        err.message.contains("exactly one character"),
        "message was {:?}",
        err.message
    );
}

// ---------------------------------------------------------------- comments

#[test]
fn skips_a_line_comment_to_the_end_of_the_line() {
    assert_eq!(kinds("1 // two\n2"), vec![Tok::Int(1), Tok::Int(2)]);
}

#[test]
fn a_line_comment_at_end_of_file_terminates_at_end_of_file() {
    assert_eq!(kinds("1 // no newline"), vec![Tok::Int(1)]);
}

#[test]
fn skips_a_block_comment() {
    assert_eq!(kinds("1 /* two */ 3"), vec![Tok::Int(1), Tok::Int(3)]);
}

#[test]
fn a_block_comment_may_span_lines() {
    assert_eq!(kinds("1 /*\n2\n*/ 3"), vec![Tok::Int(1), Tok::Int(3)]);
}

#[test]
fn block_comments_nest() {
    // `SPEC.md` section 5.2: `/* block /* comments */ nest /* correctly */ */`
    assert_eq!(
        kinds("1 /* a /* b */ c */ 3"),
        vec![Tok::Int(1), Tok::Int(3)]
    );
}

#[test]
fn an_empty_block_comment_is_allowed() {
    assert_eq!(kinds("/**/"), vec![]);
}

#[test]
fn a_comment_only_file_is_only_end_of_file() {
    assert_eq!(kinds("// just a comment\n"), vec![]);
}

#[test]
fn an_unterminated_block_comment_points_at_the_opening_slash_star() {
    let err = expect_error("let x = 1;\n/* never closed\nlet y = 2;");
    assert_eq!(err.pos, vortexc::Pos { line: 2, col: 1 });
    assert!(
        err.message.contains("closing `*/`"),
        "message was {:?}",
        err.message
    );
}

// ---------------------------------------------------------------- negative cases

#[test]
fn an_unexpected_character_names_line_column_and_expectation() {
    let err = expect_error("let x = 1;\nlet y = @;\n");
    assert_eq!(err.pos, vortexc::Pos { line: 2, col: 9 });
    assert!(
        err.message.contains("expected"),
        "message must state what was expected, was {:?}",
        err.message
    );
    assert!(
        err.message.contains("a token"),
        "message must state what was expected, was {:?}",
        err.message
    );
    assert!(
        err.snippet.contains('^'),
        "the snippet must include a caret"
    );
    assert!(
        err.to_string().contains("2:9"),
        "the rendered error must name line and column, was {:?}",
        err.to_string()
    );
}

#[test]
fn an_unexpected_character_after_a_multi_byte_character_keeps_columns_counted_in_characters() {
    // Counting characters, the emoji is one column, so `@` is at column 14.
    let err = expect_error("let s = \"\u{1F600}\"; @\n");
    assert_eq!(err.pos, vortexc::Pos { line: 1, col: 14 });
}

#[test]
fn an_unterminated_string_points_at_the_opening_quote() {
    let err = expect_error("let s = \"no end here");
    assert_eq!(err.pos, vortexc::Pos { line: 1, col: 9 });
    assert!(
        err.message.contains("closing `\"`"),
        "message was {:?}",
        err.message
    );
}

#[test]
fn a_string_may_not_contain_a_raw_newline() {
    let err = expect_error("let s = \"open\nstill open\";");
    assert_eq!(err.pos, vortexc::Pos { line: 1, col: 9 });
    assert!(
        err.message.contains("closing `\"`"),
        "message was {:?}",
        err.message
    );
}

#[test]
fn an_unknown_escape_names_the_column_and_what_was_expected() {
    let err = expect_error("let s = \"a\\qb\";");
    // The diagnostic points at the escaped character, not the backslash.
    assert_eq!(err.pos, vortexc::Pos { line: 1, col: 12 });
    assert!(
        err.message.contains("expected") && err.message.contains("escape"),
        "message must name what was expected, was {:?}",
        err.message
    );
}

#[test]
fn a_truncated_hex_escape_names_the_column_and_what_was_expected() {
    let err = expect_error("let s = \"\\x4\";");
    assert_eq!(err.pos, vortexc::Pos { line: 1, col: 11 });
    assert!(
        err.message.contains("two hexadecimal digits"),
        "message was {:?}",
        err.message
    );
}

#[test]
fn an_out_of_range_unicode_escape_is_rejected() {
    let err = expect_error("let s = \"\\u{110000}\";");
    assert!(
        err.message.contains("Unicode scalar value"),
        "message was {:?}",
        err.message
    );
}

#[test]
fn an_unterminated_character_literal_points_at_the_opening_quote() {
    let err = expect_error("let c = 'a");
    assert_eq!(err.pos, vortexc::Pos { line: 1, col: 9 });
    assert!(
        err.message.contains("closing `'`"),
        "message was {:?}",
        err.message
    );
}

#[test]
fn an_empty_character_literal_is_rejected() {
    let err = expect_error("let c = '';");
    assert_eq!(err.pos, vortexc::Pos { line: 1, col: 9 });
}

#[test]
fn a_radix_prefix_without_digits_is_rejected() {
    let err = expect_error("let n = 0x;");
    // The diagnostic points at the character where a digit was expected.
    assert_eq!(err.pos, vortexc::Pos { line: 1, col: 11 });
    assert!(
        err.message.contains("at least one digit"),
        "message was {:?}",
        err.message
    );
}

#[test]
fn an_integer_too_large_for_64_bits_is_rejected() {
    let err = expect_error("let n = 18446744073709551616;");
    assert_eq!(err.pos, vortexc::Pos { line: 1, col: 9 });
    assert!(
        err.message.contains("64 bits"),
        "message was {:?}",
        err.message
    );
}

#[test]
fn the_first_error_is_returned_and_lexing_stops() {
    // The `@` comes before the unterminated string, so the reported error is
    // the earlier one.
    let err = expect_error("@ \"unterminated");
    assert_eq!(err.pos, vortexc::Pos { line: 1, col: 1 });
}

// ---------------------------------------------------------------- operator lexing

#[test]
fn maximal_munch_prefers_the_longer_operator() {
    assert_eq!(kinds("<="), vec![Tok::LtEq]);
    assert_eq!(kinds("<<"), vec![Tok::Lt, Tok::Lt]);
    assert_eq!(kinds("=="), vec![Tok::EqEq]);
    assert_eq!(kinds("="), vec![Tok::Assign]);
    assert_eq!(kinds("!="), vec![Tok::NotEq]);
    assert_eq!(kinds("!"), vec![Tok::Bang]);
    assert_eq!(kinds("=>"), vec![Tok::FatArrow]);
    assert_eq!(kinds("->"), vec![Tok::Arrow]);
    assert_eq!(kinds("&&"), vec![Tok::AndAnd]);
    assert_eq!(kinds("&"), vec![Tok::Amp]);
    assert_eq!(kinds("||"), vec![Tok::OrOr]);
    assert_eq!(kinds("|"), vec![Tok::Pipe]);
}

#[test]
fn lexes_the_compound_assignment_operators() {
    assert_eq!(
        kinds("+= -= *= /= %="),
        vec![
            Tok::PlusAssign,
            Tok::MinusAssign,
            Tok::StarAssign,
            Tok::SlashAssign,
            Tok::PercentAssign,
        ]
    );
}

#[test]
fn lexes_the_range_and_path_operators() {
    assert_eq!(
        kinds(".. ..= :: ?"),
        vec![Tok::DotDot, Tok::DotDotEq, Tok::PathSep, Tok::Question]
    );
}

// ---------------------------------------------------------------- example files

#[test]
fn the_example_programs_lex_cleanly() {
    for (name, src) in examples() {
        if let Err(err) = lex(&src) {
            panic!("example {} failed to lex: {}", name, err);
        }
    }
}

/// Reads every `.vx` file under `examples/`, relative to the workspace root.
fn examples() -> Vec<(String, String)> {
    use std::path::{Path, PathBuf};

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate lives two levels below the workspace root")
        .to_path_buf();

    let dir = PathBuf::from(root).join("examples");
    let mut out = Vec::new();
    let entries =
        std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("cannot read {}: {}", dir.display(), e));
    for entry in entries {
        let path = entry.expect("a readable directory entry").path();
        if path.extension().is_some_and(|e| e == "vx") {
            let src = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e));
            out.push((path.display().to_string(), src));
        }
    }
    assert!(
        !out.is_empty(),
        "no .vx example files found in {}",
        dir.display()
    );
    out.sort();
    out
}
