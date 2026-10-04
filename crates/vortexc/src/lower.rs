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
enum Binding {
    /// A `var` or a `var` parameter. May be assigned to.
    Mutable(ir::Slot),
    /// A `let` or an immutable parameter. `SPEC.md` section 8 makes an
    /// assignment to one an error.
    Immutable(ir::Slot),
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
                    Binding::Mutable(slot)
                } else {
                    Binding::Immutable(slot)
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
        let result = self.block_inner(b, st);
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
    fn block_in_scope(&mut self, b: &ast::Block, st: &mut FnState) -> Result<ir::Block, LowerError> {
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
                let slot = st.alloc();
                st.declare(
                    name,
                    if *mutable {
                        Binding::Mutable(slot)
                    } else {
                        Binding::Immutable(slot)
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
                out.push(ir::Stmt::Nop(lowered));
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
                st.declare(var, Binding::Mutable(slot));
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
            ast::ExprKind::Int(v) => ir::Expr::Const(ir::Const::Int(*v)),
            ast::ExprKind::Float(v) => ir::Expr::Const(ir::Const::Float(*v)),
            ast::ExprKind::Str(s) => ir::Expr::Const(ir::Const::Str(s.clone())),
            ast::ExprKind::Char(c) => ir::Expr::Const(ir::Const::Char(*c)),
            ast::ExprKind::Bool(b) => ir::Expr::Const(ir::Const::Bool(*b)),

            // A bare name is a variable read, or a call with no arguments.
            ast::ExprKind::Ident(name) => match st.lookup(name) {
                Some(Binding::Mutable(slot)) | Some(Binding::Immutable(slot)) => {
                    ir::Expr::Load(slot)
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
                ir::Expr::Call {
                    target,
                    args: lowered,
                    arg_slots,
                    pos,
                }
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

            ast::ExprKind::Assign { name, value } => {
                let slot = match st.lookup(name) {
                    Some(Binding::Mutable(slot)) => slot,
                    Some(Binding::Immutable(_)) => {
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

            ast::ExprKind::VariantCall {
                ty,
                variant,
                args,
            } => {
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
                let b = self.expr(base, st)?;
                let i = self.expr(index, st)?;
                ir::Expr::Index {
                    base: Box::new(b),
                    index: Box::new(i),
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
                    st.declare(n, Binding::Immutable(slot));
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
                st.declare(name, Binding::Immutable(slot));
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
