# The Vortex benchmark workload

`bench/vortex/sieve.vx` implements the sieve of Eratosthenes from
`bench/c/sieve.c` and `bench/rust/src/main.rs`, and it prints the same checksum
they do: `1179908154`. It runs in the tree interpreter and carries a time in
`BENCHMARKS.md`.

## What it uses

Two forms that stage 2 could not write and stage 3 added:

    var composite = [0; limit + 1];   // a list of a computed length
    composite[j] = 1;                // index assignment

## Two things that had to be right

**The sum wraps in the baselines.** The C baseline accumulates into a
`uint32_t`, so the sum wraps at 2^32 and prints 1179908154. A Vortex `Int` is 64
bit, so the plain sum is 142913828922. The program masks to 32 bits on every
addition to match:

    fn wrap32(n: Int) -> Int {
        return n % 4294967296;
    }

Masking rather than widening the language type is deliberate. A Vortex program
that wants the true sum should not have to imitate a C overflow to interoperate
with one.

**The matrix multiply now runs.** The baseline checksum
`1179908154 3314.003906` covers a sieve and a floating point matrix multiply.
Vortex can now write the matrix multiply because `SPEC.md` section 6.1 rule 4
allows an explicit `as` cast between `Int` and `Float`. The sieve file uses it
where an `Int` expression needs to become a `Float`:
`a[i * n + j] = ((i + j) % 17) as Float;`. The Vortex row therefore covers the
whole workload, and all three paths print the full checksum.

## What made it run at all

The first working version could not finish, and the cause was an implementation
defect rather than anything about the language.

A store into a list element read the whole list out of its frame slot, cloned
all of it, changed one element and stored it back. Every store cost the length
of the list, so the sieve was quadratic.

Measured before and after, with `cargo run --release --example run_example`:

| Limit | Before | After |
| --- | --- | --- |
| 10000 | 3676 ms | 27 ms |
| 20000 | 15862 ms | 37 ms |
| 100000 | timed out after 240 s | 138 ms |
| 2000000 | timed out | 2780 ms |

The fix is to hold a list in a shared cell and mutate through it, so a store
costs the same whatever the length. The language model is unchanged: the cell
never escapes the runtime, and a Vortex program still has exactly one binding
and one owner per list, which is what `SPEC.md` section 7 requires. The cell is
`Arc` rather than `Rc` only because the interpreter runs on its own thread so
that its call depth guard can report a runaway recursion instead of exhausting
the machine stack.

## What stage 4 did next

2780 ms against 18 ms for C is the gap the bytecode VM closed. That gap was the
interpreter, not the arithmetic: every step walks the tree and dispatches again.
The frontend already lowers once to `crates/vortexc/src/ir.rs`, which is exactly
what a register machine consumes, so the VM is a new executor rather than a
rewrite. Its time and checksum are recorded in `BENCHMARKS.md`, and all three
paths now print the full `1179908154 3314.003906`.

## What was deliberately not done

A smaller workload written three times to fill the table. Three different
algorithms in one table looks like evidence and is not, so the table carries one
algorithm measured three times instead.
