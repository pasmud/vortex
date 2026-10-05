# Stage 12: closing the three constructs that guessed

Stage 11's inventory found three constructs that fell back to a default instead
of declining, so a Vortex program could be accepted, compiled, and produce a
**wrong answer with no diagnostic at all**. Every other construct the emitter
cannot handle refuses by name, which is safe: a user finds out immediately.

A construct that guesses is worse than a construct that is missing. All three
are now either carried correctly or refused by name.

## A list of a named struct: carried, was a wrong answer

`list_element_type` named only `Int`, `Float` and `Str`. A list of a declared
struct or enum fell through to `int64_t`, so every element was read through an
`int64_t` pointer and every `Float` field truncated. This was stage 11's named
first candidate, and the truncation is silent: an `Int` field is right at
`int64_t`, so a list of all-`Int` structs would have produced the right answer
and hidden the defect.

Three changes close it:

- `value_c_type` names a declared struct or enum, so a record or a variant is a
  valid element type.
- One list constructor is emitted per declared type, so `vortex_list_new_CPoint`
  exists and allocates at the struct's size rather than the size of an
  `int64_t`.
- An index read or store into a name the emitter cannot type is **refused by
  name** rather than defaulted. This is the part that matters: without it, a
  list whose element type is still unknown would keep reading at `int64_t`.

A list of a named enum is carried by the same change.

## A variant with mixed payload types: carried, was a refusal

`Pair.mixed { n: Int, r: Float }` has payload fields of different types, which a
C array cannot hold, so stage 11 refused it. It is now carried: the payloads go
in a generated struct with one field each, declared before the functions that
use it. The read side reaches them by the same field.

The uniform case, every payload the same type, is still a C array, which is
smaller and is what a single-type variant generates.

## String `+` on a value the emitter cannot see: refused, was C pointer arithmetic

`is_string_expr` decides whether a `+` is concatenation. It returned false for
anything it did not recognise, and false means "not a string", so the emitter
wrote C `+` on two `const char *`. That is address arithmetic: a Vortex program
asking to join two strings got a truncated pointer printed as a number.

`+` is now refused unless the emitter can show it is arithmetic, which means both
operands are a number literal, a recorded numeric name, a call whose declared
return type is numeric, or an arithmetic expression over those. Everything else
refuses with:

    `+` where the emitter cannot tell whether an operand is a string, so it
    cannot choose between concatenation and arithmetic

**This one needed a supporting change, and that change is the real finding.** A
`+` can only be classified from what the emitter recorded, and it recorded only
list and string bindings. Every plain number was therefore unknown, so
distinguishing arithmetic from concatenation was impossible. Three bindings are
now recorded at their initialiser's type: every `let`, a counting `for` variable,
and a `for ..in` loop variable. Without that, refusing the ambiguous case would
have rejected ordinary integer arithmetic, which it did for one turn before the
recording was added.

## The example

`examples/structlist.vx` carries all of this. It is in `examples/`, so
`scripts/check-examples.sh` runs it on every CI run, which is where it belongs:
that script is what stands between the compiled path and a silent wrong answer.

It prints, on the compiled path at `-O2`, the compiled path at `-O0`, the tree
interpreter and the VM:

    7
    8.750000
    2.500000
    12.566371
    0.000000
    12.000000

Two stores into a list element and four reads are exercised, so both directions
of the element type are covered rather than only the read. `Point` has a `Float`
field precisely because an `Int` field would not have shown the defect.

The third construct, string `+` on an unclassifiable value, has **no example that
reaches it**: every program that would reach it is refused earlier, by string
indexing or by an untyped index. So it is pinned by a unit test rather than by
the example set. That is a real gap in the example coverage and is named here
rather than left to be discovered.

## The updated inventory

| Construct | Tree | Emitter | Status |
| --- | --- | --- | --- |
| `fn`, `let`, `var`, `return`, `while`, `for ..=`, `for ..in`, `break`, `continue` | yes | yes | closed |
| `if` as a statement, `if` as a value | yes | yes | closed |
| `match` as a value, tuple and variant patterns | yes | yes | closed |
| struct declaration, field read, record expression | yes | yes | closed |
| enum declaration, variant expression, variant field syntax | yes | yes | closed |
| tuple expression, tuple pattern | yes | yes | closed |
| list literal, list repeat, index read, index store | yes | yes | closed |
| list of `Int`, `Float` or `Str` | yes | yes | closed |
| **list of a named struct** | yes | **yes, at the struct's type** | **closed, was a wrong answer** |
| **list of a named enum** | yes | **yes, at the enum's type** | **closed, was a wrong answer** |
| **variant with mixed payload types** | yes | **yes, in a generated struct** | **closed, was a refusal** |
| **string `+` on an unclassifiable value** | yes | **refused by name** | **closed, was C pointer arithmetic** |
| indexing a string | yes | refused by name | closed, stage 11 |
| an index into a name whose element type is unknown | yes | refused by name | closed, was `int64_t` |
| a `match` with no arms, or on a type the emitter cannot compare | yes | refused by name | closed |
| `for ..in` over a string | yes | **fails with a gcc error, not a named refusal** | **open, found by this stage's list** |
| an enum variant with two positional payloads | yes | refused, because a `+` on two bindings is unclassified | open, found by this stage's list |
| a `let` holding a value the emitter cannot name | yes | refused by name | closed |
| inference, generics, closures, async, modules | no | no | out of scope |

**No row in the wrong-answer column remains open.** Every construct is either
carried correctly or refused with a name and a position.

## Which constructs still have no example

The examples reach the common surface. These have no example and are covered only
by unit tests, so a defect in them would not show up in
`scripts/check-examples.sh`:

Each of these was tried on the compiled path while writing this document, and
each is either refused or untested. The three marked **works** were checked and
are correct, so they are listed here rather than in the untested column.

| Construct | Compiled path |
| --- | --- |
| `for ..in` over a string | refused, and the refusal is a gcc error rather than a named one |
| an enum variant with two positional payloads | refused: the `+` that adds them is refused because a two payload binding is unclassified |
| a `match` on a struct value rather than an enum | not reached by any example |
| a list of a `Str` built by a repeat rather than a literal | not reached by any example |
| a nested struct, a struct holding a struct | not reached by any example |
| alternative patterns in one arm, `E.a \| E.b => 1` | works, checked while writing this |
| a function called with two arguments | works, checked while writing this |
| a `+` on a call whose return type is a declared enum or struct | not reached by any example |

The first two are real gaps rather than merely untested: both are programs the
tree interpreter runs and the compiled path cannot. The first is the worse of the
two, because it fails with a gcc error naming a C type, which says nothing about
Vortex, rather than with a Vortex diagnostic. **That is a construct that guesses
its way to a confusing failure**, which is what this stage set out to remove, and
it was found by writing the list rather than by a test. It should be the next
stage's first item.

The rest are reachable and untested, so a defect in them would not show up in
`scripts/check-examples.sh`. The list is short enough to close.

## What this stage does not claim

**This does not make the emitter correct in general.** Four defects in stage 11
and four in stage 10 were all found by making examples compile and comparing
their output, so the untested surface is still whatever no example reaches. The
list above is that surface, written down rather than discovered one bug at a
time.

**This is not parity with C or Rust.** Four of five examples now compile, and
`strings.vx` still does not. That is progress on what the emitter can express,
not a claim about the language as a whole.

**No performance figure is quoted and none belongs in `BENCHMARKS.md`.** This
stage makes no measured claim.

## Why there is nothing for the benchmark guard to check

`scripts/check-benchmarks.sh` checks that every wall clock number quoted in this
repository appears in a committed transcript, and that the recorded checksums
match. `STAGE12.md` makes no measured claim: it says what the emitter carries
and what it refuses, and every figure in the table above was produced by running
the example on all four paths.

Extending the guard to a document with no measured claims would be guarding
nothing. What is checked instead is the thing this stage changed:
`scripts/check-examples.sh` runs every example through the compiled path at
`-O2` and at `-O0` and the tree interpreter and the VM, and fails on any
disagreement. `structlist.vx` was added to it, so a regression that brought back
a truncated `Float` field in a list of a struct would fail CI rather than sit
undetected until a user wrote that program.