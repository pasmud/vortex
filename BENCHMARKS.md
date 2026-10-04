# Vortex benchmarks

## NO COMPARABLE VORTEX RESULT YET

There is no Vortex time in the comparison table, and this section says why
rather than leaving it blank.

A Vortex implementation of the reference workload now exists at
`bench/vortex/sieve.vx` and it is correct. It cannot supply a comparable time,
for two reasons that were measured rather than assumed.

**Its checksum does not match the baselines.** The C and Rust rows print
`checksum 1179908154 3314.003906`, which covers a sieve over 2,000,000 and a
floating point matrix multiply. Vortex v0.1 cannot write the matrix half: there
is no `as` cast yet and `SPEC.md` section 6.1 rule 4 forbids the implicit
conversion from `Int` to `Float`. The Vortex program therefore prints a sieve
checksum only, and a row whose checksum differs from the other two would not be
a comparison.

**The sieve half is too slow in a tree interpreter to run the workload.** These
were measured on the machine below with
`cargo run --release --example run_example`:

| Limit | Sum of primes | Time |
| --- | --- | --- |
| 100 | 1060 | not timed |
| 1000 | 76127 | not timed |
| 10000 | 5736396 | 3676 ms |
| 20000 | 21171191 | 15862 ms |

Doubling the limit from 10,000 to 20,000 took the time from 3.7 seconds to 15.9
seconds, so the cost grows faster than the limit, because the sieve's own work
grows with it. The baseline uses 2,000,000, which is two orders of magnitude
beyond a size that already takes sixteen seconds. Running it was attempted and
timed out, at 2,000,000 and again at 100,000, the latter after four minutes
with no output.

These four numbers are measurements of one Vortex program. They are **not** a
comparison with C or Rust, because no C or Rust run of the same program exists,
and they are not in the comparison table. `bench/vortex/README.md` records them
with the reasoning.

No number in this file is estimated, projected or carried over from another
project. When a Vortex result is added, it comes from an actual run of
`bench/run.sh` on a machine recorded in this file, with the raw output committed
alongside, and its checksum must equal the baselines'.

## The rule for adding results

`SPEC.md` section 9 sets the rule. In short: no speed statement may be made in
this repository unless it was produced by `bench/run.sh`, on a machine recorded
here, with the raw output committed. Estimates and projections are not evidence.

A result is added by a person, not by the harness. `bench/run.sh` prints; it
does not write to this file.

## Machine specification

Recorded from the machine the harness was first run on. Re-run `bench/run.sh`
and update this section whenever results are added on a different machine.

| Property | Value |
| --- | --- |
| CPU | Intel Core i5-8500T @ 2.10GHz |
| Architecture | x86_64 |
| Kernel | Linux 6.18.35 x86_64 |
| C compiler | gcc (Debian 12.2.0-14+deb12u1) 12.2.0 |
| Rust compiler | rustc 1.99.0 (b940084d7eb6 2026-09-28) |

The harness prints the machine and toolchain it actually used at the end of
every run, so a committed transcript always carries its own provenance.

## The workload

`bench/c/sieve.c` and `bench/rust/src/main.rs` implement the same algorithm:
a sieve of Eratosthenes over the integers up to 2,000,000, summed, followed by
a small floating point matrix workload.

Each implementation prints a checksum. The checksums of the two baselines
agree, which is how the harness tells that the implementations have not drifted
apart. A Vortex implementation must print the same value. If it does not, the
comparison is invalid and is fixed before any timing is recorded.

## Why this workload

It is compute bound, it depends on nothing a runtime might special case, and it
is short enough to run repeatedly. It is a baseline, not a claim. A language
comparison resting on one workload proves very little, so later stages add more
than one workload before any conclusion is drawn about relative speed.

## Running the harness

    bench/run.sh              # every implemented workload, once
    bench/run.sh --repeats 5  # five runs each, fastest is reported

The harness prints a table with one row per language and reports the reason a
row has no time instead of inventing one. Today that table has a C row and a
Rust row carrying the same checksum, and a Vortex row reading `n/a` with the
reason.

One run per workload is noisy. Any result that goes into this file should come
from at least five repeats.