# Stage 15: the type decision is exhaustive by construction

Nine instances of one shape were found in stages 8 to 14, and **two of them
arrived through fixing another instance of it**. That is the evidence that
walking the consumers by hand does not scale: the moment a new arm is added,
everything that switches on the same classification has to be found again by
reading, and twice it was not.

This stage adds a check that makes the next one fail the build instead.

## The rule

Nothing in the emitter classifies a value by matching its node kind and hoping.
The enforcement has two layers, one compile-time and one runtime.

**Compile time.** `ExprKind::name` and `expr_type_of_decided` are both
**exhaustive `match`es with no wildcard**. `cgen::coverage::expr_type_of_decided`
is a second copy of the type decision that exists only so the check can ask every
form for its type and get `None` (no arm) rather than a default. A variant with
no arm in `name` or `expr_type_of_decided` does not compile, at the match that
lacks it. `lower.rs` `expr` is exhaustive too, so a new variant is refused there
as well. These are four sites the compiler forces an arm in before a single test
runs, and they are the real guarantee of this stage: the next unhandled variant
is a compiler error at the point of change.

**Runtime.** `ast::expr_kind_examples()` is a single listing of one value of
every form; both `ast::expr_kind_names()` and `cgen::coverage::one_of_each()`
read it, so the two readers cannot drift relative to each other. The check
asks every form in that listing through the seven classification functions the
emitter uses, and `no_form_falls_through_the_type_default` asks the same forms
through `expr_type_of_decided` (which has no default) and fails if any returns
`None`. That is the layer the brief wanted: a form with no classifier arm is a
failed test, not a silently wrong type.

The inventory array is not derived from the type by an exhaustive match, because
stable Rust without an enum-iteration crate (this project has none) cannot
enumerate an enum's variants. It is a single declaration rather than two
hand-written lists, which is the narrower thing that is provable, and it is
documented honestly in the next section.

## The evidence that it bites, and how the first attempt did not

**The first version of this check passed with an unhandled variant in the enum.**
It compared two hand-written lists of the forms, which is the failure mode the
brief named: a check that passes forever and catches nothing new. A `Probe`
variant was added to `ExprKind`, an arm was given to `name()` so the compiler
would not object, and the check still passed, because both lists were written out
by hand and neither knew about `Probe`. The first attempt also had a second
defect: `asked.len() == 7` held for every fixture because the array always had
seven elements, so a form with no classifier arms fell through `expr_type_of` to
its default `int64_t` and the test still passed. That default is the original
defect stage 14 named.

**Review finding 2 (no silent int64_t default), reproduced.** `c_type_of_expr`'s
catch-all used to answer `int64_t` for anything it did not name, and
`expr_type_of` delegated to it. That silent answer is what hid forms that had no
arm. The catch-all now returns `<undecided:NAME>` instead, and
`expr_type_of_decided` is a separate walk with **no catch-all at all**: it
returns `None` for a form with no arm. A `Probe` variant added to `ExprKind`
fails the build here first:

    error[E0004]: non-exhaustive patterns: `&ExprKind::Probe` not covered
    crates/vortexc/src/cgen.rs:2808

giving it a `None` arm so the compiler stops, then the **test** fires and names
the variant:

    these forms have no arm in the type decision and would reach its default,
    which is what turned a Float into a silently wrong answer: ["Probe"]

Both layers were reproduced and then the probe removed; `grep` confirms its
absence.

**Review finding 1 (single-source inventory), reproduced.** A `Probe` variant
given an arm in `name()` and an arm in `expr_type_of_decided` (returning `None`,
so the build succeeds) but omitted from the `expr_kind_examples` array passes
every runtime test, because both `expr_kind_names` and `one_of_each` read that
same array and neither sees the variant. The narrowing is documented in the
guarantees section: stable Rust cannot enumerate variants to prove the array
complete, so the residual case is caught by the compiler at the exhaustive
matches or by the emitter's refusal at emit time, not by the runtime test.

## What it guarantees, and what it does not

**It guarantees** that adding a variant to `ExprKind` fails the build at the
exhaustive `match` in `ExprKind::name` and again at the exhaustive `match` in
`expr_type_of_decided`, unless an arm is given to each. Those two matches have no
wildcard, so the compiler, not a list, proves the arms exist. Once an arm is
added to both, the build succeeds; a further test then proves the arm actually
decides a type rather than reaching a default.

**Review finding 1 (single-source inventory, verified, fixed, and narrowed).** The
first version of the check compared two hand-written lists, and a variant with a
`name()` arm but omitted from both drifted through. That is fixed: `expr_kind_names`
and `one_of_each` now both read `expr_kind_examples`, the single array, so the two
views cannot drift relative to each other. The original failure was reproduced
after the fix.

The residual limitation is named honestly. `ExprKind::name` and
`expr_type_of_decided` are exhaustive matches, so a new variant must get an arm
in both or the build fails. A variant that is given both arms but is omitted from
the `expr_kind_examples` array passes the build and is not seen by the runtime
tests, because stable Rust with no enum-iteration crate in this project cannot
enumerate an enum's variants to prove a hand-listed array complete. The array is
a single listing read by every consumer, so it is the place a new form must be
entered, and the compiler forces the entry to exist at the two exhaustive
matches; but omitting it from the array after those arms exist is not a runtime
failure. The backstop is the emitter's own refusal: `emit_expr`'s catch-all
returns `Unsupported::Construct` naming the form when a program actually uses it,
so the form emits no silent wrong code even if it slips past the coverage
inventory.

**Finding 2 from review (verified and fixed).** `asked.len() == 7` held for every
fixture because the array always had seven elements, so a form added to both
inventories with no classifier arms fell through `expr_type_of` to its default
`int64_t` and the test still passed. That default is the original defect stage 14
named. This is fixed in two layers:

1. `c_type_of_expr`'s catch-all now returns `<undecided:NAME>` instead of
   `int64_t`, so a fall-through is named rather than answered silently.
2. `no_form_falls_through_the_type_default` asks `expr_type_of_decided` directly,
   which has no catch-all and returns `None` for a form with no arm. The original
   failure was reproduced after the fix: a `Probe` variant with a `name()` arm and
   an `expr_type_of_decided` arm that returned `None` made the test fail, naming
   `["Probe"]`, and was then removed.

**It does not guarantee the language is correct.** A check that fails the build
on a missing variant makes the next missing variant **loud and immediate** rather
than silent or confusing. That is all it is, and the nine instances make the
difference concrete: two of them produced a gcc error naming a C type the user
never wrote, and one produced a segfault. Loud is not correct.

**It does not catch a wrong arm.** All nine instances were a missing arm, which
this catches. A right-looking arm that returns the wrong type is invisible to it
and was invisible to every other check until a program exercised it. That is the
limit the next stage should be scoped against.

**It does not cover the type decision in the other engines.** The tree
interpreter, the VM and the checker each have their own traversal, and this is a
check on the emitter's. A form could be classified correctly here and missing
there.

**The check produced no new instance.** Adding it, and running the coverage
questions over every form, found no form the emitter mishandles today. That is
evidence about how much of the remaining surface the class was covering, and it is
not evidence that the class is exhausted: the class was found by writing
programs, and no program was written for this stage beyond the ones the check
runs. `STAGE14.md` keeps the nine, and this stage adds none.

## The three walks were not folded, and why

The three classification walks in `cgen.rs` are `expr_type_of`, `numeric_in` and
`is_string_expr`, and they are still three functions. Folding them is **not a
small change**, for reasons the reading showed:

- They answer different questions with different return types: a C type string, a
  boolean, and a boolean about strings.
- They take different context: `numeric_in` needs the recorded bindings and the
  signature table, `is_string_expr` needs neither, and `expr_type_of` needs both
  (the bindings to resolve field and index reads through the name a value was
  bound to, and the signature table for call return types).
- Their arms disagree on purpose. `is_string_expr` says a `Cast` to `Str` is a
  string; `numeric_in` says the same cast is not a number; `expr_type_of` says it
  is `const char *`. One function would need three answers per form or a result
  type per question, which is the same number of places to remember a new form.

The brief asked for one check that provably bites over one check plus an
unverifiable refactor, and that is what this is. The check covers all seven
classification functions the emitter makes, including the two beyond the three
the brief named, by asking every one of them for every form.

## Acceptance

- Adding a variant to `ExprKind` that nothing handles makes `cargo build` fail with
  `error[E0004]: non-exhaustive patterns`, then after an arm is given to keep the
  build green, `cargo test` fails with a diagnostic naming the variant. Proven by
  adding `Probe`, observing both failures, and removing it. `grep` confirms its
  absence.
- All seven examples agree on tree, VM and compiled output at `-O2` and `-O0`
  (`scripts/check-examples.sh`).
- 3 coverage tests plus the existing suite all pass (229 tests total across all
  crates); `scripts/check-benchmarks.sh` passes (this stage makes no
  performance claim, so it is not extended there).
- The check runs in CI as the step "Check the emitter's type decision covers
  every expression form" — a check that only runs locally is bypassed in practice.
- `cargo fmt --check` clean, and `cargo build` clean under `-D warnings`.
- No `git checkout`, `git restore`, or any discarding command was used.
  `expr_type_of` now takes `lists` so field and index reads resolve through the
  binding table (fixing the `structs.vx` regression the first cut of this stage
  introduced) rather than guessing a type.