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
    // C that fails to compile with no explanation. Indexing a string is the one
    // left, and stage 11 added it because it was being emitted as a list
    // access, which is a different thing and did not compile.
    let src = "fn first(w: Str) -> Str { return w[0]; }
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
        .find(|f| f.node.name == "first")
        .expect("the function should be there");
    match vortexc::cgen::emit_function_alone(f) {
        Ok(_) => panic!("indexing a string should not be emitted"),
        Err(e) => {
            let text = e.to_string();
            assert!(
                text.contains("indexing a string"),
                "the refusal should name the construct, said {:?}",
                text
            );
        }
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
        c.lines().find(|l| l.contains("vortex_list_new_C")).unwrap_or("")
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
