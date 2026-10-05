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

## Stage 4, active: bytecode VM and measured optimisation

Scope: compile the lowered form to bytecode, execute it on a VM, and measure it
against the tree interpreter on the same workload.

Acceptance criteria:

- [x] The whole `examples/` set parses and runs on both engines, with identical
      output. Checked by comparing the two engines on every example, not by
      asserting one of them.
- [x] The stage 2 and 3 test suites run against the VM unchanged, selected by
      `VORTEX_ENGINE=vm cargo test --workspace`. 189 passing on each engine. No
      expectation was edited to accommodate the VM, and the second run was shown
      to really exercise the VM by breaking VM arithmetic and watching 15 tests
      fail while the tree suite still passed.
- [x] A disassembly command prints the bytecode for a function:
      `cargo run --release --example disasm -- <file.vx> [--fn <name>]`.
- [x] `bench/run.sh` reports a tree interpreter row and a VM row for the same
      workload, five repeats, raw output committed at
      `bench/results/stage4-vm.txt`. Both rows print the same checksum as the C
      and Rust baselines.
- [x] `SPEC.md` section 10.1 resolved in writing, with the measurement that
      decided it.
- [x] Every optimisation is either published with before and after numbers or
      was never made. **No optimisation shipped.** The measurement that would
      have justified one, a VM faster than the tree interpreter, did not happen,
      so the honest result is the slowdown recorded in `BENCHMARKS.md`.

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