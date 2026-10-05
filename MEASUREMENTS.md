# Stage 6 measurements

Every number here is copied from a committed transcript.
`scripts/check-benchmarks.sh` fails if a number in this file is not the number
the transcript records.

## The compiled path against the two Vortex engines

From `bench/results/stage6-compiled.txt`, produced by
`bench/measure-compiled.sh --repeats 5` on the machine in `BENCHMARKS.md`.

The function is a loop summing squares to 2000000, and all three paths return
the same value:

| Path | Runs, five repeats | Fastest | Answer |
| --- | --- | --- | --- |
| compiled C | 6, 6, 7, 9, 7 ms | 6 ms | 2666668666667000000 |
| tree interpreter | 560, 561, 561, 560, 563 ms | 560 ms | 2666668666667000000 |
| bytecode VM | 949, 949, 943, 954, 941 ms | 941 ms | 2666668666667000000 |

**The compiled path is about 93 times faster than the tree interpreter and about
157 times faster than the VM on this function.**

That is a bigger gap than the language design deserves, and the reason has to be
stated rather than left to flatter the result. A C function compiled with `-O2`
that runs a counted loop over an `int64_t` is about as fast as that loop can get.
The tree interpreter runs the same loop through a recursive walk over
expression nodes, allocating and matching on each. The VM is worse still. So the
gap measures how much the two executors cost, not how good a language this is,
and it says nothing about a workload with allocation, strings or calls, which is
where a tree walk stops being cheap.

## What this does not measure

The emitter handles scalar functions with loops. It refuses a list, an `if`
expression, a `match` and an open ended range, naming the construct and its
position. So it **cannot** carry `bench/vortex/sieve.vx`, which uses all three.

No number for the full benchmark workload is recorded, because producing one
would need an emitter that can run it. A figure next to the sieve row that came
from a different program would be exactly the kind of number this repository
exists to avoid.

## A defect the measurement found

The first run of the compiled path reported **307880128** where both engines
reported 2666668666667000000. The C entry point cast its result through `int`,
which is 32 bits on this platform, so a 64 bit answer came back truncated.

This is worth recording because it is the reason the measurement runs the same
function on all three paths and prints all three answers. A compiled path that
had only been compared against itself would have shipped that bug. The cast is
gone; the entry point now carries the declared return type.
