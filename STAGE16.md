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
`Float` uses real `f64` arithmetic, and a division or remainder whose divisor
is zero is left unfolded so the runtime keeps raising `DivideByZero` instead of
the compiler inventing a result. Only operands that are both constants fold, so
`2 + 3 * 4` compiles to `Const(14)` (the multiply folds first, then the add)
while `a + 1` keeps its `Add` opcode. The test
`bytecode::tests::constant_arithmetic_is_folded_to_a_const` asserts the folded
form, and `a_nonconstant_operand_is_not_folded` guards the non-folding case.
