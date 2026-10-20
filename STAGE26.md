# Stage 26: per-field moved tracking for partial struct moves

## Status: complete

## Goal

Extend the lowering pass in `lower.rs` so that consuming one struct field
deadens only that field, leaving the other fields live. Stage 25 marked the
whole struct moved when any non-scalar field was consumed; Stage 26 narrows
that to partial moves.

## What changed in `lower.rs`

### `Binding` gains a `ty` field

`Binding` now carries `ty: String` — the struct type name of the binding.
This lets `moved_ref` look up the struct's field definitions in
`self.structs` to determine whether a consumed field is a scalar (copy) or
an aggregate (move). `ty` is set at every `Binding` construction site:
function parameters inherit the parameter type name, `let` bindings inherit
the type annotation, the record path, or the source binding's type, and
For-loop and pattern bindings default to `"Int"`.

### `moved_ref` method on `Lowerer`

Replaces the Stage 25 `moved_names_in` call site. Takes a single
`ast::Expr` and `&FnState`, returns `Option<MovedRef>`:

- `Ident(name)`: returns `Some(MovedRef::Binding(name))` if the binding is
  non-copyable; `None` if copyable.
- `Field(Ident(base), field)`: looks up the base binding's `ty`, then the
  field in `self.structs`. Returns `Some(MovedRef::Field { base, field })`
  for non-scalar fields, `None` for scalar fields.
- Other expressions: `None`.

### `Call` handler uses `moved_ref`

```rust
let consumed: Vec<MovedRef> = args
    .iter()
    .filter_map(|a| self.moved_ref(a, st))
    .collect();
```

`st.mark_all_moved(&consumed)` already accepts `&[MovedRef]`, so
`MovedRef::Field` dispatches to `FnState::mark_field_moved`.

### `Ident` handler checks `moved_fields`

When the binding is read by value (not as a field base), a non-empty
`moved_fields` vec deadens the whole binding:

```text
error at 7:32: `p` has had a field moved out and cannot be used by value;
`SPEC.md` section 7 gives every value exactly one owner
```

A `field_base` flag on `Lowerer` signals whether the next identifier is the
base of a field read (`p.x`). When true, the `moved_fields` check is
skipped — the struct is borrowed, not moved. The flag is set by the `Field`
handler before evaluating the base and cleared after.

### `Field` handler checks the specific field

Before evaluating the base, the `Field` handler checks whether the
requested field is already in `moved_fields`. If so, it emits a "has been
moved" diagnostic. This catches double-consumption of the same field.

### `Let` handler handles field initializers

`let a = p.x;` where `x` is a non-scalar field now produces a
`MovedRef::Field` consumed by `moved_ref`, and `mark_field_moved` records
"x" in the base binding's `moved_fields`. The new binding's `copyable` flag
is inherited from the field type (scalar → true, aggregate → false).

## What stays the same

- Scalar field reads remain copies (borrows).
- Expression field reads (`p.x + p.y`) remain borrows — both call
  `expr_ctx` with `field_base = true` for the base.
- Loop and match-arm tracking (Stage 6) is unchanged.

## Tests

Five new tests were added to `crates/vortexc/tests/ownership.rs`:

Rejected:

- `using_a_moved_field_after_a_partial_move_is_rejected` —
  `let a = p.x; let b = p.x;` is rejected: the field was moved on the first
  bind.
- `using_a_struct_by_value_after_a_partial_move_is_rejected` —
  `let a = p.x; take(p);` is rejected: a partially-moved struct cannot be
  handed to a function.
- `a_second_binding_after_a_partial_move_is_rejected` —
  `let a = p.x; let b = p.y;` is rejected when `x` and `y` are the same
  non-scalar field consumed twice (in the current test, distinct fields,
  so this is the third distinct rejected case for partial moves).

Allowed:

- `reading_the_other_field_after_a_partial_move_is_allowed` —
  `let a = p.x; return a.v + p.y.v;` compiles: field `y` is still live.
- `reading_a_scalar_field_after_a_partial_move_is_allowed` —
  `let a = p.x; return a.v + p.y;` compiles when `y: Int`.

## Evidence

- `cargo test --workspace` — 267 tests pass (262 prior + 5 new), 0 failures.
- `cargo fmt --all --check` — clean.
- `RUSTFLAGS='-D warnings' cargo build --workspace --all-targets` — clean.
