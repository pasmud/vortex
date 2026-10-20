# Stage 27: the `as` cast operator

## Status: complete

## Goal

Implement `expr as Type` for numeric conversion between `Int`, `Float` and
`Char`, following SPEC.md section 6.1 rule 4, which requires explicit casts with
no implicit numeric widening.

During this stage I found the `as` cast was already implemented across the
parser, lowerer, checker, tree interpreter, bytecode compiler and VM. The
acceptance gap was in the documentation and the checker diagnostic: the SPEC
still claimed there was no `as` cast, the checker error message did not cite
SPEC 6.1 rule 4, and the benchmark harness documentation was stale. Stage 27
closes that gap.

## What was already in place

- **Parser** (`parser.rs`): `as` is a keyword and `ExprKind::Cast(Box<Expr>,
  TypeExpr)` is produced by the postfix-expression loop.
- **Lowerer** (`lower.rs`): `ast::ExprKind::Cast` lowers to
  `ir::Expr::Cast { value, to, pos }`.
- **Checker** (`checker.rs`): allows `Int`↔`Float` and `Int`↔`Char`; rejects
  all other pairs with a `TypeError`.
- **Tree interpreter** (`interp.rs`): `Int`→`Float` preserves the value via
  `n as f64`; `Float`→`Int` truncates toward zero via `f as i64`.
- **Bytecode** (`bytecode.rs`): `Op::Cast(TypeExpr)` is emitted and
  disassembled.
- **VM** (`vm.rs`): `Op::Cast` calls the `cast` function which reuses the same
  conversion logic.
- **Benchmark workload** (`bench/vortex/sieve.vx`): already used `as Float`
  at the matrix multiply row, and `BENCHMARKS.md` already recorded all three
  paths producing the checksum `1179908154 3314.003906`.

## What changed

### Checker diagnostic cites SPEC 6.1 rule 4

The rejected-cast diagnostic in `checker.rs` was updated from:

```text
cannot cast `Str` to `Int`; the only casts are between `Int`, `Float` and `Char`
```

to:

```text
cannot cast `Str` to `Int`; `SPEC.md` section 6.1 rule 4 allows only casts
between `Int`, `Float` and `Char`
```

This follows the same pattern already used by the arithmetic-operator
diagnostic at the same section: `SPEC.md section 6.1 rule 4 forbids mixing
Int and Float without an as cast`.

### Tests assert the citation

The two rejected-cast tests in `crates/vortexc/tests/checker.rs` now also
assert that the diagnostic contains `SPEC.md section 6.1 rule 4`:

- `casting_a_string_to_an_int_is_rejected` — `"text" as Int` is rejected at
  2:20 and names the rule.
- `casting_a_char_directly_to_a_float_is_rejected` — `'A' as Float` is
  rejected at 1:57 and names the rule.

### SPEC.md section 8.2 updated

Replaced the stale bullet `There is no as cast` with `as casts are
implemented`, documenting the allowed subset (`Int`↔`Float`, `Int`↔`Char`) and
that the checker rejects all other pairs with a diagnostic that cites the rule.

### DIAGNOSTICS.md updated

Added a row under "Section 6.1 rule 4, no implicit numeric widening":

| a cast between unrelated types (e.g. `Str` to `Int`, `Char` to `Float`) | points at `as`, and the message cites rule 4 |

### Benchmark documentation updated

- `bench/vortex/README.md`: replaced the "floating point half is absent"
  section with "the matrix multiply now runs", and updated "what stage 4
  should do with this" to "what stage 4 did next".
- `bench/run.sh`: replaced the stale comment that the Vortex row stays empty
  with a note that the workload now covers the full matrix multiply.

## What stays the same

- The checker only allows `Int`↔`Float` and `Int`↔`Char`. `Char`→`Float` is
  not allowed because the spec says the only way between `Int` and `Float` is
  an `as` cast — a `Char` would need two casts.
- The tree interpreter and the VM reuse the same value-conversion logic.

## Evidence

- `cargo test --workspace` — all tests pass (268: 262 original + 1 new
  three-path cast test + 5 from Stage 26), no regressions.
- `cargo fmt --all --check` — clean.
- `RUSTFLAGS='-D warnings' cargo build --workspace --all-targets` — clean.
- `a_cast_produces_the_same_output_on_all_three_paths` in
  `crates/vortexc/tests/compiled.rs` verifies that Int->Float
  (`7 as Float` -> `7.000000`) and Float->Int (`3.7 as Int` -> `3`) produce
  identical output on the tree interpreter, the bytecode VM, and the compiled-to-C
  path at both `-O2` and `-O0`.
- `BENCHMARKS.md` records all three paths (compiled to C, tree interpreter,
  bytecode VM) producing the checksum `1179908154 3314.003906` on the matrix
  multiply benchmark, verified by `scripts/check-benchmarks.sh`.
