# Vortex roadmap

Each milestone is small enough to review in one pass and has acceptance criteria
that can be checked by running a command or reading a file. One milestone is
active at a time. A stage may only start after the previous stage is merged with
green CI.

Language rules live in `SPEC.md`. If a stage needs a language rule that the spec
does not state, the stage edits the spec in the same pull request.

Test command for every stage:

    cargo test --workspace

## Stage 1, complete: foundation

Specification, lexer, tests, benchmark skeleton, CI. Merged in pull request #2.

Acceptance criteria:

- [x] `SPEC.md` exists and covers goals, non-goals, the type system, the memory
      safety model and the reasoning for choosing it, syntax and semantics, the
      stage plan, and the performance stance.
- [x] `ROADMAP.md` exists with ordered milestones and measurable criteria.
- [x] The lexer is implemented and the test command passes.
- [x] Lexer tests cover ordinary input, string and character escapes, comments
      including nested block comments, and negative cases.
- [x] At least one negative test asserts that the diagnostic names the line,
      the column and what was expected.
- [x] `bench/run.sh` runs when invoked and prints a comparison table.
- [x] `BENCHMARKS.md` records the machine specification and compiler versions and
      states that there are no results yet.
- [x] A GitHub Actions workflow runs the test command on pushes and pull
      requests.
- [x] The example programs under `examples/` are covered by a test.

## Stage 2, complete: parser, AST and a tree interpreter

Merged in pull request #4.

## Stage 3, complete: types and diagnostics

Merged in pull request #6. Every rule in `SPEC.md` section 6 is enforced or
recorded as unimplemented in `crates/vortexc/DIAGNOSTICS.md`, and the two
`SPEC.md` section 8.2 gaps are closed.

## Stage 4, complete: bytecode VM and measured optimisation

Merged in pull request #8. Twelve loop defects found by running both engines
against each other, the VM measured slower than the tree interpreter, and
`SPEC.md` section 10.1 resolved.

## Stage 5, complete: backend and memory model decided from measurement

Merged in pull request #10. The backend is an ahead-of-time compiler over
`crates/vortexc/src/ir.rs`, both named VM changes measured and reverted, linear
ownership kept, no concurrency.

## Stage 6, active: enforce linear ownership, then compile one real function

Scope: make the memory model a property rather than a document, then build the
first piece of the compiler stage 5 decided on.

Acceptance criteria:

- [x] The two tests stage 5 pinned to fail on enforcement land now fail, and are
      replaced by tests asserting the diagnostics.
- [x] Every ownership counterexample produces a diagnostic naming the rule.
- [x] A program that should compile still compiles. Nine counterexamples are
      pinned, five of which must keep compiling.
- [x] One function is compiled and runs, and its output is byte-identical
      across the compiled path, the tree interpreter and the VM, at four inputs.
- [x] Every number is copied from a committed transcript, and
      `scripts/check-benchmarks.sh` covers `MEASUREMENTS.md`.
- [x] `SPEC.md` and `DECISION.md` updated on the day enforcement landed.
- [x] `cargo test --workspace` passes with lexer, parser, interpreter, checker,
      ownership and compiled counts separated. 205 passing on both engines.

### What stage 6 found

Enforcing moves wrongly is worse than not enforcing them, so the counterexamples
were written before the enforcement. Three turned up defects in the enforcement
rather than in the examples: a struct field read was treated as a move of the
base, which broke `examples/structs.vx`; a move inside a loop body was
forgotten, because a block discarded the flag; and a move was recorded only in
the innermost scope, so a binding declared in the function body was invisible
from inside a loop.

The measurement found a fourth. The compiled path returned 307880128 where both
engines returned 2666668666667000000, because the C entry point cast through
`int`, which is 32 bits here. A compiled path checked only against itself would
have shipped it.

### What stage 7 should do

The emitter handles scalar functions with loops and refuses a list, an `if`
expression and a `match`. Until it handles those it cannot carry the benchmark
workload, so no Vortex row for the full workload exists and none is claimed.
Extending the emitter to lists, conditionals and matches is the next piece, and
with it a compiled row in the benchmark table.

Move enforcement is also partial. A move by assignment and a move out of a
live struct field are not tracked. Both are limits of what the lowering pass
decides syntactically today.

