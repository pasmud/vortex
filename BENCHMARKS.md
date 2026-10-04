# Vortex benchmarks

## NO VORTEX RESULTS YET

There are no Vortex benchmark results in this repository, and this section says
why rather than leaving it blank.

Stage 2 has a tree interpreter that runs the programs in `examples/`, but it
cannot run the benchmark workload. The C and Rust baselines both implement a
sieve of Eratosthenes over a byte array, and Vortex v0.1 has no index
assignment, so `a[i] = v` is a parse error, and no list of a computed length, so
the 2,000,001 element flag array cannot be built at all. `bench/vortex/README.md`
explains this in full and lists what stage 3 has to do about it.

The alternative would have been to write a different, weaker algorithm in all
three languages and put those numbers in one table. That would have produced
three real measurements that did not measure the same thing, and the table would
have read as a comparison it could not support. The row stays empty instead.

No number in this file is estimated, projected or carried over from another
project. When results are added, they come from an actual run of `bench/run.sh`
on a machine recorded in this file, with the raw output committed alongside.

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