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

## Stage 5, active: backend and memory model decided from measurement

Scope: decide the backend and the memory model from stage 4's evidence, then
build the smallest thing that proves the decision.

Acceptance criteria:

- [x] The decision is written down with the evidence behind it and a falsifier.
      `DECISION.md`, written before any code.
- [x] Each of the two VM changes stage 4 named is measured with before and after
      numbers. **Both reverted.** A frame allocated operand stack was worse at
      every size tried, 5419 ms at 256 slots and 5283 ms at 32, against 4939 ms.
      Boxing the string payload was inside the jitter across three rounds of
      five runs and did not shrink `Value`.
- [x] The memory and concurrency model is decided, with failure modes stated.
      Linear ownership kept and not enforced yet; no concurrency in v0.1.
- [x] One example program exercises the decided model, runs, and is measured.
      `examples/ownership.vx`, identical on both engines.
- [x] The example set still runs on both engines with identical output.
- [x] No invented or estimated number anywhere.
- [ ] The native compiler itself. **Deliberately out of scope.** The decision
      names it and the reasoning is recorded, but writing a code generator was
      not this stage, and no performance claim about one is made because none
      has been measured.

### What stage 5 concluded

The backend is an ahead-of-time compiler over `crates/vortexc/src/ir.rs`, not
the bytecode VM. The premise behind building a VM, that per node dispatch was
the cost, turned out to be wrong: a Rust tree walk recurses and the optimiser
inlines it, while a VM pays for an instruction walk and an operand stack that
the tree walk never had. The VM is kept as a test oracle, because two
independent readings of the semantics found twelve defects that one reading did
not.

### What stage 6 should do

The next piece of work is enforcement of the move model in section 7. The model
is chosen and demonstrated, and the gap is pinned by two tests that will fail
when enforcement lands. After that, the compiler itself, which consumes the
lowered form the tree interpreter already walks.

