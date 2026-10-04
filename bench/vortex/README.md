# The Vortex benchmark workload

There is no `bench/vortex` program, and there is a specific reason.

`bench/c/sieve.c` and `bench/rust/src/main.rs` both implement a sieve of
Eratosthenes over a byte array, and then a matrix multiply over a double array.
Neither of them can be written in Vortex as stage 2 stands, for two reasons that
are about the language rather than about the interpreter:

1. **There is no index assignment.** `SPEC.md` section 6.1 rule 8 says `let`
   binds an immutable name, and section 8 says an assignment to a `let` is an
   error. The only assignable name in stage 2 is a `var`, and a `var` is a
   scalar slot, not a list element. There is no `a[i] = v`. The sieve needs it
   on every pass.

2. **There is no list of a computed length.** A list literal is written with its
   elements, `[2, 3, 5]`. There is no way to write `[0; limit + 1]`, so the
   2,000,001 element flag array the sieve needs cannot be allocated at all.

Both gaps are real and both are worth closing, but closing them means adding
index assignment and a list construction form to the language. That is a change
to `SPEC.md` and it is not this stage's job to invent.

## What this means for the benchmark table

The Vortex row in `bench/run.sh` stays `n/a`, and it says this. The harness
fills in a time only for a binary that actually ran, so there is no way for a
Vortex number to appear without a Vortex program to produce it.

The alternative was to invent a different, weaker workload, write it three
times, and present the comparison as if it measured the same thing. That would
be a real algorithm in each language but it would not be the workload the two
baselines already measure, so the three rows would not be comparable and the
table would be misleading. The honest state is the one recorded here.

## What stage 3 and stage 4 should do about it

Both gaps are ordinary work and neither needs a language redesign:

- **Index assignment** is a parser and lowering change. `a[i] = v` becomes an
  `ir::Stmt::Assign` into a list, which the tree interpreter can already
  evaluate, because `index_into` and `Store` both exist. The type rules for when
  it is legal belong in stage 3.
- **A list of a computed length** is one more literal form plus a builtin, or a
  list `repeat` expression. The interpreter already holds `Value::Array`, so
  this is mostly a parser and lowering change too.

Once both exist, `bench/vortex/sieve.vx` can be written to match the baselines
exactly, its checksum has to equal theirs, and the Vortex row can carry a real
time. Until then it does not.

## What was measured

Measured on the machine in `BENCHMARKS.md`, with `cargo run --release`:

| Operation | Time |
| --- | --- |
| Tree interpreter, 10,000 `while` iterations | 14 ms |
| Tree interpreter, 50,000 `while` iterations | 26 ms |
| Tree interpreter, 2,000 string appends | 84 ms |

These were measured to decide whether a Vortex row was worth attempting at all.
They are not a comparison with C or Rust, because no C or Rust run of the same
program exists. They are not in the benchmark table and are not a speed claim.
The command that produced them is `cargo run --release --example run_example`,
and the programs are the two loops described above.

The interpreter runs about 3 million loop iterations per second at the time of
writing, which is what a tree walk over slot indexed frames costs. Stage 4
measures this properly against a bytecode VM on the same machine.
