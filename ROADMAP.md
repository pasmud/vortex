# Vortex roadmap

Each milestone is small enough to review in one pass and has acceptance criteria
that can be checked by running a command or reading a file. One milestone is
active at a time. A stage may only start after the previous stage is merged with
green CI.

Language rules live in `SPEC.md`. If a stage needs a language rule that the spec
does not state, the stage edits the spec in the same pull request.

Test command for every stage:

    cargo test --workspace

## Stage 1, active: foundation

Specification, lexer, tests, benchmark skeleton, CI.

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

## Stage 2: parser, AST and a tree interpreter

Scope: parse every program in `examples/` into an AST, and execute them in a
tree interpreter.

Acceptance criteria:

- [ ] `src/parser/` parses every file in `examples/` without error.
- [ ] Every `examples/` program runs and prints a line that a test compares to
      an expected value. The expectation lives in the test, not in the example.
- [ ] Errors from the parser name line, column and what was expected, in the
      same style as stage 1.
- [ ] The tree interpreter supports `fn`, `let`, `var`, `struct`, `enum`,
      `if`/`else`, `while`, `for`, `match`, `break`, `continue` and `return`.
- [ ] `cargo test --workspace` passes, with the interpreter tests counted
      separately from the lexer tests.
- [ ] `bench/run.sh` gains a Vortex row that runs a real algorithm, even if the
      result is a baseline number recorded with that label. No estimated
      numbers.

## Stage 3: types and diagnostics

Scope: static checking, and a regression test per diagnostic the spec promises.

Acceptance criteria:

- [ ] Every nominal, structural, Option, Result, cast and shadowing rule in
      `SPEC.md` section 6 is enforced or explicitly recorded as unimplemented.
- [ ] Every diagnostic has a regression test that asserts the message and the
      line and column.
- [ ] A documented list of every diagnostic the checker can emit, with the
      `SPEC.md` rule each one enforces.
- [ ] Type checking runs before execution, so an ill typed program never
      reaches the interpreter.
- [ ] `cargo test --workspace` passes.

## Stage 4: bytecode VM and measured optimisation

Scope: compile the AST to bytecode, add a VM, and measure against the tree
interpreter.

Acceptance criteria:

- [ ] The same `examples/` tests pass unchanged against the VM, so the VM is
      checked against the tree interpreter rather than against new expectations.
- [ ] A disassembly command prints the bytecode for a function.
- [ ] `bench/run.sh` reports tree interpreter and VM rows for the same
      algorithm, on a machine recorded in `BENCHMARKS.md`, with raw output
      committed.
- [ ] Every optimisation in this stage is listed with the measurement that
      justified it, and the result of the full benchmark run before and after.
      An optimisation with no before and after measurement is reverted.
- [ ] `SPEC.md` section 10.1 is resolved either way, in writing, before the
      stage closes.

## Stage 5: native backend, memory and concurrency model

Scope: decide a native backend and a memory and concurrency model from the
stage 4 measurements.

Acceptance criteria, all of which are decisions backed by data:

- [ ] The `cell<T>` and cycle breaking question in `SPEC.md` section 7.2 is
      decided by the stated rule, and the spec is updated with the decision and
      the measurements.
- [ ] The concurrency model is chosen from stage 4 measurements, and the spec
      documents what is shared and how ownership is transferred.
- [ ] A concurrency model is only claimed to be safe if a tested model exists.
      A claim without a test is not accepted.
- [ ] The native backend is compared against the VM on the committed harness,
      and the result is recorded in `BENCHMARKS.md` with the machine
      specification.
- [ ] `cargo test --workspace` passes.

## Later, not scheduled

- Packages and a foreign function interface. Not committed.
- Trait objects. Deferred past v0.1.
- Editors, a formatter and a language server. Not committed.