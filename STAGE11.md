# Stage 11: what the compiled path can now express

Stage 10 found that three of the four examples do not compile on the emitted
path, and that `strings.vx` indexes a string which the emitter emits as a list
access. It noted that this is the same problem `match` is: the emitter walking a
construct it does not carry.

This stage closes as much of that as it can and names what is left.

## What now compiles

Three of the four examples produce identical output on the compiled path, the
tree interpreter and the bytecode VM:

| Example | Compiled | Tree | VM |
| --- | --- | --- | --- |
| `hello.vx` | `21` `6765` | same | same |
| `ownership.vx` | `n consumed 21` `42` `12` `36` | same | same |
| `structs.vx` | `3` `4` `12.566371` `32.000000` `0.000000` `41` `395` `26` `abc` `4` `1` `FizzBuzz` `Fizz` `7` | same | same |

Each was also built at `-O0` through `VORTEX_C_OPT` and produced the same output,
because stage 10 showed that `-O2` can quietly repair the emitter.

## What does not, and why

`strings.vx` does not compile, and now says so instead of emitting C that does
not compile:

    not compiled: indexing a string at 33:15

The emitter previously emitted an index as a list access unconditionally, so
indexing a string read a `void *` as an element array. That failed to compile
with a gcc error naming a type, which says nothing about Vortex. Refusing by
name is what an emitter that guesses should not do.

## What was added

**`match`, as a GNU statement expression.** The arms are tried in order and the
first that matches supplies the value, which is what the tree interpreter
implements. Each arm becomes its own statement expression so a name bound in
one arm is not visible in another and two arms may bind the same name. The
supported patterns are a wildcard, a binding, a literal, a tuple and a variant
with positional or named payloads.

**Structs and enums as tagged C structs.** A struct is a tag plus its fields. An
enum is a tag, a variant index and an untyped payload pointer, because a
variant's payload types differ per variant and are not expressible as a C
array. Each access casts at the type the enum declared, so `circle(Float)` reads
a `double` rather than an `int64_t` truncated at the boundary.

**An `if` used as a value,** which was refused before. It becomes a statement
expression assigning the condition to a declared slot and selecting a branch on
it, with the type of the branches.

**The second `for` form,** `for x in e { ... }`, which walks a list. It was
refused as an open ended range, which it is not: `SPEC.md` section 8 defines the
second `for` form as walking a collection.

**String indexing refused by name,** as above.

## Four defects found on the way, none of which a reduction program would have found

**A `match` was declared `int64_t`.** `expr_type_of` had no case for it, so a
`match` returning a Float was truncated. This is the same class of defect stage 8
fixed for a call holding a Float, and it needed a program using `match` on a
Float value to show up.

**A Float literal was written with six decimals.** `3.14159265` became
`3.141593`, so `area(Shape.circle(2.0))` gave `12.566372` on the compiled path
against `12.566371` on the tree. Literals are now written with enough digits to
round trip.

**An enum payload was stored through an `int64_t` slot.** The generated struct
carried `p0` as `int64_t`, so a Float payload was truncated before it was read.

**A list literal of strings built an `int64_t` array.** `["a", "b", "c"]` emitted
`(int64_t[]){ "a", "b", "c" }`, which gcc rejects. The constructor is now chosen
by the element type.

## The inventory of what the tree interpreter supports and the emitter does not

One observation, not a fix. Taken from the emitter's own refusal messages rather
than by trying every example.

| Construct | Tree interpreter | Emitter |
| --- | --- | --- |
| `fn`, `let`, `var`, `return`, `while`, `for ..=`, `for ..in`, `break`, `continue` | yes | yes |
| `if` as a statement | yes | yes |
| `if` as a value | yes | yes, stage 11 |
| `match` as a value | yes | yes, stage 11 |
| struct declaration, field read, record expression | yes | yes, stage 11 |
| enum declaration, variant expression, variant field syntax | yes | yes, stage 11 |
| tuple expression, tuple pattern | yes | yes, stage 11 |
| list literal, list repeat, index read, index store | yes | yes |
| `as` cast between `Int`, `Float` and `Char` | yes | yes |
| `Str` builtins: `int_to_string`, `float_to_string`, `print`, `println` | yes | yes, `print` and `println` included |
| **indexing a string** | yes | **no**, refused by name |
| **string escapes beyond the basic ones** | yes | **no**, the emitter writes a literal as given |
| **a list of a named struct or enum** | yes | **no**, `list_element_type` names only `Int`, `Float` and `Str`, so it is read at `int64_t` |
| **an enum variant whose payload fields have different types** | yes | **no**, refused by name, because a C array cannot hold them |
| **a match with no arms, or on a type the emitter cannot compare** | yes | **no**, refused by name |
| **string `+` on a value the emitter cannot see is a string** | yes | **partly**, it is decided by the recorded bindings rather than the expression |
| **inference, generics, closures, async, modules** | no | no |

The three rows in bold that are not the string index are the ones most likely to
produce a wrong answer rather than a refusal, because each falls back to a
default rather than declining. A list of a named struct is the one that would
bite first in real code.

## What this stage does not claim

**This does not make the language fast and does not make it complete.** It makes
the compiled path able to express more of the language, which is a different
claim. Two of the four examples compiling three stages ago and three of the four
now is progress, not parity with C or Rust.

**No performance figure is quoted and none belongs in `BENCHMARKS.md`.** This
stage is about what compiles, not what is fast.

**Nothing here shows the emitter is correct in general.** The four defects above
were found by making three examples compile and comparing their output. A
construct with no example is untested here, and the inventory says which.

## Why there is nothing for the guard to check

`scripts/check-benchmarks.sh` checks that every wall clock number quoted in this
repository appears in a committed transcript, and that the recorded checksums
match. `STAGE11.md` makes no measured claim: it says what compiles and what is
refused. Every figure in the table above was produced by running the examples on
all three paths and comparing their output, and those are test failures when they
differ rather than a measurement to be traced.

Extending the guard to a document with no measured claims would be guarding
nothing, and would make the guard's job harder to read rather than easier.

What is checked instead is the thing this stage actually changed.
`scripts/check-examples.sh` runs every example through all three paths and fails
if the compiled path disagrees with the two interpreters, at `-O2` and at `-O0`.
An example the emitter refuses is listed with its reason rather than failing, so
a refusal by name stays visible instead of being treated as a regression. It
runs in CI. It was shown to bite: with the compiled path's output replaced by a
wrong value it exits 1, and it exits 0 when the paths agree.

The guard also does not check `STAGE11.md` itself. If that document's table of
what compiles drifts, `scripts/check-examples.sh` prints the truth on every CI
run, which is the same guarantee reached by a different route.