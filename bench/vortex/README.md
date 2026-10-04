# The Vortex benchmark workload

There is a Vortex program at `bench/vortex/sieve.vx`, and it does not produce a
time in the comparison table. This file records why, with the measurements that
show it.

## What the program does

It implements the sieve of Eratosthenes from `bench/c/sieve.c` and
`bench/rust/src/main.rs`, using the two features stage 3 added: a list of a
computed length, `[0; limit + 1]`, and index assignment, `composite[j] = 1`.

The algorithm is correct. These checksums were produced by running it:

| Limit | Sum of primes | Time |
| --- | --- | --- |
| 100 | 1060 | not timed |
| 1000 | 76127 | not timed |
| 10000 | 5736396 | 3676 ms |
| 20000 | 21171191 | 15862 ms |

Each was run with `cargo run --release --example run_example -- <file>` on the
machine recorded in `BENCHMARKS.md`.

## Why it is not in the table

**The workload is not the same one, so its checksum cannot match.** The C and
Rust baselines print `checksum 1179908154 3314.003906`. That covers two things:
a sieve over 2,000,000 and a floating point matrix multiply. Vortex v0.1 cannot
write the second half, because it has no implicit conversion from `Int` to
`Float` and no `as` cast yet, and section 6.1 rule 4 forbids the implicit one.
So the Vortex program prints a sieve checksum only, which does not equal the
baseline checksum. A row with a different checksum in a table that compares
checksums would be worse than no row.

**The sieve half does not finish in usable time anyway.** A tree interpreter
with no bytecode and no host interoperation is slow, and this is the honest
measurement rather than an estimate. Doubling the limit from 10,000 to 20,000
took the time from 3.7 seconds to 15.9 seconds, so the cost grows faster than
linearly in the limit, because the sieve's own work grows with it. The baseline
uses 2,000,000, which is two orders of magnitude beyond a size that already
takes sixteen seconds. Running it was attempted and timed out, twice: once at
2,000,000 and once at 100,000, the latter after four minutes without producing
output.

So the Vortex row prints `n/a` and names the reason. No Vortex time is recorded
and none is estimated.

## What stage 4 should do

This is the evidence stage 4 needs, and it is the reason stage 4 exists.

- **The bytecode VM is what makes this workload reachable.** The tree walk
  re-evaluates and re-dispatches on every step. A bytecode loop with a frame of
  slots and no dispatch per node is the obvious next step, and this measurement
  is the before number it has to beat.
- **Add `as` casts so the matrix half is expressible.** Then the checksum can
  match the baselines and the comparison becomes a real comparison rather than
  two workloads that happen to sit in one table.
- **Keep the checksum discipline.** When the Vortex row finally carries a time,
  it carries the same checksum as the other two, and `bench/run.sh` refuses to
  print a time for a program whose checksum does not match.

## What was deliberately not done

A smaller workload written three times, and three numbers in one table. That
would have produced three real measurements of three different algorithms, and
the table would have read as a comparison it could not support. The numbers
above are measurements of one Vortex program, they are labelled as such, and
they are not a comparison with C or Rust.
