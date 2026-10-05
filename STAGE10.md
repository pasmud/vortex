# Stage 10: building the emitted C at -O0

Stage 9 wrote that two of the emitter's gaps, the mangled names and the missing
constant propagation, do no harm today only because gcc at -O2 papers over them,
and predicted they would stop paying at -O0 or on a larger program. That was a
prediction. This stage measures it.

## The result

**The compiled path at -O0 prints the correct checksum.**

| Path | Output |
| --- | --- |
| compiled C at -O0 | `checksum 1179908154 3314.003906` |
| compiled C at -O2 | `checksum 1179908154 3314.003906` |
| tree interpreter | `checksum 1179908154 3314.003906` |
| bytecode VM | `checksum 1179908154 3314.003906` |

So the two gaps stage 9 named are real gaps in the emitter and are not
correctness bugs. The emitted C computes the right answer with the C compiler's
optimisations switched off.

That is worth saying plainly because the opposite was possible. If the compiled
path had been wrong here, it would have meant the path is only correct when gcc
rescues it, which would be the most serious finding in this project. It is not.

## Stage 9's prediction, and how it held

Stage 9 predicted the gaps would "stop paying at -O0 or on a larger program".
**Half of that held and half did not.**

What held: the cost of losing the optimisations is small on this workload. The
-O0 build is barely slower than the -O2 build.

| Build | Runs, ms | Fastest | Spread |
| --- | --- | --- | --- |
| Vortex C, -O2 | 49 47 56 48 49 | 47 ms | 47 to 56 |
| Vortex C, -O0 | 61 62 64 65 66 | 61 ms | 61 to 66 |
| C baseline | 17 18 20 22 18 | 17 ms | 17 to 22 |

Committed at `bench/results/stage10-o0.txt`, produced by
`bench/measure-all.sh --repeats 5`. **These are gcc figures, not Vortex
figures.** They measure what gcc at each level does with the emitted C.
`BENCHMARKS.md` compares -O2 and this document does not change it.

So the -O0 build is about 1.3 times the -O2 build on fastest-of-five, and about
3.6 times the hand written C.

What did not hold: the -O0 build is not dramatically slower, so "losing the
optimisation" costs roughly 20 ms here, not a factor of several. Stage 9 wrote
that the gaps "would stop paying", implying a larger effect than 20 ms. The
measurement says smaller.

## Why the optimisations still matter, measured

The instruction counts show the compiler really is doing less at -O2. The same
emitted C, assembled at both levels:

| Function | -O0 instructions | -O0 stack refs | -O2 instructions | -O2 stack refs |
| --- | --- | --- | --- | --- |
| `sieve_sum` | 68 | 25 | 11 | 0 |

At -O0 every value lives in a stack frame and the loop reloads it each
iteration. At -O2 it lives in a register. The whole translation unit is 1031
lines of assembly at -O0 against 535 at -O2.

So the optimisations are worth a great deal of work removed, and about 20 ms of
wall clock on this workload. Stage 9's claim that there is no spill traffic was
specifically about -O2, and that part was correct.

## The mangled names, one observation

Stage 9 named mangled names as a gap because gcc cannot tell that two locals
holding the same value are the same value. One test of that, hand written rather
than emitted, at -O0:

```c
int64_t sized(int64_t vx_0) { int64_t vx_1 = 256; return vx_0 ? vx_1 * vx_1 : 0; }
int64_t plain(int64_t n)     { int64_t s   = 256; return n ? s * s : 0; }
```

Both print 65536 and both compile to the same number of stack references at -O0
(7 each). **The mangled names cost nothing measurable at -O0 on this shape of
code.** That is one observation, not a study, and it does not rule out a case
where gcc could have merged two locals into one register and the name stops it.
It does mean stage 9 listed mangling as a gap on reasoning rather than on
evidence, and this is the evidence.

## A defect the -O0 build found in the examples

The workload was correct at -O0, but building the examples at -O0 found a real
one. A Vortex function that declares no return type was emitted as returning
`int64_t`:

```c
int64_t vortex_c_entry(void) {
    return vortex_main();   // vortex_main returns void
}
```

gcc at -O2 accepts that with a warning, so the defect survived every earlier
stage. **gcc at -O0 rejects it**: `error: void value not ignored as it ought to
be`. `hello.vx` produced no output at all at -O0 before the fix and prints
`21` then `6765` after it.

The fix is in `entry_c_type`, which reported `int64_t` for a function with no
declared return type instead of `void`, and in the generated entry, which no
longer returns or prints a value when the Vortex entry returns nothing.

This is the first thing in this project that the compiled path could not do at
all, as opposed to doing wrongly, and it was invisible at -O2 because the C
compiler was repairing it. That is worth recording as a general point about
measuring the compiled path only where the C compiler agrees.

`strings.vx`, `structs.vx` and `ownership.vx` still do not compile on the
emitted path, at -O0 or at -O2. `strings.vx` indexes a string, which the
emitter emits as a list access. The emitter should refuse that by name rather
than emit C that will not compile, and doing so is left for a stage that
addresses `match`, which is the same problem: the emitter walking a construct
it does not carry. It is not counted as a -O0 finding because it fails
identically at both levels.

## What this stage does not claim

Stage 9 replaced an unmeasured claim about spilling with a measurement, and then
made an unmeasured claim about gcc rescuing the emitter. This stage measured
that second claim and found it partly too pessimistic: the optimisations matter
more than the wall clock suggests and the mangled names matter less.

**No claim is made that the emitter should implement constant propagation or
stop mangling names.** Stage 9 asserted those were needed and this stage has not
shown that. Stage 10 shows what gcc does when it does not help, and the correct
next step is to measure a larger program, where the constant fold that -O2
currently performs for `let n = 256; let size = n * n;` may not happen.

## The engines

Neither engine is affected by a C compiler flag, and neither was touched. 206
tests pass on the tree interpreter and 206 on the VM, and all four examples
produce identical output on both.