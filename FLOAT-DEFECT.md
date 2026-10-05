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
