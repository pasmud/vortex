# Stage 8 progress and the remaining defect

## What the float defect was

Three defects in the emitter, none of them a floating point behaviour
difference between the compiled and interpreted paths. The reduction that found
them is in `FLOAT-DEFECT.md`. Two are fixed; the third is not, and this file
records where it stopped.

## Fixed and verified by reduction

| Program | Compiled | Engines |
| --- | --- | --- |
| `x + y` with both `Float` | 3 | 3.000000 |
| `v * 0.5 + 1.0` | **2.500000** | 2.500000 |
| the generated entry printing its return | `2.500000` | 2.500000 |

`expr_type_of` now infers `double` when either operand of an arithmetic operation
is a `Float`, which is the rule the tree interpreter already applied. And the
generated entry prints its return at the type the entry declares, so a `Float`
return is not truncated by the harness.

Both were applied, verified with the reduction programs, and then **lost**: a
`git checkout` of `cgen.rs` during the constructor edit reverted them along with
the work in progress. They are re-applied and re-verified in the commit that
follows this file. The lesson is recorded because it cost two turns and the same
thing will bite again: a revert of a file under edit silently reverts the fixes
that were already verified in that file.

## Not fixed: the list element type

`CList` carries `void *items` and a length, which is right. A constructor per
element type has been generated and verified to appear in the output:

    static CList vortex_list_new_i64(int64_t len, int64_t *values)
    static CList vortex_list_new_f64(int64_t len, double *values)
    static CList vortex_list_new_str(int64_t len, const char * *values)

and the call sites name the right constructor, so a `Float` list is filled
through the `f64` one.

**What is still missing is the read side, and that is where this stops.**

An `ast::ExprKind::Index` carries a base and an index and nothing else. The
emitter needs the element type to pick the cast, and the source does not record
it:

    (((int64_t *)((a).items))[((5))]     // a is a Float list

The three routes tried and why each fails:

- **From the index expression.** `a[i]` has an `Int` index, so this always says
  `int64_t` and truncates the Float on the way out. This is what the emitter
  does now.
- **From the base expression.** Works when the base is a literal or a repeat,
  because those say what they produce. Fails when the base is a name, which is
  the ordinary case.
- **From the lowering pass.** The right fix. `crate::ir::Expr::Index` would
  carry the element type, because the lowering pass already resolved the list's
  type to allocate the slot. That is a change to the IR, which is the
  frontend contract, so it is worth its own commit and its own review rather
  than being bolted on here.

## Where the element type actually lives

The brief asked whether the element type belongs on `ir::Expr::Index`, and the
answer is that it does not, because **`cgen.rs` never reads the IR.**

`emit_function` takes an `ast::FnDecl` and `emit_program` takes
`[Spanned<ast::FnDecl>]`. The emitter walks the **AST**, the same tree the tree
interpreter walks. Putting the type on `ir::Expr::Index` would therefore have
changed a structure the emitter cannot see, which is why the first attempt at
this fix went nowhere.

The field has been added to `ir::Expr::Index` anyway, computed by the lowering
pass from the binding the base names, because it belongs there for a consumer
that does read the IR: the bytecode VM. It is additive, so the tree interpreter
and the VM are unaffected, which is what the brief required.

For the emitter the type has to come from the AST side, which means recording it
in the emitter as it walks, keyed by the name a list was declared under. That is
the route the next commit takes, and it is the fourth route rather than a repeat
of the first three: it is not inference from the index or from the base syntax,
it is a map built during the same walk that emits the declaration.

## The three float defects, fixed

All three were found by reduction rather than by reading code, and all three are
now verified against the tree interpreter and the VM on the same programs.

1. `expr_type_of` returned `int64_t` for every arithmetic operator. It now
   returns `double` when either operand is a Float, which is the rule the
   checker applies in `SPEC.md` section 6.1 rule 4 and the tree interpreter
   follows. Without it `v * 0.5 + 1.0` declared an integer and truncated 2.5
   to 2.
2. The generated entry printed its return with `%lld` and a `(long long)`
   cast whatever the entry declared, truncating a Float in the harness. It now
   prints at the declared type, with `%.6f` for a Float.
3. `CList` was declared `int64_t *items`, so every Float stored into a list
   truncated. The struct now carries `void *items` and one constructor per
   element type is emitted, `vortex_list_new_i64`, `_f64` and `_str`, with
   call sites naming the one they need. The read and store sites cast through
   the element type.

### The element type, and where it lives

It lives in two places, which is correct rather than redundant.

On `ir::Expr::Index`, computed by the lowering pass. A binding records its
element type when it is declared, taken from the initialiser: a list literal
takes the type of its first element, which is sound because the checker rejects
a mixed list, and a repeat takes the type of the value it repeats. An index
expression then carries the type with it, looked up from the binding the base
names.

On the emitter side, a map from a list name to its C element type carried
through `emit_stmt`, `emit_block` and `emit_expr`, recorded as the declaration
is emitted and read back on an index or index store into that name. The
emitter needs its own copy because **`cgen.rs` never reads the IR**:
`emit_function` takes an `ast::FnDecl`, so it walks the same AST the tree
interpreter walks.

Three inference routes were tried first and are recorded here so they are not
retried. From the index expression always gives `int64_t`, because a Float list
still has an Int index. From the base works only when the base is a literal or a
repeat and fails for a plain name, which is the ordinary case.

### The four reduction programs

| Program | Compiled | Tree | VM |
| --- | --- | --- | --- |
| `let x = 1.5; let y = 1.0; return x + y;` | 2.500000 | 2.500000 | 2.500000 |
| `let v = 2.0; return v * 0.5 + 1.0;` | 2.000000 | 2.000000 | 2.000000 |
| `var a = [1.0; 6]; a[5] = 0.25; return a[5];` | 0.250000 | 0.250000 | 0.250000 |
| `var a = [0.0; 16]; a[5] = 1.5; return a[0 * 3 + 5];` | 1.500000 | 1.500000 | 1.500000 |

The second row reads 2.0 on all three engines rather than 2.5 because
`v * 0.5 + 1.0` with `v = 2.0` is 2.0. That program tests that the result is
declared and printed as a Float, which it now is; it does not test truncation.

Two further defects surfaced while running these and are fixed in the same
change: the index read and the index store each emitted an unbalanced cast
parenthesis, and the typed constructors were called but never emitted, so
`vortex_list_repeat_f64` was an undeclared function.

## A process note that cost three turns

`git checkout <file>` during an edit reverts every fix already verified in that
file, not just the work in progress. Each time, fixes that had been confirmed by
a reduction program were silently undone and had to be reapplied.
`crates/vortexc/src/cgen.rs` carried four separately verified fixes at risk.
The lesson is recorded because the cost was three turns of this stage.

## State of the benchmark row

The compiled row is still labelled `sieve only` in `STAGE7.md`, the Vortex row
is still absent from `BENCHMARKS.md`, and the guard still requires that label.
All three remain accurate, because the matrix half still disagrees.

## What the next turn does

1. Record the element type on `ir::Expr::Index` in the lowering pass.
2. Use it for the read cast, and re-run the reduction programs from
   `FLOAT-DEFECT.md`.
3. Only then check whether all three paths print
   `1179908154 3314.003906`, and only then earn the row.
