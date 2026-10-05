//! The compiled path, checked against the two engines already in the tree.
//!
//! `DECISION.md` decided the backend is an ahead-of-time compiler over
//! `crates/vortexc/src/ir.rs` and made no performance claim about it. These
//! tests make the claim that can be checked: that a compiled path exists and
//! agrees with the tree interpreter and the VM.
//!
//! Agreement is the whole point. A compiled path that disagrees with the two
//! engines is worse than no compiled path, so every case here compares all
//! three. Nothing here asserts that the compiled path is faster, because it
//! has not been measured on a workload where that would mean anything.

use std::process::Command;

use vortexc::ast::{FnDecl, Item, Spanned};
use vortexc::Error;

/// A program written so the emitted C is valid: no tail expression, no list,
/// every parameter a scalar, and a `main` that returns the value.
fn program(n: i64) -> String {
    format!(
        "fn sum_squares(n: Int) -> Int {{
    var total = 0;
    var i = 1;
    while i <= n {{
        total = total + i * i;
        i = i + 1;
    }}
    return total;
}}

fn main() -> Int {{
    let r = sum_squares({});
    return r;
}}
",
        n
    )
}

fn tree(src: &str) -> String {
    value(vortexc::run_source(src, &mut std::io::sink()))
}

fn vm(src: &str) -> String {
    value(vortexc::run_on_vm(src, &mut std::io::sink()))
}

fn value(r: Result<vortexc::Value, Error>) -> String {
    match r {
        Ok(v) => vortexc::interp::display(&v),
        Err(e) => format!("error: {}", e),
    }
}

/// Emits the program, compiles it with the system C compiler, and runs it.
fn compiled(src: &str) -> Result<String, String> {
    let ast = match vortexc::parse(src) {
        Ok(a) => a,
        Err(e) => return Err(format!("parse: {}", e)),
    };
    let items = ast.items.clone();
    let functions: Vec<Spanned<FnDecl>> = ast
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Function(f) => Some(f.clone()),
            _ => None,
        })
        .collect();

    // The argument the Vortex `main` passes, so the compiled path runs on the
    // same input as the engines rather than on a value chosen here.
    let arg = argument(&functions, "sum_squares");
    let c = match vortexc::cgen::emit_program(&items, "sum_squares", &[arg]) {
        Ok(c) => c,
        Err(e) => return Err(format!("not emitted: {}", e)),
    };

    // A directory per call. Keying by process id is not enough, because two
    // tests share one process and the second overwrote the first's binary,
    // which then failed with "Text file busy".
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("vortex-cgen-test-{}-{}", std::process::id(), n));
    let _ = std::fs::create_dir_all(&dir);
    let cfile = dir.join("program.c");
    let bin = dir.join("program");
    std::fs::write(&cfile, &c).map_err(|e| e.to_string())?;

    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let build = Command::new(&cc)
        .args(["-O2", "-o"])
        .arg(&bin)
        .arg(&cfile)
        .output()
        .map_err(|e| format!("could not run the C compiler: {}", e))?;
    if !build.status.success() {
        return Err(format!(
            "the C compiler rejected the generated source:\n{}",
            String::from_utf8_lossy(&build.stderr)
        ));
    }

    let out = Command::new(&bin)
        .output()
        .map_err(|e| format!("could not run the compiled program: {}", e))?;
    // The generated binary writes what main returned to standard error, and a
    // program that prints an answer writes it to standard output instead.
    let returned = String::from_utf8_lossy(&out.stderr)
        .lines()
        .find(|l| l.starts_with("vortex_returned "))
        .map(|l| l.trim_start_matches("vortex_returned ").to_string());
    Ok(match returned {
        Some(v) if !v.is_empty() => v,
        _ => String::from_utf8_lossy(&out.stdout).trim().to_string(),
    })
}

/// What the tree interpreter printed to standard output.
fn tree_stdout(src: &str) -> String {
    let mut out: Vec<u8> = Vec::new();
    let _ = vortexc::run_source(src, &mut out);
    String::from_utf8_lossy(&out).trim().to_string()
}

/// What the bytecode VM printed to standard output.
fn vm_stdout(src: &str) -> String {
    let mut out: Vec<u8> = Vec::new();
    let _ = vortexc::run_on_vm(src, &mut out);
    String::from_utf8_lossy(&out).trim().to_string()
}

/// What the tree interpreter reported on standard error, which is where a
/// diagnostic goes.
fn tree_error(src: &str) -> String {
    value(vortexc::run_source(src, &mut std::io::sink()))
}

/// Compiles and runs a program, returning what it printed to standard output.
///
/// The earlier helper returns the entry's return value, which is right for the
/// loops it was written for and wrong for a program that prints. A test that
/// only matched strings in the generated C could not run the program at all, and
/// so could not tell a working index from a byte offset that happened to agree
/// on ASCII. These tests run it.
fn compiled_stdout(src: &str, entry: &str) -> Result<String, String> {
    let ast = match vortexc::parse(src) {
        Ok(a) => a,
        Err(e) => return Err(format!("parse: {}", e)),
    };
    let c = match vortexc::cgen::emit_program(&ast.items, entry, &[]) {
        Ok(c) => c,
        Err(e) => return Err(format!("not emitted: {}", e)),
    };
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("vortex-cgen-out-{}-{}", std::process::id(), n));
    let _ = std::fs::create_dir_all(&dir);
    let cfile = dir.join("program.c");
    let bin = dir.join("program");
    std::fs::write(&cfile, &c).map_err(|e| e.to_string())?;

    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let mut seen: Option<String> = None;
    for opt in ["-O2", "-O0"] {
        let build = Command::new(&cc)
            .arg(opt)
            .args(["-o"])
            .arg(&bin)
            .arg(&cfile)
            .output()
            .map_err(|e| format!("could not run the C compiler: {}", e))?;
        if !build.status.success() {
            return Err(format!(
                "the C compiler at {} rejected the generated source:\n{}",
                opt,
                String::from_utf8_lossy(&build.stderr)
            ));
        }
        let out = Command::new(&bin)
            .output()
            .map_err(|e| format!("could not run the compiled program: {}", e))?;
        if !out.status.success() {
            return Err(format!(
                "the compiled program at {} failed: {}",
                opt,
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        // Both levels are checked and have to agree with each other, because
        // `-O2` can quietly repair an emitter mistake and `-O0` cannot. A
        // difference between them is reported rather than passed over.
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if let Some(first) = &seen {
            if *first != stdout {
                return Err(format!(
                    "the compiled program printed {:?} at -O2 and {:?} at -O0",
                    first, stdout
                ));
            }
        } else {
            seen = Some(stdout);
        }
    }
    Ok(seen.unwrap_or_default())
}

/// The literal argument the Vortex `main` passes to the named function.
fn argument(functions: &[Spanned<FnDecl>], func: &str) -> String {
    for f in functions {
        if f.node.name != "main" {
            continue;
        }
        for stmt in &f.node.body.stmts {
            let e = match &stmt.kind {
                vortexc::ast::StmtKind::Expr(e) => e,
                vortexc::ast::StmtKind::Let { init, .. } => init,
                _ => continue,
            };
            if let vortexc::ast::ExprKind::Call { callee, args } = &e.kind {
                if callee == func {
                    if let Some(vortexc::ast::ExprKind::Int(v)) = args.first().map(|a| &a.kind) {
                        return format!("INT64_C({})", v);
                    }
                }
            }
        }
    }
    "INT64_C(0)".to_string()
}

// ---------------------------------------------------------------- agreement

#[test]
fn the_compiled_path_agrees_with_the_tree_interpreter_and_the_vm() {
    // Each n is a different loop trip count, so agreement on one input would be
    // weak evidence.
    for n in [10, 25, 100, 1000] {
        let src = program(n);
        let compiled = compiled(&src).unwrap_or_else(|e| panic!("n={}: {}", n, e));
        let tree = tree(&src);
        let vm = vm(&src);

        assert_eq!(
            compiled, tree,
            "the compiled path and the tree interpreter disagree at n={}",
            n
        );
        assert_eq!(
            compiled, vm,
            "the compiled path and the vm disagree at n={}",
            n
        );
    }
}

#[test]
fn the_compiled_path_gets_the_expected_answer_not_just_the_same_one() {
    // Agreement between three engines is not evidence if all three are wrong in
    // the same way, so the answer is checked against the known sum of squares.
    let src = program(10);
    assert_eq!(tree(&src), "385");
    assert_eq!(compiled(&src).unwrap(), "385");
    assert_eq!(program(25).as_str(), program(25).as_str());
    assert_eq!(tree(&program(25)), "5525");
}

// ---------------------------------------------------------------- what it does not do

#[test]
fn an_if_expression_is_emitted_as_a_ternary() {
    // An if in a value position became a C conditional, which keeps a block
    // value out of the emitted source entirely.
    let src = "fn f(n: Int) -> Int { if n > 1 { 1 } else { 0 } }
               fn main() -> Int { let r = f(1); return r; }";
    let ast = vortexc::parse(src).expect("should parse");
    let functions: Vec<_> = ast
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Function(f) => Some(f.clone()),
            _ => None,
        })
        .collect();
    let f = functions
        .iter()
        .find(|f| f.node.name == "f")
        .expect("the function should be there");
    // A block whose branches both return is emitted as a statement if, which
    // is the same shape the interpreter runs. The value comes from the return,
    // not from a conditional expression.
    let c = vortexc::cgen::emit_function_alone(f).expect("an if should emit now");
    assert!(
        c.contains("if ("),
        "the branch should be an if, said {:?}",
        c
    );
    assert!(
        c.contains("return"),
        "each branch should return, said {:?}",
        c
    );
}

#[test]
fn the_emitter_still_refuses_rather_than_emitting_broken_c() {
    // A construct the emitter does not handle must produce a named reason, not
    // C that fails to compile with no explanation. Indexing a string was the
    // one that was refused from stage 11; stage 14 carries it, so the remaining
    // refusal here is a store into a string, which the tree interpreter does not
    // define either.
    let src = "fn poke(w: Str) { w[0] = 65; }
               fn main() -> Int { return 0; }";
    let ast = vortexc::parse(src).expect("should parse");
    let functions: Vec<_> = ast
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Function(f) => Some(f.clone()),
            _ => None,
        })
        .collect();
    let f = functions
        .iter()
        .find(|f| f.node.name == "poke")
        .expect("the function should be there");
    match vortexc::cgen::emit_function_alone(f) {
        Ok(_) => panic!("storing into a string should not be emitted"),
        Err(e) => {
            let text = e.to_string();
            assert!(
                text.contains("storing into a string"),
                "the refusal should name the construct, said {:?}",
                text
            );
        }
    }
}

/// A string index counts characters, not bytes, and the test runs the C.
///
/// The first version of this test used the ASCII string "Vortex" and only
/// matched a string in the generated C. For ASCII a byte offset and a character
/// index are the same number, so the test could not tell a correct index from
/// one that read the wrong byte, and it never ran the program. That is the stage
/// 7 sieve lesson again: an integer-only workload could not reveal a `Float`
/// defect, and an ASCII-only test could not reveal a byte-offset defect.
///
/// This one uses "héllo", whose second character is two bytes, compiles and runs
/// the C at `-O2` and at `-O0`, and asserts the known answer on all three paths.
#[test]
fn a_string_index_counts_characters_not_bytes() {
    let src = "fn main() {
        let w = \"héllo\";
        print(w[2]);
        print(w[0]);
        println(\"\");
    }";
    assert_eq!(tree_stdout(src), "lh", "the tree interpreter's own answer");
    assert_eq!(vm_stdout(src), "lh", "the VM's own answer");
    match compiled_stdout(src, "main") {
        Ok(out) => assert_eq!(out, "lh", "the compiled path at -O2 and -O0"),
        Err(e) => panic!("the string index should compile and run: {}", e),
    }
}

/// A string index past the end is rejected rather than reading past the NUL.
///
/// Reading past the terminator is undefined behaviour in C. The tree interpreter
/// and the VM report a bad index naming the index and the character count, and
/// the compiled path has to fail rather than return a placeholder.
#[test]
fn a_string_index_past_the_end_is_rejected() {
    let src = "fn main() {
        let w = \"héllo\";
        print(w[9]);
        println(\"\");
    }";
    let tree_out = tree_error(src);
    assert!(
        tree_out.contains("past the end of 5 characters"),
        "the tree interpreter should report the bad index, said {:?}",
        tree_out
    );
    // Five characters, not six bytes: the count is characters, because the
    // diagnostic says characters.
    let vm_out = value(vortexc::run_on_vm(src, &mut std::io::sink()));
    assert!(
        vm_out.contains("past the end of 5 characters"),
        "the VM should report the same, said {:?}",
        vm_out
    );
    match compiled_stdout(src, "main") {
        Ok(out) => panic!("a bad index should not print {:?}, it should fail", out),
        Err(e) => assert!(
            e.contains("past the end of 5 characters"),
            "the compiled path should name the index and the character count, said {:?}",
            e
        ),
    }
}

/// A `match` is emitted, and the arms are tried in order, which is what the
/// tree interpreter does.
#[test]
fn a_match_is_emitted() {
    let src = "fn f(n: Int) -> Int { let x = match n { 0 => 1, _ => 2 }; return x; }
               fn main() -> Int { return f(0); }";
    let ast = vortexc::parse(src).expect("should parse");
    let c = vortexc::cgen::emit_program(&ast.items, "main", &[]).expect("a match should emit");
    assert!(
        c.contains("=="),
        "a match on a literal should emit a comparison, said {:?}",
        c.lines().find(|l| l.contains("?")).unwrap_or("")
    );
}

/// A `match` returns the type of its arms, not Int.
///
/// Without this a `match` returning a Float was declared int64_t and truncated,
/// which is the same class of defect stage 8 fixed for a call holding a Float.
#[test]
fn a_match_takes_the_type_of_its_arms() {
    let src = "fn f(n: Int) -> Float { let x = match n { 0 => 1.5, _ => 2.5 }; return x; }
               fn main() -> Float { return f(0); }";
    let ast = vortexc::parse(src).expect("should parse");
    let c = vortexc::cgen::emit_program(&ast.items, "main", &[]).expect("a match should emit");
    assert!(
        c.contains("double x ="),
        "a let holding a Float match should be declared double, said {:?}",
        c.lines().find(|l| l.contains(" x = ")).unwrap_or("")
    );
}

#[test]
fn a_list_program_is_emitted_as_a_pointer_and_a_length() {
    // A Vortex list is one owner and a known size, which is what SPEC.md
    // section 7 describes, so it becomes a C struct with a pointer and a length.
    let src = "fn main() -> Int { let a = [1, 2]; return a[0]; }";
    let ast = vortexc::parse(src).expect("should parse");
    let functions: Vec<_> = ast
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Function(f) => Some(f.clone()),
            _ => None,
        })
        .collect();
    let f = functions
        .iter()
        .find(|f| f.node.name == "main")
        .expect("main should be there");
    let c = vortexc::cgen::emit_function_alone(f).expect("a list should emit now");
    assert!(
        c.contains("CList"),
        "the declaration should be a list, said {:?}",
        c
    );
    assert!(
        c.contains("vortex_list_new"),
        "a list literal should build a list, said {:?}",
        c
    );
}

/// A function that declares no return type returns nothing.
///
/// This was emitted as returning `int64_t`, so the generated entry read
/// `return vortex_main();` on a `void` function. gcc at -O2 accepts that with
/// a warning and gcc at -O0 rejects it, which is how it survived several stages.
#[test]
fn a_void_entry_emits_a_void_wrapper() {
    let src = "fn go() {\n    println(\"done\");\n}\n";
    let ast = vortexc::parse(src).expect("a void function should parse");
    let c =
        vortexc::cgen::emit_program(&ast.items, "go", &[]).expect("a void function should emit");
    assert!(
        !c.contains("int64_t vortex_c_entry"),
        "the entry wrapper should be void, said {:?}",
        c.lines()
            .find(|l| l.contains("vortex_c_entry"))
            .unwrap_or("")
    );
    assert!(
        c.contains("void vortex_c_entry(void)"),
        "the entry wrapper should declare void, said {:?}",
        c.lines()
            .find(|l| l.contains("vortex_c_entry"))
            .unwrap_or("")
    );
}

/// A list of a declared struct carries its elements at the struct's type.
///
/// The element type used to default to `int64_t` for anything that was not
/// `Int`, `Float` or `Str`, so a `Float` field truncated with no diagnostic.
/// The `Float` field is the point: an `Int` field would have been right at
/// `int64_t` and would not have shown the defect.
#[test]
fn a_list_of_a_struct_keeps_its_float_field() {
    let src = "struct P { x: Int, y: Float }
               fn main() -> Int {
                   var ps = [P { x: 1, y: 2.5 }; 2];
                   ps[1] = P { x: 7, y: 8.75 };
                   println(float_to_string(ps[1].y));
                   return 0;
               }";
    let ast = vortexc::parse(src).expect("should parse");
    let c = vortexc::cgen::emit_program(&ast.items, "main", &[])
        .expect("a list of a struct should emit");
    // The element type has to be the struct, not an int64_t.
    assert!(
        c.contains("vortex_list_new_C"),
        "a list of a struct should use the struct's constructor, said {:?}",
        c.lines()
            .find(|l| l.contains("vortex_list_new_C"))
            .unwrap_or("")
    );
    assert!(
        !c.contains("((int64_t *)(ps).items)"),
        "the elements should not be read at int64_t"
    );
}

/// A variant whose payload fields differ in type is carried, not refused.
///
/// Those are not a C array, so they go in a generated struct with one field
/// each. This was a refusal; stage 12 replaced it with support.
#[test]
fn a_variant_with_mixed_payload_types_is_carried() {
    let src = "enum Pair { mixed { n: Int, r: Float } }
               fn pick(p: Pair) -> Int {
                   return match p { Pair.mixed { n, r } => n };
               }
               fn main() -> Int { return pick(Pair.mixed { n: 5, r: 1.5 }); }";
    let ast = vortexc::parse(src).expect("should parse");
    let c = vortexc::cgen::emit_program(&ast.items, "main", &[])
        .expect("a mixed payload variant should emit");
    assert!(
        c.contains("int64_t f0; double f1;"),
        "a mixed payload should be a struct with one field per payload, said {:?}",
        c.lines().find(|l| l.contains("f0")).unwrap_or("")
    );
}

/// An index into a name the emitter cannot type is refused, not guessed.
///
/// This used to read at `int64_t`, which is right for an `Int` list and
/// truncates every `Float` element, so the answer was wrong with no
/// diagnostic.
#[test]
fn an_untyped_index_is_refused_rather_than_read_at_int64() {
    let src = "fn take(n: Int) -> Int { return hidden[n]; }
               fn hidden(n: Int) -> Int { return 0; }
               fn main() -> Int { return take(0); }";
    let ast = vortexc::parse(src).expect("should parse");
    let err = vortexc::cgen::emit_program(&ast.items, "main", &[])
        .expect_err("an index into an untyped name should be refused");
    let text = err.to_string();
    assert!(
        text.contains("element type the emitter cannot name"),
        "the refusal should say the element type is unknown, said {:?}",
        text
    );
}

/// `for c in s` walks a string by character, not by byte.
///
/// The emitter assumed every `for ..in` walked a list, so it bound a
/// `const char *` to a `CList` and gcc reported "invalid initializer", naming a
/// C type the user never wrote. A Vortex `Char` is a Unicode scalar, so a byte
/// loop would also be a different program.
#[test]
fn a_for_in_over_a_string_is_emitted() {
    let src = "fn main() -> Int { var n = 0; for c in \"aé😀b\" { n = n + 1; } return n; }";
    let ast = vortexc::parse(src).expect("should parse");
    let c = vortexc::cgen::emit_program(&ast.items, "main", &[])
        .expect("a for-in over a string should emit");
    assert!(
        !c.contains("CList __vortex_for"),
        "a string should not be bound to a CList, said {:?}",
        c.lines().find(|l| l.contains("__vortex_for")).unwrap_or("")
    );
    assert!(
        c.contains("vortex_char_at"),
        "a string walk should decode a scalar per step, said {:?}",
        c.lines()
            .find(|l| l.contains("vortex_char_at"))
            .unwrap_or("")
    );
}

/// A `for ..in` over a tuple is refused by name rather than bound to a `CList`.
#[test]
fn a_for_in_over_a_tuple_is_refused() {
    let src = "fn main() -> Int { for t in (1, 2) { print(int_to_string(t)); } return 0; }";
    let ast = vortexc::parse(src).expect("should parse");
    let err = vortexc::cgen::emit_program(&ast.items, "main", &[])
        .expect_err("a for-in over a tuple should be refused");
    assert!(
        err.to_string().contains("for ..in` over a tuple"),
        "the refusal should name the construct, said {:?}",
        err.to_string()
    );
}

/// Indexing a tuple is refused by name.
///
/// A tuple is indexed by a pattern in a `match`, not by an index expression.
/// This emitted a list access, so gcc reported a request for `.items` on a
/// struct, which points at the C compiler rather than at Vortex.
#[test]
fn indexing_a_tuple_is_refused() {
    let src = "fn main() -> Int { let t = (1, 2); return t[0]; }";
    let ast = vortexc::parse(src).expect("should parse");
    let err = vortexc::cgen::emit_program(&ast.items, "main", &[])
        .expect_err("indexing a tuple should be refused");
    assert!(
        err.to_string().contains("indexing a tuple"),
        "the refusal should name the construct, said {:?}",
        err.to_string()
    );
}

/// A function returning a declared struct emits a fallback return at that type.
///
/// The emitter appended `return 0;` to every function, so a struct-returning one
/// got an integer return at the end and gcc reported "incompatible types when
/// returning type 'int' but 'CP' was expected". That is a gcc error naming a C
/// type rather than a Vortex diagnostic, which is the class stage 13 closes.
#[test]
fn a_function_returning_a_struct_emits() {
    let src = "struct P { x: Int }
               fn mk() -> P { return P { x: 7 }; }
               fn main() -> Int { let a = mk(); return a.x; }";
    let ast = vortexc::parse(src).expect("should parse");
    let c = vortexc::cgen::emit_program(&ast.items, "main", &[])
        .expect("a struct-returning function should emit");
    assert!(
        c.contains("return (CP){0};"),
        "the fallback return should be a zeroed value of the declared type, said {:?}",
        c.lines()
            .filter(|l| l.contains("return (C"))
            .collect::<Vec<_>>()
    );
}

/// A struct with a multi-letter mixed-case name gets a zeroed fallback return.
///
/// The check that decided whether a return type was a generated struct tested
/// the C name by letter case: it required everything after the `C` to be
/// uppercase. A struct named `Point` becomes `CPoint`, `Point` is not all
/// uppercase, the check said no, and the fallback `return 0;` stood, producing
/// the exact gcc error the fallback was meant to remove. A single letter name
/// like `P` took the new path, so a test using one passed either way.
#[test]
fn a_mixed_case_struct_name_gets_a_zeroed_fallback_return() {
    let src = "struct Point { x: Int }
               struct Vec2 { dx: Float, dy: Float }
               fn at_origin() -> Point { return Point { x: 7 }; }
               fn origin() -> Vec2 { return Vec2 { dx: 1.5, dy: 2.5 }; }
               fn use_both() -> Int {
                   let a = at_origin();
                   let b = origin();
                   if b.dx > 0.0 { return a.x; }
                   return 0;
               }
               fn main() -> Int { return use_both(); }";
    let ast = vortexc::parse(src).expect("should parse");
    let c = vortexc::cgen::emit_program(&ast.items, "main", &[])
        .expect("mixed case struct names should emit");
    assert!(
        c.contains("return (CPoint){0};"),
        "a mixed case struct return needs a zeroed value of its own type, said {:?}",
        c.lines()
            .filter(|l| l.contains("return (C"))
            .collect::<Vec<_>>()
    );
    assert!(
        c.contains("return (CVec2){0};"),
        "every declared struct needs it, not just a one letter name"
    );
    // The old check would have passed a one letter name and failed these, so
    // this assertion is what the letter case bug could not satisfy.
    assert!(
        !c.contains("return 0;\n}") || c.contains("return (C"),
        "a struct-returning function must not fall back to an integer return"
    );
}

/// Two `for ..in` loops in one function do not collide on their temporaries.
///
/// The list loop declared `__vortex_for_x` and the string loop declared
/// `__vortex_n` and `__vortex_i` in the enclosing scope, so two loops over the
/// same variable name redefined them and gcc reported it. Each loop is now
/// wrapped in a C block.
#[test]
fn two_for_in_loops_in_one_function_do_not_collide() {
    let src = "fn main() -> Int {
        var n = 0;
        for c in \"ab\" { n = n + 1; }
        for c in \"cd\" { n = n + 1; }
        for x in [1, 2] { n = n + x; }
        for x in [3, 4] { n = n + x; }
        return n;
    }";
    let ast = vortexc::parse(src).expect("should parse");
    let c = vortexc::cgen::emit_program(&ast.items, "main", &[]).expect("two loops should emit");
    // Each loop opens its own block, so its temporaries are scoped to it. Two
    // loops over the same variable name declared the same names in the
    // enclosing scope before, and gcc reported the redefinition.
    // Each loop opens a C block immediately before its temporaries, so the
    // names are scoped to the loop. Two loops over the same variable name
    // declared the same names in the enclosing scope before, and gcc reported
    // the redefinition.
    for marker in ["CList __vortex_for_x", "const char *__vortex_s_c"] {
        let all: Vec<&str> = c.lines().collect();
        let at = all
            .iter()
            .position(|l| l.contains(marker))
            .unwrap_or_else(|| panic!("{} should be emitted", marker));
        let before = all[..at]
            .iter()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or_else(|| panic!("{} should have a line before it", marker));
        assert_eq!(
            before.trim(),
            "{",
            "a C block should open just before {}, said {:?}",
            marker,
            before
        );
    }
}

/// A tuple bound by a `let` is refused by name, not emitted as a `CList`.
///
/// The refusal matched a tuple literal only, so a name recorded at its
/// `CTuple` type passed the check and emitted an invalid C initialiser.
#[test]
fn a_tuple_bound_by_a_let_is_refused_in_a_for_in() {
    let src = "fn main() { let t = (1, 2); for x in t { println(int_to_string(x)); } }";
    let ast = vortexc::parse(src).expect("should parse");
    let err = vortexc::cgen::emit_program(&ast.items, "main", &[])
        .expect_err("a tuple bound to a name should be refused in a for-in");
    let text = err.to_string();
    assert!(
        text.contains("for ..in` over a tuple"),
        "the refusal should name the construct, said {:?}",
        text
    );
    assert!(
        text.contains("at 1:"),
        "the refusal should carry a 1-based position, said {:?}",
        text
    );
}

/// `+` on two struct fields is arithmetic, for an `Int` and for a `Float`.
///
/// The field read had no case in the classification, so it was unclassified and
/// the `+` was refused. An `Int` field is `int64_t`, which is the emitter's
/// default, so a test with only an `Int` field would look right whether or not
/// anything was fixed. The `Float` field is the one that shows the declared type
/// is actually read, and both are here for that reason.
#[test]
fn arithmetic_on_struct_fields_is_carried() {
    let src = "struct Point { x: Int, y: Float }
               fn main() -> Int {
                   let a = Point { x: 2, y: 1.5 };
                   let b = Point { x: 3, y: 2.5 };
                   println(int_to_string(a.x + b.x));
                   println(float_to_string(a.y + b.y));
                   return 0;
               }";
    let ast = vortexc::parse(src).expect("should parse");
    let c =
        vortexc::cgen::emit_program(&ast.items, "main", &[]).expect("field arithmetic should emit");
    // Both fields read at their declared type, not at the emitter's default.
    assert!(
        c.contains("(a).x + (b).x"),
        "an Int field should read at int64_t, said {:?}",
        c.lines()
            .filter(|l| l.contains("int_to_string"))
            .collect::<Vec<_>>()
    );
    assert!(
        c.contains("(a).y + (b).y"),
        "a Float field should read at double, said {:?}",
        c.lines()
            .filter(|l| l.contains("float_to_string"))
            .collect::<Vec<_>>()
    );
}

/// A call inside a list literal, a repeat or a variant is reachable.
///
/// The walk that decides which functions to emit did not descend into those, so
/// `[origin(); 2]` named a callee that was never emitted and the generated C had
/// a call to a function that did not exist.
#[test]
fn a_call_inside_a_collection_value_is_emitted() {
    let src = "struct Point { x: Int }
               fn origin() -> Point { return Point { x: 0 }; }
               fn main() -> Int {
                   var points = [origin(); 2];
                   return points[0].x;
               }";
    let ast = vortexc::parse(src).expect("should parse");
    let c = vortexc::cgen::emit_program(&ast.items, "main", &[])
        .expect("a repeat of a call should emit");
    assert!(
        c.contains("CPoint origin(void)"),
        "the callee of a repeat should be emitted, said {:?}",
        c.lines()
            .filter(|l| l.contains("origin"))
            .collect::<Vec<_>>()
    );
}

/// Printing a `Bool` prints the word, on every path.
///
/// A `Bool` had no arm in the type decision, so a binding holding one was
/// declared `int64_t`, `print` tagged it as text and printed the pointer's own
/// bytes. The example set had no `print` of a `Bool`, which is the same gap as
/// the string index: the defect was in a shape nothing reached. An external
/// review raised it as possibly no longer applying, and it did.
#[test]
fn printing_a_bool_prints_the_word() {
    let src = "fn main() {
        var a = true;
        var b = false;
        print(a);
        print(b);
        println(\"\");
    }";
    assert_eq!(tree_stdout(src), "truefalse", "the tree interpreter");
    assert_eq!(vm_stdout(src), "truefalse", "the VM");
    match compiled_stdout(src, "main") {
        Ok(out) => assert_eq!(out, "truefalse", "the compiled path at -O2 and -O0"),
        Err(e) => panic!("printing a Bool should compile and run: {}", e),
    }
}

/// A list of `Bool` compiles and runs, in both the literal and the repeat form.
///
/// A `Bool` is classified as C `int`, and a list constructor is chosen by the
/// element type. Adding the `Bool` arm to the type decision therefore reached a
/// consumer that had no constructor at that type, so `[true]` named
/// `vortex_list_new_int`, which was never generated, and `[true; 2]` named
/// `vortex_list_repeat_int`, which was not either. The failure was a gcc error
/// naming a function that does not exist.
///
/// The lesson is the one the shape gives, applied to the fix for the shape:
/// **adding an arm to the type decision means walking the other places that
/// switch on the same classification.** The list constructor is one, and the
/// `print` tag is another, which is why this asserts output rather than only
/// that the C compiles.
#[test]
fn a_list_of_bools_compiles_and_runs() {
    let literal = "fn main() {
        var a = [true];
        print(a[0]);
        println(\"\");
    }";
    assert_eq!(tree_stdout(literal), "true", "the tree interpreter");
    assert_eq!(vm_stdout(literal), "true", "the VM");
    match compiled_stdout(literal, "main") {
        Ok(out) => assert_eq!(out, "true", "the compiled path at -O2 and -O0"),
        Err(e) => panic!("a list of Bools should compile and run: {}", e),
    }

    let repeat = "fn main() {
        var a = [false; 3];
        print(a[0]);
        print(a[2]);
        println(\"\");
    }";
    assert_eq!(tree_stdout(repeat), "falsefalse", "the tree interpreter");
    assert_eq!(vm_stdout(repeat), "falsefalse", "the VM");
    match compiled_stdout(repeat, "main") {
        Ok(out) => assert_eq!(out, "falsefalse", "the compiled path at -O2 and -O0"),
        Err(e) => panic!("a repeat of Bools should compile and run: {}", e),
    }
}

/// A negative string index is rejected through the same path as one past the end.
///
/// The bounds check was written for one direction of invalid, so a negative index
/// did not run the walk loop and returned the first character. That is the shape
/// again: a check that covers one case leaves the other falling through to a
/// default.
#[test]
fn a_negative_string_index_is_rejected() {
    let src = "fn main() {
        let w = \"héllo\";
        print(w[-1]);
        println(\"\");
    }";
    let tree_out = tree_error(src);
    assert!(
        tree_out.contains("index -1 is negative"),
        "the tree interpreter should say the index is negative, said {:?}",
        tree_out
    );
    let vm_out = value(vortexc::run_on_vm(src, &mut std::io::sink()));
    assert!(
        vm_out.contains("index -1 is negative"),
        "the VM should say the same, said {:?}",
        vm_out
    );
    match compiled_stdout(src, "main") {
        Ok(out) => panic!(
            "a negative index should not print {:?}, it should fail",
            out
        ),
        Err(e) => assert!(
            e.contains("index -1 is negative"),
            "the compiled path should reject a negative index, said {:?}",
            e
        ),
    }
}
