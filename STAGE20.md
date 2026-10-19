# Stage 20: Generic trait impls and trait bounds

## Status: complete

## Goal

Implement generic function monomorphisation and trait bound checking in the
`mono` pass, extending Stage 18's concrete-type trait method resolution to
generic functions.

## What was implemented

### Syntax

Generic type parameter syntax uses square brackets after the function name:

```
fn id[T](x: T) -> T { return x; }
fn f[T: Foo](x: T) -> Int { return x.bar(); }
```

The parser's `generics()` function was changed from angle brackets `<T>` to
square brackets `[T]`, matching the notation used throughout `SPEC.md` and the
stage briefs. The `[` token is already in the lexer token set (`section 5.8`),
and no existing test used `<T>` syntax, so the change is backward compatible.

### Monomorphisation

`monomorphise()` now skips generic functions in the initial emission loop.
Instead, generic functions are instantiated lazily when a call site provides
enough type information to infer every type parameter. The inference is performed
by `infer_subst()`, which matches argument types against parameter types:

- If a parameter type is a named type parameter (e.g. `T`), the corresponding
  argument type is recorded as the concrete type for that parameter.
- If a parameter type is concrete (e.g. `Int`), the argument type must match.
- If the same type parameter maps to different concrete types across multiple
  occurrences, a type mismatch error is emitted.
- If a type parameter cannot be inferred from any argument, a
  "cannot infer type parameter" error is emitted.

When inference succeeds, `emit_fn()` is called with the inferred substitution
and a mangled name (`vortex_fn_T1_T2`), producing a distinct concrete function
per type argument set. The call expression is rewritten to use the mangled name.

Generic functions that are never called are not emitted, so no dead code with
unresolved type parameters reaches the checker.

### Trait bound checking

`check_trait_bounds()` is called after type inference succeeds. For each type
parameter that has a trait bound, the concrete type is resolved through the
substitution and checked against existing `impl` blocks via `implements_trait()`.
If the concrete type does not implement the required trait, an error is emitted:

```
the type parameter `T` resolves to type `Str`, which does not implement trait `Foo`
```

This error is produced at the mono stage, before lowering.

### Reuse of existing infrastructure

The implementation reuses the `Substitution`, `apply()`, `emit_fn()`, and
`Record`/`Cast`/`VariantCall`/`VariantRecord` transform logic from Stage 19.
Trait method resolution (`resolve_trait_method`) in `transform_call` step 4
continues to work unchanged because the specialized function body is transformed
with the inferred substitution, so `Self` and type parameters are already
resolved to concrete types before trait method calls are processed.

## Tests

Seven tests added to `crates/vortexc/tests/checker.rs`:

- `a_generic_function_is_monomorphised_at_each_call_site`: `id(42)` and `id("hi")
  produce two distinct specialized functions.
- `a_generic_function_with_two_type_parameters`: `pair[T, U]` is instantiated
  with different type pairs.
- `a_generic_function_with_a_trait_bound_is_accepted_when_satisfied`: trait bound
  on `Int` succeeds and the trait method resolves.
- `a_trait_bound_violation_is_rejected_at_monomorphisation`: `Str` does not
  implement `Foo`, rejected at the mono stage.
- `a_generic_function_with_no_inferable_type_argument_is_rejected`: `id()` with
  no arguments cannot infer `T`, rejected at the mono stage.
- `a_generic_function_with_inconsistent_type_arguments_is_rejected`:
  `pair(1, "hi")` where both params are `T` produces a type mismatch error.
- `a_generic_function_that_calls_a_trait_method_is_accepted`: a generic function
  whose body calls a trait method on its type parameter, monomorphised and
  resolved correctly.

## Evidence

- `cargo test --workspace` — 244 tests pass, 0 failures (237 prior + 7 new).
- `cargo fmt --all --check` — clean.
- `cargo build --all-targets` with `RUSTFLAGS=-D warnings` — clean.

## What remains

- `TypeExpr` now has a `Generic(String, Vec<TypeExpr>)` variant (implemented in
  `STAGE22.md`), so parameterized type arguments in type annotations (e.g.
  `x: Vec[T]`) are supported in the `mono` pass. Type arguments are still
  inferred exclusively from call site argument types, not from annotations.
