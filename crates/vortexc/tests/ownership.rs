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

// ---------------------------------------------------------------- what it does not give yet
//
// These are the properties `DECISION.md` says are unenforced. Each says what
// the diagnostic would look like. The names say NOT rejected because that is the
// state today: each asserts the program still compiles, so the day enforcement
// lands these fail and have to be rewritten rather than quietly passing.

/// A use after a move. `SPEC.md` section 7 says the moved-from binding is
/// dead, and reading it should be an error naming the binding and the move.
///
/// Today this program compiles and runs, which is the gap. The diagnostic
/// stage 5 decided to leave open would be:
///
///     error at 2:14: `n` has been moved into the call to `take` on line 1 and
///     cannot be used again; `SPEC.md` section 7 gives every value one owner
#[test]
fn a_use_after_a_move_is_not_rejected_yet() {
    let src = "fn take(n: Int) -> Int { return n * 2; }
               fn main() -> Int { let n = 21; let a = take(n); return a + n; }";
    // It runs today. That is the point of the test: it pins the gap so the day
    // enforcement lands, this test fails and has to be rewritten.
    assert!(
        run(Engine::Tree, src).is_ok(),
        "a use after a move still compiles; when enforcement arrives this \
assertion must be replaced by one that expects the diagnostic above"
    );
}

/// A second binding for a moved value, which is how aliasing would be
/// introduced.
///
/// Today this compiles too. The diagnostic would name the second binding.
#[test]
fn a_second_binding_after_a_move_is_not_rejected_yet() {
    let src = "fn take(n: Int) -> Int { return n * 2; }
               fn main() -> Int { let n = 21; let a = take(n); let b = n; return a + b; }";
    assert!(
        run(Engine::Tree, src).is_ok(),
        "a second binding after a move still compiles; when enforcement \
arrives this assertion must be replaced by one that expects the diagnostic"
    );
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
