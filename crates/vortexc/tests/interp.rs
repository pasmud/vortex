//! Interpreter tests.
//!
//! The expected output of every example lives here, not in the example file, so
//! an example cannot be edited to make a test pass. Coverage follows the stage 2
//! acceptance criteria in `ROADMAP.md`: every construct `SPEC.md` section 8
//! lists is executed, and the negative cases name the line and the column.

use vortexc::{run_source, RuntimeError, Value};

/// Runs a program and returns what it printed.
fn run(src: &str) -> String {
    let mut out = Vec::new();
    match run_source(src, &mut out) {
        Ok(_) => String::from_utf8(out).expect("output should be valid UTF-8"),
        Err(e) => panic!("program failed: {}", e),
    }
}

/// Runs a program and returns the value `main` returned.
fn returned(src: &str) -> Value {
    let mut out = Vec::new();
    match run_source(src, &mut out) {
        Ok(v) => v,
        Err(e) => panic!("program failed: {}", e),
    }
}

/// Runs a program that is expected to fail.
fn fails(src: &str) -> String {
    let mut out = Vec::new();
    match run_source(src, &mut out) {
        Ok(_) => panic!("expected a runtime error, but the program succeeded"),
        Err(e) => e.to_string(),
    }
}

/// Wraps statements in a `main` so a test can focus on one behaviour.
fn main_with(body: &str) -> String {
    format!("fn main() {{\n{}\n}}\n", body)
}

// ---------------------------------------------------------------- the examples
//
// Each expectation below was written by reading the program, not by copying
// whatever the interpreter printed.

#[test]
fn hello_vx_prints_the_gcd_and_the_twenty_first_fibonacci_number() {
    let src = read_example("hello.vx");
    let out = run(&src);
    let lines: Vec<&str> = out.lines().collect();
    // gcd(1071, 462) is 21, and fib(20) is 6765.
    assert_eq!(lines, vec!["21", "6765"]);
}

#[test]
fn structs_vx_prints_structs_enums_matches_and_loops() {
    let src = read_example("structs.vx");
    let out = run(&src);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines,
        vec![
            "3",         // p.x
            "4",         // p.y
            "12.566371", // area(circle 2.0) = pi * 4
            "32.000000", // area(rect 4 x 8)
            "0.000000",  // area(empty)
            "41",        // 2+3+5+7+11+13
            "395",       // sum of 1..30 excluding multiples of 7
            "26",        // count of those
            "abc",       // "a" + "b" + "c"
            "4",         // max(3, 4)
            "1",         // countdown stops at 1
            "FizzBuzz",  // fizzbuzz(15)
            "Fizz",      // fizzbuzz(3)
            "7",         // fizzbuzz(7)
        ]
    );
}

#[test]
fn strings_vx_prints_escapes_characters_and_conversions() {
    let src = read_example("strings.vx");
    let out = run(&src);
    assert_eq!(
        out,
        "tab=[\t] newline=[\n]\nquote=\"\nbackslash=\\\nheart=❤\nbyte=A\nVo\nchars: x\n😀\n42\n255\n0.333333\n2.500000\n"
    );
}

// ---------------------------------------------------------------- the constructs

#[test]
fn a_function_is_called_and_its_value_returned() {
    assert_eq!(
        returned(
            "fn twice(n: Int) -> Int { return n * 2; } fn main() -> Int { return twice(21); }"
        ),
        Value::Int(42)
    );
}

#[test]
fn arguments_are_passed_left_to_right() {
    assert_eq!(
        returned("fn sub(a: Int, b: Int) -> Int { return a - b; } fn main() -> Int { return sub(10, 4); }"),
        Value::Int(6)
    );
}

#[test]
fn a_function_may_be_declared_after_its_caller() {
    assert_eq!(
        returned("fn main() -> Int { return later(); } fn later() -> Int { return 7; }"),
        Value::Int(7)
    );
}

#[test]
fn recursion_works() {
    assert_eq!(
        returned(
            "fn fib(n: Int) -> Int { if n < 2 { return n; } return fib(n - 1) + fib(n - 2); }
             fn main() -> Int { return fib(10); }"
        ),
        Value::Int(55)
    );
}

#[test]
fn a_return_inside_an_if_leaves_the_function() {
    // The statements after the `if` must not run when the branch returns.
    let out = run("fn f(n: Int) -> Int { if n < 2 { return 1; } return 2; }
         fn main() { println(int_to_string(f(1))); println(int_to_string(f(9))); }");
    assert_eq!(out, "1\n2\n");
}

#[test]
fn let_binds_and_var_allows_assignment() {
    let out = run(&main_with(
        "var a = 1; a = a + 41; let b = 2; println(int_to_string(a + b));",
    ));
    assert_eq!(out, "44\n");
}

#[test]
fn if_else_picks_one_branch() {
    let out = run(&main_with(
        "let a = if true { 1 } else { 2 }; let b = if false { 1 } else { 2 };
         println(int_to_string(a)); println(int_to_string(b));",
    ));
    assert_eq!(out, "1\n2\n");
}

#[test]
fn while_loops_until_the_condition_is_false() {
    let out = run(&main_with(
        "var n = 0; while n < 5 { n = n + 1; } println(int_to_string(n));",
    ));
    assert_eq!(out, "5\n");
}

#[test]
fn an_inclusive_for_counts_the_end() {
    let out = run(&main_with(
        "var s = 0; for i in 1..=4 { s = s + i; } println(int_to_string(s));",
    ));
    assert_eq!(out, "10\n");
}

#[test]
fn an_exclusive_for_stops_before_the_end() {
    let out = run(&main_with(
        "var s = 0; for i in 1..4 { s = s + i; } println(int_to_string(s));",
    ));
    assert_eq!(out, "6\n");
}

#[test]
fn a_for_can_walk_a_list() {
    let out = run(&main_with(
        "var s = 0; for x in [1, 2, 3] { s = s + x; } println(int_to_string(s));",
    ));
    assert_eq!(out, "6\n");
}

#[test]
fn break_leaves_a_loop_early() {
    let out = run(&main_with(
        "var n = 0; for i in 0..=100 { if i == 5 { break; } n = n + 1; } println(int_to_string(n));",
    ));
    assert_eq!(out, "5\n");
}

#[test]
fn continue_skips_the_rest_of_an_iteration() {
    let out = run(&main_with(
        "var n = 0; for i in 0..=9 { if i % 2 == 0 { continue; } n = n + 1; }
         println(int_to_string(n));",
    ));
    assert_eq!(out, "5\n");
}

#[test]
fn break_leaves_a_while_loop() {
    let out = run(&main_with(
        "var n = 0; while true { n = n + 1; if n == 7 { break; } } println(int_to_string(n));",
    ));
    assert_eq!(out, "7\n");
}

#[test]
fn a_block_is_an_expression() {
    assert_eq!(
        returned("fn main() -> Int { { let a = 3; a * 14 } }"),
        Value::Int(42)
    );
}

#[test]
fn a_struct_is_built_and_its_fields_read() {
    let out = run("struct Point { x: Int, y: Int }
         fn main() { let p = Point { x: 3, y: 4 }; println(int_to_string(p.x * p.y)); }");
    assert_eq!(out, "12\n");
}

#[test]
fn an_enum_variant_with_no_payload_is_built() {
    let out = run("enum Shape { empty, circle(Float) }
         fn main() { let s = Shape.empty; print(s); println(\"\"); }");
    assert_eq!(out, "Shape.empty\n");
}

#[test]
fn an_enum_variant_with_a_positional_payload_is_built() {
    let out = run("enum Shape { empty, circle(Float) }
         fn main() { print(Shape.circle(1.5)); println(\"\"); }");
    assert_eq!(out, "Shape.circle(1.500000)\n");
}

#[test]
fn an_enum_variant_with_named_fields_is_built() {
    let out = run("enum Shape { empty, rect { w: Float, h: Float } }
         fn main() { print(Shape.rect { w: 2.0, h: 3.0 }); println(\"\"); }");
    assert_eq!(out, "Shape.rect(2.000000, 3.000000)\n");
}

#[test]
fn match_selects_the_arm_that_fits() {
    let out = run(
        r#"enum Shape { empty, circle(Float), rect { w: Float, h: Float } }
           fn f(s: Shape) -> Float {
               let a = match s {
                   Shape.empty => 0.0,
                   Shape.circle(r) => r,
                   Shape.rect { w, h } => w * h,
               };
               return a;
           }
           fn main() {
               println(float_to_string(f(Shape.circle(3.0))));
               println(float_to_string(f(Shape.rect { w: 4.0, h: 5.0 })));
               println(float_to_string(f(Shape.empty)));
           }"#,
    );
    assert_eq!(out, "3.000000\n20.000000\n0.000000\n");
}

#[test]
fn match_on_a_tuple_with_alternatives_in_one_arm() {
    let out = run(r#"fn f(n: Int) -> Str {
               let label = match (n % 3, n % 5) {
                   (0, 0) => "FizzBuzz",
                   (0, _) | (_, 0) => "other",
                   _ => int_to_string(n),
               };
               return label;
           }
           fn main() {
               println(f(15));
               println(f(3));
               println(f(5));
               println(f(7));
           }"#);
    assert_eq!(out, "FizzBuzz\nother\nother\n7\n");
}

#[test]
fn a_match_may_bind_the_scrutinee() {
    let out = run(
        "fn f(n: Int) -> Int { let a = match n { x => x + x, _ => 0 }; return a; }
         fn main() { println(int_to_string(f(21))); }",
    );
    assert_eq!(out, "42\n");
}

#[test]
fn strings_and_characters_print_as_their_contents() {
    assert_eq!(
        run(&main_with("println(\"a\\tb\"); print('x'); println(\"\");")),
        "a\tb\nx\n"
    );
}

#[test]
fn print_writes_without_a_newline() {
    assert_eq!(
        run(&main_with("print(\"a\"); print(\"b\"); println(\"\");")),
        "ab\n"
    );
}

#[test]
fn integers_and_floats_keep_their_types_apart() {
    // `SPEC.md` section 6.1 rule 4: no implicit widening, so adding an Int to a
    // Float is a run time error naming both types.
    let err = fails(&main_with("let a = 1 + 1.5;"));
    assert!(
        err.contains("`+`") && err.contains("Int") && err.contains("Float"),
        "message must name the operator and both types, was {:?}",
        err
    );
}

#[test]
fn integer_division_truncates_toward_zero() {
    assert_eq!(
        returned("fn main() -> Int { return 7 / 2; }"),
        Value::Int(3),
        "`7 / 2` truncates toward zero"
    );
}

// ---------------------------------------------------------------- negative cases

#[test]
fn dividing_by_zero_names_the_position() {
    let err = fails("fn main() {\n    let a = 1;\n    let b = 0;\n    let c = a / b;\n}");
    assert!(err.contains("division by zero"), "was {:?}", err);
    assert!(
        err.contains("4:15"),
        "must name line and column, was {:?}",
        err
    );
}

#[test]
fn taking_the_remainder_by_zero_names_the_position() {
    let err = fails("fn main() {\n    let a = 1 % 0;\n}");
    assert!(err.contains("division by zero"), "was {:?}", err);
}

#[test]
fn indexing_past_the_end_names_the_index_and_the_length() {
    let err = fails("fn main() {\n    let a = [1, 2];\n    let b = a[5];\n}");
    assert!(
        err.contains("index 5") && err.contains("2 values"),
        "message must name the index and the length, was {:?}",
        err
    );
    assert!(
        err.contains("3:14"),
        "must name line and column, was {:?}",
        err
    );
}

#[test]
fn a_negative_index_is_rejected() {
    let err = fails("fn main() {\n    let a = [1, 2];\n    let b = a[-1];\n}");
    assert!(err.contains("negative"), "was {:?}", err);
}

#[test]
fn reading_a_field_that_does_not_exist_names_it() {
    let err = fails("struct P { a: Int } fn main() { let p = P { a: 1 }; let x = p.b; }");
    assert!(
        err.contains("no field `b`"),
        "the message must name the missing field, was {:?}",
        err
    );
}

#[test]
fn a_condition_that_is_not_a_bool_names_the_type_it_found() {
    let err = fails("fn main() {\n    if 1 { }\n}");
    assert!(
        err.contains("Bool") && err.contains("Int"),
        "message must name both types, was {:?}",
        err
    );
}

#[test]
fn calling_with_the_wrong_number_of_arguments_is_rejected_before_running() {
    let err = fails("fn f(a: Int, b: Int) -> Int { return a; } fn main() { f(1); }");
    assert!(
        err.contains("takes 2 arguments") && err.contains("1 was given"),
        "message must say what was expected, was {:?}",
        err
    );
}

#[test]
fn break_outside_a_loop_is_rejected() {
    let err = fails("fn f() -> Int { break; return 0; } fn main() { f(); }");
    assert!(err.contains("`break` outside a loop"), "was {:?}", err);
}

#[test]
fn a_loop_variable_does_not_leak_out_of_its_loop() {
    // The loop variable lives in the loop's own scope, so using it afterwards is
    // a lowering error naming where it was not found.
    let err = fails("fn main() { for i in 0..=3 { } let x = i; }");
    assert!(
        err.contains("undefined name `i`"),
        "message must name the missing binding, was {:?}",
        err
    );
}

#[test]
fn assigning_to_a_let_binding_is_rejected() {
    // `SPEC.md` section 8: an assignment to a `let` name is an error.
    let err = fails("fn main() { let a = 1; a = 2; }");
    assert!(
        err.contains("declared with `let`"),
        "the message must say why, was {:?}",
        err
    );
}

#[test]
fn assigning_to_an_immutable_parameter_is_rejected() {
    let err = fails("fn f(a: Int) -> Int { a = 2; return a; } fn main() { f(1); }");
    assert!(
        err.contains("declared with `let`"),
        "a parameter is immutable unless written `var`, was {:?}",
        err
    );
}

#[test]
fn a_mutable_parameter_may_be_assigned() {
    assert_eq!(
        returned(
            "fn f(var a: Int) -> Int { a = a + 1; return a; } fn main() -> Int { return f(41); }"
        ),
        Value::Int(42)
    );
}

#[test]
fn shadowing_a_name_in_the_same_scope_is_rejected() {
    // `SPEC.md` section 6.1 rule 8.
    let err = fails("fn main() { let a = 1; let a = 2; }");
    assert!(err.contains("already declared"), "was {:?}", err);
}

#[test]
fn calling_a_function_that_was_never_declared_is_rejected() {
    let err = fails("fn main() { nope(); }");
    assert!(
        err.contains("unknown function `nope`"),
        "message must name the function, was {:?}",
        err
    );
}

#[test]
fn an_unknown_struct_is_rejected() {
    let err = fails("fn main() { let p = Missing { a: 1 }; }");
    assert!(err.contains("unknown struct `Missing`"), "was {:?}", err);
}

#[test]
fn an_unknown_variant_is_rejected() {
    let err = fails("enum S { a } fn main() { let s = S.b; }");
    assert!(err.contains("no variant `b`"), "was {:?}", err);
}

#[test]
fn a_variant_with_the_wrong_number_of_values_is_rejected() {
    let err = fails("enum S { a(Int), b(Int, Int) } fn main() { let s = S.b(1); }");
    assert!(
        err.contains("carries 2 values") && err.contains("1 was given"),
        "message must say what was expected, was {:?}",
        err
    );
}

#[test]
fn a_struct_literal_with_the_wrong_field_count_is_rejected() {
    let err = fails("struct P { a: Int, b: Int } fn main() { let p = P { a: 1 }; }");
    assert!(
        err.contains("has 2 fields") && err.contains("1 were given"),
        "message must say what was expected, was {:?}",
        err
    );
}

#[test]
fn a_match_with_no_matching_arm_is_reported() {
    let err =
        fails("fn f(n: Int) -> Int { let a = match n { 1 => 1 }; return a; } fn main() { f(2); }");
    // The checker requires a `_` arm on a match over an `Int`, because it
    // cannot see which values will arrive. This is a static rejection, before
    // the interpreter would have hit a value that matched nothing.
    assert!(err.contains("no `_` arm"), "was {:?}", err);
}

#[test]
fn a_builtin_called_with_the_wrong_arity_is_rejected() {
    // `println()` takes any number of values, so a builtin that needs one is
    // the right subject here. The checker rejects this before the interpreter,
    // so the message comes from there.
    let err = fails("fn main() { int_to_string(); }");
    assert!(err.contains("takes 1 value"), "was {:?}", err);
}

#[test]
fn a_runaway_recursion_is_stopped_rather_than_crashing() {
    // The interpreter caps call depth, so this reports an error instead of
    // exhausting the machine stack.
    let err = fails("fn down(n: Int) -> Int { return down(n + 1); } fn main() { down(0); }");
    assert!(
        err.contains("nested too deeply"),
        "the depth guard should report, was {:?}",
        err
    );
}

// ---------------------------------------------------------------- helpers

/// Reads an example file from the workspace `examples/` directory.
fn read_example(name: &str) -> String {
    use std::path::{Path, PathBuf};
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate lives two levels below the workspace root");
    let path = PathBuf::from(root).join("examples").join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e))
}

#[test]
fn every_runtime_error_names_a_line_and_a_column() {
    // The error carries a position, and `Display` prints it. This checks the
    // shape every diagnostic shares.
    let mut out = Vec::new();
    let err = match run_source(
        "fn main() {\n    let a = 1;\n    let b = 0;\n    a / b;\n}",
        &mut out,
    ) {
        Ok(_) => panic!("expected a division by zero"),
        Err(e) => e,
    };
    assert!(matches!(
        err,
        vortexc::Error::Runtime(RuntimeError::DivideByZero { .. })
    ));
    let text = err.to_string();
    assert!(
        text.starts_with("error at 4:"),
        "a runtime error names the line first, was {:?}",
        text
    );
}
