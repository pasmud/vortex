# Stage 9: register allocation in the compiled path

`BENCHMARKS.md` named register allocation as the largest single item in the gap
between a compiled Vortex program and a hand written C one, and marked it
UNMEASURED. This stage measures it. The measurement is negative, and the reason
is more useful than a number would have been.

## The claim being tested

The stated claim was that the emitter spills every value to a stack slot and
never allocates a register, so a loop body is a sequence of loads and stores.

## What the emitter actually produces

The emitter writes a Vortex local as a C local of the corresponding type. The
simplest case, a loop with an integer accumulator:

```vortex
fn sum_to(n: Int) -> Int {
    var i = 1;
    var total = 0;
    while i <= n {
        total = total + i;
        i = i + 1;
    }
    return total;
}
```

becomes

```c
int64_t sum_to(int64_t n) {
    int64_t i = INT64_C(1);
    int64_t total = INT64_C(0);
    while ((i <= n)) {
        total = (total + i);
        i = (i + INT64_C(1));
    }
    return total;
}
```

These are C locals, not memory the emitter allocated. Whether they end up in
registers is gcc's decision, and gcc makes it at -O2.

## The measurement

Compiled at `gcc -O2 -S` and the emitted assembly inspected directly:

| Function | Instruction lines | Stack references |
| --- | --- | --- |
| `sieve_sum` | 11 | 0 |
| `matrix_work` | 145 | 0 |
| `vortex_main` | 161 | 4 |
| `vortex_c_entry` | 4 | 0 |

Seven stack references across the whole translation unit, and the loop in
`sum_to` compiles to eight instructions with no memory operand at all:

```asm
sum_to:
        testq   %rdi, %rdi
        jle     .L4
        addq    $1, %rdi
        xorl    %edx, %edx
        movl    $1, %eax
.L3:
        addq    %rax, %rdx
        addq    $1, %rax
        cmpq    %rdi, %rax
        jne     .L3
```

Both hot functions keep every live value in a register. The one line in
`matrix_work` that matched a stack operand is `leaq 0(%rbp,%r8), %rsi`, which
computes an address rather than spilling a value.

**There are no spills to remove.** Register allocation in the emitter would
allocate registers that the C compiler has already allocated.

## Before and after

No source change was made, because there is nothing to change: the emitter
already produces code whose live values are in registers. Both measurements
were taken on the same machine in the same session, five repeats each, every
run printed, committed at `bench/results/stage9-before.txt` and
`bench/results/stage9-after.txt`.

| Path | Before, ms | After, ms |
| --- | --- | --- |
| Vortex, compiled to C | 55 46 44 43 52 | 48 50 55 51 48 |
| Vortex, tree interpreter | 3335 3262 3261 3268 3256 | 3254 3268 3293 3259 3285 |
| Vortex, bytecode VM | 4957 5015 4996 4978 4983 | 4986 5019 4987 4995 5020 |

The compiled row spans 43 to 55 ms before and 48 to 55 ms after. Those overlap,
and they are the same code, so the difference is noise. Comparing against the
spread committed at `bench/results/stage8-compiled-full.txt`, which spans 42 to
53 ms, there is no movement to attribute to anything.

The C baseline measured 17 to 20 ms before and 17 to 28 ms after on the same
runs, which is the same noise at a smaller scale.

## Result: the optimisation was not applicable, and nothing was changed

This is a different outcome from a reverted optimisation. Stage 5 tried two VM
changes, measured them, found the gains inside the noise and reverted them. Here
the measurement that came first showed the premise does not hold: there is no
spill traffic for the optimisation to remove.

No source change is retained from this stage, so there is nothing to revert.

## What the gap to C actually is

The compiled path is about 2.5 times slower than the C baseline, and this stage
removes the most plausible remaining explanation for it. What is left, from
looking at the emitted assembly rather than at the emitter:

**Allocation.** `matrix_work` calls `calloc` three times, once per list. The C
baseline declares its three arrays `static double a[N][N]`, which the loader
zeroes into `.bss` and the optimiser can often fold away. Three calls is not
much, but it is work the baseline does not do.

**Mangled names.** Every Vortex name becomes `vx_` plus an index, so gcc cannot
treat two locals as the same value across a boundary. This is a small effect but
it removes information gcc would otherwise use.

**No inlining decisions.** `sieve_sum` and `matrix_work` are emitted as separate
functions and gcc inlines them because it can see the whole program. A larger
program would not get that, so the emitter's lack of an inlining pass would start
to matter. `STAGE7.md` already argued inlining is worth little for this
workload, and the assembly confirms it for this workload.

**Constant propagation.** `let n = 256; let size = n * n;` is not folded, because
the emitter writes the multiplication out. At -O2 gcc folds it anyway. This too
would stop being true at -O0 or on a larger program.

The last two are cases where gcc currently rescues the emitter. That is not
something to rely on, and it is the next place to look.

## What this stage does not claim

No speedup is claimed, because there is no change to attribute one to. The
compiled row in `BENCHMARKS.md` is unchanged at 42 ms against a C baseline of
17 ms, and the gap has not narrowed.

The two engines were not touched: 206 tests pass on the tree interpreter and 206
on the VM, and all four examples produce identical output on both.