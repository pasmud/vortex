# Stage 5 decision: the backend and the memory model

This is the decision stage 4 left open, written before any code so the code can
be judged against it rather than the other way round.

## The question

Stage 4 measured the bytecode VM as about 1.5 times slower than the tree
interpreter, and named two untried changes as the things that might reverse it.
Stage 5 tried both. Neither paid for itself. So the question is no longer "how do
we make the VM fast" but the prior one: **is a bytecode VM the right execution
path for this language at all?**

## What was measured

All from `bench/measure-engines.sh`, raw output in `bench/results/`.

### The baseline, on merged stage 4

| Engine | Runs, five repeats | Fastest |
| --- | --- | --- |
| tree | 3227, 3240, 3223, 3247, 3233 ms | 3223 ms |
| VM | 4982, 4981, 4935, 4940, 4949 ms | 4935 ms |

Both print `checksum 1179908154`, so both measured the same program. The spread
across runs is 24 ms for the tree and 47 ms for the VM. Anything smaller than
that on this host is not evidence of anything, which is what decided the two
changes below.

### Change one: a frame allocated operand stack

Stage 4 named this first. The operand stack was a `Vec<Value>` that grew and
shrank as an expression nested, so every push could reallocate. This replaced it
with a fixed size array and an explicit stack pointer, sized from the program's
actual need.

| Array size | VM, fastest of five |
| --- | --- |
| 256 slots (18 KB per frame) | 5419 ms |
| 32 slots (2.3 KB per frame) | 5283 ms |

**Worse than the 4935 ms baseline at both sizes.** The array has to be filled
with `Value::Unit` on every frame, and at 72 bytes a `Value` that fill is the
cost. A 256 slot array initialises 18 KB per function call, which is far more
than the pushes it was meant to save. Shrinking to 32 slots recovered part of it
and still lost. This change is reverted.

The reason is worth recording: it was never a reallocation problem. The `Vec`
never reallocated in practice, because the stack reached its high water mark in
the first few instructions and stayed there for the life of the frame. The change
replaced a cheap push with an expensive frame construction.

### Change two: avoiding the Value clone on load and store

Stage 4 named this second. `Value` is 72 bytes, so every slot load and store
copied 72 bytes. This boxed the string payload behind an `Arc`, which is the
safest way to make a clone cheaper.

| Engine | Before | After |
| --- | --- | --- |
| VM | 4939 ms | 4920 ms, then 4962 and 4965 ms on two further rounds |
| tree | 3223 ms | 3216 ms, then 3244 and 3241 ms |

**Inside the jitter, and so reverted.** Two further rounds of five repeats each
put the VM at 4962 and 4965 ms against a 4935 ms baseline, which is slower. The
first round's 4920 ms was the lucky run of nine, not a result. Boxing the
string also did not shrink `Value` at all: it stayed 72 bytes, because `Vec<Value>`
in the tuple and struct variants is what fills the enum, not the string.

The full fix for this cost is to hold frame slots uninitialised and hand out
references, which is what a production VM does. That needs `unsafe`. See the
decision below for why it is not done now.

## Decision one: the native backend is an ahead-of-time compiler

**Decision: the native backend compiles the lowered form to native code. It does
not consume the bytecode VM, and the VM is not on the path to it.**

The evidence is the two measurements above. The bytecode VM pays for an
instruction walk, an enum match per instruction, and an operand stack push and
pop per operand. A Rust tree walk pays for none of that, because a Vortex call
becomes native calls that the optimiser already inlines and keeps in registers.
The tree interpreter is not fast either, at 3230 ms against C at 18 ms, but it is
the faster of the two Vortex engines and it is the one whose shape a compiler can
consume: `crates/vortexc/src/ir.rs` is a tree of calls, arguments and slots, which
is close to what a code generator wants. A bytecode program is a flat array, and
compiling it to native code means recovering the tree structure the compiler
already threw away.

So the compiler consumes the same lowered form the tree interpreter walks, which
is what lowering once in stage 2 was for. This is not a change of plan. Stage 4
resolved section 10.1 the same way and this measurement confirms it.

**Falsifier for this decision:** a bytecode VM that beats the tree interpreter on
the benchmark workload by more than the 47 ms jitter, after the frame allocated
stack and the uninitialised slots have both been tried. If the VM wins, a VM is
the better intermediate for a compiler as well as for an interpreter, and this
decision is wrong. The two changes below are the ones that would have to be
tried first.

## Decision two: the VM is retained as a test oracle, not discarded

**Decision: keep `crates/vortexc/src/vm.rs`, and keep running the whole example
set and the stage 2 and 3 suite on both engines.**

The VM is worth keeping for a reason the measurement supports. It is a second,
independent reading of the semantics. In stage 4, comparing the two engines on
the example set found twelve defects, eight of them silent wrong answers rather
than crashes, and every one invisible from the examples that already passed.
That was the single most productive piece of engineering in the project so far
and it cost nothing to keep, because the VM already existed.

It is not kept as a fast path, because it is not one. Its value is that it is
written differently from the tree interpreter, so it makes different mistakes.
A third implementation, the compiler, will make a third set of mistakes, and the
same comparison finds them.

**Falsifier for this decision:** the differential testing stops finding defects.
Concretely, if two full rounds of the example set and both suites on both
engines find nothing new, the VM has stopped being an independent reading and is
just maintenance. At that point it should be deleted rather than kept out of
sentiment.

## Decision three: the memory model is linear ownership, and it is not enforced yet

**Decision: keep the linear ownership model of `SPEC.md` section 7, and say
plainly that the compiler does not enforce it today.**

What exists now: every value is owned by one binding, lists are behind a shared
cell, and a list store mutates in place rather than copying. What does not exist:
any check that a moved value is not used again. `SPEC.md` section 8.2 records
this, and the honest position is that the model is chosen and not yet enforced.

This is not a regression. Enforcement was never in stage 5's scope and the
previous stages did not claim it. What stage 5 decides is that the model is
right, so enforcement is worth doing later.

**Why linear ownership rather than a collector or a borrow checker.** A collector
gives up deterministic destruction, which matters for the file handles and
sockets the language will eventually want, and it turns allocation into the
dominant cost of exactly the pointer chasing workload this project is built to
be fast at. A borrow checker needs borrows and lifetimes, and the cost is a class
of error whose projection is frequently not the line the programmer has to
change. Linearity gets most of the safety with one rule to state and one rule to
check.

**Its failure modes, and how a programmer would meet them.** The main one is
aliasing: with one owner per value, a graph with cycles cannot be written
directly, and a shared structure has to be an arena index or an explicit parent
pointer. A programmer meets this as a compile error when enforcement arrives, not
as a wrong answer, and that is the property worth having. The second is that
deterministic deallocation means a value is dropped at the end of its scope, so
releasing an external resource happens without the programmer writing it.

`SPEC.md` section 7.2 already frames this as Option A against Option B, keep
linearity or add a `cell<T>` box with a narrow collector. Stage 3 already needed
a cell for a list so the tree interpreter could store into one without copying
it, which is a small local version of Option B. That is not a precedent for a
collector, because the cell never escapes the runtime and a Vortex program still
has one binding per list. The decision rule in section 7.2 stands: adopt a
collector only if a committed benchmark shows linearity is the limiting factor.

## Decision four: no concurrency in v0.1

**Decision: Vortex is single threaded. There is no concurrency model, and none is
planned for v0.1.**

The reason is that there is nothing to make concurrent yet. The memory model is
chosen but not enforced, so there are no ownership boundaries to transfer across
a thread. Adding threads before enforcement would mean inventing a story about
what is shared while the answer is still open.

The evidence is stage 4's own defect list as much as anything: twelve loop
defects, eight silent, all found by running two engines against each other.
Correctness work of that size is not finished. Concurrency would multiply the
places a mistake can hide rather than reduce them.

**What single threading costs a programmer.** Blocking on the only thread, so a
slow call stalls the program. No parallelism for a CPU bound loop, which is the
case this project cares most about and the one it is slowest at. Both are real
and both are acceptable while the single threaded number is 3230 ms against C
at 18 ms, because concurrency does not close a gap that large anyway.

**Failure modes introduced by this decision:** none new, because nothing was
added. That is the point. The failure mode to watch is the opposite one, which is
adding concurrency later and having the memory model not yet enforced.

**Falsifier:** a measured workload where the tree interpreter spends most of its
time in one tight loop and the rest of the language is idle, such that a single
thread leaves most of the machine unused. Threads would pay for themselves then.

## What was deliberately not done

- **Uninitialised frame slots in the VM.** This is the change most likely to
  make the VM competitive, since it removes the 72 byte copy on every load and
  store that change two only partly addressed. It needs `unsafe`, and
  `SPEC.md` section 7.3 restricts unsafe to modules that also declare a foreign
  interface, which a VM does not. Doing it properly means amending section 7.3
  first and reviewing the code that results, which is a stage of its own and not
  something to slip in here. It is named as the falsifier condition above.
- **A tree interpreter that is faster.** The tree interpreter is the engine the
  compiler will consume, so it is where optimisation effort belongs, and the
  obvious first win is a flat instruction form for the hot path rather than a
  recursive walk. That is stage 4 work reopened deliberately, not a stage 5
  deliverable.
- **Any performance claim about a native backend.** None has been written, so
  none is made.