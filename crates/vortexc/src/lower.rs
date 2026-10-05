//! Lowering, from the surface tree into the form an executor walks.
//!
//! See [`crate::ir`] for what this produces and why. This pass resolves names,
//! decides the target of every call, and gives every block and expression the
//! position the parser recorded, so that both a lowering error and a run time
//! error can name a line and a column.

use std::collections::HashMap;

use crate::ast;
use crate::ir;
use crate::span::Pos;

pub use crate::ir::LowerError;

/// The builtin functions every program may call. Stage 3 checks their types.
pub const BUILTINS: &[&str] = &[
    "print",
    "println",
    "int_to_string",
    "float_to_string",
    "string_to_int",
    "sqrt",
    "abs",
    "min",
    "max",
    "now_ns",
];

/// The type names a cast may name, which is the built in numeric pair plus
/// `Int` and `Float` themselves. `SPEC.md` section 6.1 rule 4 only needs a way
/// between Int and Float, and nothing else is legal yet.
pub const CAST_TYPES: &[&str] = &["Int", "Float", "Char"];

/// Lowers a parsed program.
pub fn lower(program: &ast::Program) -> Result<ir::Program, LowerError> {
    let mut l = Lowerer {
        functions: HashMap::new(),
        arity: HashMap::new(),
        structs: HashMap::new(),
        enums: HashMap::new(),
    };

    // Every name is collected before any body is lowered, so a call may refer
    // to a function declared further down the file and a literal may name a
    // type declared later in it.
    for item in &program.items {
        match item {
            ast::Item::Function(f) => {
                if l.functions.insert(f.node.name.clone(), ()).is_some() {
                    return Err(LowerError {
                        message: format!("function `{}` is declared twice", f.node.name),
                        pos: f.pos,
                    });
                }
                l.arity.insert(f.node.name.clone(), f.node.params.len());
            }
            ast::Item::Struct(s) => {
                l.structs.insert(s.node.name.clone(), s.node.fields.clone());
            }
            ast::Item::Enum(e) => {
                l.enums.insert(e.node.name.clone(), e.node.variants.clone());
            }
        }
    }

    let mut items = Vec::new();
    for item in &program.items {
        match item {
            ast::Item::Function(f) => items.push(ir::Item::Function(l.function(f)?)),
            ast::Item::Struct(s) => items.push(ir::Item::Struct(ir::StructDef {
                name: s.node.name.clone(),
                fields: s.node.fields.clone(),
            })),
            ast::Item::Enum(e) => items.push(ir::Item::Enum(ir::EnumDef {
                name: e.node.name.clone(),
                variants: e.node.variants.clone(),
            })),
        }
    }

    Ok(ir::Program { items })
}

struct Lowerer {
    functions: HashMap<String, ()>,
    /// How many parameters each function takes, so a call can reserve room for
    /// its arguments.
    arity: HashMap<String, usize>,
    structs: HashMap<String, Vec<ast::Field>>,
    enums: HashMap<String, Vec<ast::Variant>>,
}

/// What a name currently refers to.
#[derive(Clone, Copy)]
struct Binding {
    slot: ir::Slot,
    /// Whether this binding may be assigned to. `SPEC.md` section 8 makes an
    /// assignment to anything else an error.
    mutable: bool,
    /// Whether the value this binding owns has been moved out.
    ///
    /// `SPEC.md` section 7 says every value has exactly one owner, that
    /// passing or assigning moves it, and that the moved-from binding is dead.
    /// The scalar types are `Copy` in that section, so moving one copies it and
    /// the binding stays live; only the aggregate types make it dead.
    moved: bool,
    /// Whether this binding holds a scalar, and so survives a move.
    copyable: bool,
    /// The element type when this binding holds a list, and `None` otherwise.
    ///
    /// This is computed when the binding is declared, from the initialiser:
    /// a list literal or a repeat says what it produces, and anything else is
    /// not a list. An index expression carries the type with it, so the
    /// executor never has to recover it from the syntax, where it is not
    /// present: `a[i]` has an `Int` index whatever `a` holds.
    element: Option<Element>,
}

/// The element type of a list, which the executor needs in order to read or
/// write one element with the right C type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Element {
    Int,
    Float,
    Str,
}

/// Whether a lowered expression is a scalar, so the binding it feeds survives a
/// move.
///
/// Only the forms the lowering pass already knows are classified. Anything it
/// cannot classify is treated as copyable, because the cost of that is a missed
/// diagnostic and the cost of the other choice is rejecting a program that
/// should compile.
fn ir_expr_is_copyable(e: &ir::Expr) -> bool {
    match e {
        ir::Expr::Const { value, .. } => matches!(
            value,
            ir::Const::Int(_) | ir::Const::Float(_) | ir::Const::Bool(_) | ir::Const::Char(_)
        ),
        ir::Expr::Binary { op, .. } => matches!(
            op,
            ast::BinOp::Add | ast::BinOp::Sub | ast::BinOp::Mul | ast::BinOp::Div | ast::BinOp::Rem
        ),
        ir::Expr::Load { .. }
        | ir::Expr::Cast { .. }
        | ir::Expr::Store { .. }
        | ir::Expr::Unary { .. } => true,
        _ => false,
    }
}

/// The names in a call's arguments whose values the call consumes.
///
/// Only a plain name and a field read of a plain name are recognised, because
/// those are the forms whose ownership can be decided syntactically. Anything
/// more complex, such as an arithmetic expression, builds a value that the call
/// consumes without any binding being moved.
fn moved_names_in(args: &[ast::Expr]) -> Vec<String> {
    args.iter()
        .filter_map(|a| match &a.kind {
            ast::ExprKind::Ident(n) => Some(n.clone()),
            // A field read is not a move. `SPEC.md` section 7 gives the
            // struct its own fields, and reading one copies the field, which
            // for every field type in v0.1 is a scalar. Marking the base moved
            // here rejected `p.x` followed by `p.y`, which is a program that
            // should compile.
            ast::ExprKind::Field(..) => None,
            _ => None,
        })
        .collect()
}

/// Whether a value of this type survives being moved.
///
/// `SPEC.md` section 7 says values that are scalars or aggregates of scalars
/// are `Copy` and are duplicated on assignment rather than moved. Everything
/// else is an aggregate that owns something, so moving it kills the binding.
/// The element type a list initialiser produces, if it produces a list.
///
/// A literal takes the type of its first element and a repeat takes the type of
/// the value it repeats. A list literal is checked elsewhere to be uniform, so
/// the first element decides the whole list.
fn element_of(e: &ast::Expr) -> Option<Element> {
    match &e.kind {
        ast::ExprKind::Array(items) => items.first().and_then(element_of_value),
        ast::ExprKind::Repeat { value, .. } => element_of_value(value),
        ast::ExprKind::Cast(_, t) => element_from_name(t.name()),
        _ => None,
    }
}

/// The element type of a value, when it is one a list can hold.
fn element_of_value(e: &ast::Expr) -> Option<Element> {
    match &e.kind {
        ast::ExprKind::Int(_) => Some(Element::Int),
        ast::ExprKind::Float(_) => Some(Element::Float),
        ast::ExprKind::Str(_) => Some(Element::Str),
        ast::ExprKind::Cast(_, t) => element_from_name(t.name()),
        _ => None,
    }
}

fn element_from_name(name: &str) -> Option<Element> {
    match name {
        "Int" => Some(Element::Int),
        "Float" => Some(Element::Float),
        "Str" => Some(Element::Str),
        _ => None,
    }
}

fn is_scalar_type(ty: &ast::TypeExpr) -> bool {
    matches!(
        ty.name(),
        "Int" | "Float" | "Bool" | "Char" | "Str" | "Unit"
    )
}

/// Per function state: the scopes open right now, and the next free slot.
struct FnState {
    scopes: Vec<HashMap<String, Binding>>,
    next: ir::Slot,
}

impl FnState {
    fn new() -> Self {
        FnState {
            scopes: vec![HashMap::new()],
            next: 0,
        }
    }

    fn alloc(&mut self) -> ir::Slot {
        let s = self.next;
        self.next += 1;
        s
    }

    fn declare(&mut self, name: &str, binding: Binding) {
        self.scopes
            .last_mut()
            .expect("a scope is always open")
            .insert(name.to_string(), binding);
    }

    fn lookup(&self, name: &str) -> Option<Binding> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied())
    }

    /// Carries the moved flags of the scope at `depth` back to the scope that
    /// encloses it, so a move inside a loop body outlives the body.
    fn carry_moved_out(&mut self, depth: usize) {
        let moved: Vec<(String, ir::Slot)> = self
            .scopes
            .get(depth)
            .map(|scope| {
                scope
                    .iter()
                    .filter(|(_, b)| b.moved)
                    .map(|(n, b)| (n.clone(), b.slot))
                    .collect()
            })
            .unwrap_or_default();
        if moved.is_empty() {
            return;
        }
        for (name, slot) in moved {
            for scope in self.scopes[..depth].iter_mut().rev() {
                if let Some(b) = scope.get_mut(&name) {
                    if b.slot == slot && !b.copyable {
                        b.moved = true;
                        break;
                    }
                }
            }
        }
    }

    /// The element type a list expression produces, if it is a list.
    fn element_of_expr(&self, e: &ast::Expr) -> Option<Element> {
        match &e.kind {
            ast::ExprKind::Ident(n) => self.element_of(n),
            ast::ExprKind::Paren(inner) => self.element_of_expr(inner),
            _ => element_of(e),
        }
    }

    /// The element type of the list a name owns, if it owns a list.
    fn element_of(&self, name: &str) -> Option<Element> {
        for scope in self.scopes.iter().rev() {
            if let Some(b) = scope.get(name) {
                return b.element;
            }
        }
        None
    }

    /// Clears the moved flag on a scalar binding, so reading it makes it live
    /// again. `SPEC.md` section 7 copies a scalar rather than moving it.
    fn clear_moved(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(b) = scope.get_mut(name) {
                b.moved = false;
                return;
            }
        }
    }

    /// Marks a binding moved, unless it holds a scalar.
    fn mark_moved(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(b) = scope.get_mut(name) {
                if !b.copyable {
                    b.moved = true;
                }
                return;
            }
        }
    }

    /// Marks every aggregate in a list of names moved, as a call taking several
    /// arguments does.
    fn mark_all_moved(&mut self, names: &[String]) {
        for n in names {
            self.mark_moved(n);
        }
    }

    /// Whether the name is declared in the innermost scope, so that shadowing
    /// an outer binding of the same name can be rejected.
    fn declared_here(&self, name: &str) -> bool {
        self.scopes
            .last()
            .expect("a scope is always open")
            .contains_key(name)
    }
}

impl Lowerer {
    fn function(&mut self, f: &ast::Spanned<ast::FnDecl>) -> Result<ir::Fn, LowerError> {
        let mut st = FnState::new();

        let mut params = Vec::new();
        for p in &f.node.params {
            let copyable = is_scalar_type(&p.ty);
            if st.declared_here(&p.name) {
                return Err(LowerError {
                    message: format!("parameter `{}` is declared twice", p.name),
                    pos: f.pos,
                });
            }
            let slot = st.alloc();
            st.declare(
                &p.name,
                if p.mutable {
                    Binding {
                        slot,
                        mutable: true,
                        moved: false,
                        copyable,
                        element: element_from_name(p.ty.name()),
                    }
                } else {
                    Binding {
                        slot,
                        mutable: false,
                        moved: false,
                        copyable,
                        element: element_from_name(p.ty.name()),
                    }
                },
            );
            params.push(ir::Param {
                slot,
                name: p.name.clone(),
                mutable: p.mutable,
                ty: p.ty.clone(),
            });
        }

        let body = self.block(&f.node.body, &mut st)?;

        Ok(ir::Fn {
            name: f.node.name.clone(),
            params,
            ret: f.node.ret.clone(),
            body,
            frame_size: st.next,
            pos: f.pos,
        })
    }

    fn block(&mut self, b: &ast::Block, st: &mut FnState) -> Result<ir::Block, LowerError> {
        // A function body already has a scope open for its parameters.
        st.scopes.push(HashMap::new());
        let depth = st.scopes.len();
        let result = self.block_inner(b, st);
        // A move inside a loop body outlives the body, because the body runs
        // again. Carrying the flags out is what makes `take(p)` twice inside a
        // while an error rather than silently compiling.
        st.carry_moved_out(depth);
        st.scopes.pop();
        result
    }

    fn block_inner(&mut self, b: &ast::Block, st: &mut FnState) -> Result<ir::Block, LowerError> {
        let mut stmts = Vec::new();
        for s in &b.stmts {
            self.stmt(s, st, &mut stmts)?;
        }
        let tail = match &b.tail {
            Some(e) => Some(Box::new(self.expr(e, st)?)),
            None => None,
        };
        Ok(ir::Block {
            stmts,
            tail,
            pos: b.pos,
        })
    }

    /// Lowers a block inside an already open scope, used for an `if` or `else`
    /// branch, which does not get a frame of its own.
    fn block_in_scope(
        &mut self,
        b: &ast::Block,
        st: &mut FnState,
    ) -> Result<ir::Block, LowerError> {
        let mut stmts = Vec::new();
        for s in &b.stmts {
            self.stmt(s, st, &mut stmts)?;
        }
        let tail = match &b.tail {
            Some(e) => Some(Box::new(self.expr(e, st)?)),
            None => None,
        };
        Ok(ir::Block {
            stmts,
            tail,
            pos: b.pos,
        })
    }

    fn stmt(
        &mut self,
        s: &ast::Stmt,
        st: &mut FnState,
        out: &mut Vec<ir::Stmt>,
    ) -> Result<(), LowerError> {
        match &s.kind {
            ast::StmtKind::Let {
                name,
                mutable,
                init,
            } => {
                if st.declared_here(name) {
                    return Err(LowerError {
                        message: format!(
                            "`{}` is already declared in this scope; `SPEC.md` section 6.1 rule 8 forbids shadowing",
                            name
                        ),
                        pos: s.pos,
                    });
                }
                let value = self.expr(init, st)?;
                let copyable = ir_expr_is_copyable(&value);
                let slot = st.alloc();
                st.declare(
                    name,
                    if *mutable {
                        Binding {
                            slot,
                            mutable: true,
                            moved: false,
                            copyable,
                            // The element type comes from the initialiser: a
                            // list literal or a repeat says what it produces.
                            element: element_of(init),
                        }
                    } else {
                        Binding {
                            slot,
                            mutable: false,
                            moved: false,
                            copyable,
                            element: element_of(init),
                        }
                    },
                );
                out.push(ir::Stmt::Let {
                    slot,
                    init: value,
                    pos: s.pos,
                });
            }

            ast::StmtKind::Return(e) => {
                let value = self.expr(e, st)?;
                out.push(ir::Stmt::Return(value));
            }

            ast::StmtKind::Expr(e) => {
                let lowered = self.expr(e, st)?;
                out.push(ir::Stmt::Nop {
                    expr: lowered,
                    pos: s.pos,
                });
            }

            ast::StmtKind::While { cond, body } => {
                let c = self.expr(cond, st)?;
                let b = self.block(body, st)?;
                out.push(ir::Stmt::While {
                    cond: c,
                    body: b,
                    pos: s.pos,
                });
            }

            ast::StmtKind::For {
                var,
                start,
                end,
                inclusive,
                body,
            } => {
                // A header with a `..` or `..=` counts; anything else walks a
                // list or a string. Which one it is is decided by the syntax,
                // not by the type, because this stage does not check types.
                let iterable = match end {
                    Some(_) => None,
                    None => Some(self.expr(start, st)?),
                };
                let (s_expr, e_expr) = match (&iterable, end) {
                    (Some(_), _) => (None, None),
                    (None, Some(e)) => (Some(self.expr(start, st)?), Some(self.expr(e, st)?)),
                    (None, None) => {
                        return Err(LowerError {
                            message: "a `for` header needs a range or a list".into(),
                            pos: s.pos,
                        })
                    }
                };
                // The loop variable lives in the loop's own scope, so it does
                // not leak into the body of the loop after it.
                let slot = st.alloc();
                st.scopes.push(HashMap::new());
                // A for variable is always an Int, so it is a scalar.
                st.declare(
                    var,
                    Binding {
                        slot,
                        mutable: true,
                        moved: false,
                        copyable: true,
                        element: Some(Element::Int),
                    },
                );
                let b = self.block_in_scope(body, st)?;
                st.scopes.pop();
                match (s_expr, e_expr) {
                    (Some(start), Some(end)) => out.push(ir::Stmt::For {
                        var_slot: slot,
                        start,
                        end,
                        inclusive: *inclusive,
                        body: b,
                        pos: s.pos,
                    }),
                    _ => out.push(ir::Stmt::ForEach {
                        var_slot: slot,
                        iterable: iterable.expect("a range free `for` has an iterable"),
                        body: b,
                        pos: s.pos,
                    }),
                }
            }

            ast::StmtKind::Break => out.push(ir::Stmt::Break),
            ast::StmtKind::Continue => out.push(ir::Stmt::Continue),

            ast::StmtKind::Block(inner) => {
                // A block statement gets a scope, and its statements run in the
                // enclosing block.
                let b = self.block(inner, st)?;
                for inner_stmt in b.stmts {
                    out.push(inner_stmt);
                }
            }
        }
        Ok(())
    }

    fn expr(&mut self, e: &ast::Expr, st: &mut FnState) -> Result<ir::Expr, LowerError> {
        let pos = e.pos;
        Ok(match &e.kind {
            ast::ExprKind::Int(v) => ir::Expr::Const {
                value: ir::Const::Int(*v),
                pos,
            },
            ast::ExprKind::Float(v) => ir::Expr::Const {
                value: ir::Const::Float(*v),
                pos,
            },
            ast::ExprKind::Str(s) => ir::Expr::Const {
                value: ir::Const::Str(s.clone()),
                pos,
            },
            ast::ExprKind::Char(c) => ir::Expr::Const {
                value: ir::Const::Char(*c),
                pos,
            },
            ast::ExprKind::Bool(b) => ir::Expr::Const {
                value: ir::Const::Bool(*b),
                pos,
            },

            // A bare name is a variable read, or a call with no arguments.
            ast::ExprKind::Ident(name) => match st.lookup(name) {
                Some(b) => {
                    if b.moved && !b.copyable {
                        return Err(LowerError {
                            message: format!(
                                "`{}` has been moved and cannot be used again; `SPEC.md` section 7 gives every value exactly one owner",
                                name
                            ),
                            pos,
                        });
                    }
                    if b.moved {
                        // A scalar was copied rather than moved, so the
                        // binding is live again.
                        st.clear_moved(name);
                    }
                    ir::Expr::Load { slot: b.slot, pos }
                }
                None => {
                    if self.functions.contains_key(name) {
                        ir::Expr::Call {
                            target: ir::CallTarget::Function(name.clone()),
                            args: Vec::new(),
                            arg_slots: Vec::new(),
                            pos,
                        }
                    } else if BUILTINS.contains(&name.as_str()) {
                        ir::Expr::Call {
                            target: ir::CallTarget::Builtin(name.clone()),
                            args: Vec::new(),
                            arg_slots: Vec::new(),
                            pos,
                        }
                    } else {
                        return Err(LowerError {
                            message: format!("undefined name `{}`", name),
                            pos,
                        });
                    }
                }
            },

            ast::ExprKind::Call { callee, args } => {
                // A call takes ownership of the values it is handed, so an
                // aggregate argument is moved into it. A struct literal builds
                // a new value and is not one.
                let consumed = moved_names_in(args);
                let target = self.call_target(callee, pos)?;
                // A callee's arguments live in its own frame, so the caller
                // reserves one slot per argument. Without this a function that
                // only takes parameters would have nowhere to receive them.
                let mut arg_slots = Vec::new();
                if let ir::CallTarget::Function(_) = target {
                    if let Some(want) = self.arity.get(callee).copied() {
                        if args.len() != want {
                            return Err(LowerError {
                                message: format!(
                                    "`{}` takes {} argument{}, but {} {} given",
                                    callee,
                                    want,
                                    if want == 1 { "" } else { "s" },
                                    args.len(),
                                    if args.len() == 1 { "was" } else { "were" }
                                ),
                                pos,
                            });
                        }
                        for _ in 0..want {
                            arg_slots.push(st.alloc());
                        }
                    }
                }
                let mut lowered = Vec::with_capacity(args.len());
                for a in args {
                    lowered.push(self.expr(a, st)?);
                }
                let call = ir::Expr::Call {
                    target,
                    args: lowered,
                    arg_slots,
                    pos,
                };
                // The call has taken ownership, so an aggregate argument's
                // binding is dead from here on.
                st.mark_all_moved(&consumed);
                call
            }

            ast::ExprKind::Neg(inner) => ir::Expr::Unary {
                neg: true,
                operand: Box::new(self.expr(inner, st)?),
                pos,
            },
            ast::ExprKind::Not(inner) => ir::Expr::Unary {
                neg: false,
                operand: Box::new(self.expr(inner, st)?),
                pos,
            },

            ast::ExprKind::Assign { name, index, value } => {
                // `a[i] = v` stores into a list element, which the interpreter
                // already knows how to do. The name it belongs to still has to
                // be mutable, exactly as a plain assignment does.
                if let Some((base_expr, index_expr)) = index {
                    // The list is held in a slot, so the executor can write the
                    // mutated list back after changing one element. The name
                    // itself must be a `var`, since this changes it.
                    let base_slot = match st.lookup(name) {
                        Some(b) if b.mutable => b.slot,
                        Some(_) => {
                            return Err(LowerError {
                                message: format!(
                                    "cannot assign into `{}` because it was declared with `let`",
                                    name
                                ),
                                pos,
                            })
                        }
                        None => {
                            return Err(LowerError {
                                message: format!("undefined name `{}`", name),
                                pos,
                            })
                        }
                    };
                    // The base expression is evaluated for its own errors only;
                    // it is a name, so the slot is what identifies the list.
                    let _ = self.expr(base_expr, st)?;
                    let idx = self.expr(index_expr, st)?;
                    let v = self.expr(value, st)?;
                    return Ok(ir::Expr::IndexStore {
                        slot: base_slot,
                        index: Box::new(idx),
                        value: Box::new(v),
                        pos,
                    });
                }

                let slot = match st.lookup(name) {
                    Some(b) if b.mutable => b.slot,
                    Some(_) => {
                        return Err(LowerError {
                            message: format!(
                                "cannot assign to `{}` because it was declared with `let`",
                                name
                            ),
                            pos,
                        })
                    }
                    None => {
                        return Err(LowerError {
                            message: format!("undefined name `{}`", name),
                            pos,
                        })
                    }
                };
                let v = self.expr(value, st)?;
                ir::Expr::Store {
                    slot,
                    value: Box::new(v),
                    pos,
                }
            }

            ast::ExprKind::Binary { op, lhs, rhs } => {
                let l = self.expr(lhs, st)?;
                let r = self.expr(rhs, st)?;
                ir::Expr::Binary {
                    op: *op,
                    lhs: Box::new(l),
                    rhs: Box::new(r),
                    pos,
                }
            }

            ast::ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                let c = self.expr(cond, st)?;
                // An if expression does not introduce a name of its own, so
                // both branches share the enclosing scope and frame.
                st.scopes.push(HashMap::new());
                let t = self.block_in_scope(then, st);
                let t = match t {
                    Ok(t) => t,
                    Err(e) => {
                        st.scopes.pop();
                        return Err(e);
                    }
                };
                let o = match otherwise {
                    None => None,
                    Some(else_box) => match self.else_expr(else_box, st) {
                        Ok(e) => Some(Box::new(e)),
                        Err(e) => {
                            st.scopes.pop();
                            return Err(e);
                        }
                    },
                };
                st.scopes.pop();
                ir::Expr::If {
                    cond: Box::new(c),
                    then: Box::new(t),
                    otherwise: o,
                    pos,
                }
            }

            ast::ExprKind::Match { scrutinee, arms } => {
                let s = self.expr(scrutinee, st)?;
                let mut lowered = Vec::new();
                for arm in arms {
                    // Each alternative in an arm lowers to its own lowered arm,
                    // sharing one lowered body per alternative so that pattern
                    // bindings are visible in it.
                    for pat in &arm.patterns {
                        st.scopes.push(HashMap::new());
                        let pattern = self.pattern(pat, st);
                        let pattern = match pattern {
                            Ok(p) => p,
                            Err(e) => {
                                st.scopes.pop();
                                return Err(e);
                            }
                        };
                        let body = match self.expr(&arm.body, st) {
                            Ok(b) => b,
                            Err(e) => {
                                st.scopes.pop();
                                return Err(e);
                            }
                        };
                        st.scopes.pop();
                        lowered.push(ir::Arm { pattern, body });
                    }
                }
                ir::Expr::Match {
                    scrutinee: Box::new(s),
                    arms: lowered,
                    pos,
                }
            }

            // A struct literal becomes a constructor call, so an executor does
            // not carry a second case for building a struct.
            ast::ExprKind::Record { ty, fields } => {
                let declared = match self.structs.get(ty) {
                    Some(f) => f.clone(),
                    None => {
                        return Err(LowerError {
                            message: format!("unknown struct `{}`", ty),
                            pos,
                        })
                    }
                };
                if declared.len() != fields.len() {
                    return Err(LowerError {
                        message: format!(
                            "struct `{}` has {} fields, but {} were given",
                            ty,
                            declared.len(),
                            fields.len()
                        ),
                        pos,
                    });
                }
                let mut lowered = Vec::with_capacity(fields.len());
                for (name, value) in fields {
                    if !declared.iter().any(|f| f.name == *name) {
                        return Err(LowerError {
                            message: format!("struct `{}` has no field `{}`", ty, name),
                            pos,
                        });
                    }
                    lowered.push(self.expr(value, st)?);
                }
                ir::Expr::Call {
                    target: ir::CallTarget::Struct(ty.clone()),
                    args: lowered,
                    arg_slots: Vec::new(),
                    pos,
                }
            }

            ast::ExprKind::Variant { ty, variant } => {
                self.variant(ty, variant, pos, 0)?;
                ir::Expr::Call {
                    target: ir::CallTarget::Variant {
                        ty: ty.clone(),
                        variant: variant.clone(),
                    },
                    args: Vec::new(),
                    arg_slots: Vec::new(),
                    pos,
                }
            }

            ast::ExprKind::VariantCall { ty, variant, args } => {
                self.variant(ty, variant, pos, args.len())?;
                let mut lowered = Vec::with_capacity(args.len());
                for a in args {
                    lowered.push(self.expr(a, st)?);
                }
                ir::Expr::Call {
                    target: ir::CallTarget::Variant {
                        ty: ty.clone(),
                        variant: variant.clone(),
                    },
                    args: lowered,
                    arg_slots: Vec::new(),
                    pos,
                }
            }

            ast::ExprKind::VariantRecord {
                ty,
                variant,
                fields,
            } => {
                self.variant(ty, variant, pos, fields.len())?;
                let mut lowered = Vec::with_capacity(fields.len());
                for (_, value) in fields {
                    lowered.push(self.expr(value, st)?);
                }
                ir::Expr::Call {
                    target: ir::CallTarget::Variant {
                        ty: ty.clone(),
                        variant: variant.clone(),
                    },
                    args: lowered,
                    arg_slots: Vec::new(),
                    pos,
                }
            }

            // `e as T` is a cast builtin, so an executor has one case for it
            // rather than one per source and target type.
            ast::ExprKind::Cast(inner, ty) => {
                let v = self.expr(inner, st)?;
                ir::Expr::Cast {
                    value: Box::new(v),
                    to: ty.clone(),
                    pos,
                }
            }

            // `[value; count]` builds a list of a length only known at run
            // time, which is what the benchmark workload needs.
            ast::ExprKind::Repeat { value, count } => {
                let v = self.expr(value, st)?;
                let c = self.expr(count, st)?;
                ir::Expr::Repeat {
                    value: Box::new(v),
                    count: Box::new(c),
                    pos,
                }
            }

            ast::ExprKind::Tuple(items) => {
                let mut lowered = Vec::with_capacity(items.len());
                for i in items {
                    lowered.push(self.expr(i, st)?);
                }
                ir::Expr::List {
                    items: lowered,
                    tuple: true,
                    pos,
                }
            }

            ast::ExprKind::Paren(inner) => self.expr(inner, st)?,
            ast::ExprKind::Block(b) => {
                let lowered = self.block(b, st)?;
                ir::Expr::BlockValue(Box::new(lowered))
            }
            ast::ExprKind::Try(inner) => ir::Expr::Try {
                inner: Box::new(self.expr(inner, st)?),
                pos,
            },
            ast::ExprKind::Array(items) => {
                let mut lowered = Vec::with_capacity(items.len());
                for i in items {
                    lowered.push(self.expr(i, st)?);
                }
                ir::Expr::List {
                    items: lowered,
                    tuple: false,
                    pos,
                }
            }
            ast::ExprKind::Index(base, index) => {
                // The element type is read from the binding the base names,
                // because that is where the lowering pass recorded it when it
                // declared the name. A base that is a literal or a repeat knows
                // its own element type. Anything else is not a list and the
                // type is absent rather than guessed at.
                let element = st.element_of_expr(base);
                let b = self.expr(base, st)?;
                let i = self.expr(index, st)?;
                ir::Expr::Index {
                    base: Box::new(b),
                    index: Box::new(i),
                    element,
                    pos,
                }
            }
            // `Shape.empty` parses as a field read, because the parser cannot
            // tell a field from a variant without knowing the types. Here the
            // base is known, so a base that names an enum is a variant with no
            // payload.
            ast::ExprKind::Field(base, name) => {
                if let ast::ExprKind::Ident(ty) = &base.kind {
                    if self.enums.contains_key(ty) {
                        self.variant(ty, name, pos, 0)?;
                        return Ok(ir::Expr::Call {
                            target: ir::CallTarget::Variant {
                                ty: ty.clone(),
                                variant: name.clone(),
                            },
                            args: Vec::new(),
                            arg_slots: Vec::new(),
                            pos,
                        });
                    }
                }
                let b = self.expr(base, st)?;
                ir::Expr::Field {
                    base: Box::new(b),
                    name: name.clone(),
                    pos,
                }
            }
        })
    }

    fn else_expr(&mut self, e: &ast::Else, st: &mut FnState) -> Result<ir::Expr, LowerError> {
        Ok(match e {
            ast::Else::Block(b) => ir::Expr::BlockValue(Box::new(self.block_in_scope(b, st)?)),
            ast::Else::If(spanned) => self.expr(&spanned.node, st)?,
        })
    }

    /// Decides what a call name refers to.
    fn call_target(&self, callee: &str, pos: Pos) -> Result<ir::CallTarget, LowerError> {
        if self.functions.contains_key(callee) {
            Ok(ir::CallTarget::Function(callee.to_string()))
        } else if BUILTINS.contains(&callee) {
            Ok(ir::CallTarget::Builtin(callee.to_string()))
        } else if self.structs.contains_key(callee) {
            Ok(ir::CallTarget::Struct(callee.to_string()))
        } else {
            Err(LowerError {
                message: format!("call to unknown function `{}`", callee),
                pos,
            })
        }
    }

    /// Checks a variant exists and carries the number of values given.
    fn variant(&self, ty: &str, variant: &str, pos: Pos, given: usize) -> Result<(), LowerError> {
        let variants = match self.enums.get(ty) {
            Some(v) => v,
            None => {
                return Err(LowerError {
                    message: format!("unknown enum `{}`", ty),
                    pos,
                })
            }
        };
        match variants.iter().find(|v| v.name == variant) {
            None => Err(LowerError {
                message: format!("enum `{}` has no variant `{}`", ty, variant),
                pos,
            }),
            Some(v) => {
                let want = v.payloads.len().max(v.named.len());
                if want != given {
                    Err(LowerError {
                        message: format!(
                            "variant `{}.{}` carries {} value{}, but {} {} given",
                            ty,
                            variant,
                            want,
                            if want == 1 { "" } else { "s" },
                            given,
                            if given == 1 { "was" } else { "were" }
                        ),
                        pos,
                    })
                } else {
                    Ok(())
                }
            }
        }
    }

    fn pattern(&mut self, p: &ast::Pattern, st: &mut FnState) -> Result<ir::Pattern, LowerError> {
        Ok(match &p.kind {
            ast::PatternKind::Wildcard => ir::Pattern::Wildcard,
            ast::PatternKind::Tuple(items) => {
                let mut lowered = Vec::with_capacity(items.len());
                for i in items {
                    lowered.push(self.pattern(i, st)?);
                }
                ir::Pattern::Tuple(lowered)
            }
            ast::PatternKind::Literal(e) => match const_of(e) {
                Some(c) => ir::Pattern::Literal(c),
                None => {
                    return Err(LowerError {
                        message: "a pattern literal must be a constant".into(),
                        pos: p.pos,
                    })
                }
            },
            ast::PatternKind::Variant {
                ty,
                variant,
                bindings,
            } => {
                self.variant(ty, variant, p.pos, bindings.len())?;
                let mut slots = Vec::with_capacity(bindings.len());
                for n in bindings {
                    let slot = st.alloc();
                    st.declare(
                        n,
                        Binding {
                            slot,
                            mutable: false,
                            moved: false,
                            copyable: true,
                            element: Some(Element::Int),
                        },
                    );
                    slots.push(ir::Binding {
                        slot,
                        name: n.clone(),
                    });
                }
                ir::Pattern::Variant {
                    ty: ty.clone(),
                    variant: variant.clone(),
                    bindings: slots,
                }
            }
            ast::PatternKind::Binding(name) => {
                let slot = st.alloc();
                st.declare(
                    name,
                    Binding {
                        slot,
                        mutable: false,
                        moved: false,
                        copyable: true,
                        // A match binding is an Int or a Float payload in
                        // v0.1, so it is never a list.
                        element: None,
                    },
                );
                ir::Pattern::Binding(ir::Binding {
                    slot,
                    name: name.clone(),
                })
            }
        })
    }
}

fn const_of(e: &ast::Expr) -> Option<ir::Const> {
    match &e.kind {
        ast::ExprKind::Int(v) => Some(ir::Const::Int(*v)),
        ast::ExprKind::Float(v) => Some(ir::Const::Float(*v)),
        ast::ExprKind::Str(s) => Some(ir::Const::Str(s.clone())),
        ast::ExprKind::Char(c) => Some(ir::Const::Char(*c)),
        ast::ExprKind::Bool(b) => Some(ir::Const::Bool(*b)),
        _ => None,
    }
}
