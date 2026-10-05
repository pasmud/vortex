# Stage 13: walking a string, and the two-payload variant

Stage 12 wrote the list of constructs no example reached and found two programs
the tree interpreter runs and the compiled path cannot. It called the first the
worst thing in the repository at that moment. Both are now carried.

## `for x in s` over a string: carried, was a gcc error

The emitter assumed every `for ..in` walked a list. It bound the collection to a
`CList` and then read it as one, so a string produced

```c
CList __vortex_for0_c = "abc";
```

and gcc reported

    error: invalid initializer

That is the failure mode stage 12 existed to remove: a diagnostic pointing at the
C compiler and naming a C type the user never wrote, rather than at the Vortex
construct. **It is now a character walk.**

A Vortex `Char` is a Unicode scalar, not a byte, so the loop decodes one scalar
per step and advances past it. The generated C carries two helpers,
`vortex_char_at` and `vortex_char_len`, and the loop is a `while` rather than a
`for` because a C `for` increments as well and a character's length is not one.

The distinction is not cosmetic. `"aé😀b"` is nine bytes and four characters. A
byte loop iterates nine times and produces a different answer; this iterates
four times, and the example prints the count so a regression is visible.

## An enum variant with two positional payloads: carried, was a refusal

Stage 12 recorded every `let`, counting `for` variable and `for ..in` variable at
its initialiser type, and stage 12's inventory predicted this might then simply
work. It did not, for a reason the recording did not reach: **a pattern's
bindings are ordinary names in its arm body, and they were not recorded.**

A `+` on a name the emitter cannot classify is refused rather than guessed, so
`Size.pair(a, b) => a + (b as Int)` was refused. Recording a pattern's bindings,
each at the payload's declared type, is what closes it.

## The class, and why it was wider than one construct

The brief asked whether fixing the first of these would show the class to be
wider than one construct. It was, and the class has a name:

**the emitter walking a collection whose C representation it does not have.**

`for ..in` assumed a list. Indexing assumed a list. Both are places where a
Vortex value is read as though it were a `CList`, and a `CList` is
`{ void *items; int64_t len; }`. A Vortex value that is not a list does not have
an `items` field, so the emitted C names a member that does not exist.

Three collection-shaped values are not lists, and all three were holes:

| Value | Before | After |
| --- | --- | --- |
| a string | bound to a `CList`, gcc: invalid initializer | carried as a character walk |
| a tuple, in `for ..in` | bound to a `CList`, gcc: invalid initializer | refused: `` `for ..in` over a tuple `` |
| a tuple, indexed | read at `.items`, gcc: no such member | refused: `indexing a tuple` |

The last two were not in stage 12's list. They were found by looking for the
pattern rather than for the symptom: the question "what else is read as a
`CList`" has one answer, and after this change there are no unguarded ones left.
The one remaining `CList` binding in the emitter is inside the list branch of
`for ..in`, which is guarded by a string check, a tuple check and an element-type
refusal.

**The tuple cases refuse rather than carry, and that is a deliberate limit rather
than an oversight.** A tuple has a real generated C representation,
`CTuple2` through `CTuple4`, and a `for ..in` over one could be emitted as an
index over its fields. It is not done here because stage 13 is about removing
guesses, and carrying a tuple walk would be a feature rather than a fix. It is
recorded below as the obvious next item.

## The example

`examples/striter.vx` carries both fixes and is in `examples/`, so
`scripts/check-examples.sh` runs it on every CI run. It prints, identically on
the compiled path at `-O2`, the compiled path at `-O0`, the tree interpreter and
the VM:

    97 233 128512 98
    4
    3
    6

The `4` is the character count of `"aé😀b"` and the codepoints above it are what
the three engines agree on. `3` and `6` come from a two-payload and a
three-payload positional variant.

## The updated inventory

| Construct | Tree | Emitter | Status |
| --- | --- | --- | --- |
| everything stage 12 listed as closed | yes | yes | unchanged, still closed |
| `for ..in` over a string | yes | yes, by character | **closed, was a gcc error** |
| an enum variant with two or more positional payloads | yes | yes | **closed, was a refusal** |
| `for ..in` over a tuple | yes | refused by name | **closed, was a gcc error** |
| indexing a tuple | yes | refused by name | **closed, was a gcc error** |
| a list of a `Str` built by a repeat, `[ "x"; 2 ]` | yes | refused by name: `indexing a string` | closed, was untested |
| a nested struct, a struct holding a struct | yes | yes, checked while writing this | closed |
| a function returning a declared struct or enum | yes | yes, checked while writing this | closed, was a gcc error |
| a `match` on a struct value | no | not expressible | closed, the parser has no syntax for it |

**No construct now fails with a C compiler error**, with one exception named
below. Every value the emitter
cannot represent correctly is either carried or refused with a Vortex
diagnostic that names the construct and its position. That is the class stage 13
set out to close, and it is closed rather than reduced.

### Nothing in the list is open, and four rows moved while it was being written

All four moved by checking rather than by assuming, and two of them found
something.

**A function returning a declared struct** was a gcc error, which is the class
this stage exists to close and which the list had not caught. The emitter appends
a fallback `return 0;` to every function, so a function returning a struct got
`return 0;` after its real return, and gcc reported ``incompatible types when
returning type 'int' but 'CP' was expected``. The fallback is now a zeroed value
of the declared type. This is a third instance of the same shape: a value the
emitter treated as a number because it had no other case for it.

**A nested struct** works, checked here rather than assumed.

Two more moved by checking:

**A list of `Str` built by a repeat, `[ "x"; 2 ]`,** is refused by name with
`indexing a string`, because indexing a string is refused whatever built the
string. The tree interpreter runs it, so the construct is closed on the emitter
side with a diagnostic rather than a gcc error, which is what the rule asks for.

**A `match` on a struct value is not expressible at all.** The parser has no
syntax for it: `match p { P { x } => x }` fails with ``expected `=>`, found `{` ``.
The tree interpreter does not run it either, so it is not a gap in the compiled
path but a gap in the language, and it is named here rather than left to be
discovered by a user. Which is a different kind of finding, and the first one in
this repository that is about the surface syntax rather than the emitter.

**The list of open rows is now empty, and that is not the same as the emitter
being correct.** Every construct named in these documents is either carried or
refused by name, because each was written down and then tried. The surface that
is still unknown is whatever nobody wrote down, and stages 11, 12 and 13 found
nine defects between them by writing down one more construct each time. The next
stage should keep doing that rather than assume the list is complete, because
this document proved itself wrong twice while it was being written.

## What this stage does not claim

**This does not make the emitter correct in general.** Eight defects in stages
10, 11 and 12 were all found by writing an example that exercised something no
example had reached, and stage 13 found three more by asking what else is read as
a list rather than by writing examples. The untested surface shrinks one example
at a time and is not closed: the two rows above are what is left of it.

****Five of six examples compiling is not parity with C or Rust.** `strings.vx`
still does not compile, and two tuple constructs now refuse where they previously
produced a C compiler error. A construct that says so is a better state than one
that guesses, and neither is completeness.

**No performance figure is quoted and none belongs in `BENCHMARKS.md`.** This
stage makes no measured claim, and `BENCHMARKS.md`, `SPEC.md` and `ROADMAP.md`
are untouched.

## Why there is nothing for the benchmark guard to check

`scripts/check-benchmarks.sh` checks that every wall clock number quoted in this
repository appears in a committed transcript, and that the recorded checksums
match. `STAGE13.md` makes no measured claim: it says what the emitter carries
and what it refuses, and every figure above was produced by running the example
on all four paths.

Extending the guard to a document with no measured claims would be guarding
nothing. What is checked instead is the thing this stage changed, and
`scripts/check-examples.sh` is where it is checked: it runs every example
through the compiled path at `-O2` and at `-O0` and the tree interpreter and the
VM, and fails on any disagreement. `striter.vx` was added to it, so a regression
that turned a character walk back into a byte walk would fail CI rather than
print a different answer for a user.