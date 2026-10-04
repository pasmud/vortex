# vortex

Operator-owned internal project, managed through Git_system_Zero.

This initial commit establishes the base branch. Product changes follow issues, reviewed pull requests and verified tests.

Vortex is a programming language. It is written in Rust, and the goal is
readable syntax, precise diagnostics and an explicit memory safety model. See
`SPEC.md` for the language specification, `ROADMAP.md` for the stages and
`BENCHMARKS.md` for the performance position.

## Current state

Stage 1 of the roadmap. The specification exists, the lexer is implemented and
tested, the benchmark harness runs, and CI runs the tests. There is no
evaluator yet, so no Vortex program can run and there are no benchmark results.

## Layout

| Path | Purpose |
| --- | --- |
| `SPEC.md` | The language specification. Every later stage matches it. |
| `ROADMAP.md` | Ordered milestones with measurable acceptance criteria. |
| `BENCHMARKS.md` | Machine specification and compiler versions. No results yet. |
| `crates/vortexc/` | The compiler. Stage 1 contains the lexer. |
| `examples/` | Vortex source files used by the tests. |
| `bench/` | The benchmark harness, `bench/run.sh`, and the C and Rust baselines. |
| `scripts/` | Small repository checks used by CI. |

## Working on Vortex

The test command for the whole repository is:

    cargo test --workspace

Run the benchmark harness with:

    bench/run.sh

A pull request needs an issue, a branch named `agent/<slug>`, passing tests and
a description that says what was actually run. Claims about speed or memory
safety need the committed measurement that backs them.

## Performance position

Vortex aims to be competitive with C and Rust on realistic workloads. It does
not claim to be faster than C. `SPEC.md` section 9 and `BENCHMARKS.md` state
the rule: no speed statement is made without a measurement from
`bench/run.sh` on a recorded machine.