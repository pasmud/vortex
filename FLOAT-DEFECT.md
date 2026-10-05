# Stage 8: the compiled float path

## What the float defect was, and how it was narrowed

**It was three separate defects in the emitter, not one, and none of them was a
floating point behaviour difference between the compiled and interpreted paths.**
All three are in `crates/vortexc/src/cgen.rs` and all three are about type
information being lost between the lowered form and the emitted C.

The wrong answer was reproducible: the compiled path reported a matrix value of
**3143** where both engines report **3314.003906**, while the sieve half matched
exactly at 1179908154.

### How it was narrowed

By reduction, not by reading the code. Four programs, each the smallest thing
that still failed:

| Program | Compiled | Engines | What it isolated |
| --- | --- | --- | --- |
| `let x = 1 as Float; let y = 2 as Float; return x + y;` | 3 | 3.000000 | addition is fine |
| `let w = v * 0.5 + 1.0; return w;` | 2 | 2.500000 | **arithmetic loses Float** |
| `var a = [1.0; 6]; a[5] = 0.25; return a[5];` | 0 | 0.250000 | **a Float in a list truncates** |
| `var a = [0.0; 16]; a[5] = 1.5; return a[0 * n + 5];` | 1 | 1.500000 | **an indexed Float read truncates** |

### Defect one: arithmetic never inferred Float

`expr_type_of` returned `int64_t` for every arithmetic operation unless the
operator was a comparison. So `let w = v * 0.5 + 1.0` declared `int64_t w`,
and the C truncated 2.5 to 2.

The emitted C was:

    int64_t w = ((v * 0.500000) + 1.000000);

**Fixed** by inferring `double` when either operand is, which is the rule the
tree interpreter already applies. After the fix that program returns
2.500000 on all three paths.

### Defect two: a list was always an array of int64_t

`CList` was declared as `int64_t *items`, and both list constructors allocated
an `int64_t` array. Every Float stored into a list was truncated: `0.25` became
`0` and `1.5` became `1`. The sieve half was unaffected because it stores only
integers, which is exactly why the integer half matched and the float half did
not.

**Not yet fixed.** The list struct is now declared with `void *items` and each
access casts to the element type, and the call sites name the right
constructor, but the constructor bodies still need the same treatment. That is
the next commit. The diagnosis is recorded here rather than left as a claim,
because the reduction above is what makes the fix checkable.

### Defect three: the harness printed the return as a long long

The generated entry wrote its returned value with `%lld` and a `(long long)`
cast whatever the entry declared. A Float return truncated there, so the harness
would have reported 2 rather than 2.500000 even after defects one and two were
fixed. This one **is** fixed: the value is printed at the type the entry
declares, with `%.6f` for a Float.

## Why the sieve half matched and the matrix half did not

Worth stating because it is the reason the bug survived stage 7. The sieve stores
only `Int` into its list and returns an `Int`, so it passed through all three
defects unchanged. The matrix stores `Float` and returns a `Float`, so it hit all
three. A workload that had exercised only integers would have reported the
compiled path as fully working.

## What this means for the benchmark row

The Vortex row still does not go in `BENCHMARKS.md`. Two of the three defects
are fixed and verified by reduction; the list element type is not, so the
compiled path still disagrees on the matrix half. The guard still requires the
`sieve only` label, which remains accurate.
