# Stage 21: Generic trait impls

## Status: complete

## Goal

Implement generic trait impls (`impl<T> Trait for Type<T>`) in the mono pass,
extending Stage 20's generic function monomorphisation and Stage 18's
concrete-type trait method resolution to support generic impl blocks matched by
structural unification rather than name equality.

## What was implemented

### Syntax

Impl blocks now accept an optional generic parameter list using the same
`[T]` square-bracket syntax introduced in Stage 20:

```
impl[T] Foo for T {
    fn bar(&self) -> Int { return 42; }
}
```

`impl_decl()` calls the existing `generics()` helper after the `impl` keyword
and before the type expressions, reusing the same parser code as generic
functions. `ImplDecl` gained a `generics: Vec<GenericParam>` field.

### Structural unification of trait method calls

`resolve_trait_method` was changed from `&self` to `&mut self` so it can emit
generic impl methods lazily. The matching logic now has two phases:

1. **Non-generic impls**: matched by name equality, as before
   (`decl.self_ty == type_name`). These continue to be emitted eagerly in
   `monomorphise()`.

2. **Generic impls**: matched structurally. The helper `is_blanket_impl`
   checks whether `decl.self_ty` is one of the impl's own type parameters
   (i.e. `impl[T] Trait for T`). When it is, any concrete receiver type unifies:
   the type parameter `T` and `Self` are both bound to the receiver type, and
   `check_trait_bounds` verifies any bounds on the impl's parameters (e.g.
   `impl[T: Bar] Trait for T` requires the receiver type to implement `Bar`).
   The method is then emitted under a mangled name that uses the concrete
   receiver type (e.g. `vortex_Foo_Int_bar`), not the type parameter name.

`implements_trait` was updated to mirror this: generic impls whose `self_ty`
is a type parameter match any receiver type.

### Monomorphisation of generic impl methods

Generic impls are skipped in the initial emission loop in `monomorphise()` and
emitted on demand by `resolve_trait_method` via `emit_fn`. The `emitted`
set prevents duplicate emission if the same specialization is reached more than
once. The substitution produced by unification replaces `Self` and the type
parameter with the concrete receiver type throughout the method body and
signature.

## Tests

Three tests added to `crates/vortexc/tests/checker.rs`:

- `a_generic_trait_method_call_on_a_type_parameter_resolves_via_structural_unification`:
  a blanket impl `impl[T] Foo for T` resolves `x.bar()` inside a generic
  function `f[T](x: T)` when `T` is monomorphised to `Int`.
- `a_trait_method_call_on_a_concrete_type_resolves_through_a_generic_impl`:
  the same blanket impl is reached through a trait-bounded generic function
  `g[T: Bar]` called with a concrete argument.
- `a_trait_bound_violation_on_a_generic_impl_names_the_type_parameter_concrete_type_and_trait`:
  `impl[T: Greet] Foo for T` fails to resolve `bar` on `Int` because `Int`
  does not implement `Greet`, producing a diagnostic that names `T`, `Int`,
  and `Greet`.

## Evidence

- `cargo test --workspace` — 204 tests pass, 0 failures (201 prior + 3 new).
- `cargo fmt --all --check` — clean.
- `cargo build --workspace --all-targets` with `RUSTFLAGS=-D warnings` — clean.

## What remains

None — all items in "What remains" are addressed by Stage 22.
