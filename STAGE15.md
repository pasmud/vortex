# Stage 15: the type decision is exhaustive by construction

Nine instances of one shape were found in stages 8 to 14, and **two of them
arrived through fixing another instance of it**. That is the evidence that
walking the consumers by hand does not scale: the moment a new arm is added,
everything that switches on the same classification has to be found again by
reading, and twice it was not.

This stage adds a check that makes the next one fail the build instead.

## The rule

Nothing in the emitter classifies a value by matching its node kind and hoping.
Every classification goes through `ExprKind::name`, which is an **exhaustive
`match` with no wildcard**, so a variant with no name does not compile.

`ast::expr_kind_names()` builds one value of every form and asks each for its
name. It is derived from the type, so a new variant contributes its name to the
list without anyone editing the list.

`cgen::coverage::the_coverage_list_covers_every_form_the_type_has` compares the
forms the type has against the forms the coverage list exercises, and names what
is missing:

    the type has forms the type decision was not checked against: ["Probe"].
    Add each to one_of_each and answer the type question for it, or refuse it by
    name.

`cgen::coverage::the_type_decision_covers_every_expression_form` then asks every
classification the emitter makes about a value, so a form with no arm in any of
them is exercised rather than skipped.

**Three tests, and all three run in CI** as their own step, because a check that
only runs locally is a check that will be bypassed the first time it is
inconvenient.

## The evidence that it bites, and how the first attempt did not

**The first version of this check passed with an unhandled variant in the enum.**
It compared against a hand-written list of the forms that exist today, which is
the failure mode the brief named: a check that passes forever and catches nothing
new. A `Probe` variant was added to `ExprKind`, an arm was given to `name()` so
the compiler would not object, and the check still passed, because the list was
written out by hand and did not know about `Probe`.

That is the whole reason the list is now derived from the type. The proof, done
twice:

1. `Probe` added to `ExprKind` with an arm in `name()`. The build fails
   immediately:
   `error[E0004]: non-exhaustive patterns: ExprKind::Probe not covered`.
   That is the first layer, and it is the compiler rather than the check.
2. The same `Probe`, given an arm in `lower.rs` so the compiler reports the next
   place instead. Now the **check** fires, and it names the variant:
   `the type has forms the type decision was not checked against: ["Probe"]`.

Both were run, both failed as described, and the probe was then removed and its
absence confirmed by `grep`.

**What the check would not have caught.** The compiler's exhaustiveness is a
compile error naming a variant, which is the point of doing it this way rather
than with a hand-written list. The check adds a second layer at a different
place: it says the emitter's type questions have been *asked* about every form,
which the compiler cannot say, because `cgen.rs` uses wildcards and so is
exhaustive by fallback rather than by arm.

## What it guarantees, and what it does not

**It guarantees** that adding a variant to `ExprKind` fails the build twice over:
once because `name` has no arm for it, and once because the coverage list has no
entry naming it. Neither failure can be satisfied by editing a list of forms,
because both lists are derived.

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
  signature table, `is_string_expr` needs neither, and `expr_type_of` needs the
  signature table but not the bindings.
- Their arms disagree on purpose. `is_string_expr` says a `Cast` to `Str` is a
  string; `numeric_in` says the same cast is not a number; `expr_type_of` says it
  is `const char *`. One function would need three answers per form or a result
  type per question, which is the same number of places to remember a new form.

The brief asked for one check that provably bites over one check plus an
unverifiable refactor, and that is what this is. The check covers all seven
classification functions the emitter makes, including the two beyond the three
the brief named, by asking every one of them for every form.

## What remains unexamined, unchanged from stage 14

Only `+` asks whether an operand is numeric, because only `+` is ambiguous
between concatenation and arithmetic. Every other operator is arithmetic by
definition, so a missing arm there produces a gcc error rather than a wrong
answer. `-` and `*` on `Int` and `Float` fields were checked while stage 14 was
written and agree. `-`, `/`, the comparisons and `as` on a value with no recorded
type have not been checked.

Two tuple constructs still refuse by name where a C array cannot hold them, and a
`match` on a struct value is still not expressible because the parser has no
syntax for it. All seven examples compiling is a fact about which constructs the
examples reach, not a claim about the language.

## No measured claim, so nothing for the benchmark guard to check

`scripts/check-benchmarks.sh` checks that every wall clock number quoted in this
repository appears in a committed transcript, and that the recorded checksums
match. This stage makes no measured claim: it adds a check and names a limit, and
there is no figure in it to trace.

Extending the guard to a document with no measured claims would be guarding
nothing. What runs in CI instead is the check itself, as its own step named
`Check the emitter's type decision covers every expression form`, so a form added
without an arm fails the pipeline rather than being noted in a document.
`scripts/check-examples.sh` continues to run all seven examples on the compiled
path at `-O2` and at `-O0` and the tree interpreter and the VM.