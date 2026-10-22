# Stage 28: extend the emitter to handle lists of named struct and enum types

## Status: complete

## Goal

Extend the emitter (compiled-to-C path) to correctly handle:

1. A list of a named struct — the element type must not fall back to `int64_t`
2. An enum with mixed-type payload fields — use a tag + union rather than a flat C array
3. A list of named enum variants — the element type must not fall back to `int64_t`

The acceptance gap in STAGE11.md was that `list_element_type` only recognized `Int`, `Float`, `Str` and defaulted everything else to `int64_t`, producing wrong output for struct/enum lists. This stage closes that gap and adds differential runtime tests across all three paths.

## What was already in place

- **Parser** (`parser.rs`): list literals `[...]` and repeat `[e; n]` syntax are already handled
- **Lowerer** (`lower.rs`): list AST nodes (`ast::ExprKind::List`) lower to IR nodes
- **Checker** (`checker.rs`): structural type checking of struct and enum literals
- **Tree interpreter** (`interp.rs`): list iteration and field access
- **VM** (`vm.rs`): list iteration and field access with uniform tag-based dispatch
- **Existing example** `examples/structlist.vx`: tests list-of-struct and list-of-enum with literal and repeat syntax, passing on all three paths — but only text-based C output assertions, not runtime differential tests

## What changed

### Added three differential tests in `crates/vortexc/tests/compiled.rs`

- **`a_list_of_a_struct_runs_on_all_three_paths`**: a program that creates a list of a named struct `[P { x: Int, y: Float }]`, reads its fields after an assignment, and prints them on tree, VM, and compiled paths at `-O2` and `-O0`. All three produce `7\n8.750000\n4.500000`.

- **`a_variant_with_mixed_payload_types_runs_on_all_three_paths`**: an enum `Pair { mixed { n: Int, r: Float } }` with two functions that destructure the payload via match. Both functions compile and run on all three paths, producing `5\n1.500000`. The emitter now emits a tag + union representation instead of refusing the program.

- **`a_list_of_named_enum_variants_runs_on_all_three_paths`**: a program that creates a list of enum variants `[Shape.circle(2.0), Shape.empty, Shape.sized { w: 3.0, h: 4 }]`, computes `area()` on each, and prints on tree, VM, and compiled paths. All three produce `12.566371\n0.000000\n12.000000`. The emitter now correctly handles enum element types in lists.

### Updated C emitter (`compiled/`)

- **`list_element_type`**: now dispatches on the inner type of a list literal — for `StructDef` and `EnumDef` it emits the correct C struct/union type instead of falling back to `int64_t`
- **Struct list emission**: the C struct is allocated on the stack or heap as an array of `StructDef` values; each element is stored with its full layout (no truncation of `Float` fields)
- **Mixed-payload enum emission**: the emitter now emits a C struct with a discriminant tag field (`Int`) followed by a union of the possible payload types, so each variant's fields have the correct types
- **Enum list emission**: a list of enum variants is emitted as an array of the tagged union type, with each element carrying its own discriminant

### ROADMAP.md

- Stage 28 entry added documenting the new differential tests and emitter changes

## Evidence

- `cargo test --workspace` — all 268 tests pass (265 original + 3 new three-path tests), no regressions
- `cargo fmt --all --check` — clean
- `RUSTFLAGS='-D warnings' cargo build --workspace --all-targets` — clean
- `scripts/check-examples.sh` — all examples pass, identical on all three paths at -O2 and -O0
- `scripts/check-benchmarks.sh` — check satisfied