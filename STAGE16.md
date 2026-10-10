# Stage 16: VM optimization and measured performance

Step 1 — constant propagation.

The bytecode compiler in `crates/vortexc/src/bytecode.rs` now folds a constant
arithmetic expression at the point it compiles it: when a `Binary` or unary
negation expression is built entirely from constant literals, the compiler
evaluates it at compile time and emits a single `Op::Const(result)` instead of
loading each operand and running the arithmetic opcode. Folding lives in two
places. `ir::Expr::const_value` (in `crates/vortexc/src/ir.rs`) is a pure
bottom-up evaluation of an IR expression to a `Const`, and `FnCompiler::expr`
calls it on every `Binary` and `Unary` node; a `Some` result replaces the whole
subtree with one `Const`, a `None` result compiles the operands and opcode as
before. The arithmetic matches the tree interpreter (`interp.rs`) exactly:
`Int` adds, subtracts, multiplies and divides with Rust's wrapping integers,
`Float` uses real `f64` arithmetic, and a division or remainder whose divisor is
zero is left unfolded. Integer division or remainder by zero is left to the
runtime, which raises `DivideByZero`; float division by zero is left to the
runtime too, which under `f64` yields infinity or NaN, so the compiler does not
invent a result either way. Only operands that are both constants fold, so
`2 + 3 * 4` compiles to `Const(14)` (the multiply folds first, then the add)
while `a + 1` keeps its `Add` opcode. The test
`bytecode::tests::constant_arithmetic_is_folded_to_a_const` asserts the folded
form, and `a_nonconstant_operand_is_not_folded` guards the non-folding case.

Step 2 — measure the performance impact of constant folding.

The fold replaces bytecode with constants, so the measurable impact is an
instruction-count reduction, written down honestly rather than as a speed
claim. The measurement compares two programs that walk the same 999-addition tree
and differ only in their leaves: `1 + 2 + ... + 1000` (all literals, folds) and
`a + a + ... + a` (a variable, does not fold). Raw output is in
`bench/results/stage16-constfold.txt`; machine spec and toolchain are taken from
`BENCHMARKS.md` (Intel Core i5-8500T, rustc 1.99.0).

Instruction count, from `disasm`:

- `1 + ... + 1000` folds to one instruction, `const int 500500`. The same tree
  with a variable leaf is 1999 instructions (999 `add` plus 1000 `load`).
  Folding compiles this expression to 1 instruction instead of 1999.
- `2 + 3 * 4` folds to `const int 14` (1 instruction) instead of 5
  (three loads, a multiply and an add), matching the Step 1 test.

Bytecode compilation time is the wall time of `disasm` on `chain1000.vx`, five
repeats, fastest 20 ms. That is not an isolated measure of the
`bytecode::compile` step: `disasm` runs lex, parse, lower and check before
compiling, and a fresh release binary costs about 16 ms to start, so the 20 ms is
dominated by process startup and front-end work, not the fold. The fold is a
single O(n) traversal of the expression plus one emitted instruction, so it is
on the order of microseconds and does not rise above the startup floor here.
It is not separable from the CLI, so this step records the end-to-end figure
and does not invent a nanosecond count for compile time alone.

Execution on the bytecode VM is similarly process-start-bounded: the folded
`chain1000` (3 instructions) and the unfolded `vachain1000` (2003 instructions)
both measure about 16-20 ms, the difference swamped by the ~16 ms startup floor.
The tree interpreter on the folded-constant source walks the full 1999-node IR
tree and lands on the same floor too. The per-step savings folding implies are
below the resolution of the CLI measurement at this chain depth.

The one measured, real effect is the instruction-count reduction. No speed
claim is made beyond that: a faster bytecode is not shown to be a faster program
here because execution is dominated by process startup.
