//! Ownership tests.
//!
//! `DECISION.md` records that stage 5 kept the linear ownership model of
//! `SPEC.md` section 7 and did not enforce it. These tests hold that line from
//! both sides: the properties the model does give are checked, and the
//! properties it does not yet give are written down as the diagnostic they
//! would produce, so the gap is visible rather than implied.
//!
//! They also check that `examples/ownership.vx` runs on both engines and prints
//! the same thing, which is the artifact that demonstrates the decision.

use vortexc::{run_on_vm, run_source, Error};

/// Which engine a test runs against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Tree,
    Vm,
}

fn run(engine: Engine, src: &str) -> Result<String, Error> {
    let mut out = Vec::new();
    let result = match engine {
        Engine::Tree => run_source(src, &mut out),
        Engine::Vm => run_on_vm(src, &mut out),
    };
    result.map(|_| String::from_utf8(out).expect("output should be valid UTF-8"))
}

/// Runs a program that is expected to be rejected, on one engine.
fn rejects(engine: Engine, src: &str) -> Error {
    match run(engine, src) {
        Ok(_) => panic!("expected {:?} to reject this program", engine),
        Err(e) => e,
    }
}

/// Both engines, so a property is checked of the semantics rather than of one
/// executor.
const ENGINES: [Engine; 2] = [Engine::Tree, Engine::Vm];

// ---------------------------------------------------------------- the artifact

#[test]
fn the_ownership_example_runs_and_prints_the_same_on_both_engines() {
    let src = std::fs::read_to_string(example_path()).expect("cannot read the example");
    let tree = run(Engine::Tree, &src).expect("the tree interpreter should run it");
    let vm = run(Engine::Vm, &src).expect("the vm should run it");
    assert_eq!(tree, vm, "the two engines must print the same thing");
    // The expectation lives here, not in the example.
    assert_eq!(tree, "n consumed 21\n42\n12\n36\n");
}

// ---------------------------------------------------------------- what the model gives

#[test]
fn a_value_moves_into_a_call_and_comes_back_only_as_the_result() {
    for engine in ENGINES {
        let src = "fn take(n: Int) -> Int { return n * 2; }
                   fn main() -> Int { let n = 21; return take(n); }";
        let mut out = Vec::new();
        let result = match engine {
            Engine::Tree => run_source(src, &mut out),
            Engine::Vm => run_on_vm(src, &mut out),
        };
        assert_eq!(vortexc::interp::display(&result.unwrap()), "42");
    }
}

#[test]
fn a_store_into_a_list_changes_the_list_in_place() {
    // One owner, and the store is O(1) rather than a copy of the list. A store
    // into a binding that is not `var` is rejected, which is what keeps the
    // model honest rather than advisory.
    for engine in ENGINES {
        let src = "fn main() -> Int {
                       var a = [1, 2, 3];
                       a[0] = 99;
                       return a[0];
                   }";
        let mut out = Vec::new();
        let result = match engine {
            Engine::Tree => run_source(src, &mut out),
            Engine::Vm => run_on_vm(src, &mut out),
        };
        assert_eq!(vortexc::interp::display(&result.unwrap()), "99");
    }
}

#[test]
fn a_store_into_a_let_list_is_rejected_on_both_engines() {
    // Section 8.1 makes a store into an element a mutation of the binding, so
    // the binding has to be `var`. This is the rule that stops aliasing being
    // introduced by a second binding.
    for engine in ENGINES {
        let e = rejects(
            engine,
            "fn main() -> Int { let a = [1, 2, 3]; a[0] = 9; return a[0]; }",
        );
        assert!(
            e.to_string().contains("declared with `let`"),
            "{:?} should say the binding is immutable, said {:?}",
            engine,
            e.to_string()
        );
    }
}

// ---------------------------------------------------------------- what it now gives
//
// Stage 6 enforces the model. These replace the two tests stage 5 wrote to fail
// the day this landed, and the diagnostics they assert are the ones
// `SPEC.md` section 7 requires: every value has exactly one owner, passing it
// moves it, and the moved-from binding is dead.

/// A use after a move of a non-scalar value.
///
/// `SPEC.md` section 7: every value has exactly one owner, and a moved-from
/// binding is dead. The diagnostic names the binding, the rule, and a position.
#[test]
fn a_use_after_a_move_is_rejected() {
    let src = "struct P { x: Int, y: Int }
               fn take(p: P) -> Int { return p.x; }
               fn main() -> Int { let p = P { x: 1, y: 2 }; let a = take(p); return a + p.x; }";
    for engine in ENGINES {
        let e = rejects(engine, src);
        let text = e.to_string();
        assert!(
            text.contains("`p` has been moved and cannot be used again"),
            "{:?} should say the binding is dead, said {:?}",
            engine,
            text
        );
        assert!(
            text.contains("section 7"),
            "the diagnostic must cite the rule it enforces, said {:?}",
            text
        );
        assert!(
            text.contains("3:89"),
            "the diagnostic must name a line and column, said {:?}",
            text
        );
    }
}

/// A second binding for an already-moved value.
///
/// The same rule reached from the other side: the value has one owner, so it
/// cannot be bound again.
#[test]
fn a_second_binding_after_a_move_is_rejected() {
    let src = "struct P { x: Int, y: Int }
               fn take(p: P) -> Int { return p.x; }
               fn main() -> Int { let p = P { x: 1, y: 2 }; let a = take(p); let b = p; return a; }";
    for engine in ENGINES {
        let e = rejects(engine, src);
        let text = e.to_string();
        assert!(
            text.contains("has been moved and cannot be used again"),
            "{:?} should say the binding is dead, said {:?}",
            engine,
            text
        );
        assert!(
            text.contains("section 7"),
            "the diagnostic must cite the rule it enforces, said {:?}",
            text
        );
    }
}

// ---------------------------------------------------------------- the counterexamples
//
// Each of these compiled before stage 6. They are the cases where enforcing
// moves wrongly would reject a program that should compile, so each is pinned.

/// A scalar survives a move, because `SPEC.md` section 7 says scalars are
/// `Copy` and are duplicated rather than moved. Rejecting this would break
/// almost every program.
#[test]
fn a_scalar_is_copied_rather_than_moved() {
    let src = "fn take(n: Int) -> Int { return n * 2; }
               fn main() -> Int { let n = 21; let a = take(n); return a + n; }";
    for engine in ENGINES {
        let mut out = Vec::new();
        let result = match engine {
            Engine::Tree => run_source(src, &mut out),
            Engine::Vm => run_on_vm(src, &mut out),
        };
        assert_eq!(
            vortexc::interp::display(&result.unwrap()),
            "63",
            "{:?} should compile",
            engine
        );
    }
}

/// A scalar moved twice is still fine, for the same reason.
#[test]
fn a_scalar_may_be_moved_twice() {
    let src = "fn take(n: Int) -> Int { return n * 2; }
               fn main() -> Int { let n = 21; let a = take(n); let b = take(n); return a + b; }";
    for engine in ENGINES {
        let mut out = Vec::new();
        let result = match engine {
            Engine::Tree => run_source(src, &mut out),
            Engine::Vm => run_on_vm(src, &mut out),
        };
        assert_eq!(
            vortexc::interp::display(&result.unwrap()),
            "84",
            "{:?} should compile",
            engine
        );
    }
}

/// A value moved inside a loop body is rejected at the second move, which is
/// where it becomes dead.
#[test]
fn a_move_inside_a_loop_is_rejected() {
    let src = "struct P { x: Int, y: Int }
               fn take(p: P) -> Int { return p.x; }
               fn main() -> Int {
                   var p = P { x: 1, y: 2 };
                   var i = 0;
                   while i < 2 {
                       let a = take(p);
                       let b = take(p);
                       i = i + 1;
                   }
                   return 0;
               }";
    for engine in ENGINES {
        let e = rejects(engine, src);
        assert!(
            e.to_string().contains("has been moved"),
            "{:?} should reject the second move, said {:?}",
            engine,
            e.to_string()
        );
    }
}

/// A move inside a match arm that binds by value is allowed, because the bound
/// name holds a scalar. Pinned so the day that changes it is deliberate.
#[test]
fn a_move_inside_a_match_arm_is_allowed_for_a_scalar_binding() {
    let src = "enum S { one(Int), two }
               fn take(n: Int) -> Int { return n * 2; }
               fn f(s: S) -> Int {
                   let a = match s {
                       S.one(v) => take(v) + v,
                       S.two => 0,
                   };
                   return a;
               }
               fn main() -> Int { return 0; }";
    for engine in ENGINES {
        assert!(
            run(engine, src).is_ok(),
            "{:?} should compile, because a bound Int is a scalar",
            engine
        );
    }
}

/// A struct field read does not move the struct, so reading two fields in
/// sequence is fine. This was a real false positive during stage 6.
#[test]
fn reading_two_fields_of_one_struct_is_allowed() {
    let src = "struct P { x: Int, y: Int }
               fn main() -> Int { let p = P { x: 1, y: 2 }; return p.x + p.y; }";
    for engine in ENGINES {
        let mut out = Vec::new();
        let result = match engine {
            Engine::Tree => run_source(src, &mut out),
            Engine::Vm => run_on_vm(src, &mut out),
        };
        assert_eq!(
            vortexc::interp::display(&result.unwrap()),
            "3",
            "{:?} should compile",
            engine
        );
    }
}

/// Moving a struct into a function and not using it again is the ordinary case
/// the model has to allow.
#[test]
fn moving_an_aggregate_and_not_using_it_again_is_allowed() {
    let src = "struct P { x: Int, y: Int }
               fn take(p: P) -> Int { return p.x; }
               fn main() -> Int { let p = P { x: 7, y: 8 }; return take(p); }";
    for engine in ENGINES {
        let mut out = Vec::new();
        let result = match engine {
            Engine::Tree => run_source(src, &mut out),
            Engine::Vm => run_on_vm(src, &mut out),
        };
        assert_eq!(
            vortexc::interp::display(&result.unwrap()),
            "7",
            "{:?} should compile",
            engine
        );
    }
}

// ---------------------------------------------------------------- helpers

fn example_path() -> std::path::PathBuf {
    use std::path::{Path, PathBuf};
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate lives two levels below the workspace root");
    PathBuf::from(root).join("examples").join("ownership.vx")
}
