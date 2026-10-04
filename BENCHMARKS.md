# Vortex benchmarks

## A recorded Vortex result

The comparison table has a Vortex row. It carries the same checksum as the C and
Rust rows, and it was produced by `bench/run.sh` on the machine below.

| Language | Wall clock | Checksum |
| --- | --- | --- |
| C | 18 ms | 1179908154 |
| Rust | 17 ms | 1179908154 |
| Vortex, tree interpreter | 2780 ms | 1179908154 |

The raw harness output is committed at
`bench/results/stage3-tree-interpreter.txt`, produced by:

    bench/run.sh --repeats 5

**Vortex is about 155 times slower than C on this workload.** That is a tree
interpreter with no bytecode and no host interoperation, which is what stage 2
built and stage 3 type checked. It is not a statement about the design of the
language, only about where this implementation stands today. Stage 4 exists to
improve exactly this number, and the measurement above is the before number it
has to beat.

Nothing here claims Vortex is fast. It does not yet have the evidence to.

## What the three rows measure

All three implement the sieve of Eratosthenes from `bench/c/sieve.c` and
`bench/rust/src/main.rs`, and all three print `checksum 1179908154`.

The Vortex checksum matches, and getting there required one non obvious step.
The C baseline accumulates into a `uint32_t`, so the sum wraps at 2^32. Vortex
`Int` is 64 bit, so a plain sum gives 142913828922 instead. `bench/vortex/sieve.vx`
masks to 32 bits on every addition to match the baseline arithmetic exactly.
Masking rather than widening the Vortex type is deliberate: a Vortex program
that wants the true sum should not have to imitate a C overflow.

The Vortex row covers the sieve only. The baseline checksum also covers a
floating point matrix multiply, which Vortex cannot yet write because there is no
`as` cast and `SPEC.md` section 6.1 rule 4 forbids the implicit `Int` to `Float`
conversion. The integer part of the checksum is what the harness compares, and
it is the same in all three. The floating point half is simply absent from the
Vortex row, and this is stated rather than papered over.

## What made it fast enough to run at all

The first working Vortex sieve could not finish. limit 10000 took 3676 ms and
limit 20000 took 15862 ms, so the baseline's 2000000 was unreachable; it timed
out twice, at 2000000 and again at 100000 after four minutes.

The cause was an implementation defect rather than the language. A store into a
list element read the whole list out of its frame slot, cloned all of it,
changed one element and stored it back. Every store cost the length of the
list, which made the sieve quadratic. The fix was to hold a list in a shared
cell and mutate through it, so a store costs the same whatever the length.
`bench/vortex/README.md` records the before and after.

This is worth stating plainly because it is the sort of thing a single run hides:
the sieve went from not finishing to 2780 ms, a change of several orders of
magnitude, from one data structure choice.

## Machine specification

| Property | Value |
| --- | --- |
| CPU | Intel Core i5-8500T @ 2.10GHz |
| Architecture | x86_64 |
| Kernel | Linux 6.18.35 x86_64 |
| C compiler | gcc (Debian 12.2.0-14+deb12u1) 12.2.0 |
| Rust compiler | rustc 1.99.0 (b940084d7eb6 2026-09-28) |

The harness prints the machine and toolchain it actually used at the end of
every run, so a committed transcript always carries its own provenance.

## The rule for adding results

`SPEC.md` section 9 sets the rule. In short: no speed statement may be made in
this repository unless it was produced by `bench/run.sh`, on a machine recorded
here, with the raw output committed. Estimates and projections are not evidence.

A result is added by a person, not by the harness. `bench/run.sh` prints; it
does not write to this file. `scripts/check-benchmarks.sh` checks that the
machine specification and the committed raw output are present, that the
recorded numbers appear in this file, and that all three rows of the committed
output carry the same checksum.

## Why one workload is not enough

A language comparison resting on one workload proves very little, and this table
proves that directly: it says the tree interpreter is 155 times slower than C on
a sieve, which says nothing about integer arithmetic in general, because the
dominant cost here is the interpreter's dispatch and its list handling rather
than the arithmetic. Later stages add more than one workload before any
conclusion is drawn about relative speed.

## Running the harness

    bench/run.sh              # every implemented workload, once
    bench/run.sh --repeats 5  # five runs each, fastest is reported

The harness prints a row only for a binary that actually ran, and it compares
the Vortex checksum against the C baseline before printing a Vortex time. A row
whose checksum does not match reads `n/a` and says so, so a measurement of a
different workload cannot be presented as a comparison.

One run per workload is noisy. Any result that goes into this file should come
from at least five repeats.
