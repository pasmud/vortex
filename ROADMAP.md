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

> Resolved in Stage 25: the lowering pass now records assignment moves
> (`let q = p;`) and field moves for non-scalar fields, deadening the
> source binding and rejecting a subsequent use with a diagnostics.

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

## Stage 11, complete: emit `match` and close what the emitter cannot carry

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

## Stage 12 onwards

Stages 12 through 18 extended the parser, the tree interpreter, the checker and
the bytecode VM. Each stage is recorded in its own `STAGE*.md` file.

## Stage 18, complete: trait method resolution in the monomorphiser

Scope: resolve trait method calls to concrete impl methods during the `mono`
pass, by matching the receiver type against `impl` blocks. Concrete receiver
types only; generic trait impls and trait bounds are left for a later stage.

Acceptance criteria:

- [x] A trait method call on a concrete type resolves to the correct impl method.
- [x] An unresolved trait method is rejected at the mono stage.
- [x] A trait method call on a struct type resolves to the struct's impl method.
- [x] 237 tests pass, `cargo fmt --all --check` and `cargo build --all-targets`
      with `RUSTFLAGS=-D warnings` are clean.

Merged in pull request #33.

## Stage 19, complete: function type checking in the mono pass

Scope: check argument types against parameter types in the `mono` pass, and
apply substitutions to `Record`, `Cast`, `VariantCall` and `VariantRecord`
type names.

Acceptance criteria:

- [x] An argument of the wrong type is rejected at the mono stage with
      "expected type `X`, found type `Y`".
- [x] 237 tests pass, `cargo fmt --all --check` and `cargo build --all-targets`
      with `RUSTFLAGS=-D warnings` are clean.

Merged in pull request #36.

## Stage 20, complete: generic function monomorphisation and trait bounds

Scope: implement generic function monomorphisation and trait bound checking in
the `mono` pass, extending Stage 18's concrete-type trait method resolution to
generic functions.

Acceptance criteria:

- [x] Generic functions with type parameters `[T]` and trait bounds `[T: Trait]`
      are parsed.
- [x] Type arguments are inferred from call site argument types.
- [x] A separate concrete function is produced per distinct set of type arguments.
- [x] A trait bound violation emits a diagnostic naming the type parameter,
      concrete type, and unsatisfied trait.
- [x] All 244 tests pass (237 prior + 7 new).
- [x] `cargo fmt --all --check` and `cargo build --all-targets` with
      `RUSTFLAGS=-D warnings` are clean.

`STAGE20.md` records the result.

## Stage 21, complete: generic trait impls

Scope: implement generic trait impls (`impl<T> Trait for Type<T>`) in the
`mono` pass, extending Stage 20's generic function monomorphisation and
Stage 18's concrete-type trait method resolution to support generic impl
blocks matched by structural unification rather than name equality.

Acceptance criteria:

- [x] Impl blocks parse generic parameter lists `[T]` using the existing
      `generics()` parser helper.
- [x] A generic trait method call on a generic type parameter resolves to the
      generic impl's method via structural unification, not name equality.
- [x] A trait method call on a concrete instantiation of a generic type
      resolves to the correct monomorphised function.
- [x] A trait bound violation on a generic impl emits a diagnostic naming the
      type parameter, concrete type, and unsatisfied trait.
- [x] All 204 tests pass (201 prior + 3 new).
- [x] `cargo fmt --all --check` and `cargo build --all-targets` with
      `RUSTFLAGS=-D warnings` are clean.

`STAGE21.md` records the result.

## Stage 22, complete: parameterized type arguments

Scope: implement parameterized type arguments (e.g. `Vec[T]`) in type annotations
and generic impl self types (e.g. `impl[T] Trait for Concrete[T]`) in the `mono`
pass, extending Stage 21's generic trait impls with structural unification for
parameterized self types.

Acceptance criteria:

- [x] `TypeExpr` has a `Generic(String, Vec<TypeExpr>)` variant; `parse_type()`
      parses `Name[T1, T2, ...]` as `TypeExpr::Generic`.
- [x] A parameterized type annotation (e.g. `fn f(x: Wrapper[Int])`) parses and
      type-checks in the mono pass.
- [x] A generic impl with a parameterized self type
      (`impl[T] Trait for Concrete[T]`) parses and resolves to the correct
      monomorphised method for a concrete instantiation (`Concrete[Int]`).
- [x] A trait bound violation on a parameterized self type emits a diagnostic
      naming the type parameter, concrete type, and unsatisfied trait.
- [x] All 247 tests pass (244 prior + 3 new).
- [x] `cargo fmt --all --check` and `cargo build --all-targets` with
      `RUSTFLAGS=-D warnings` are clean.

`STAGE22.md` records the result.

## Stage 23, complete: complete generics

Scope: complete the generics system started in Stages 20-22 with three items:
type inference from parameterized types, structural field type checking, and
optional let type annotations. All three land in the `mono` pass (and `checker`
for let annotations), closing the generics roadmap.

Acceptance criteria:

- [x] `infer_subst` recursively unifies type arguments within parameterized types.
  A call like `f(w)` where `x: Wrapper[T]` and `w` has type `Wrapper[Int]`
  infers `T = Int` via recursive `TypeExpr::Generic` traversal.
- [x] Struct field types are checked structurally in the `mono` pass. A field
  declared `Wrapper[Int]` accepts a `Wrapper[Int]` value and rejects a
  `Wrapper[Str]` value; a bare `Wrapper` is accepted.
- [x] `let` statements accept optional type annotations (`let x: Int = 5`).
  The annotation is checked against the initializer in both the `mono` pass and
  the checker.
- [x] `STAGE23.md` records the result.
- [x] All 257 tests pass.
- [x] `cargo fmt --all --check` and `cargo build --all-targets` with
  `RUSTFLAGS=-D warnings` are clean.

`STAGE23.md` records the result.

## Stage 24, complete: fix documentation for move-after-use enforcement

Merged in pull request #41.

Scope: Stage 6 implemented move-after-use enforcement, but Stage 23's review left
the documentation claiming it was unimplemented. This stage corrected the record.

Acceptance criteria:

- [x] `STAGE23.md` status is marked complete.
- [x] `SPEC.md` section 7.2 updated to say enforcement is implemented.
- [x] `DIAGNOSTICS.md` updated: a `mono` stage is listed, generics are marked
      checked, and move-semantics is marked implemented.
- [x] `ROADMAP.md` gains this Stage 24 entry.
- [x] `ownership.rs` module doc records the enforcement location.
- [x] `cargo test --workspace` passes, `cargo fmt --all --check` is clean, and
      `RUSTFLAGS='-D warnings' cargo build --workspace --all-targets` is clean.

## Stage 25, complete: complete move-after-use enforcement

Scope: extend the lowering pass to track the two move forms that Stage 6 left
untracked, closing the gap in the memory-safety model.

Acceptance criteria:

- [x] A move by assignment (`let q = p;`) deadens the source binding `p`. A
      subsequent use of `p` is rejected with a "has been moved" diagnostic that
      names the binding, cites `SPEC.md` section 7, and reports `line:column`.
- [x] A scalar bound by assignment (`let q = n;`) is a copy, not a move. `n` is
      still usable afterwards.
- [x] The new binding after an assignment move (`let q = p; q.x`) owns the value
      and can be used.
- [x] A struct field read consumed by a function call (`take(p.x)`) where the
      field is a scalar stays a copy: the struct is not moved and `p.y` is still
      usable. Non-scalar fields are marked moved on consumption.
- [x] `crates/vortexc/tests/ownership.rs` adds tests with real diagnostic
      assertions (no placeholders, no `#[ignore]`).
- [x] `STAGE25.md` records the result.
- [x] `SPEC.md` section 7 updated to cite the Stage 25 extension.
- [x] `ROADMAP.md` marks Stage 11 complete and adds this Stage 25 entry.
- [x] `cargo test --workspace` passes with no regressions (262 tests).
- [x] `cargo fmt --all --check` is clean.
- [x] `RUSTFLAGS='-D warnings' cargo build --workspace --all-targets` is clean.
