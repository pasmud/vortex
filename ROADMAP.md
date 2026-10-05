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

## Stage 6, complete: enforce linear ownership, then compile one real function

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

## Stage 7, complete: carry the sieve workload through the compiler

Scope: extend the emitter until it carries the real benchmark workload, then
publish the first honest compiled number rather than a partial one.

Acceptance criteria:

- [x] List construction and indexing, `if` expressions, `match` and ranges each
      land one at a time, each with a test.
- [x] The compiled row says what it covers. It was labelled `sieve only`
      because the matrix half disagreed, and the label was accurate when
      written.
- [x] The guard checks every quoted number against the committed transcript.

`STAGE7.md` records the result.

## Stage 8, complete: fix the compiled float path and earn the row

Scope: four defects kept the compiled path from running the whole workload.

Acceptance criteria:

- [x] All three paths print `checksum 1179908154 3314.003906`.
- [x] The compiled row in `BENCHMARKS.md` is a full workload row with no
      sieve-only label, and the guard requires that checksum on it.
- [x] The label removal is recorded in `STAGE7.md`, because removing a caveat is
      as much a claim as adding one.

`FLOAT-DEFECT.md` records the four defects.

## Stage 9, complete: measure register allocation

Scope: measure the claim that the emitter spills every value to a stack slot.

Acceptance criteria:

- [x] Before and after on the same workload, five repeats, raw transcripts.
- [x] The result compared against the committed spread, not a remembered number.
- [x] A gain inside the noise is reverted, and the revert recorded.

The premise did not hold: gcc at -O2 already keeps every live value in a
register, so there was nothing to remove. `STAGE9.md` records it.

## Stage 10, complete: test stage 9's prediction at -O0

Scope: stage 9 predicted two gaps only did harm because gcc hid them. That was
a prediction, so it is measured.

Acceptance criteria:

- [x] The compiled path at -O0 reports its checksum, and any mismatch is narrowed
      by reduction.
- [x] `-O0` and `-O2` measured with five repeats each, transcript committed.
- [x] The `-O0` figure labelled as gcc's, not as Vortex performance.

The prediction held on correctness and was too pessimistic on cost. Building the
examples at -O0 also found a real defect that -O2 was repairing. `STAGE10.md`
records it.

## Stage 11, active: emit `match` and close what the emitter cannot carry

Scope: a language whose compiled path cannot compile three of its own four
examples is not usable.

Acceptance criteria:

- [x] `match` emitted with the semantics the tree interpreter implements.
- [x] String indexing refused by name rather than emitted as a list access.
- [x] Every example that compiles produces identical output on the compiled
      path, the tree interpreter and the VM, at `-O2` and at `-O0`.
- [x] 207 tests on each engine still pass, plus new tests for what this adds.
- [x] An inventory of what else the tree interpreter supports and the emitter
      does not, so the next stage knows the size of the gap.
- [x] Any example that still does not compile is named with its reason.

No performance figure is claimed and none reaches `BENCHMARKS.md`.
`STAGE11.md` records the result, including why the guard has nothing to check
in it and what is checked instead.
