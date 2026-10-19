# Stage 22: Parameterized type arguments

## Status: complete

## Goal

Implement parameterized type arguments (e.g. `Vec[T]`) in type annotations and
generic impl self types (e.g. `impl[T] Trait for Concrete[T]`) in the mono pass,
extending Stage 21's generic trait impls with structural unification for
parameterized self types.

## What was implemented

### AST

`TypeExpr` gained a `Generic(String, Vec<TypeExpr>)` variant to represent
parameterized types such as `Wrapper[Int]`. The `name()` method returns the
base type name for both `Named` and `Generic`, so existing code that compares
by name continues to work.

`ImplDecl.self_ty` changed from `String` to `TypeExpr`, allowing impl self types
to carry type arguments (e.g. `impl[T] Greet for Wrapper[T]` where `self_ty` is
`Generic("Wrapper", [Named("T")])`).

### Parser

`type_expr()` now parses `Name[T1, T2, ...]` as `TypeExpr::Generic(name, args)`,
reusing the existing `[...]` square-bracket token consumption. This works in all
type-expression contexts: function parameter types, return types, struct field
types, and impl self types.

`impl_decl()` parses the self type after `for` with `self.type_expr()` instead
of `expect_ident`, so `impl[T] Greet for Wrapper[T]` captures the full
parameterized self type.

### Monomorphisation

`Substitution::apply` now recurses into `TypeExpr::Generic` arguments, so
substitutions like `{T -> Int}` correctly transform `Wrapper[T]` into
`Wrapper[Int]`.

`resolve_trait_method` was rewritten to use structural unification for all impl
matching. The helper `unify_impl_type` unifies the impl's `self_ty` against the
concrete receiver type:

1. **Non-generic impls**: `Named("Int")` unifies with `Named("Int")` (name
   equality) and also with `Generic("Wrapper", [Named("Int")])` when the base
   name matches.
2. **Generic blanket impls** (`impl[T] Trait for T`): the type parameter `T`
   binds to the receiver type.
3. **Parameterized generic impls** (`impl[T] Trait for Wrapper[T]`): the base
   names must match and type arguments are unified recursively, binding `T` to
   the corresponding concrete argument.

After unification, `Self` is bound to the concrete self type (computed by
applying the unification substitution to `decl.self_ty`), then `check_trait_bounds`
verifies any bounds on the impl's generic parameters.

### Checker

The checker's `lookup_type` function already applies `subst.apply()` to the
receiver type. With `Substitution::apply` now handling `Generic`, parameterized
receiver types flow through correctly. No other checker changes are needed — the
checker uses `name()` which extracts the base type name.

## Tests

Three tests added to `crates/vortexc/tests/checker.rs`:

- `a_parameterized_type_annotation_type_checks_and_resolves`: a non-generic impl
  `impl Greet for Wrapper` is matched structurally against a receiver of type
  `Wrapper[Int]` (from a parameterized type annotation `fn f(x: Wrapper[Int])`).
- `a_generic_impl_with_a_parameterized_self_type_resolves_for_a_concrete_instantiation`:
  `impl[T] Greet for Wrapper[T]` is matched structurally when the receiver type
  is `Wrapper[Int]`, binding `T -> Int`.
- `a_trait_bound_violation_on_a_parameterized_self_type_names_t_concrete_and_trait`:
  `impl[T: Foo] Greet for Wrapper[T]` fails because `Int` does not implement
  `Foo`, producing a diagnostic that names `T`, `Int`, and `Foo`.

## Evidence

- `cargo test --workspace` — 247 tests pass, 0 failures.
- `cargo fmt --all --check` — clean.
- `cargo build --workspace --all-targets` with `RUSTFLAGS=-D warnings` — clean.

## What remains

- `infer_subst` only infers type parameters from top-level type parameter names,
  not from type arguments nested inside parameterized types (e.g. inferring `T`
  from `Wrapper[T]` in a call `f(some_wrapper)`).
- Struct field types are not checked against declared field types in the mono
  pass (only type names are compared, not type arguments).
- A parameterized type annotation like `let x: Wrapper[Int] = ...` is not
  supported because `let` statements do not have type annotations.
