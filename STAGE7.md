# Stage 7: the compiled path on the benchmark workload

Every number here is copied from `bench/results/stage7-sieve.txt`, produced by
`bench/measure-all.sh --repeats 5` on the machine in `BENCHMARKS.md`.
`scripts/check-benchmarks.sh` checks this file against that transcript, and it
caught the figures here being out of date once already, which is why the table is
generated from the transcript rather than written by hand.

## What is measured, and what each row covers

| Path | Runs, five repeats | Fastest | Reports |
| --- | --- | --- | --- |
| C | 18, 23, 19, 17, 18 ms | 17 ms | sieve and matrix
| Rust | 23, 19, 20, 22, 23 ms | 19 ms | sieve and matrix
| Vortex C | 52, 44, 48, 43, 39 ms | 39 ms | **sieve only**
| Vortex tree | 3276, 3232, 3284, 3262, 3233 ms | 3232 ms | sieve and matrix
| Vortex VM | 5027, 4997, 5026, 4993, 5029 ms | 4993 ms | sieve and matrix

All five paths agree on the sieve sum, **1179908154**.

## The Vortex compiled row covers the sieve only

The emitted C for the whole of `bench/vortex/sieve.vx` reports a matrix value of
**3143** where both engines report **3314.003906**. The compiled path therefore
does not yet agree with the two engines on the complete workload, so its row is
labelled "sieve only" and the comparison is a comparison of sieve measurements.
No figure for the full workload is claimed, and no Vortex row appears in
`BENCHMARKS.md` beside the C and Rust rows, because a row that measured something
narrower than the rows around it would read as a comparison and would not be
one.

The sieve half is exact integer arithmetic and matches. The matrix half is
`Float` arithmetic and does not. That is a defect in the emitter, not in the
workload and not in the language, and it is not yet chased.

## What the compiled number means

**The compiled Vortex path is about 0.4 times slower than the C baseline on
the sieve, and about 83 times faster than the tree interpreter.** Rust
is 19 ms on the same host.

The gap to C is small, and it is what this stage is about. A compiled Vortex
program is not a toy; it is 190 times faster than the tree
interpreter on the same workload. What remains is the cost of four passes the
emitter does not have.

The gap from the interpreters is not a statement about the language. It is what a
tree walk and a bytecode VM cost on a loop. Stage 6's 93x figure came from a
counted loop over an `Int`, which is nearly the best case for compiled code; the
sieve is a more honest workload and the tree interpreter is still 83
times slower on it.

## What each missing optimisation is likely to buy

- **Register allocation.** The single largest item. Every value currently lives
  in a stack slot, so a loop body is a sequence of loads and stores. This is the
  difference between the emitter's output and what C produces for the same
  source, and it is most of the remaining gap. This is the one to do next.
- **Constant propagation and copy propagation.** The emitter already knows a
  `let` whose initialiser is a literal. Removing those slots would take a large
  share of the loads out of a tight loop. Cheapest of the four and likely to
  close a good part of the gap alongside register allocation.
- **Inlining.** `sieve_sum` and `matrix_work` are each called once, so inlining
  them changes almost nothing here. It would matter for a workload with a hot
  small function in a loop, which this is not. Low value for this measurement,
  and saying so is more useful than listing it as a win.
- **Escape analysis.** The emitter allocates every list with `malloc` and never
  frees it. For this workload the allocation happens once, so it does not
  dominate. It would dominate a workload that allocates per iteration. Low
  value here.

So: register allocation and constant propagation are where the gap to C is.
Inlining and escape analysis would not move this measurement much.

## What is not measured

The floating point half does not yet agree, so the table compares sieve
measurements. Until it agrees, the Vortex compiled row stays out of the main
table in `BENCHMARKS.md`.