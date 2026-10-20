# Stage 25: Complete move-after-use enforcement

## Status: complete

## Goal

Extend the lowering pass in `lower.rs` to track the two move forms that
Stage 6 left untracked:

- A move by assignment (`let q = p;`) where `p` is an aggregate.
- A move out of a struct field when a non-scalar field is consumed by value
  (e.g. passed to a function call).

The gap is documented in `ROADMAP.md` Stage 6, "What stage 7 should do":
"Move enforcement is also partial. A move by assignment and a move out of a
live struct field are not tracked."

## What changed in `lower.rs`

### Assignment moves (the `Let` handler)

When `let q = p;` appears and `p` is a plain binding, the lowering pass now
calls `st.mark_moved(name)` after evaluating the initializer. `mark_moved`
skips bindings whose `copyable` flag is true, so a scalar source (`Int`,
`Bool`, etc.) is duplicated rather than moved and remains usable.

The new binding `q` inherits the source binding's `copyable` flag instead of
using the `ir_expr_is_copyable` heuristic, which classified every `Load` as
copyable and would have wrongly marked a struct copy as `Copy`.

### Field moves (`moved_names_in`)

`moved_names_in` was a free function that always returned `None` for a
`Field(_, _)` expression, so consuming a field by a call never marked the
base binding. It is now a method on `Lowerer` that inspects
`self.structs` to look up the field type:

- A scalar field (`Int`, `Float`, `Bool`, `Char`, `Str`) is `Copy`; passing
  `p.x` to a function copies the field and the struct is not moved.
- A non-scalar field (a struct or list type) is moved; passing it to a
  function marks the struct binding moved, matching the assignment rule.

In v0.1 every struct field is a scalar, so the non-scalar branch has no
runtime effect yet, but the infrastructure is in place.

## What stays the same

- Field reads in expressions (`p.x + p.y`) remain borrows, not moves.
- Scalars remain `Copy` on both assignment and call passing.
- Moves inside loop bodies and match arms are still tracked (Stage 6).

## Tests

Five new tests were added to `crates/vortexc/tests/ownership.rs`:

Rejected:

- `an_assignment_move_deadens_the_source` — `let q = p; ... p.x` is rejected.
- `a_second_binding_after_an_assignment_move_is_rejected` —
  `let q = p; let r = p;` is rejected.

Allowed:

- `a_scalar_is_copied_on_assignment` — `let q = n; n` compiles; `Int` is `Copy`.
- `an_assignment_move_starts_a_new_owner` — `let q = p; q.x + q.y` compiles.
- `consuming_a_scalar_field_by_a_call_does_not_move_the_struct` —
  `take(p.x); p.y` compiles; the field read is a copy.

## Evidence

- `cargo test --workspace` — 262 tests pass (257 prior + 5 new), 0 failures.
- `cargo fmt --all --check` — clean.
- `RUSTFLAGS='-D warnings' cargo build --workspace --all-targets` — clean.

## What remains

Per-field moved tracking for partial moves (moving one field of a struct
leaves the other fields live) is not implemented. The current enforcement
marks the whole struct binding moved when any non-scalar field is consumed,
which is safe (no false negatives) but will produce false positives once v0.1
gains non-scalar struct fields. This is recorded as a follow-up.

## Correction (Stage 26)

The original count of "seven new tests" was wrong. The pull request that
merged Stage 25 added five tests, not seven, and the list above previously
named a sixth test — `a_second_binding_of_a_scalar_after_assignment_is_allowed`
— that was never committed to the test file. The count is corrected to five.
