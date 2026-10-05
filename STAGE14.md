# Stage 14: the shape of the unclassifiable-value defect

Stage 13's inventory listed one open item: a `+` on a field read of a declared
struct was unclassified and refused. That is now carried, and this document
writes down the **shape** of the defect class it belongs to, so the next stage
starts from a description rather than from another symptom.

It also carries `strings.vx`, which was the only example of six that did not
compile.

## The shape

The emitter decides a value's C type in a small number of places, and each is a
`match` on the expression node. A node with no arm there has no type.

**What makes a value unclassifiable in this emitter is that no arm in the type
decision names its node kind.** That is the whole shape. Everything else follows
from it:

- The type is a `match`, and a `match` has no default that is honest. The two
  defaults that were tried both failed in opposite directions. Defaulting to
  `int64_t` produced a silently wrong answer for a `Float`; refusing anything
  unclassified produced a refusal for a program that is plainly arithmetic.
- **Which of the two happens depends on where the missing arm sat, not on how
  wrong the value is.** The same missing case is a wrong answer in one function
  and a refusal in another. That is what makes the class hard to reason about and
  why the inventory is the thing that has kept this honest.
- The consequence is only ever visible for a value whose default is not the
  right answer. An `Int` is `int64_t` everywhere, so a missing arm for an `Int`
  looks correct, and a fix that only handles `Int` is a fix that cannot be
  distinguished from no fix at all.

That last point is the one to carry forward. **The tests for this class have to
use a value whose natural type is not the default**, or they pass against
broken code. The letter-case defect the external review found is the same
mistake in a different place: a check that only a one-letter name satisfied.

## The seven instances, in the order they were found

| # | Missing arm for | What happened | Found by |
| --- | --- | --- | --- |
| 1 | `match` | declared `int64_t`, a `Float` result truncated | stage 8, a reduction program |
| 2 | a call | declared `int64_t`, a `Float` return truncated | stage 8, the full workload |
| 3 | a pattern's bindings | unrecorded, so a `+` on one was refused | stage 13, a two-payload variant |
| 4 | a field read | unclassified, so a `+` on one was refused | stage 13's inventory |
| 5 | a function returning a declared struct | fallback `return 0;`, a gcc type error | stage 10, building the examples at `-O0` |
| 6 | `string_to_int` | no case at all, so a `+` on it was refused | stage 14, `strings.vx` |
| 7 | a call inside a collection value | the callee was never emitted | stage 14, `fieldarith.vx` |

Instances 1, 2 and 5 were **silently wrong answers**. Instances 3, 4 and 6 were
refusals. Instance 7 was a gcc error. Three outcomes from one shape, and which
one you get is decided by where the missing arm sat.

**Four of the seven were found by writing a program that exercised something no
program had reached, and three by asking a question about the shape rather than
by writing a program.** Asking "what else is read as a `CList`" found the tuple
cases in stage 13. Asking "what has no arm in the type decision" found
instances 4, 6 and 7 here. The second method finds more per turn, because it
asks about the class rather than about a symptom.

## What this stage carried

**Arithmetic on struct fields**, which was instance 4. A field read resolves
through its base's recorded type: a name holding a struct, a list element whose
element type is a struct, or a call that returned one. The `Int` and `Float`
cases are both in `examples/fieldarith.vx`, because an `Int` field is `int64_t`
and therefore looks right whether or not anything was fixed.

**`string_to_int`**, which was instance 6. The emitter had no case for the
builtin at all, so a `+` on its result was refused. It is now a generated helper
with a recorded return type.

**Calls inside collection values**, which was instance 7. The walk that decides
which functions to emit did not descend into a list literal, a repeat, a variant
or a `match` arm, so `[origin(); 2]` named a callee that was never emitted.

**A string index reads a `Char`**, by character and not by byte, which is what
the tree interpreter already did and what `examples/strings.vx` depends on. The
decoding helper the string walk generates is the same one, so there is one
definition of a character rather than two.

**`print` takes a type tag and a value** rather than a `const char *`. The tree
interpreter formats whatever it is given, and the C helper did not, so printing a
`Char`, which is what a string index produces, passed an `int32_t` as a pointer
and the program segfaulted. A `Char` above `U+FFFF` is encoded as UTF-8 and
written as bytes, because `fputc` takes an `int` and a scalar such as `U+1F600`
does not fit the byte it writes.

**All seven examples now compile** and produce identical output on the compiled
path at `-O2`, the compiled path at `-O0`, the tree interpreter and the VM.

## What this stage does not claim

**The defect class is not closed.** A shape named in a document is a better
starting point than a list of symptoms, and it is not proof the class has no
members left. What this stage did was read the type decisions and ask which node
kinds have no arm. That found three instances in one turn, which is evidence
that the question is worth asking, and it is not evidence that asking it once
was enough.

**These parts of the shape are still unexamined**, and each is a place where a
missing arm could still sit:

- **Operators other than `+`.** Only `+` asks whether an operand is numeric,
  because only `+` is ambiguous between concatenation and arithmetic. Every
  other operator is arithmetic by definition, so a missing arm there produces a
  gcc error rather than a wrong answer. `*` and `-` on an `Int` field and a
  `Float` field were checked while writing this and both agree with the tree.
  `-`, `/`, the comparisons and `as` on a field of a value with no recorded type
  have not been checked.
- **Node kinds added later.** Every new expression form the parser produces is a
  candidate for having no arm, and nothing enforces that. A compile-time check
  that every `ExprKind` variant appears in the type decision would close the
  class by construction rather than by inspection, and is the obvious next
  thing to build. It is not built here.
- **The classification walks are three separate functions** rather than one. A
  single function with one match per node kind would make the coverage visible,
  and today a new node kind has to be remembered in three places.
- **The `Print` and `print_tag_of` pair** is a new instance waiting to happen: a
  value whose type is none of the three tags the helper reads falls through to
  the text branch and prints a pointer. It was found and fixed in this stage
  rather than in a later one, by accident, and the same shape applies to any
  other place that maps a type onto a representation.

**All seven examples compiling is not parity with C or Rust.** It is a fact
about which constructs the examples reach, not a claim about the language. Two tuple constructs still refuse by name where a
C array cannot hold them, and a `match` on a struct value is still not
expressible because the parser has no syntax for it.

**No performance figure is quoted and none belongs in `BENCHMARKS.md`.** This
stage makes no measured claim.

## Why there is nothing for the benchmark guard to check

`scripts/check-benchmarks.sh` checks that every wall clock number quoted in this
repository appears in a committed transcript, and that the recorded checksums
match. `STAGE14.md` makes no measured claim: it says which constructs the emitter
carries and which it refuses, and every figure above was produced by running the
examples on all four paths.

Extending the guard to a document with no measured claims would be guarding
nothing. What is checked instead is the thing this stage changed, and
`scripts/check-examples.sh` is where it is checked: it runs every example
through the compiled path at `-O2` and at `-O0` and the tree interpreter and the
VM, and fails on any disagreement. `fieldarith.vx` and `strings.vx` are both
covered by it, so a regression that made a `Float` field truncate or a string
index fail to compile would fail CI rather than reach a user.