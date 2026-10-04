//! Parser tests.
//!
//! Coverage follows the stage 2 acceptance criteria in `ROADMAP.md`: the
//! programs in `examples/` parse, the constructs `SPEC.md` section 5 and 8
//! describe are accepted, and negative cases name the line, the column and what
//! was expected, in the same style as the stage 1 lexer tests.

use vortexc::ast::*;
use vortexc::parser::parse;
use vortexc::Pos;

/// Asserts a source parses, returning the program.
fn ok(src: &str) -> Program {
    parse(src).unwrap_or_else(|e| panic!("expected a parse, got:\n{}", e))
}

/// Asserts a source fails, returning the error.
fn err(src: &str) -> vortexc::parser::ParseError {
    match parse(src) {
        Ok(p) => panic!(
            "expected a parse error, but parsed {} items from {:?}",
            p.items.len(),
            src
        ),
        Err(e) => e,
    }
}

/// The only function in a parsed program.
fn only_fn(program: &Program) -> &Spanned<FnDecl> {
    assert_eq!(program.items.len(), 1, "expected exactly one item");
    match &program.items[0] {
        Item::Function(f) => f,
        other => panic!("expected a function, found {:?}", other),
    }
}

// ---------------------------------------------------------------- the examples

#[test]
fn every_example_program_parses() {
    for (name, src) in examples() {
        if let Err(e) = parse(&src) {
            panic!("example {} failed to parse: {}", name, e);
        }
    }
}

#[test]
fn a_program_may_declare_functions_structs_and_enums() {
    let p = ok(
        r#"
        struct Point { x: Int, y: Int }
        enum Shape { empty, circle(Float) }
        fn main() {}
        "#,
    );
    assert_eq!(p.items.len(), 3);
    assert!(matches!(p.items[0], Item::Struct(_)));
    assert!(matches!(p.items[1], Item::Enum(_)));
    assert!(matches!(p.items[2], Item::Function(_)));
}

// ---------------------------------------------------------------- functions

#[test]
fn parses_a_function_with_no_parameters_or_return_type() {
    let p = ok("fn main() { }");
    let f = only_fn(&p);
    assert_eq!(f.node.name, "main");
    assert!(f.node.params.is_empty());
    assert!(f.node.ret.is_none());
}

#[test]
fn parses_a_function_with_parameters_and_a_return_type() {
    let p = ok("fn add(a: Int, b: Int) -> Int { return a + b; }");
    let f = only_fn(&p);
    assert_eq!(f.node.params.len(), 2);
    assert_eq!(f.node.params[0].name, "a");
    assert_eq!(f.node.params[1].ty, TypeExpr::Named("Int".into()));
    assert_eq!(f.node.ret, Some(TypeExpr::Named("Int".into())));
}

#[test]
fn a_parameter_is_immutable_unless_written_var() {
    // `SPEC.md` section 8: a parameter is immutable unless it is `var`.
    let p = ok("fn f(a: Int, var b: Int) -> Int { return a + b; }");
    let f = only_fn(&p);
    assert!(!f.node.params[0].mutable, "`a` should be immutable");
    assert!(f.node.params[1].mutable, "`b` should be mutable");
}

#[test]
fn accepts_a_trailing_comma_in_a_parameter_list() {
    let p = ok("fn f(a: Int, b: Int,) -> Int { return a; }");
    assert_eq!(only_fn(&p).node.params.len(), 2);
}

#[test]
fn accepts_a_trailing_comma_in_a_field_list() {
    let p = ok("struct Point { x: Int, y: Int, }");
    match &p.items[0] {
        Item::Struct(s) => assert_eq!(s.node.fields.len(), 2),
        other => panic!("expected a struct, found {:?}", other),
    }
}

// ---------------------------------------------------------------- structs and enums

#[test]
fn parses_an_enum_with_all_three_variant_shapes() {
    let p = ok(
        r#"
        enum Shape {
            empty,
            circle(Float),
            rect { w: Float, h: Float },
        }
        "#,
    );
    match &p.items[0] {
        Item::Enum(e) => {
            let v = &e.node.variants;
            assert_eq!(v[0].name, "empty");
            assert!(v[0].payloads.is_empty() && v[0].named.is_empty());
            assert_eq!(v[1].payloads.len(), 1);
            assert_eq!(v[2].named.len(), 2);
            assert_eq!(v[2].named[0].name, "w");
        }
        other => panic!("expected an enum, found {:?}", other),
    }
}

#[test]
fn a_variant_path_type_is_accepted() {
    let p = ok("fn f(s: Shape.rect) -> Float { return 0.0; }");
    let f = only_fn(&p);
    assert_eq!(
        f.node.params[0].ty,
        TypeExpr::Named("Shape.rect".into()),
        "`Shape.rect` should read as one path type"
    );
}

// ---------------------------------------------------------------- statements

#[test]
fn parses_let_and_var_declarations() {
    let p = ok("fn main() { let a = 1; var b = 2; }");
    let stmts = &only_fn(&p).node.body.stmts;
    assert_eq!(stmts.len(), 2);
    match (&stmts[0].kind, &stmts[1].kind) {
        (StmtKind::Let { name, mutable, .. }, StmtKind::Let { name: n2, .. }) => {
            assert_eq!(name, "a");
            assert!(!mutable);
            assert_eq!(n2, "b");
        }
        other => panic!("unexpected statements: {:?}", other),
    }
}

#[test]
fn parses_an_if_as_a_statement() {
    let p = ok("fn main() { if true { return 1; } return 0; }");
    assert_eq!(only_fn(&p).node.body.stmts.len(), 2);
}

#[test]
fn parses_an_if_as_an_expression_with_an_else() {
    let p = ok("fn main() { let a = if true { 1 } else { 2 }; }");
    let stmts = &only_fn(&p).node.body.stmts;
    match &stmts[0].kind {
        StmtKind::Let { init, .. } => match &init.kind {
            ExprKind::If { otherwise, .. } => {
                assert!(otherwise.is_some(), "the `else` branch should be present");
            }
            other => panic!("expected an if expression, found {:?}", other),
        },
        other => panic!("expected a let, found {:?}", other),
    }
}

#[test]
fn parses_an_if_with_an_if_else_chain() {
    let p = ok("fn main() { let a = if true { 1 } else if false { 2 } else { 3 }; }");
    assert_eq!(only_fn(&p).node.body.stmts.len(), 1);
}

#[test]
fn parses_while_and_for_in_both_forms() {
    let p = ok(
        r#"
        fn main() {
            while true { break; }
            for i in 0..=10 { continue; }
            for x in [1, 2] { println(x); }
        }
        "#,
    );
    let stmts = &only_fn(&p).node.body.stmts;
    assert_eq!(stmts.len(), 3);
    match &stmts[1].kind {
        StmtKind::For { end, inclusive, .. } => {
            assert!(end.is_some(), "a counting `for` needs an end");
            assert!(inclusive, "`..=` is inclusive");
        }
        other => panic!("expected a for, found {:?}", other),
    }
    match &stmts[2].kind {
        StmtKind::For { end, .. } => {
            assert!(end.is_none(), "`for x in e` has no range end");
        }
        other => panic!("expected a for, found {:?}", other),
    }
}

#[test]
fn parses_a_block_as_the_tail_of_a_function() {
    // `SPEC.md` section 8: a block is an expression, so its final expression is
    // its value.
    let p = ok("fn f() -> Int { 42 }");
    let f = only_fn(&p);
    assert!(f.node.body.tail.is_some(), "the block should have a tail");
}

// ---------------------------------------------------------------- expressions

#[test]
fn parses_literals() {
    let p = ok(r#"fn main() { let a = 1; let b = 1.5; let c = "s"; let d = 'c'; }"#);
    assert_eq!(only_fn(&p).node.body.stmts.len(), 4);
}

#[test]
fn parses_every_literal_form() {
    let p = ok(
        r#"
        fn main() {
            let a = 1_000;
            let b = 0xFF;
            let c = 0o17;
            let d = 0b1010;
            let e = 1.5e3;
            let f = true;
        }
        "#,
    );
    assert_eq!(only_fn(&p).node.body.stmts.len(), 6);
}

#[test]
fn parses_unary_and_binary_operators() {
    let p = ok("fn main() { let a = -1 + !true * 2 - 3 / 4 % 5; }");
    assert_eq!(only_fn(&p).node.body.stmts.len(), 1);
}

#[test]
fn parses_comparisons_and_logic() {
    let p = ok("fn main() { let a = 1 < 2 && 3 >= 4 || 5 == 6 || 7 != 8; }");
    assert_eq!(only_fn(&p).node.body.stmts.len(), 1);
}

#[test]
fn binary_operators_bind_by_precedence() {
    // `1 + 2 * 3` is `1 + (2 * 3)`, not `(1 + 2) * 3`.
    let p = ok("fn main() { let a = 1 + 2 * 3; }");
    let stmts = &only_fn(&p).node.body.stmts;
    match &stmts[0].kind {
        StmtKind::Let { init, .. } => match &init.kind {
            ExprKind::Binary { op, rhs, .. } => {
                assert_eq!(*op, BinOp::Add);
                match &rhs.kind {
                    ExprKind::Binary { op, .. } => assert_eq!(*op, BinOp::Mul),
                    other => panic!("expected a multiply on the right, found {:?}", other),
                }
            }
            other => panic!("expected a binary, found {:?}", other),
        },
        other => panic!("expected a let, found {:?}", other),
    }
}

#[test]
fn parses_an_assignment_as_a_statement() {
    let p = ok("fn main() { var a = 0; a = 5; }");
    let stmts = &only_fn(&p).node.body.stmts;
    assert_eq!(stmts.len(), 2);
    match &stmts[1].kind {
        StmtKind::Expr(e) => match &e.kind {
            ExprKind::Assign { name, .. } => assert_eq!(name, "a"),
            other => panic!("expected an assignment, found {:?}", other),
        },
        other => panic!("expected an expression, found {:?}", other),
    }
}

#[test]
fn parses_calls_arrays_indexing_and_field_reads() {
    let p = ok("fn main() { let a = f(1, 2); let b = [1, 2][0]; let c = p.x; }");
    assert_eq!(only_fn(&p).node.body.stmts.len(), 3);
}

#[test]
fn parses_a_struct_literal() {
    let p = ok("struct Point { x: Int, y: Int } fn main() { let p = Point { x: 1, y: 2 }; }");
    let stmts = &f_of(&p).node.body.stmts;
    match &stmts[0].kind {
        StmtKind::Let { init, .. } => match &init.kind {
            ExprKind::Record { ty, fields } => {
                assert_eq!(ty, "Point");
                assert_eq!(fields.len(), 2);
            }
            other => panic!("expected a record, found {:?}", other),
        },
        other => panic!("expected a let, found {:?}", other),
    }
}

#[test]
fn a_braces_block_after_an_if_condition_is_not_a_struct_literal() {
    // The ambiguity between `if c { .. }` and `Point { .. }` is resolved by
    // only allowing a struct literal where a statement cannot start. If the
    // parser got this wrong, this program would not parse.
    let p = ok("struct P { a: Int } fn main() { if true { let p = P { a: 1 }; } }");
    assert_eq!(f_of(&p).node.body.stmts.len(), 1);
}

#[test]
fn parses_a_tuple_and_the_parenthesised_form() {
    let p = ok("fn main() { let a = (1, 2); let b = (3); }");
    let stmts = &only_fn(&p).node.body.stmts;
    match &stmts[0].kind {
        StmtKind::Let { init, .. } => assert!(matches!(init.kind, ExprKind::Tuple(ref v) if v.len() == 2)),
        other => panic!("expected a let, found {:?}", other),
    }
    match &stmts[1].kind {
        StmtKind::Let { init, .. } => assert!(matches!(init.kind, ExprKind::Paren(_))),
        other => panic!("expected a let, found {:?}", other),
    }
}

#[test]
fn parses_a_try_operator() {
    let p = ok("fn main() { let a = f()?; }");
    assert_eq!(only_fn(&p).node.body.stmts.len(), 1);
}

// ---------------------------------------------------------------- match and patterns

#[test]
fn parses_a_match_on_a_tuple_with_several_alternatives_in_one_arm() {
    let p = ok(
        r#"
        fn f(i: Int) -> Str {
            let label = match (i % 3, i % 5) {
                (0, 0) => "FizzBuzz",
                (0, _) | (_, 0) => "other",
                _ => int_to_string(i),
            };
            return label;
        }
        "#,
    );
    let f = only_fn(&p);
    match &f.node.body.stmts[0].kind {
        StmtKind::Let { init, .. } => match &init.kind {
            ExprKind::Match { arms, .. } => {
                assert_eq!(arms.len(), 3, "the middle arm holds two alternatives");
                assert_eq!(arms[1].patterns.len(), 2);
            }
            other => panic!("expected a match, found {:?}", other),
        },
        other => panic!("expected a let, found {:?}", other),
    }
}

#[test]
fn parses_variant_patterns_with_positional_and_named_bindings() {
    let p = ok(
        r#"
        enum Shape { empty, circle(Float), rect { w: Float, h: Float } }
        fn f(s: Shape) -> Float {
            let a = match s {
                Shape.empty => 0.0,
                Shape.circle(r) => r,
                Shape.rect { w, h } => w * h,
            };
            return a;
        }
        "#,
    );
    match &f_of(&p).node.body.stmts[0].kind {
        StmtKind::Let { init, .. } => match &init.kind {
            ExprKind::Match { arms, .. } => match &arms[1].patterns[0].kind {
                PatternKind::Variant { bindings, .. } => assert_eq!(bindings.len(), 1),
                other => panic!("expected a variant pattern, found {:?}", other),
            },
            other => panic!("expected a match, found {:?}", other),
        },
        other => panic!("expected a let, found {:?}", other),
    }
}

#[test]
fn parses_a_binding_pattern() {
    let p = ok("fn f(x: Int) -> Int { let a = match x { y => y, _ => 0 }; return a; }");
    match &f_of(&p).node.body.stmts[0].kind {
        StmtKind::Let { init, .. } => match &init.kind {
            ExprKind::Match { arms, .. } => assert!(matches!(
                arms[0].patterns[0].kind,
                PatternKind::Binding(_)
            )),
            other => panic!("expected a match, found {:?}", other),
        },
        other => panic!("expected a let, found {:?}", other),
    }
}

fn f_of(program: &Program) -> &Spanned<FnDecl> {
    match &program.items[program.items.len() - 1] {
        Item::Function(f) => f,
        other => panic!("expected a function, found {:?}", other),
    }
}

// ---------------------------------------------------------------- negative cases
//
// Each of these asserts the line, the column and what was expected.

#[test]
fn a_missing_semicolon_names_the_line_column_and_expectation() {
    let err = err("fn main() {\n    let a = 1\n    let b = 2;\n}");
    assert_eq!(err.pos, Pos { line: 3, col: 5 }, "the caret belongs on the next line");
    assert!(
        err.message.contains("expected `;`"),
        "the message must say what was expected, was {:?}",
        err.message
    );
    assert!(
        err.snippet.contains('^'),
        "the snippet must carry a caret"
    );
    assert!(
        err.to_string().contains("3:5"),
        "the rendered error must name line and column, was {:?}",
        err.to_string()
    );
}

#[test]
fn a_missing_closing_brace_names_line_column_and_expectation() {
    let err = err("fn main() {\n    let a = 1;\n");
    // The source ends on line 3, so the `}` is expected just past line 2.
    assert_eq!(err.pos, Pos { line: 3, col: 1 });
    assert!(
        err.message.contains("`}`"),
        "message must name the closing brace, was {:?}",
        err.message
    );
}

#[test]
fn an_unclosed_string_is_reported_by_the_lexer_through_the_parser() {
    let err = err("fn main() {\n    let a = \"open;\n}");
    assert_eq!(err.pos, Pos { line: 2, col: 13 });
    assert!(
        err.message.contains("closing `\"`"),
        "a lex error should keep its own message, was {:?}",
        err.message
    );
}

#[test]
fn a_top_level_item_must_be_a_keyword() {
    let err = err("let x = 1;");
    assert_eq!(err.pos, Pos { line: 1, col: 1 });
    assert!(
        err.message.contains("`fn`") && err.message.contains("found"),
        "message must name what was expected and what was found, was {:?}",
        err.message
    );
}

#[test]
fn a_missing_function_name_names_the_column() {
    let err = err("fn (a: Int) {}");
    assert_eq!(err.pos, Pos { line: 1, col: 4 });
    assert!(
        err.message.contains("function name"),
        "message was {:?}",
        err.message
    );
}

#[test]
fn a_missing_parameter_type_names_the_column() {
    let err = err("fn f(a) {}");
    assert_eq!(err.pos, Pos { line: 1, col: 7 });
    assert!(
        err.message.contains("`:`"),
        "message was {:?}",
        err.message
    );
}

#[test]
fn a_missing_colon_in_a_field_list_names_the_column() {
    let err = err("struct Point { x Int }");
    assert_eq!(err.pos, Pos { line: 1, col: 18 });
    assert!(
        err.message.contains("`:`"),
        "message was {:?}",
        err.message
    );
}

#[test]
fn a_missing_in_after_a_for_variable_names_the_column() {
    let err = err("fn main() { for i 0..=3 { } }");
    assert_eq!(err.pos, Pos { line: 1, col: 19 });
    assert!(
        err.message.contains("`in`"),
        "message was {:?}",
        err.message
    );
}

#[test]
fn a_missing_match_arrow_names_the_column() {
    let err = err("fn f(x: Int) -> Int { let a = match x { 0 1, _ => 2 }; return a; }");
    assert_eq!(err.pos.line, 1);
    assert!(
        err.message.contains("`=>`"),
        "message was {:?}",
        err.message
    );
}

#[test]
fn a_call_on_something_that_is_not_a_name_names_the_column() {
    let err = err("fn main() { let a = 1(2); }");
    assert_eq!(err.pos, Pos { line: 1, col: 22 });
    assert!(
        err.message.contains("function name"),
        "message was {:?}",
        err.message
    );
}

#[test]
fn a_keyword_where_an_expression_belongs_names_the_column() {
    let err = err("fn main() { let a = while; }");
    assert_eq!(err.pos, Pos { line: 1, col: 21 });
    assert!(
        err.message.contains("expression") && err.message.contains("keyword"),
        "message must say what was expected, was {:?}",
        err.message
    );
}

#[test]
fn the_error_position_counts_characters_not_bytes() {
    // The emoji is one column wide, so the caret is at column 24, not at a
    // byte offset.
    let err = err("fn main() { let s = \"\u{1F600}\"; let = 1; }");
    assert_eq!(err.pos, Pos { line: 1, col: 30 });
}

// ---------------------------------------------------------------- helpers

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
    let entries = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", dir.display(), e));
    for entry in entries {
        let path = entry.expect("a readable directory entry").path();
        if path.extension().is_some_and(|e| e == "vx") {
            let src = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e));
            out.push((path.display().to_string(), src));
        }
    }
    assert!(!out.is_empty(), "no .vx example files found in {}", dir.display());
    out.sort();
    out
}