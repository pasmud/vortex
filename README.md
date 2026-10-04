# vortex

Operator-owned internal project, managed through Git_system_Zero.

This initial commit establishes the base branch. Product changes follow issues, reviewed pull requests and verified tests.

Vortex is a programming language. It is written in Rust, and the goal is
readable syntax, precise diagnostics and an explicit memory safety model. See
`SPEC.md` for the language specification, `ROADMAP.md` for the stages and
`BENCHMARKS.md` for the performance position.

## Current state

Stage 2 of the roadmap. The lexer, the parser, the AST, the lowering pass and
the tree interpreter are implemented and tested, and every program in
`examples/` runs. The type checker has not started.

There is still no Vortex benchmark result. Vortex v0.1 cannot write the
baseline workload, because there is no index assignment and no list of a
computed length. `bench/vortex/README.md` explains that, and
`SPEC.md` section 8.2 lists both gaps. No Vortex time is recorded or estimated.

## Layout

| Path | Purpose |
| --- | --- |
| `SPEC.md` | The language specification. Every later stage matches it. |
| `ROADMAP.md` | Ordered milestones with measurable acceptance criteria. |
| `BENCHMARKS.md` | Machine specification and compiler versions. No Vortex results yet. |
| `crates/vortexc/src/lexer.rs` | Tokens, from stage 1. |
| `crates/vortexc/src/parser.rs` | Recursive descent parser. |
| `crates/vortexc/src/ast.rs` | The surface syntax tree. |
| `crates/vortexc/src/ir.rs` | The lowered form shared by the interpreter and the future VM. |
| `crates/vortexc/src/lower.rs` | Name resolution and the lowering pass. |
| `crates/vortexc/src/interp.rs` | The tree interpreter. |
| `examples/` | Vortex source files, executed by the interpreter tests. |
| `bench/` | The harness, `bench/run.sh`, the C and Rust baselines. |
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