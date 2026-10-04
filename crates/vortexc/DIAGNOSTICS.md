# Vortex diagnostics

Every diagnostic the compiler can emit, and the `SPEC.md` rule it enforces.
Each one has a regression test in `crates/vortexc/tests/` asserting the exact
message together with the line and the column.

The compiler is four stages. A program stops at the first one that rejects it,
so which stage owns a rule matters: an undefined name is caught during
lowering, not by the checker, and a test that expects the checker to produce it
is testing the wrong stage.

| Stage | File | Owns |
| --- | --- | --- |
| lex | `crates/vortexc/src/lexer.rs` | Tokens, escapes, comments. `SPEC.md` section 5 |
| parse | `crates/vortexc/src/parser.rs` | Syntax. `SPEC.md` section 5, section 8 |
| lower | `crates/vortexc/src/lower.rs` | Names, scopes, calls, mutability |
| check | `crates/vortexc/src/checker.rs` | Types. `SPEC.md` section 6 |
| run | `crates/vortexc/src/interp.rs` | Division by zero, bounds, depth |

Positions are 1-based for both line and column, and a column counts Unicode
scalar values rather than bytes. See `SPEC.md` section 4.

## Lexer diagnostics

Tests in `crates/vortexc/tests/lexer.rs`.

| Diagnostic | Rule |
| --- | --- |
| a character that starts no token | section 5.8, maximal munch and the token set |
| an unknown escape | section 5.6, the escape table |
| a truncated `\x` | section 5.6, exactly two hex digits |
| an out of range `\u{...}` | section 5.6, one to six hex digits |
| an unterminated string | section 5.6, points at the opening quote |
| an unterminated character literal | section 5.7, points at the opening quote |
| an empty or two character literal | section 5.7, exactly one scalar value |
| an unterminated block comment | section 5.2, points at the opening `/*` |
| a radix prefix with no digits | section 5.4 |
| an integer too large for 64 bits | section 5.4 |

There is no backspace escape. `\b` produces the unknown escape diagnostic, and
section 5.6 says so.

## Parser diagnostics

Tests in `crates/vortexc/tests/parser.rs`.

| Diagnostic | Rule |
| --- | --- |
| a missing semicolon at the end of a statement | section 8, statements end with `;` |
| a missing closing brace | section 8, a block is delimited |
| a top level item that is not `fn`, `struct` or `enum` | section 8 |
| a missing function name | section 8 |
| a missing `:` before a type | section 8, `name: Type` |
| a missing `in` in a `for` header | section 8, `for x in a..=b` |
| a missing `=>` in a match arm | section 8 |
| a call on something that is not a name | section 8, `f(...)` |
| a keyword where an expression belongs | section 5.3, the keyword list |

## Lowering diagnostics

Tests in `crates/vortexc/tests/parser.rs` and `crates/vortexc/tests/interp.rs`.

These are name and scope rules, which the checker cannot see because they are
decided while a name is resolved to a slot.

| Diagnostic | Rule |
| --- | --- |
| an undefined name | section 6.1 rule 7, a name is fixed by its declaration |
| assignment to a binding declared with `let` | section 8, an assignment to a `let` is an error |
| shadowing in the same scope | section 6.1 rule 8 |
| a call to a function that was never declared | section 8 |
| a call with the wrong number of arguments | section 8 |
| an unknown struct in a literal | section 6.2 |
| a struct literal with a field that does not exist | section 6.2 |
| a struct literal with the wrong number of fields | section 6.2 |
| an unknown enum or variant | section 6.2 |
| a variant with the wrong number of values | section 6.2 |
| a `for` with neither a range nor a list | section 8 |

## Checker diagnostics

Tests in `crates/vortexc/tests/checker.rs`. This is the list the stage 3 brief
asks for.

### Section 6.1 rule 1, types are static and checked before execution

| Diagnostic | Notes |
| --- | --- |
| an `if` condition that is not `Bool` | points at the condition |
| a `while` condition that is not `Bool` | points at the condition |
| a `for` range whose ends are not both `Int` | points at whichever end is wrong |
| `for` over something that is not a list, tuple or string | points at the iterable |
| an index that is not an `Int` | points at the index |
| a return whose type does not match the signature | points at the returned expression |
| a function that can reach its end without returning | points at the function |
| a tail expression that does not match the declared return type | points at the expression |
| an argument whose type does not match the signature | points at the call |
| a builtin called with the wrong number of values | points at the call |
| a builtin argument of the wrong type | points at the call |

### Section 6.1 rule 2, named types are nominal

| Diagnostic | Notes |
| --- | --- |
| an unknown type name | points at the function declaring it |
| comparing or mixing two structs that have identical fields | points at the expression; they are different types because each declaration has its own id |
| a field that does not exist on a struct | points at the field read |
| a field given the wrong type | points at the literal |
| a struct literal missing a field | points at the literal |

### Section 6.1 rule 3, records and tuples are structural

| Diagnostic | Notes |
| --- | --- |
| a list mixing two element types | points at the first element that disagrees |
| a tuple index past the end | points at the index; a compile time error rather than a run time one |

### Section 6.1 rule 4, no implicit numeric widening

| Diagnostic | Notes |
| --- | --- |
| an arithmetic operator applied to `Int` and `Float` | points at the operator, and the message cites rule 4 |
| a comparison of `Int` with `Float` | points at the operator |
| a subtraction or assignment mixing the two | points at the operator |

`Int` and `Float` operations do not mix. The only way between them is an `as`
cast.

### Section 6.1 rule 5 and 6, `Option` and `Result`

| Diagnostic | Notes |
| --- | --- |
| `?` applied to something that is not a `Result` or `Option` | points at the `?` |

### Section 6.1 rule 7, inference is local

| Diagnostic | Notes |
| --- | --- |
| storing a value whose type differs from the slot | points at the store |
| assigning a different type to a name | section 6.1 rule 7 fixes the type at the declaration |

### Match and exhaustiveness

| Diagnostic | Rule |
| --- | --- |
| a match arm that can never match the subject | dead code, named rather than ignored |
| a match with no `_` arm that also does not cover every variant of an enum | a value that matches nothing would have no value |
| arms of a match producing different types | the value of the match would depend on which arm ran |

Exhaustiveness is computed from the enum declaration, not assumed. A match that
names every variant is accepted without a `_` arm, and a test in
`crates/vortexc/tests/checker.rs` pins that down so the rule cannot later become
"always demand a wildcard". A match over an `Int` or a `Str` still needs a `_`,
because the compiler cannot see which values will arrive.

## Interpreter diagnostics

Tests in `crates/vortexc/tests/interp.rs`. These are the checks that can be
sound without types. Everything a type rule can prove is caught earlier.

| Diagnostic | Notes |
| --- | --- |
| division by zero | points at the operator |
| an index past the end of a list | names the index and the length |
| a negative index | names the index |
| an index into something that cannot be indexed | names the type |
| `break` or `continue` outside a loop | |
| calls nested too deeply | a runaway recursion reports rather than exhausting the machine stack |

## What is not checked yet

Stated plainly rather than left silent, as the stage 3 brief requires.

- **Traits and generics.** `SPEC.md` section 6.3 says generic functions are
  monomorphised and trait bounds resolve at compile time. The parser accepts no
  generic syntax and `trait` is a keyword with no meaning yet, so nothing about
  monomorphisation is implemented. Section 6.3 is unimplemented.
- **Records as a type annotation.** Section 6.1 rule 3 calls anonymous record
  types structural. Vortex writes a struct literal only against a declared
  name, so there is no syntax for an anonymous record type. Structural typing is
  implemented for tuples and for list elements, which are the structural types
  the syntax can actually express.
- **Variance and lifetime rules.** Section 7 states a move semantics model.
  Stage 3 does not yet check that a moved value is not used again, so a use
  after a move still compiles. Section 7 is not yet enforced; that is the
  natural companion to the memory work in stage 5.
- **No dynamic dispatch.** Section 6.3 says there are no trait objects, so
  there is no v table and nothing to check. That part of the rule holds by
  construction.
