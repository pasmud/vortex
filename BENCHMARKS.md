# Vortex benchmarks

## A recorded Vortex result

The comparison table has Vortex rows for both execution engines. **All four rows
run the same program**, a sieve of Eratosthenes over 2,000,000 plus a 256 by 256
floating point matrix multiply, and all four print the same checksum. The
checksum is shown in full, including the float, so a reader can see that the
same work was measured rather than being asked to take it on trust.

| Language and engine | Wall clock | Sieve sum | Matrix sum | Full checksum |
| --- | --- | --- | --- | --- |
| C | 18 ms | 1179908154 | 3314.003906 | `1179908154 3314.003906` |
| Rust | 18 ms | 1179908154 | 3314.003906 | `1179908154 3314.003906` |
| Vortex, tree interpreter | 3217 ms | 1179908154 | 3314.003906 | `1179908154 3314.003906` |
| Vortex, bytecode VM | 4913 ms | 1179908154 | 3314.003906 | `1179908154 3314.003906` |

Raw harness output for the tree interpreter row is committed at
`bench/results/stage3-tree-interpreter.txt`, and for both Vortex rows at
`bench/results/stage4-vm.txt`. Both were produced by:

    bench/run.sh --repeats 5

### The VM is slower than the tree interpreter

**The bytecode VM is about 1.53 times slower than the tree interpreter on
this workload: 4913 ms against 3217 ms.** That is a disappointing number and
it is published rather than omitted, because it is a fact about this
implementation.

Every figure in this table is copied from the committed transcript above, and
`scripts/check-benchmarks.sh` fails if a number here is not the number the
transcript records. That check exists because an earlier revision of this file
was written by hand from one run while the transcript was from another, and all
four numbers disagreed. `CORRECTIONS.md` records which way each one drifted: three
of the four were pessimistic and one was optimistic by 1 ms.

The reason is not the dispatch the VM was built to remove. A tree walk in Rust
recurses, so each Vortex call becomes native calls that the optimiser already
inlines and keeps in registers. The VM instead walks a `Vec<Instr>`, matching on
an enum per instruction and pushing and popping a `Vec<Value>` for every
operand, so it adds work the tree walk never had. Removing per node dispatch
was the wrong diagnosis of where the time went.

What that means for stage 4 and stage 5 is concrete. A bytecode VM in a
dynamically typed tree interpreter is not automatically faster than walking a
tree, and a VM in this design should not be expected to be. The measurements
that would change it are a frame-allocated operand stack rather than a `Vec`, and
avoiding the `Value` clone on every store and load. Neither has been tried, so
neither is claimed.

**No Vortex speed claim is made beyond this table.** Vortex is roughly
273 times slower than C here. That is a fact about a stage 4 tree
interpreter and a stage 4 bytecode VM, not about the design of the language, and
not about where it could end up.

The Vortex matrix half needed the `as` cast, which stage 4 added. Before it, the
Vortex row covered the sieve only and the table said so. It no longer needs to,
and this table reflects what was measured rather than what was possible before.

## What the three rows measure

All three implement the workload in `bench/c/sieve.c` and
`bench/rust/src/main.rs`, and all three print `checksum 1179908154 3314.003906`.

Matching that needed two non obvious steps, both recorded rather than stumbled on.

The C baseline accumulates into a `uint32_t`, so the sum wraps at 2^32. Vortex
`Int` is 64 bit, so a plain sum gives 142913828922 instead. `bench/vortex/sieve.vx`
masks to 32 bits on every addition to match the baseline arithmetic exactly.
Masking rather than widening the Vortex type is deliberate: a Vortex program
that wants the true sum should not have to imitate a C overflow.

The matrix half needs `Int` to `Float` and back, which `SPEC.md` section 6.1
rule 4 forbids implicitly and requires to be written as `as`. Stage 4 added the
cast, and with it the float half became expressible. An earlier revision of this
file recorded that the Vortex row was sieve only; that was true when the cast
did not exist and is corrected here.

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
recorded numbers appear in this file, and that all four rows of the committed
output carry the same checksum.

## Why one workload is not enough

A language comparison resting on one workload proves very little, and this table
proves that directly: it says the bytecode VM is about 1.53 times slower
than the tree interpreter, and about 273 times slower than C, on a sieve.
That says nothing about integer arithmetic in general, because the dominant cost
here is the interpreter's execution model rather than the arithmetic. Later stages add more than one workload before any
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
