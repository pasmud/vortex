//! Checker tests.
//!
//! One test per diagnostic, asserting the message and the line and column, in
//! the same style as the stage 1 lexer tests and the stage 2 parser tests.
//! The list of diagnostics and the `SPEC.md` rule each one enforces is in
//! `crates/vortexc/DIAGNOSTICS.md`.
//!
//! Every test here goes through `run_source`, which runs parse, lower, check and
//! only then run. A program that fails the checker therefore never reaches the
//! interpreter, and the tests that check a diagnostic prove that by showing the
//! error came from the check stage.

use vortexc::{check_source, Error};

/// The error from a program that must not pass the checker.
fn rejected(src: &str) -> Error {
    match check_source(src) {
        Ok(_) => panic!(
            "expected a type error, but the program checked cleanly:\n{}",
            src
        ),
        Err(e) => e,
    }
}

/// A type error, asserted to be the check stage rather than a later one.
fn type_error(src: &str) -> String {
    let e = rejected(src);
    assert_eq!(
        e.stage(),
        "check",
        "expected the checker to reject this, got {:?} from the {} stage",
        e,
        e.stage()
    );
    e.to_string()
}

/// A rejection by an earlier stage, which is where that rule lives.
///
/// Lowering owns undefined names, assignment to an immutable binding, unknown
/// types and argument counts, so those programs never reach the checker. The
/// test records that rather than pretending the checker produced the error.
fn earlier_stage(src: &str, stage: &str) -> String {
    let e = rejected(src);
    assert_eq!(
        e.stage(),
        stage,
        "expected the {} stage to reject this, got {:?} from the {} stage",
        stage,
        e,
        e.stage()
    );
    e.to_string()
}

/// Asserts the diagnostic, its line and column, and a fragment of its message.
fn assert_at(src: &str, line: u32, col: u32, fragment: &str) {
    let text = type_error(src);
    let position = format!("{}:{}", line, col);
    assert!(
        text.contains(&position),
        "expected the diagnostic to name {} but it was {:?}",
        position,
        text
    );
    assert!(
        text.contains(fragment),
        "expected the message to mention {:?} but it was {:?}",
        fragment,
        text
    );
}

/// Asserts a program passes the checker and runs.
fn accepted(src: &str) {
    let mut out = Vec::new();
    if let Err(e) = vortexc::run_source(src, &mut out) {
        panic!("expected this to run, got {} from the {} stage", e, e.stage());
    }
}

// ---------------------------------------------------------------- checking runs first

#[test]
fn an_ill_typed_program_never_reaches_the_interpreter() {
    // The program would print and then divide by zero if it ran. It is rejected
    // by the checker, so the interpreter is never entered and no output exists.
    let src = "fn main() {\n    let a = \"text\";\n    let b = a - 1;\n    println(int_to_string(b));\n}";
    let mut out = Vec::new();
    let err = match vortexc::run_source(src, &mut out) {
        Ok(_) => panic!("expected the checker to reject this program"),
        Err(e) => e,
    };
    assert_eq!(err.stage(), "check");
    assert!(
        out.is_empty(),
        "nothing should have been printed, but {:?} was",
        String::from_utf8_lossy(&out)
    );
}

#[test]
fn a_well_typed_program_runs_and_prints() {
    let mut out = Vec::new();
    vortexc::run_source("fn main() { println(int_to_string(1 + 1)); }", &mut out).expect("should run");
    assert_eq!(String::from_utf8_lossy(&out), "2\n");
}

// ---------------------------------------------------------------- section 6.1 rule 1, static types

#[test]
fn a_non_bool_condition_is_rejected() {
    assert_at(
        "fn main() {\n    if 1 { }\n}",
        2,
        8,
        "`if` condition must be `Bool`, found `Int`",
    );
}

#[test]
fn a_non_bool_while_condition_is_rejected() {
    assert_at(
        "fn main() {\n    while 1.5 { }\n}",
        2,
    11,
        "`while` condition must be `Bool`, found `Float`",
    );
}

#[test]
fn a_range_that_is_not_int_is_rejected() {
    assert_at(
        "fn main() {\n    for i in 0..=1.5 { }\n}",
        2,
    18,
        "both ends must be `Int`",
    );
}

#[test]
fn a_non_int_index_is_rejected() {
    assert_at(
        "fn main() {\n    let a = [1, 2];\n    let b = a[\"x\"];\n}",
        3,
    15,
        "an index must be an `Int`, found `Str`",
    );
}

#[test]
fn a_return_of_the_wrong_type_is_rejected() {
    assert_at(
        "fn f() -> Int {\n    return \"text\";\n}",
        2,
    12,
        "this returns `Str` where `Int` is expected",
    );
}

#[test]
fn a_function_that_cannot_reach_its_return_is_rejected() {
    assert_at(
        "fn f() -> Int {\n    let a = 1;\n}\nfn main() { }",
        1,
    1,
        "can reach the end without returning",
    );
}

// ---------------------------------------------------------------- section 6.1 rule 2, nominal

#[test]
fn two_structs_with_identical_fields_are_different_types() {
    // Both are `{ x: Int, y: Int }` in shape, but section 6.1 rule 2 makes them
    // different types, so they do not mix.
    assert_at(
        "struct A { x: Int, y: Int }\nstruct B { x: Int, y: Int }\nfn main() {\n    let a = A { x: 1, y: 2 };\n    let b = B { x: 1, y: 2 };\n    let c = a == b;\n}",
        6,
    15,
    "cannot compare",
    );
}

#[test]
fn an_unknown_struct_is_rejected() {
    assert_at(
        "fn main() {\n    let p = Missing { x: 1 };\n}",
        2,
    13,
        "unknown struct `Missing`",
    );
}

#[test]
fn a_field_that_does_not_exist_is_rejected() {
    assert_at(
        "struct P { x: Int }\nfn main() {\n    let p = P { x: 1 };\n    let y = p.z;\n}",
        4,
    14,
        "has no field `z`",
    );
}

#[test]
fn a_field_of_the_wrong_type_is_rejected() {
    assert_at(
        "struct P { x: Int, y: Int }\nfn main() {\n    let p = P { x: 1, y: \"s\" };\n}",
        3,
    13,
        "field `y` of `P` is `Str` where `Int` is expected",
    );
}

#[test]
fn a_missing_field_is_rejected() {
    assert_at(
        "struct P { x: Int, y: Int }\nfn main() {\n    let p = P { x: 1 };\n}",
        3,
    21,
        "`P` has 2 fields, found 1 value",
    );
}

#[test]
fn an_unknown_type_name_is_rejected() {
    assert_at(
        "fn f(x: Nonesuch) -> Int { return 1; }\nfn main() { }",
        1,
    1,
        "unknown type `Nonesuch`",
    );
}

// ---------------------------------------------------------------- section 6.1 rule 3, structural

#[test]
fn a_tuple_is_typed_by_its_elements() {
    accepted("fn main() { let t = (1, 2.5); let a = t.0; }");
}

#[test]
fn a_wrong_sized_tuple_is_rejected() {
    assert_at(
        "fn main() {\n    let t = (1, 2);\n    let a = t.9;\n}",
        3,
    14,
        "cannot read a field of `()`",
    );
}

#[test]
fn a_list_with_mixed_element_types_is_rejected() {
    assert_at(
        "fn main() {\n    let a = [1, \"two\"];\n}",
        2,
    17,
        "every element of a list has one type",
    );
}

// ---------------------------------------------------------------- section 6.1 rule 4, no widening

#[test]
fn mixing_int_and_float_in_an_arithmetic_expression_is_rejected() {
    assert_at(
        "fn main() {\n    let a = 1 + 1.5;\n}",
        2,
    15,
        "rule 4 forbids mixing `Int` and `Float`",
    );
}

#[test]
fn comparing_int_with_float_is_rejected() {
    assert_at(
        "fn main() {\n    let a = 1 < 1.5;\n}",
        2,
    15,
        "cannot compare `Int` with `Float`",
    );
}

#[test]
fn subtracting_a_float_from_an_int_is_rejected() {
    assert_at(
        "fn main() {\n    var a = 10;\n    a = a - 0.5;\n}",
        3,
    11,
        "rule 4 forbids mixing `Int` and `Float`",
    );
}

#[test]
fn int_and_float_arithmetic_is_accepted_on_their_own() {
    accepted("fn main() { let a = 1 + 2; let b = 1.5 + 2.5; }");
}

// ---------------------------------------------------------------- section 6.1 rule 7, one type per name

#[test]
fn assigning_a_different_type_to_a_let_is_rejected() {
    assert_at(
        "fn main() {\n    let a = 1;\n    a = 1.5;\n}",
        3,
    9,
        "section 6.1 rule 7 fixes a name's type",
    );
}

#[test]
fn assigning_the_right_type_to_a_var_is_accepted() {
    accepted("fn main() { var a = 1; a = 2; }");
}

// ---------------------------------------------------------------- section 6.1 rule 8, shadowing

#[test]
fn shadowing_a_let_in_the_same_scope_is_rejected() {
    assert_at(
        "fn main() {\n    let a = 1;\n    let a = 2;\n}",
        3,
    9,
        "already declared in this scope",
    );
}

#[test]
fn a_name_may_be_reused_in_a_nested_scope() {
    accepted("fn main() { let a = 1; { let a = 2; } }");
}

// ---------------------------------------------------------------- arguments and returns

#[test]
fn an_argument_of_the_wrong_type_is_rejected() {
    assert_at(
        "fn f(a: Int) -> Int { return a; }\nfn main() {\n    let x = f(\"text\");\n}",
        3,
    14,
        "argument 1 of `f` is `Str` where `Int` is expected",
    );
}

#[test]
fn a_return_type_that_does_not_match_the_signature_is_rejected() {
    assert_at(
        "fn f() -> Int { return 1.5; }\nfn main() { }",
        1,
    1,
        "this returns `Float` where `Int` is expected",
    );
}

// ---------------------------------------------------------------- enums and match

#[test]
fn an_enum_payload_of_the_wrong_type_is_rejected() {
    assert_at(
        "enum S { a(Int) }\nfn main() {\n    let s = S.a(\"text\");\n}",
        3,
    14,
        "value 1 of `S.a` is `Str` where `Int` is expected",
    );
}

#[test]
fn an_unknown_variant_is_rejected() {
    assert_at(
        "enum S { a }\nfn main() {\n    let s = S.b;\n}",
        3,
    14,
        "enum `S` has no variant `b`",
    );
}

#[test]
fn a_match_whose_arms_disagree_is_rejected() {
    assert_at(
        "fn f(n: Int) -> Int {\n    let a = match n { 1 => 1, _ => \"s\" };\n    return a;\n}\nfn main() { }",
        2,
    36,
        "this arm produces `Str` where an earlier arm produces `Int`",
    );
}

#[test]
fn a_match_over_int_without_a_wildcard_is_rejected() {
    assert_at(
        "fn f(n: Int) -> Int {\n    let a = match n { 1 => 1 };\n    return a;\n}\nfn main() { }",
        2,
    28,
        "no `_` arm",
    );
}

#[test]
fn a_match_covering_every_variant_of_an_enum_needs_no_wildcard() {
    accepted(
        "enum S { a, b }\nfn f(s: S) -> Int {\n    let x = match s { S.a => 1, S.b => 2 };\n    return x;\n}\nfn main() { }",
    );
}

#[test]
fn an_arm_that_can_never_match_is_rejected() {
    assert_at(
        "fn f(n: Int) -> Int {\n    let a = match n { \"text\" => 1, _ => 2 };\n    return a;\n}\nfn main() { }",
        2,
    28,
        "this arm can never match an `Int`",
    );
}

// ---------------------------------------------------------------- builtins

#[test]
fn a_builtin_with_the_wrong_argument_type_is_rejected() {
    assert_at(
        "fn main() {\n    let s = int_to_string(1.5);\n}",
        2,
    26,
        "argument 1 of `int_to_string` is `Float` where `Int` is expected",
    );
}

#[test]
fn a_builtin_with_the_wrong_arity_is_rejected() {
    assert_at(
        "fn main() {\n    let s = int_to_string();\n}",
        2,
    26,
        "`int_to_string` takes 1 value, found 0",
    );
}

#[test]
fn print_accepts_any_number_of_any_type() {
    accepted("fn main() { println(1); println(\"s\"); println(1.5, 2, true); }");
}

// ---------------------------------------------------------------- what is enforced
//
// These tests say plainly which rules are checked, so a rule that is dropped
// later fails here rather than silently.

#[test]
fn int_and_float_operations_do_not_mix_without_a_cast() {
    // Section 6.1 rule 4. `as` arrives with the rest of stage 3; until it
    // exists there is no way between them, which is what these tests pin down.
    for src in [
        "fn main() { let a = 1 + 1.5; }",
        "fn main() { let a = 1.5 * 2; }",
        "fn main() { let a = 1 - 2.0; }",
        "fn main() { let a = 1 / 2.0; }",
        "fn main() { let a = 1 % 2.0; }",
    ] {
        let e = type_error(src);
        assert!(
            e.contains("rule 4"),
            "rule 4 should be what rejects this, was {:?}",
            e
        );
    }
}

#[test]
fn a_struct_field_cannot_hold_another_structs_type() {
    // Rule 2 again, through a field rather than a comparison.
    assert_at(
        "struct A { n: Int }\nstruct B { n: Int }\nstruct Holder { a: A }\nfn main() {\n    let h = Holder { a: B { n: 1 } };\n}",
        5,
    13,
        "field `a` of `Holder` is `B` where `A` is expected",
    );
}
