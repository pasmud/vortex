# Stage 23: Complete generics

## Status: complete

## Goal

Complete the generics system by implementing three acceptance items:

1. `infer_subst` recursively unifies type arguments within parameterized
   types so that type parameters nested inside `Wrapper[T]` are inferred
   from the corresponding concrete argument.
2. Struct field types are checked structurally in the mono pass, comparing
   type arguments so that `Wrapper[Int]` is accepted where `Wrapper[T]` is
   expected but `Wrapper[Str]` is rejected.
3. `let` statements accept optional type annotations (`let x: Int = 5;`)
   that are checked against the initializer.

## Acceptance criteria

### 1. Type inference from parameterized types

`infer_subst` must walk the type argument list of a `Generic` type
parameter and bind the matching type parameter. For example, given
`fn f[T](x: Wrapper[T])` called with an argument of type `Wrapper[Int]`,
`T` is inferred as `Int`.

### 2. Structural field type checking

In the mono pass `Record` handler, when both the declared field type and
the provided value type are parameterised (`Generic`), compare type
arguments recursively using `types_compatible`.

### 3. Let type annotations

The parser accepts `let name: Type = init;`. The mono pass applies the
substitution to the annotation, stores the annotated type as the
variable's type in the environment, and checks compatibility against the
initializer. The checker resolves the annotation and validates it
against the initializer type.

## Tests

Seven tests added to `crates/vortexc/tests/checker.rs`:

- `a_type_parameter_nested_in_a_parameterized_type_is_inferred`
- `a_type_parameter_nested_in_a_parameterized_type_cannot_be_inferred_from_a_bare_name`
- `a_struct_literal_with_a_bare_value_is_accepted_where_a_parameterised_field_is_expected`
- `a_struct_literal_with_a_mismatched_type_argument_is_rejected`
- `a_let_with_a_type_annotation_matching_the_initializer_compiles`
- `a_let_with_a_type_annotation_mismatched_with_the_initializer_is_rejected`
- `a_let_with_a_parameterised_type_annotation_matching_the_initializer_compiles`

## Evidence

- `cargo test --workspace` — 257 tests pass, 0 failures.
- `cargo fmt --all --check` — clean.
- `cargo build --workspace --all-targets` with `RUSTFLAGS=-D warnings` — clean.

## What remains

None.
