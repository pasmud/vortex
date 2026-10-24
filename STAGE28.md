# Stage 28: extend the emitter to handle lists of named struct and enum types

## Status: complete

## Goal

Close the STAGE11.md inventory gap where the emitter (compiled-to-C path) could
not correctly emit lists of named struct and enum types, and variants with
mixed-type payload fields. The gap was that `list_element_type` only recognized
`Int`, `Float`, and `Str`, so list element types of a named struct or enum
fell back to `int64_t`, truncating every `Float` field and producing wrong
output. Mixed-payload variants were refused by name because a flat C array
cannot hold values of different types.

## What was already in place

The emitter on `main` already handles these cases:

- `list_element_type` delegates to `value_c_type`, which resolves `Record`,
  `Variant`, `VariantCall`, and `VariantRecord` expressions through
  `c_type_named` to the C type of the declared struct or enum. It does **not**
  fall back to `int64_t` for a named struct or enum element type.
- `PAYLOAD_STRUCTS` (emitted ahead of function bodies in `emit_program`)
  generates a C struct with one field per payload type (`f0`, `f1`, ...) for
  variants whose payload fields have different types, and stores a pointer to
  it. Variants whose payload is all one type use a C array of that type. Both
  sit behind a `.tag` / `.variant` discriminant in the C struct emitted for the
  enum.
- `examples/structlist.vx` exercises a list of a named struct, a list of enum
  variants, and a mixed-payload variant, and passes on all three paths at
  `-O2` and `-O0`.

## What changed in this commit

### Added three differential tests in `crates/vortexc/tests/compiled.rs`

The existing tests for these constructs (`a_list_of_a_struct_keeps_its_float_field`
and `a_variant_with_mixed_payload_types_is_carried`) only assert on the
generated C text. Stage 28 adds three tests that run the program on the tree
interpreter, the VM, and the compiled path (at both `-O2` and `-O0`) and
compare the stdout strings:

- `a_list_of_a_struct_runs_on_all_three_paths` -- a list of a named struct
  `[P { x: Int, y: Float }]` built with literal syntax, a store into an
  element, and field reads. All three paths produce `7\n8.750000\n4.500000`.
- `a_variant_with_mixed_payload_types_runs_on_all_three_paths` -- an enum
  `Pair { mixed { n: Int, r: Float } }` with two match-based accessors. All
  three paths produce `5\n1.500000`.
- `a_list_of_named_enum_variants_runs_on_all_three_paths` -- a list of enum
  variants `[Shape.circle(2.0), Shape.empty, Shape.sized { w: 3.0, h: 4 }]`
  passed through `area()`. All three paths produce `12.566371\n0.000000\n12.000000`.

### Documentation

- `STAGE28.md` created.
- `ROADMAP.md` updated with the Stage 28 entry.

## Evidence

- `cargo test --workspace` -- all 271 tests pass (268 original + 3 new), no regressions
- `cargo fmt --all --check` -- clean
- `RUSTFLAGS='-D warnings' cargo build --workspace --all-targets` -- clean
- `scripts/check-examples.sh` -- all examples pass, identical on all three paths at -O2 and -O0
- `scripts/check-benchmarks.sh` -- check satisfied

## Notes

- No changes were needed to `cgen.rs`. The emitter already handled list element
  types and mixed-payload variants correctly (from Stages 12 and 13). Stage 28
  adds the differential runtime test coverage that was missing.
- The `structlist.vx` example served as the differential check throughout, but
  the three new tests make that coverage explicit in the unit test suite.
