# Corrections to the record

A claim in this repository's history was wrong. It is recorded here rather than
quietly dropped, because the point of this project is that the record can be
checked.

## Which direction the stage 4 benchmark drift went

The commit `a395ed2` message, and the pull request description that went with
it, said that all four documented benchmark numbers were *faster* than the
transcript they came from. That was wrong.

The drift at head `736209f`, recomputed from the artifacts rather than from
memory:

| Row | Documented | Transcript | Documented minus transcript | Direction |
| --- | --- | --- | --- | --- |
| C | 18 ms | 16 ms | +2 ms | documented slower |
| Rust | 18 ms | 19 ms | -1 ms | documented faster |
| Vortex tree interpreter | 3256 ms | 3250 ms | +6 ms | documented slower |
| Vortex bytecode VM | 5015 ms | 4939 ms | +76 ms | documented slower |

**Three of the four documented figures were slower than the transcript. One, the
Rust row, was faster by 1 ms.** So the direction was mixed and mostly
pessimistic, not uniformly optimistic.

A mixed sign with most rows marginally pessimistic is what documentation written
up from a separate harness run looks like. It is not the shape of anything else.

## What this changes

Nothing about the work. The defect was real either way: the four documented
numbers did not come from the committed transcript, and in a project whose
justification is traceable performance evidence, a reader who diffs the document
against the transcript should not find numbers of unknown provenance.

The fix is unchanged and was already made at `a395ed2`:

- `bench/run.sh --repeats 5` re-run, and its output committed as
  `bench/results/stage4-vm.txt`.
- Every number in `BENCHMARKS.md` copied from that transcript rather than typed.
- `scripts/check-benchmarks.sh` extended so that any wall clock number the
  document quotes must be the number the committed transcript records for that
  row.

The conclusion never depended on the direction. The VM is about 1.5 times slower
than the tree interpreter on this workload, and publishing that was the right
call whichever way the individual runs fell.

## Why this file exists

The error was in prose about a defect, in a commit message and a pull request
description, not in code or in a number. A reader who trusted the
characterisation would have drawn a conclusion about intent that the artifacts
do not support. Recording the correction keeps the record honest and gives the
next reader the arithmetic rather than the assertion.