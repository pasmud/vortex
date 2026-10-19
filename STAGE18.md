# Stage 18: Trait method resolution

## Goal

Resolve trait method calls (`x.method(args)`) so that a program with trait
declarations and impl blocks can be type-checked and run. Concrete types only;
generic trait impls and trait bounds are left for a later stage.

## Step 1 — Monomorphisation pass

A new `mono.rs` module runs between parsing and lowering. It transforms the
parsed `Program` in two passes:

1. **Emit trait impl methods** as standalone functions with mangled names
   (`vortex_TraitName_TypeName_method`). Each method's `Substitution::self_for`
   replaces `Self` in parameter types and the body.

2. **Transform function bodies** to resolve trait method calls. When
   `Call { callee: "method", args: [receiver, ...] }` is not a known function
   or builtin, the receiver's type is looked up (from literals, identifiers, or
   expression inference). The impl blocks are searched for a matching method on
   that type. If found, the callee is rewritten to the mangled name. If not
   found, a diagnostic is emitted: `no method \`method\` found for type \`Type\``.

The pass also handles non-trait expressions by transforming sub-expressions
(recursively) while tracking variable types in a scoped environment. Builtin
functions are recognized so their calls are not misclassified as trait methods.

## Step 2 — Parser support for `self`

The parser's `primary()` function now accepts `self` as an expression
(`ExprKind::Ident("self")`). This is needed because trait method bodies
reference the receiver as `self`, but the parser previously rejected `self` as
a keyword in expression position.

## Step 3 — Tests

Two tests added to `crates/vortexc/tests/checker.rs`:

- `resolves_trait_method_on_concrete_type`: calls `x.bar()` on an `Int` value
  where `impl Foo for Int` provides `bar`. The call resolves and the program
  runs, printing `1`.

- `rejects_unresolved_trait_method`: calls `x.baz()` where `baz` is not in the
  impl block. The mono pass emits `no method \`baz\` found for type \`Int\``
  at the lower stage.

## Evidence

- `cargo test` — 236 tests pass, 0 failures.
- `cargo fmt --check` — clean.
- `cargo build --all-targets` with `RUSTFLAGS=-D warnings` — clean.
