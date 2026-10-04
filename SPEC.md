# Vortex language specification

Status: draft 0.1, stage 1. This document is written before the implementation.
Where the implementation and this document disagree, the document is the bug and
the change goes through a pull request that edits both.

## 1. Goals

- Readable syntax. A program should look close to the English description of
  what it does.
- Precise diagnostics. Every error names a line and column and says what the
  parser or lexer expected. Silent coercion and silent failure are not allowed.
- Explicit safety. The memory model is a stated contract, not an emergent
  property of the runtime.
- Honest performance evidence. Any speed statement in this repository must be
  backed by the committed benchmark harness, the recorded machine and the raw
  output. See section 9.

## 2. What Vortex will not try to be

- Not a replacement for C in systems kernels or drivers. There is no `unsafe`
  block in v0.1.
- Not a scripting language. Types are static and checked before execution.
- Not a language with runtime checks on every arithmetic operation. Bounds,
  overflow and type checks are inserted where the type system cannot prove them
  absent.
- Not a concurrent language in v0.1. See section 7.
- Not a dynamic language with types added later. Inference is local and
  predictable.

## 3. Implementation language and test runner

The compiler is written in Rust and uses the built in `cargo test` harness.

The reason is evidence, not taste. Rust is one of the two languages Vortex
intends to be measured against, and a measurement is only meaningful if the
compiler itself is not the bottleneck. A Rust implementation keeps the host
language's memory safety and its optimiser for free, and its zero cost
abstractions mean the tree interpreter in stage 2 measures the interpreter rather
than the harness. `cargo test` needs no extra dependency, so CI has one
dependency to keep current, which is a smaller supply chain than adding a test
framework. The test command for the whole repository is:

    cargo test --workspace

## 4. Source file conventions

- Source files use the extension `.vx`.
- A file is UTF-8. Invalid UTF-8 is a lex error, not a replacement character.
- Positions are **1-based**. Both line and column start at 1.
- A column counts **Unicode scalar values**, not bytes. A multi byte character
  advances the column by exactly one, so a column maps to what a reader counts
  in an editor.
- A tab advances the column by one. The caret in a rendered diagnostic sits
  under the tab.
- Line numbers advance on `\n` only. A `\r\n` file has the `\r` counted as an
  ordinary character at the end of the line.
- A file that ends with a newline has no extra empty line. The end of file token
  reports the position just past the last character, on the last line.

## 5. Lexical structure

### 5.1 Whitespace

Spaces, tabs, carriage returns and newlines separate tokens and carry no
meaning.

### 5.2 Comments

    // line comment, ends at the next newline
    /* block comment */
    /* block /* comments */ nest /* correctly */ */

Block comments nest. An unterminated block comment is a lex error that points at
the position of the opening `/*` and says a closing `*/` was expected. Line
comments are not tokens and never appear in the token stream.

### 5.3 Identifiers and keywords

An identifier starts with an ASCII letter or `_` and continues with ASCII
letters, digits or `_`. Identifiers are case sensitive.

Keywords in v0.1:

    as  break  continue  else  enum  false  fn  for  if  impl  import  in
    let  match  return  self  struct  trait  true  var  while

The list grows in later stages. A stage that adds a keyword must list it here
first.

### 5.4 Integer literals

    1234        decimal
    1_234_000   underscores are separators and carry no meaning
    0xFF        hexadecimal
    0o755       octal
    0b1010_1010 binary

### 5.5 Float literals

A float needs a `.` followed by at least one digit, or an exponent.

    1.0    3.14159    1.5e10    2e-9    6.02e23

An integer immediately followed by `.` that is not followed by a digit is
lexed as the integer then the `.` operator, so `1.to_string()` is written
`(1).to_string()`. This rule is deliberate and is tested.

### 5.6 String literals

Strings are double quoted and may not contain a raw newline. Escape sequences
are decoded during lexing, so the token carries the decoded value.

    \0     null              \\     backslash
    \n     newline           \"     double quote
    \r     carriage return   \'     single quote
    \t     tab               \xNN   byte, exactly two hex digits
    \u{...} unicode scalar value, one to six hex digits

There is **no backspace escape in v0.1**. `\b` is a lex error that names the
column and says a known escape sequence was expected, because a reader who sees
`\b` in most languages will expect it to work here. If it is added later, this
table is the place to add it.

A string escape and a character escape draw from the same table. `'A'` and
`"A"` are the same value, and so are `'\x41'` and `"A"`. A `\xNN` escape
decodes one byte as a scalar value, so `"\x41"` is the character `A` and not the
integer 65; converting between a byte and an `Int` is a `Char` to `Int`
conversion, not an escape.

An unknown escape, a truncated `\x` and an out of range `\u{...}` are lex errors
that name what was expected. An unterminated string is a lex error that points
at the opening quote and says a closing `"` was expected.

### 5.7 Character literals

    'a'    '\n'    '\u{1F600}'    '\''

Exactly one scalar value. An empty literal `''`, a literal of two characters
`'ab'` and an unterminated literal `'a` are lex errors.

### 5.8 Operators and punctuation

    +   -   *   /   %
    =   +=  -=  *=  /=  %=
    ==  !=  <   <=  >   >=
    &&  ||  !
    ->  =>  ..  ..=  ::  ?
    (   )   {   }   [   ]   .   ,   ;   :

Maximal munch: `..=` lexes as one token, `==` as one token, `=` then `=` is two
tokens. A character that starts no token, such as `@` or a backtick, is a lex
error that says a token was expected.

## 6. Types

### 6.1 Rules

1. Types are static and checked before execution. There is no `any` type.
2. Named types declared with `struct` and `enum` are **nominal**. Two structs
   with identical fields are different types.
3. Anonymous record types and tuples are **structural**.
4. **No implicit numeric widening.** `Int` and `Float` mix only through an
   explicit `as` cast.
5. **No null.** A value that may be absent has type `Option<T>`, written
   `none` or `some(x)`.
6. A fallible operation returns `Result<T, E>`, written `ok(v)` or `err(e)`.
   The `?` operator propagates an `Err` out of the enclosing function.
7. Inference is local. A variable's type is fixed by its declaration and does
   not change to satisfy a later use.
8. Shadowing a `let` binding is an error. A name has one type in a scope.
9. Recursion needs no `mut`. A function name is not a binding and has no
   mutability, so a function may call itself however it likes. See section 8.1
   for how a parameter is made mutable, which is a different question.

### 6.2 Declarations

    struct Point { x: Int, y: Int }

    enum Shape {
        circle(Float),
        rect { w: Float, h: Float },
        empty,
    }

    trait Area {
        fn area(self) -> Float
    }

A struct field list may have trailing commas. An `enum` variant may carry
positional payloads, named fields, or neither. Constructing a value names the
type and the variant: `Shape.rect { w: 2.0, h: 3.0 }`.

### 6.3 Generics and dispatch

Generic functions and types are monomorphised. Every use produces a separate
concrete function. Trait bounds are checked at the call site and resolved at
compile time. Vortex v0.1 has **no trait objects**, so there is no dynamic
dispatch and no v table. Runtime type checks exist only for `as` casts between
related types, which are checked at runtime and which the type system cannot
prove.

## 7. Memory safety model

**Vortex uses linear ownership with move semantics, deterministic deallocation,
and no shared mutable state.**

Every value has exactly one owner. Assigning, passing or returning a value
moves it, and the source binding is dead afterwards. Dead bindings cannot be
read. When the owner's scope ends, the value is deallocated immediately and
deterministically. Values that are scalars or aggregates of scalars are `Copy`
and are duplicated on assignment instead of moved.

### 7.1 Why this and not the alternatives

**Not a borrow checker.** A borrow checker needs two features that interact
badly: borrows and lifetimes. Users spend most of their time reading lifetime
errors whose real cause is further away, and the projection onto a borrow is
frequently not the line the user has to change. Linearity gets most of the same
guarantees with one rule to state and one rule to check, which suits a language
whose stated goal is precise diagnostics.

**Not region based inference.** Inferred lifetimes hide the cost of a program in
the type system. A typo silently infers a longer region and the program keeps
running. Vortex prefers an explicit rule the reader can check by eye.

**Not tracing garbage collection.** Tracing collection gives up deterministic
destruction, which matters for file handles, sockets and foreign resources, and
it turns allocation into the dominant cost of pointer chasing workloads.
Vortex will not claim performance it cannot measure, and starting from a
collector would mean starting with a known cost and hoping the rest of the
design repays it.

### 7.2 What the choice costs, stated plainly

No aliasing means no shared subtrees, no parent pointers and no cycles. A
directed graph with cycles is not directly expressible. This is the main
weakness of the model and it is not solved yet.

Stage 5 decides between two options, and it must decide with measurements rather
than taste:

- **Option A: keep linearity.** Graphs use arena indices or an explicit parent
  field. The model stays simple and allocation stays cheap.
- **Option B: add a single owner `cell<T>` box** with interior mutability, and
  a collector that only inspects cells to break cycles. The cost is a bounded,
  narrow collector rather than a general one, but it reintroduces pauses.

The decision rule is fixed in advance: adopt Option B only if a committed
benchmark shows Option A is the limiting factor on realistic workloads.

### 7.3 Unsafe code

Vortex v0.1 has no `unsafe`. If a later stage adds it, it is restricted to
modules that also declare foreign interfaces, and this section must describe how
such a module is reviewed. That work is not scheduled yet.

## 8. Syntax and semantics

`let` binds an immutable name. `var` binds a mutable name. An assignment to a
`let` name is a type error, not a warning. Blocks are expressions and evaluate
to their final expression when it is not terminated by `;`.

### 8.1 Parameters and mutability

A **parameter is immutable by default**, exactly like a `let`. Writing `var`
before the parameter's name makes that parameter mutable:

    fn gcd(var a: Int, var b: Int) -> Int {
        while b != 0 {
            let t = b;
            b = a % b;
            a = t;
        }
        return a;
    }

This is the only difference from a `let`: `a` and `b` above may be assigned,
because they are written `var`, and nothing else about them changes. A parameter
without `var` may not be assigned, and an attempt is an error that names the
parameter and says it was declared with `let`.

There is no `let mut x` and no separate mutable type syntax. `var` on the
binding is the whole rule, so a reader can check mutability by looking at the
one word in front of the name.

Rule 9 of section 6.1 says recursion needs no `mut`, and that is unchanged: a
function may call itself freely, because a function name is not a binding and
has no mutability. The `gcd` above recurses in no way at all; it only shows
mutable parameters.

    let area = if w > h { w * h } else { h * w }

Control flow is `if` and `else`, `while`, `for x in a..=b`, `break`,
`continue`, `return`, and `match` on an enum value or a tuple:

    let label = match (i % 3, i % 5) {
        (0, 0) => "FizzBuzz",
        (0, _) => "Fizz",
        (_, 0) => "Buzz",
        _ => int_to_string(i),
    };

A `for` loop comes in two forms. `for x in a..=b` counts, and `for x in e` walks
a list, a tuple or a string. The loop variable is scoped to the loop, so it is
not visible after it.

### 8.2 What v0.1 cannot write

Stated here so a reader is not surprised, because each of these is a gap a
programmer will meet immediately.

- **There is no index assignment.** `a[i] = v` is a parse error. The only
  assignable names are `var` bindings, and a `var` is a whole value, not a list
  element. This is why `bench/vortex` has no program; see the note there.
- **There is no list of a computed length.** A list literal lists its elements,
  `[2, 3, 5]`. There is no `[0; n]` form, so a list whose size is only known at
  run time cannot be built.
- **There is no `as` cast.** Section 6.1 rule 4 and section 6.3 both refer to
  one, and it arrives with the type checker in stage 3.
- **There is no `Option` or `Result` construction.** Rules 5 and 6 of section
  6.1 describe them; the `none`, `some`, `ok` and `err` forms arrive in stage 3.

Modules come later. `import` is reserved in v0.1 so that adding it does not
break programs.

## 9. Performance stance

Vortex aims to be competitive with C and Rust on realistic workloads.

Vortex does **not** claim to be faster than C, and it is not expected to be.
This paragraph is the commitment: no document, commit message, issue or pull
request in this repository may state or imply a speed result that is not
produced by `bench/run.sh`, on a machine recorded in `BENCHMARKS.md`, with the
raw output committed. Estimates, projections and "should be" are not evidence.

The optimisation strategy for the stages ahead is:

1. Monomorphise generics, and do not box implicitly.
2. Insert runtime checks only where the type system cannot prove them absent.
3. Measure before adopting an optimisation, and require that it does not
   regress another benchmark.

`BENCHMARKS.md` contains **no Vortex results**. Stage 2 has a tree
interpreter, but Vortex cannot yet write the benchmark workload, because there
is no index assignment and no list of a computed length. `bench/vortex/README.md`
records that, and it also holds the measurements taken while deciding whether a
Vortex row was worth attempting. Nothing in this repository claims a Vortex speed
result until one is measured.

## 10. Stage plan

1. **Foundation.** This document, `ROADMAP.md`, the lexer with tests, a
   runnable benchmark skeleton and CI.
2. **Frontend.** Parser, AST and a tree interpreter that executes
   `examples/`.
3. **Types and diagnostics.** Static checker with regression tests for every
   diagnostic the spec promises.
4. **Bytecode VM.** Compilation to bytecode, a VM, and measured optimisation
   against the tree interpreter.
5. **Backend.** A native backend and a memory and concurrency model, chosen
   from stage 4 measurements and from section 7.2.

### 10.1 A recorded concern about stage 4

A bytecode VM built in stage 4 can become dead work if stage 5 moves to a native
backend, because the execution engine is replaced rather than extended. The
mitigation is to lower the AST once, in stage 2, into a form that both the tree
interpreter and the VM consume, so stage 4 adds a consumer instead of
rewriting the frontend. The stage order stays as written; this note exists so
the risk is visible before stage 2 starts rather than discovered during stage 4.

## 11. Open questions

These are undecided and each one is tracked in `ROADMAP.md` when it blocks a
stage.

- Trait objects and dynamic dispatch. Deferred past v0.1.
- Whether `cell<T>` and cycle breaking are added. Decision rule in 7.2.
- Concurrency model. Threads with ownership transfer, or a single threaded
  runtime with tasks. Stage 5, after stage 4 measurements.
- Whether a package manager and a foreign function interface are in scope at all.