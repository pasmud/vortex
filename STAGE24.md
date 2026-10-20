# Stage 24: Verify move-after-use enforcement and fix documentation

## Status: complete

## Goal

Verify that move-after-use enforcement (specified by `SPEC.md` section 7) is
fully implemented and that all ownership diagnostics are in place. Fix the
documentation gaps that left `SPEC.md` section 7.2 and `DIAGNOSTICS.md`
stating enforcement was not yet done.

## What was found

The enforcement was implemented in `STAGE06.md` in the `lower` stage
(`crates/vortexc/src/lower.rs`). The implementation is complete:

- A struct moved into a function call and then reused is rejected with a
  diagnostic that names the binding, the line and column, and cites section 7.
- Scalars (`Int`, `Bool`, etc.) are `Copy` and are not moved.
- Struct field reads borrow rather than move, so `p.x + p.y` compiles.
- Moves inside loop bodies are rejected at the second move.
- Moves inside match arms are allowed for scalar bindings.

All 12 tests in `crates/vortexc/tests/ownership.rs` pass, including the
counterexample tests that assert the model is not over-enforced.

## Documentation fixes

- `STAGE23.md`: `Status` changed from `in progress` to `complete`.
- `SPEC.md` section 7.2: updated the text that said "Enforcement is not yet
  implemented" to state that enforcement is implemented in stage 6 and how it
  works.
- `DIAGNOSTICS.md`: added the `mono` stage to the stage table; moved generics
  out of "What is not checked yet" into a new section noting they are checked;
  updated the move-semantics entry to state enforcement is implemented, with
  diagnostics pinned in `ownership.rs`.
- `ROADMAP.md`: added a Stage 23 entry.

## Tests

All 12 tests in `crates/vortexc/tests/ownership.rs` pass:

- `the_ownership_example_runs_and_prints_the_same_on_both_engines`
- `a_value_moves_into_a_call_and_comes_back_only_as_the_result`
- `a_store_into_a_list_changes_the_list_in_place`
- `a_store_into_a_let_list_is_rejected_on_both_engines`
- `a_use_after_a_move_is_rejected`
- `a_second_binding_after_a_move_is_rejected`
- `a_scalar_is_copied_rather_than_moved`
- `a_scalar_may_be_moved_twice`
- `a_move_inside_a_loop_is_rejected`
- `a_move_inside_a_match_arm_is_allowed_for_a_scalar_binding`
- `reading_two_fields_of_one_struct_is_allowed`
- `moving_an_aggregate_and_not_using_it_again_is_allowed`

## Evidence

- `cargo test --workspace` — all tests pass, 0 failures.
- `cargo fmt --all --check` — clean.
- `cargo build --workspace --all-targets` with `RUSTFLAGS=-D warnings` — clean.

## What remains

None for this stage.
