//! The static type checker.
//!
//! `SPEC.md` section 6.1 rule 1 says types are static and checked before
//! execution, so this runs after lowering and before the interpreter. An ill
//! typed program never reaches [`crate::interp`].
//!
//! Every diagnostic here names a rule in `SPEC.md` section 6. The mapping is in
//! `crates/vortexc/src/checker/DIAGNOSTICS.md` and in the doc comment on each
//! variant of [`TypeError`].

use std::collections::HashMap;

use crate::ast;
use crate::ir;
use crate::span::Pos;
use crate::types::{arith_of, ordered, Decls, Type};

/// Checks a lowered program.
///
/// Returns the first error, or the declarations the program made, which the
/// checker needs to report a type name in a diagnostic rather than an id.
pub fn check(program: &ir::Program) -> Result<Decls, TypeError> {
    let mut c = Checker::new(program);
    c.run()
}

/// A type error, with the position the parser recorded.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeError {
    pub message: String,
    pub pos: Pos,
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "error at {}: {}", self.pos, self.message)
    }
}

impl std::error::Error for TypeError {}

/// A function's signature, gathered before any body is checked so a call may
/// refer to a function declared further down the file.
#[derive(Debug, Clone)]
struct Signature {
    params: Vec<Type>,
    ret: Type,
}

struct Checker<'a> {
    program: &'a ir::Program,
    decls: Decls,
    /// The type of each function, filled in before bodies are checked.
    signatures: HashMap<String, Signature>,
    /// Which declaration each name refers to, for the built in names too.
    builtins: HashMap<String, (Vec<Type>, Type)>,
}

impl<'a> Checker<'a> {
    fn new(program: &'a ir::Program) -> Self {
        Checker {
            program,
            decls: Decls::new(),
            signatures: HashMap::new(),
            builtins: builtin_signatures(),
        }
    }

    fn error<T>(&self, pos: Pos, message: String) -> Result<T, TypeError> {
        Err(TypeError { message, pos })
    }

    fn run(&mut self) -> Result<Decls, TypeError> {
        // Declare every struct and enum first, so a struct may refer to another
        // declared after it.
        for item in &self.program.items {
            match item {
                ir::Item::Struct(s) => {
                    let id = self.decls.add_struct(&s.name, Vec::new());
                    let _ = id;
                }
                ir::Item::Enum(_) => {}
                ir::Item::Function(_) => {}
            }
        }

        // Two passes over the fields, because a field may name a struct that is
        // declared later in the file. Field names are resolved first so every
        // struct has an id, then each field type is looked up.
        let mut struct_fields: Vec<(String, Vec<(String, ast::TypeExpr)>)> = Vec::new();
        for item in &self.program.items {
            if let ir::Item::Struct(s) = item {
                struct_fields.push((
                    s.name.clone(),
                    s.fields
                        .iter()
                        .map(|f| (f.name.clone(), f.ty.clone()))
                        .collect(),
                ));
            }
        }
        for (name, fields) in &struct_fields {
            let id = self
                .decls
                .find(name)
                .expect("the struct was declared in the first pass");
            let mut resolved = Vec::new();
            for (fname, fty) in fields {
                resolved.push((fname.clone(), self.resolve_type(fty.name())?));
            }
            self.decls.get_mut(id).fields = resolved;
        }

        // Enums, which may carry a struct type as a payload.
        for item in &self.program.items {
            if let ir::Item::Enum(e) = item {
                let mut variants = Vec::new();
                for v in &e.variants {
                    let mut payloads = Vec::new();
                    for p in &v.payloads {
                        payloads.push(self.resolve_type(p.name())?);
                    }
                    let mut named = Vec::new();
                    for f in &v.named {
                        named.push(f.name.clone());
                        payloads.push(self.resolve_type(f.ty.name())?);
                    }
                    variants.push(crate::types::VariantDecl {
                        name: v.name.clone(),
                        payloads,
                        named,
                    });
                }
                self.decls.add_enum(&e.name, variants);
            }
        }

        // Signatures, so a body may call a function declared later.
        for item in &self.program.items {
            if let ir::Item::Function(f) = item {
                let mut params = Vec::new();
                for p in &f.params {
                    params.push(self.resolve_type(p.ty.name())?);
                }
                let ret = match &f.ret {
                    Some(t) => self.resolve_type(t.name())?,
                    None => Type::Unit,
                };
                self.signatures
                    .insert(f.name.clone(), Signature { params, ret });
            }
        }

        // Now the bodies, with every name known.
        for item in &self.program.items {
            if let ir::Item::Function(f) = item {
                self.function(f)?;
            }
        }

        Ok(self.decls.clone())
    }

    /// Turns a type as written into a [`Type`].
    fn resolve_type(&self, text: &str) -> Result<Type, TypeError> {
        match text {
            "Int" => return Ok(Type::Int),
            "Float" => return Ok(Type::Float),
            "Bool" => return Ok(Type::Bool),
            "Str" => return Ok(Type::Str),
            "Char" => return Ok(Type::Char),
            _ => {}
        }
        match self.decls.find(text) {
            Some(id) => Ok(Type::Named(id)),
            None => Err(TypeError {
                message: format!("unknown type `{}`", text),
                pos: Pos::START,
            }),
        }
    }

    fn function(&mut self, f: &ir::Fn) -> Result<(), TypeError> {
        let sig = self
            .signatures
            .get(&f.name)
            .cloned()
            .expect("the signature was collected before any body");

        let mut env = Env {
            scopes: vec![HashMap::new()],
            return_type: Some(sig.ret.clone()),
            always_returns: true,
            slot_names: HashMap::new(),
            slot_types: HashMap::new(),
        };

        for (p, ty) in f.params.iter().zip(&sig.params) {
            env.declare(&p.name, ty.clone(), p.slot);
            env.slot_types.insert(p.slot, ty.clone());
        }

        // A body that ends in `return` produces the declared return type; one
        // that falls off the end produces `()`, which is what a missing return
        // means.
        let body_type = self.block(&f.body, &mut env, Some(&sig.ret))?;

        if matches!(body_type, Type::Unit) && sig.ret != Type::Unit {
            // A function declared to return a value must return it on every
            // path. The end of the body is the one path that is easy to miss.
            return self.error(
                f.pos,
                format!(
                    "`{}` is declared to return `{}` but can reach the end without returning",
                    f.name,
                    self.name_of(&sig.ret)
                ),
            );
        }

        Ok(())
    }

    /// Checks a block and returns the type of its value.
    /// Checks a block and returns the type of its value.
    ///
    /// A block whose every statement is a `return` produces the enclosing
    /// function's return type rather than `()`, because it can never fall off
    /// the end. That is what lets a function written as a series of returns
    /// satisfy a declared return type.
    fn block(
        &mut self,
        b: &ir::Block,
        env: &mut Env,
        expected: Option<&Type>,
    ) -> Result<Type, TypeError> {
        env.push();
        let mut returns = false;
        for stmt in &b.stmts {
            self.stmt(stmt, env)?;
            if matches!(stmt, ir::Stmt::Return(_)) {
                returns = true;
            }
        }
        let result = match &b.tail {
            Some(t) => {
                returns = false;
                self.expr(t, env)?
            }
            None if returns => env.return_type.clone().unwrap_or(Type::Unit),
            None => Type::Unit,
        };
        env.pop();

        // A block used where a value is expected must produce a compatible one.
        // This is checked only where there is something to check against,
        // because a block statement in statement position produces nothing.
        if let Some(want) = expected {
            if want != &Type::Unit && !self.compatible(want, &result) {
                return self.error(
                    b.pos,
                    format!(
                        "this block produces `{}` where `{}` is expected",
                        self.name_of(&result),
                        self.name_of(want)
                    ),
                );
            }
        }
        Ok(result)
    }

    fn stmt(&mut self, s: &ir::Stmt, env: &mut Env) -> Result<(), TypeError> {
        match s {
            ir::Stmt::Let { slot, init, pos } => {
                let t = self.expr(init, env)?;
                // The name's type is fixed here and never changes to suit a
                // later use, which is rule 7.
                env.bind_slot(*slot, t, *pos);
                let _ = slot;
                Ok(())
            }
            ir::Stmt::Assign { slot, value, pos } => {
                let current = env.slot_type(*slot);
                let assigned = self.expr(value, env)?;
                match current {
                    Some(want) => {
                        if !self.compatible(&want, &assigned) {
                            return self.error(
                                *pos,
                                format!(
                                    "`{}` has type `{}` and cannot hold a `{}`; `SPEC.md` section 6.1 rule 7 fixes a name's type at its declaration",
                                    env.slot_name(*slot).unwrap_or_else(|| "this name".into()),
                                    self.name_of(&want),
                                    self.name_of(&assigned)
                                ),
                            );
                        }
                        Ok(())
                    }
                    None => self.error(
                        *pos,
                        format!("assignment to slot {} which has no declared type", slot),
                    ),
                }
            }
            ir::Stmt::Return(e) => {
                let t = self.expr(e, env)?;
                if let Some(want) = env.return_type.clone() {
                    if !self.compatible(&want, &t) {
                        return self.error(
                            e.pos(),
                            format!(
                                "this returns `{}` where `{}` is expected",
                                self.name_of(&t),
                                self.name_of(&want)
                            ),
                        );
                    }
                }
                Ok(())
            }
            ir::Stmt::Nop(e) => self.expr(e, env).map(|_| ()),
            ir::Stmt::While { cond, body, .. } => {
                let c = self.expr(cond, env)?;
                if c != Type::Bool {
                    return self.error(
                        cond.pos(),
                        format!(
                            "a `while` condition must be `Bool`, found `{}`",
                            self.name_of(&c)
                        ),
                    );
                }
                env.push();
                self.block(body, env, None)?;
                env.pop();
                Ok(())
            }
            ir::Stmt::For {
                var_slot,
                start,
                end,
                inclusive,
                body,
                pos,
            } => {
                let s = self.expr(start, env)?;
                let e = self.expr(end, env)?;
                if s != Type::Int || e != Type::Int {
                    return self.error(
                        *pos,
                        format!(
                            "a `for` range runs from `{}` to `{}`; both ends must be `Int`",
                            self.name_of(&s),
                            self.name_of(&e)
                        ),
                    );
                }
                env.push();
                env.bind_slot(*var_slot, Type::Int, *pos);
                self.block(body, env, None)?;
                env.pop();
                let _ = inclusive;
                Ok(())
            }
            ir::Stmt::ForEach {
                var_slot,
                iterable,
                body,
                pos,
            } => {
                let it = self.expr(iterable, env)?;
                let item = match &it {
                    Type::Array(t) => (**t).clone(),
                    Type::Tuple(ts) => ts.first().cloned().unwrap_or(Type::Unit),
                    Type::Str => Type::Char,
                    other => {
                        return self.error(
                            *pos,
                            format!(
                                "`for` can walk a list, a tuple or a string, found `{}`",
                                self.name_of(other)
                            ),
                        )
                    }
                };
                env.push();
                env.bind_slot(*var_slot, item, *pos);
                self.block(body, env, None)?;
                env.pop();
                Ok(())
            }
            ir::Stmt::Break | ir::Stmt::Continue => Ok(()),
        }
    }

    fn expr(&mut self, e: &ir::Expr, env: &mut Env) -> Result<Type, TypeError> {
        Ok(match e {
            ir::Expr::Const(c) => const_type(c),
            ir::Expr::Load(slot) => env.slot_type(*slot).unwrap_or(Type::Unit),

            ir::Expr::Store { slot, value, pos } => {
                let want = env.slot_type(*slot);
                let got = self.expr(value, env)?;
                if let Some(want) = want {
                    if !self.compatible(&want, &got) {
                        return self.error(
                            *pos,
                            format!(
                                "cannot store a `{}` where a `{}` is expected",
                                self.name_of(&got),
                                self.name_of(&want)
                            ),
                        );
                    }
                }
                got
            }

            ir::Expr::Unary { neg, operand, pos } => {
                let t = self.expr(operand, env)?;
                if *neg {
                    match t {
                        Type::Int | Type::Float => {}
                        other => {
                            return self.error(
                                *pos,
                                format!(
                                    "`-` needs an `Int` or a `Float`, found `{}`",
                                    self.name_of(&other)
                                ),
                            )
                        }
                    }
                    t
                } else {
                    if t != Type::Bool {
                        return self.error(
                            *pos,
                            format!("`!` needs a `Bool`, found `{}`", self.name_of(&t)),
                        );
                    }
                    t
                }
            }

            ir::Expr::Binary { op, lhs, rhs, pos } => {
                let l = self.expr(lhs, env)?;
                let r = self.expr(rhs, env)?;
                self.binary_op(*op, &l, &r, *pos)?
            }

            ir::Expr::List { items, tuple, pos } => {
                let mut types = Vec::with_capacity(items.len());
                for i in items {
                    types.push(self.expr(i, env)?);
                }
                if *tuple {
                    Type::Tuple(types)
                } else {
                    // Every element of a list has one type. A mixed list is a
                    // common beginner mistake and naming it here is more useful
                    // than letting it fail at run time.
                    let first = types.first().cloned().unwrap_or(Type::Unit);
                    for (i, t) in types.iter().enumerate() {
                        if !self.compatible(&first, t) {
                            return self.error(
                                items[i].pos(),
                                format!(
                                    "this list mixes `{}` and `{}`; every element of a list has one type",
                                    self.name_of(&first),
                                    self.name_of(t)
                                ),
                            );
                        }
                    }
                    let _ = pos;
                    Type::Array(Box::new(first))
                }
            }

            ir::Expr::Index { base, index, pos } => {
                let b = self.expr(base, env)?;
                let i = self.expr(index, env)?;
                if i != Type::Int {
                    return self.error(
                        index.pos(),
                        format!("an index must be an `Int`, found `{}`", self.name_of(&i)),
                    );
                }
                match b {
                    Type::Array(t) => *t,
                    Type::Tuple(ts) => ts.first().cloned().unwrap_or(Type::Unit),
                    Type::Str => Type::Char,
                    other => {
                        return self.error(
                            *pos,
                            format!("`{}` cannot be indexed", self.name_of(&other)),
                        )
                    }
                }
            }

            ir::Expr::Field { base, name, pos } => {
                let b = self.expr(base, env)?;
                match b {
                    Type::Named(id) => {
                        let ty = self.decls.field_type(id, name).cloned();
                        match ty {
                            Some(t) => t,
                            None => {
                                return self.error(
                                    *pos,
                                    format!(
                                        "`{}` has no field `{}`",
                                        self.decls.get(id).name,
                                        name
                                    ),
                                )
                            }
                        }
                    }
                    other => {
                        return self.error(
                            *pos,
                            format!(
                                "cannot read a field of `{}`, which is not a struct",
                                self.name_of(&other)
                            ),
                        )
                    }
                }
            }

            ir::Expr::Try { inner, pos } => {
                let t = self.expr(inner, env)?;
                match t {
                    Type::Result(ok, _) => *ok,
                    Type::Option(inner_t) => *inner_t,
                    other => {
                        return self.error(
                            *pos,
                            format!(
                                "`?` needs a `Result` or an `Option` to work on, found `{}`",
                                self.name_of(&other)
                            ),
                        )
                    }
                }
            }

            ir::Expr::BlockValue(b) => self.block(b, env, None)?,

            ir::Expr::If {
                cond,
                then,
                otherwise,
                pos,
            } => {
                let c = self.expr(cond, env)?;
                if c != Type::Bool {
                    return self.error(
                        cond.pos(),
                        format!(
                            "an `if` condition must be `Bool`, found `{}`",
                            self.name_of(&c)
                        ),
                    );
                }
                let t = self.block(then, env, None)?;
                match otherwise {
                    None => Type::Unit,
                    Some(other) => {
                        let e = self.expr(other, env)?;
                        // Both branches must agree, or the value of the `if`
                        // depends on which way it went.
                        if !self.compatible(&t, &e) && t != Type::Unit && e != Type::Unit {
                            return self.error(
                                *pos,
                                format!(
                                    "the two branches produce `{}` and `{}`; an `if` used as a value needs one type",
                                    self.name_of(&t),
                                    self.name_of(&e)
                                ),
                            );
                        }
                        if t == Type::Unit {
                            e
                        } else {
                            t
                        }
                    }
                }
            }

            ir::Expr::Call {
                target, args, pos, ..
            } => {
                let mut arg_types = Vec::with_capacity(args.len());
                for a in args {
                    arg_types.push(self.expr(a, env)?);
                }
                self.call(target, &arg_types, *pos)?
            }

            ir::Expr::Match {
                scrutinee,
                arms,
                pos,
            } => {
                let subject = self.expr(scrutinee, env)?;
                let mut result: Option<Type> = None;
                let mut wildcard = false;
                // The variants an enum `match` names, so that one naming every
                // variant is recognised as exhaustive without a `_` arm.
                let mut covered: Vec<String> = Vec::new();
                let is_enum = matches!(subject, Type::Named(id) if self.decls.get(id).is_enum);

                for arm in arms {
                    if !self.pattern_matches(&arm.pattern, &subject, env, *pos)? {
                        // An arm whose pattern cannot match the subject is dead
                        // code, and is worth naming rather than ignoring.
                        return self.error(
                            *pos,
                            format!("this arm can never match a `{}`", self.name_of(&subject)),
                        );
                    }
                    match &arm.pattern {
                        ir::Pattern::Wildcard => wildcard = true,
                        ir::Pattern::Variant { variant, .. } => covered.push(variant.clone()),
                        _ => {}
                    }
                    let t = self.expr(&arm.body, env)?;
                    match &result {
                        None => result = Some(t),
                        Some(first) => {
                            if !self.compatible(first, &t) {
                                return self.error(
                                    arm.body.pos(),
                                    format!(
                                        "this arm produces `{}` where an earlier arm produces `{}`",
                                        self.name_of(&t),
                                        self.name_of(first)
                                    ),
                                );
                            }
                        }
                    }
                }

                // A `match` on an enum is exhaustive when it names every
                // variant. Any other subject needs a `_`, because the checker
                // cannot see which values of a `Str` or an `Int` will arrive.
                if !wildcard {
                    let covers_all = is_enum && self.covers_every_variant(&subject, &covered);
                    if !covers_all {
                        return self.error(
                            *pos,
                            format!(
                                "this `match` has no `_` arm, so a `{}` that matches nothing has no value",
                                self.name_of(&subject)
                            ),
                        );
                    }
                }
                result.unwrap_or(Type::Unit)
            }
        })
    }

    fn binary_op(&self, op: ast::BinOp, l: &Type, r: &Type, pos: Pos) -> Result<Type, TypeError> {
        use ast::BinOp::*;
        match op {
            Eq | Ne => {
                // Equality needs both sides to be the same type. Comparing an
                // `Int` to a `Float` is almost always a mistake, and rule 4
                // forbids mixing them.
                if !self.compatible(l, r) {
                    return self.error(
                        pos,
                        format!(
                            "cannot compare `{}` with `{}`; the two types must match",
                            self.name_of(l),
                            self.name_of(r)
                        ),
                    );
                }
                Ok(Type::Bool)
            }
            And | Or => {
                if l != &Type::Bool || r != &Type::Bool {
                    return self.error(
                        pos,
                        format!(
                            "`{}` needs two `Bool` values, found `{}` and `{}`",
                            op.spelling(),
                            self.name_of(l),
                            self.name_of(r)
                        ),
                    );
                }
                Ok(Type::Bool)
            }
            Lt | Le | Gt | Ge => {
                if !ordered(l, r) {
                    return self.error(
                        pos,
                        format!(
                            "`{}` cannot compare `{}` with `{}`",
                            op.spelling(),
                            self.name_of(l),
                            self.name_of(r)
                        ),
                    );
                }
                Ok(Type::Bool)
            }
            Add | Sub | Mul | Div | Rem => {
                match arith_of(l, r) {
                    Some(crate::types::Arith::Int) => Ok(Type::Int),
                    Some(crate::types::Arith::Float) => Ok(Type::Float),
                    Some(crate::types::Arith::Str) => {
                        if op == Add {
                            Ok(Type::Str)
                        } else {
                            self.error(
                                pos,
                                format!(
                                    "`{}` is not defined for `Str`; only `+` is",
                                    op.spelling()
                                ),
                            )
                        }
                    }
                    None => self.error(
                        pos,
                        format!(
                            "`{}` cannot be applied to `{}` and `{}`; `SPEC.md` section 6.1 rule 4 forbids mixing `Int` and `Float` without an `as` cast",
                            op.spelling(),
                            self.name_of(l),
                            self.name_of(r)
                        ),
                    ),
                }
            }
        }
    }

    fn call(
        &mut self,
        target: &ir::CallTarget,
        args: &[Type],
        pos: Pos,
    ) -> Result<Type, TypeError> {
        match target {
            ir::CallTarget::Function(name) => {
                let sig = self.signatures.get(name).cloned();
                match sig {
                    None => self.error(pos, format!("call to unknown function `{}`", name)),
                    Some(sig) => {
                        if sig.params.len() != args.len() {
                            return self.error(
                                pos,
                                format!(
                                    "`{}` takes {} value{}, found {}",
                                    name,
                                    sig.params.len(),
                                    if sig.params.len() == 1 { "" } else { "s" },
                                    args.len()
                                ),
                            );
                        }
                        for (i, (want, got)) in sig.params.iter().zip(args).enumerate() {
                            if !self.compatible(want, got) {
                                return self.error(
                                    pos,
                                    format!(
                                        "argument {} of `{}` is `{}` where `{}` is expected",
                                        i + 1,
                                        name,
                                        self.name_of(got),
                                        self.name_of(want)
                                    ),
                                );
                            }
                        }
                        Ok(sig.ret)
                    }
                }
            }

            ir::CallTarget::Struct(name) => {
                let id = match self.decls.find(name) {
                    Some(id) => id,
                    None => return self.error(pos, format!("unknown struct `{}`", name)),
                };
                let fields = self.decls.get(id).fields.clone();
                if fields.len() != args.len() {
                    return self.error(
                        pos,
                        format!(
                            "`{}` has {} field{}, found {} value{}",
                            name,
                            fields.len(),
                            if fields.len() == 1 { "" } else { "s" },
                            args.len(),
                            if args.len() == 1 { "" } else { "s" }
                        ),
                    );
                }
                for (i, (fname, want)) in fields.iter().enumerate() {
                    if !self.compatible(want, &args[i]) {
                        return self.error(
                            pos,
                            format!(
                                "field `{}` of `{}` is `{}` where `{}` is expected",
                                fname,
                                name,
                                self.name_of(&args[i]),
                                self.name_of(want)
                            ),
                        );
                    }
                }
                Ok(Type::Named(id))
            }

            ir::CallTarget::Variant { ty, variant } => {
                let id = match self.decls.find(ty) {
                    Some(id) => id,
                    None => return self.error(pos, format!("unknown enum `{}`", ty)),
                };
                let decl = match self.decls.variant(id, variant) {
                    Some(v) => v.clone(),
                    None => {
                        return self
                            .error(pos, format!("enum `{}` has no variant `{}`", ty, variant))
                    }
                };
                if decl.payloads.len() != args.len() {
                    return self.error(
                        pos,
                        format!(
                            "`{}.{}` carries {} value{}, found {}",
                            ty,
                            variant,
                            decl.payloads.len(),
                            if decl.payloads.len() == 1 { "" } else { "s" },
                            args.len()
                        ),
                    );
                }
                for (i, want) in decl.payloads.iter().enumerate() {
                    if !self.compatible(want, &args[i]) {
                        return self.error(
                            pos,
                            format!(
                                "value {} of `{}.{}` is `{}` where `{}` is expected",
                                i + 1,
                                ty,
                                variant,
                                self.name_of(&args[i]),
                                self.name_of(want)
                            ),
                        );
                    }
                }
                Ok(Type::Named(id))
            }

            ir::CallTarget::Builtin(name) => {
                let sig = self.builtins.get(name).cloned();
                match sig {
                    None => self.error(pos, format!("unknown function `{}`", name)),
                    Some((params, ret)) => {
                        let variadic = name == "print" || name == "println";
                        if !variadic && params.len() != args.len() {
                            return self.error(
                                pos,
                                format!(
                                    "`{}` takes {} value{}, found {}",
                                    name,
                                    params.len(),
                                    if params.len() == 1 { "" } else { "s" },
                                    args.len()
                                ),
                            );
                        }
                        // `print` and `println` take any number of values, so
                        // their arity is checked rather than their types. Any
                        // type may be printed, so nothing more is checked here.
                        if name == "print" || name == "println" {
                            return Ok(ret);
                        }
                        for (i, (want, got)) in params.iter().zip(args).enumerate() {
                            if !self.compatible(want, got) {
                                return self.error(
                                    pos,
                                    format!(
                                        "argument {} of `{}` is `{}` where `{}` is expected",
                                        i + 1,
                                        name,
                                        self.name_of(got),
                                        self.name_of(want)
                                    ),
                                );
                            }
                        }
                        Ok(ret)
                    }
                }
            }
        }
    }

    /// Whether a pattern can match a value of the given type.
    ///
    /// This is what catches an arm that can never run, which is otherwise a
    /// silent mistake.
    fn pattern_matches(
        &mut self,
        p: &ir::Pattern,
        subject: &Type,
        env: &mut Env,
        _pos: Pos,
    ) -> Result<bool, TypeError> {
        Ok(match p {
            ir::Pattern::Wildcard => true,
            ir::Pattern::Binding(b) => {
                // A binding takes the type of what it matched.
                env.bind_slot(b.slot, subject.clone(), _pos);
                true
            }
            ir::Pattern::Literal(c) => const_type(c) == *subject,
            ir::Pattern::Tuple(items) => match subject {
                Type::Tuple(ts) => {
                    if ts.len() != items.len() {
                        return Ok(false);
                    }
                    for (p, t) in items.iter().zip(ts) {
                        if !self.pattern_matches(p, t, env, _pos)? {
                            return Ok(false);
                        }
                    }
                    true
                }
                _ => false,
            },
            ir::Pattern::Variant {
                ty,
                variant,
                bindings,
            } => {
                let id = match self.decls.find(ty) {
                    Some(id) => id,
                    None => return Ok(false),
                };
                // The pattern must name an enum, and the value must be one.
                if subject != &Type::Named(id) {
                    return Ok(false);
                }
                match self.decls.variant(id, variant) {
                    None => false,
                    Some(decl) => {
                        if decl.payloads.len() != bindings.len() {
                            return Ok(false);
                        }
                        // Each binding takes the type of the payload it names.
                        for (b, want) in bindings.iter().zip(&decl.payloads) {
                            env.bind_slot(b.slot, want.clone(), Pos::START);
                        }
                        true
                    }
                }
            }
        })
    }

    /// Whether a set of variant names covers every variant of an enum.
    fn covers_every_variant(&self, subject: &Type, covered: &[String]) -> bool {
        let id = match subject {
            Type::Named(id) => *id,
            _ => return false,
        };
        let decl = self.decls.get(id);
        if !decl.is_enum || decl.variants.is_empty() {
            return false;
        }
        decl.variants
            .iter()
            .all(|v| covered.iter().any(|c| c == &v.name))
    }

    /// Whether a value of type `got` may be used where `want` is expected.
    ///
    /// Rule 4 is the interesting one: an `Int` is not a `Float` and the two
    /// mix only through a cast.
    fn compatible(&self, want: &Type, got: &Type) -> bool {
        if want == got {
            return true;
        }
        // `()` is the type of a statement, and any statement is acceptable
        // where one is expected.
        if *want == Type::Unit {
            return true;
        }
        false
    }

    /// A readable name for a type, for a diagnostic.
    pub fn name_of(&self, t: &Type) -> String {
        match t {
            Type::Named(id) => self.decls.get(*id).name.clone(),
            other => other.to_string(),
        }
    }
}

fn const_type(c: &ir::Const) -> Type {
    match c {
        ir::Const::Int(_) => Type::Int,
        ir::Const::Float(_) => Type::Float,
        ir::Const::Str(_) => Type::Str,
        ir::Const::Char(_) => Type::Char,
        ir::Const::Bool(_) => Type::Bool,
    }
}

/// The signature of every builtin.
///
/// `print` and `println` take any number of values, so their parameter list is
/// empty and their arity is checked separately.
fn builtin_signatures() -> HashMap<String, (Vec<Type>, Type)> {
    let mut m = HashMap::new();
    m.insert("print".to_string(), (vec![], Type::Unit));
    m.insert("println".to_string(), (vec![], Type::Unit));
    m.insert("int_to_string".to_string(), (vec![Type::Int], Type::Str));
    m.insert(
        "float_to_string".to_string(),
        (vec![Type::Float], Type::Str),
    );
    m.insert("string_to_int".to_string(), (vec![Type::Str], Type::Int));
    m.insert("sqrt".to_string(), (vec![Type::Float], Type::Float));
    m.insert("min".to_string(), (vec![Type::Int, Type::Int], Type::Int));
    m.insert("max".to_string(), (vec![Type::Int, Type::Int], Type::Int));
    m.insert("now_ns".to_string(), (vec![], Type::Int));
    m
}

/// The names in scope while checking a function.
struct Env {
    scopes: Vec<HashMap<String, Scoped>>,
    return_type: Option<Type>,
    /// Whether the statement being checked ends every path with a `return`.
    /// A block that always returns produces its declared type rather than `()`.
    always_returns: bool,
    /// The slot each name was given, so a diagnostic can name it.
    slot_names: HashMap<usize, String>,
    /// The type of every slot this function has introduced, so a read after
    /// the binding left scope still has a type.
    slot_types: HashMap<usize, Type>,
}

/// One name in scope: its type, and whether it was written `var`.
#[derive(Debug, Clone)]
struct Scoped {
    ty: Type,
    slot: usize,
}

impl Env {
    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop(&mut self) {
        self.scopes.pop();
    }

    fn declare(&mut self, name: &str, ty: Type, slot: usize) {
        self.scopes
            .last_mut()
            .expect("a scope is always open")
            .insert(name.to_string(), Scoped { ty, slot });
        self.slot_names.insert(slot, name.to_string());
    }

    /// Binds a slot that a `let` introduced.
    ///
    /// The lowering pass resolves a name to a slot but does not carry the name
    /// onto the statement, so the slot is recorded directly. Keeping the type
    /// in a flat table as well as in the scope means a later read of the same
    /// slot finds the type even where the binding has gone out of scope.
    fn bind_slot(&mut self, slot: usize, ty: Type, _pos: Pos) {
        self.slot_types.insert(slot, ty.clone());
        if let Some(name) = self.slot_names.get(&slot).cloned() {
            self.scopes
                .last_mut()
                .expect("a scope is always open")
                .insert(name, Scoped { ty, slot });
        }
    }

    /// The type of a slot, from the scope if it is still open and from the flat
    /// table otherwise.
    fn known_slot_type(&self, slot: usize) -> Option<Type> {
        for scope in self.scopes.iter().rev() {
            for entry in scope.values() {
                if entry.slot == slot {
                    return Some(entry.ty.clone());
                }
            }
        }
        self.slot_types.get(&slot).cloned()
    }

    fn slot_type(&self, slot: usize) -> Option<Type> {
        self.known_slot_type(slot)
    }

    fn slot_name(&self, slot: usize) -> Option<String> {
        self.slot_names.get(&slot).cloned()
    }
}
