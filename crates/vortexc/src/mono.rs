//! Monomorphisation pass (SPEC.md section 6.3).
//!
//! This runs after parsing and before lowering. It consumes trait
//! declarations and `impl` blocks and resolves trait method calls (`x.m()`) to
//! concrete calls. The program that reaches lowering, the checker, the
//! bytecode VM, the tree interpreter and the C emitter therefore contains only
//! concrete functions and concrete calls: no type parameters, no trait objects
//! and no dynamic dispatch.
//!
//! Trait method calls are resolved on concrete types only. A generic function
//! body is transformed in place (method calls inside it are resolved) but
//! generic instantiation itself is deferred until a future stage.

use crate::ast::*;
use crate::ir::LowerError;
use crate::span::Pos;
use std::collections::{HashMap, HashSet};

/// The monomorphised program. Trait and impl blocks are consumed: the output
/// contains only the original concrete functions plus the trait impl methods
/// emitted as concrete functions.
pub fn monomorphise(program: &Program) -> Result<Program, LowerError> {
    let ctx = Ctx::build(program);
    let mut m = Mono {
        ctx,
        items: Vec::new(),
        sigs: HashMap::new(),
        emitted: HashSet::new(),
        transforming: HashSet::new(),
    };

    // Structs and enums pass through unchanged; they are needed by both the
    // lowering and the C emitter for field/variant resolution.
    for item in &program.items {
        if matches!(item, Item::Struct(_) | Item::Enum(_)) {
            m.items.push(item.clone());
        }
    }

    // Impl methods are concrete per impl (the receiver type is fixed), so emit
    // them first: their signatures are then available for trait-method call
    // resolution in function bodies.
    for decl in &program.impls {
        for method in &decl.methods {
            let name = m.mangle_impl_method(decl, &method.name);
            let subst = Substitution::self_for(&decl.self_ty);
            m.emit_fn(&method, &subst, &name, decl.pos)?;
        }
    }

    // Non-generic top-level functions are emitted under their original names.
    // Generic functions are left as-is for now; concrete types only.
    for item in &program.items {
        if let Item::Function(f) = item {
            m.emit_fn(&f.node, &Substitution::empty(), &f.node.name, f.pos)?;
        }
    }

    Ok(Program {
        items: m.items,
        traits: Vec::new(),
        impls: Vec::new(),
    })
}

#[allow(dead_code)]
struct Ctx {
    /// Struct names, for field-type inference.
    structs: HashMap<String, Vec<Field>>,
    /// Enum name -> variants, for variant construction resolution.
    enums: HashMap<String, Vec<Variant>>,
    /// Function name -> template (generic parameters, params, ret, body).
    fns: HashMap<String, FnTemplate>,
    /// Trait name -> declaration (methods).
    traits: HashMap<String, TraitDecl>,
    /// Impl blocks, searched by method name and receiver type.
    impls: Vec<ImplDecl>,
}

#[derive(Clone)]
#[allow(dead_code)]
struct FnTemplate {
    generics: Vec<GenericParam>,
    params: Vec<Param>,
    ret: Option<TypeExpr>,
    body: Block,
}

/// A name -> concrete-type substitution, applied when a type parameter is
/// replaced by a real type at a call site. Currently only used for `Self`.
#[derive(Default, Clone)]
struct Substitution {
    map: Vec<(String, TypeExpr)>,
}

impl Substitution {
    fn empty() -> Self {
        Substitution { map: Vec::new() }
    }

    /// Binds every occurrence of `Self` to the impl's receiver type.
    fn self_for(self_ty: &str) -> Self {
        Substitution {
            map: vec![("Self".to_string(), TypeExpr::Named(self_ty.to_string()))],
        }
    }

    fn apply(&self, ty: &TypeExpr) -> TypeExpr {
        match ty {
            TypeExpr::Named(n) => {
                for (k, v) in self.map.iter().rev() {
                    if k == n {
                        return v.clone();
                    }
                }
                ty.clone()
            }
        }
    }

    fn apply_param(&self, p: &Param) -> Param {
        Param {
            name: p.name.clone(),
            ty: self.apply(&p.ty),
            mutable: p.mutable,
        }
    }
}

/// The type of an emitted concrete function, for return-type inference.
#[allow(dead_code)]
struct Sig {
    params: Vec<TypeExpr>,
    ret: Option<TypeExpr>,
}

struct Mono {
    ctx: Ctx,
    /// The output items: structs, enums, and concrete function instances.
    items: Vec<Item>,
    /// Name -> signature of every emitted concrete function (original and
    /// trait-impl), for return-type inference.
    sigs: HashMap<String, Sig>,
    /// Names already emitted, so each function is emitted once.
    emitted: HashSet<String>,
    /// Names whose body is being transformed right now, to break recursion.
    transforming: HashSet<String>,
}

impl Ctx {
    fn build(program: &Program) -> Self {
        let mut structs = HashMap::new();
        let mut enums = HashMap::new();
        let mut fns = HashMap::new();
        for item in &program.items {
            match item {
                Item::Struct(s) => {
                    structs.insert(s.node.name.clone(), s.node.fields.clone());
                }
                Item::Enum(e) => {
                    enums.insert(e.node.name.clone(), e.node.variants.clone());
                }
                Item::Function(f) => {
                    fns.insert(
                        f.node.name.clone(),
                        FnTemplate {
                            generics: f.node.generics.clone(),
                            params: f.node.params.clone(),
                            ret: f.node.ret.clone(),
                            body: f.node.body.clone(),
                        },
                    );
                }
            }
        }
        let traits = program
            .traits
            .iter()
            .map(|t| (t.name.clone(), t.clone()))
            .collect();
        Ctx {
            structs,
            enums,
            fns,
            traits,
            impls: program.impls.clone(),
        }
    }
}

impl Mono {
    /// Emits a concrete function under `name`, transforming its body with
    /// `subst` so that type arguments and trait calls are resolved. Idempotent:
    /// a function is emitted once, and a recursive call to it resolves against
    /// its signature without re-transforming the body.
    fn emit_fn(
        &mut self,
        f: &FnDecl,
        subst: &Substitution,
        name: &str,
        pos: Pos,
    ) -> Result<(), LowerError> {
        if self.emitted.contains(name) || self.transforming.contains(name) {
            return Ok(());
        }
        self.transforming.insert(name.to_string());

        // Register the signature before transforming the body, so a recursive
        // reference to this function resolves to its return type.
        let params: Vec<TypeExpr> = f.params.iter().map(|p| subst.apply(&p.ty)).collect();
        let ret = f.ret.as_ref().map(|r| subst.apply(r));
        self.sigs.insert(
            name.to_string(),
            Sig {
                params: params.clone(),
                ret: ret.clone(),
            },
        );

        let mut env: Vec<HashMap<String, TypeExpr>> = Vec::new();
        env.push(HashMap::new());
        for p in &f.params {
            env[0].insert(p.name.clone(), subst.apply(&p.ty));
        }
        let (body, _) = self.transform_block(&f.body, subst, &mut env)?;

        self.items.push(Item::Function(Spanned::new(
            FnDecl {
                name: name.to_string(),
                generics: Vec::new(),
                params: f.params.iter().map(|p| subst.apply_param(p)).collect(),
                ret,
                body,
            },
            pos,
        )));
        self.emitted.insert(name.to_string());
        self.transforming.remove(name);
        Ok(())
    }

    /// Mangles an impl method name: `AddOne` impl for `Int`, method `add_one`
    /// -> `vortex_AddOne_Int_add_one`. Inherent impls (no trait) use the type
    /// name alone: `vortex_Int_bar`.
    fn mangle_impl_method(&self, decl: &ImplDecl, method: &str) -> String {
        match &decl.trait_ty {
            Some(trait_ty) => format!("vortex_{}_{}_{}", trait_ty, decl.self_ty, method),
            None => format!("vortex_{}_{}", decl.self_ty, method),
        }
    }

    /// Resolves a desugared call `callee(args)` where a trait method was written
    /// `base.callee(args)` and the receiver is `args[0]`. Returns the rewritten
    /// callee name and the call's result type.
    fn transform_call(
        &mut self,
        callee: &str,
        args: &[Expr],
        subst: &Substitution,
        env: &mut Vec<HashMap<String, TypeExpr>>,
        pos: Pos,
    ) -> Result<(Expr, TypeExpr), LowerError> {
        // Transform the arguments first, so any trait call among them is
        // resolved before this call's receiver type is determined.
        let mut arg_exprs = Vec::with_capacity(args.len());
        let mut arg_types = Vec::with_capacity(args.len());
        for a in args {
            let (e, t) = self.transform_expr(a, subst, env)?;
            arg_exprs.push(e);
            arg_types.push(t);
        }

        // 1) A named function (concrete or generic). Leave the callee alone.
        if let Some(tpl) = self.ctx.fns.get(callee) {
            return Ok((
                Expr::new(
                    pos,
                    ExprKind::Call {
                        callee: callee.to_string(),
                        args: arg_exprs,
                    },
                ),
                tpl.ret
                    .as_ref()
                    .map(|r| subst.apply(r))
                    .unwrap_or(TypeExpr::Named("Unit".to_string())),
            ));
        }

        // 2) A builtin function. Leave the callee alone.
        if crate::lower::BUILTINS.contains(&callee) {
            return Ok((
                Expr::new(
                    pos,
                    ExprKind::Call {
                        callee: callee.to_string(),
                        args: arg_exprs,
                    },
                ),
                builtin_return_type(callee, &arg_types),
            ));
        }

        // 3) A trait-method function whose name is already mangled. Leave it
        // alone — it was emitted by emit_fn and is in the signature table.
        if self.sigs.contains_key(callee) {
            return Ok((
                Expr::new(
                    pos,
                    ExprKind::Call {
                        callee: callee.to_string(),
                        args: arg_exprs,
                    },
                ),
                self.sigs[callee]
                    .ret
                    .clone()
                    .unwrap_or(TypeExpr::Named("Unit".to_string())),
            ));
        }

        // 4) A desugared `base.name(args)` where `name` is not a function: a
        // trait method call. Resolve the receiver's concrete type and find the
        // matching impl method.
        if !arg_exprs.is_empty() {
            let receiver_type = arg_types.get(0).cloned();
            if let Some(rtype) = receiver_type {
                let (mangled, ret_ty) = self.resolve_trait_method(callee, &rtype, pos)?;
                let self_subst = Substitution::self_for(rtype.name());
                let result_ty = ret_ty
                    .map(|r| self_subst.apply(&r))
                    .unwrap_or(TypeExpr::Named("Unit".to_string()));
                return Ok((
                    Expr::new(
                        pos,
                        ExprKind::Call {
                            callee: mangled,
                            args: arg_exprs,
                        },
                    ),
                    result_ty,
                ));
            }
        }

        // 5) Not a function, not a method — the callee is unknown.
        Err(LowerError {
            message: format!("call to unknown function `{}`", callee),
            pos,
        })
    }

    /// Searches impl blocks for `receiver_type` and returns the mangled name
    /// of `method` along with its declared return type.
    fn resolve_trait_method(
        &self,
        method: &str,
        receiver_type: &TypeExpr,
        pos: Pos,
    ) -> Result<(String, Option<TypeExpr>), LowerError> {
        let type_name = receiver_type.name();
        for decl in &self.ctx.impls {
            if decl.self_ty == type_name {
                for fn_decl in &decl.methods {
                    if fn_decl.name == method {
                        let mangled = self.mangle_impl_method(decl, method);
                        let self_subst = Substitution::self_for(type_name);
                        let ret = fn_decl.ret.as_ref().map(|r| self_subst.apply(r));
                        return Ok((mangled, ret));
                    }
                }
            }
        }
        Err(LowerError {
            message: format!("no method `{}` found for type `{}`", method, type_name),
            pos,
        })
    }

    /// Walks a block, transforming every statement and the tail expression.
    /// Returns the transformed block and the type of its tail (Unit if there is
    /// no tail).
    fn transform_block(
        &mut self,
        body: &Block,
        subst: &Substitution,
        env: &mut Vec<HashMap<String, TypeExpr>>,
    ) -> Result<(Block, TypeExpr), LowerError> {
        let mut new_stmts = Vec::with_capacity(body.stmts.len());
        env.push(HashMap::new());

        for stmt in &body.stmts {
            new_stmts.push(self.transform_stmt(stmt, subst, env)?);
        }

        let (new_tail, tail_ty) = match &body.tail {
            Some(t) => {
                let (e, ty) = self.transform_expr(t, subst, env)?;
                (Some(Box::new(e)), ty)
            }
            None => (None, TypeExpr::Named("Unit".to_string())),
        };

        env.pop();

        Ok((
            Block {
                pos: body.pos,
                stmts: new_stmts,
                tail: new_tail,
            },
            tail_ty,
        ))
    }

    fn transform_stmt(
        &mut self,
        stmt: &Stmt,
        subst: &Substitution,
        env: &mut Vec<HashMap<String, TypeExpr>>,
    ) -> Result<Stmt, LowerError> {
        let pos = stmt.pos;
        let kind = match &stmt.kind {
            StmtKind::Let {
                name,
                mutable,
                init,
            } => {
                let (e, ty) = self.transform_expr(init, subst, env)?;
                env.last_mut().unwrap().insert(name.clone(), ty);
                StmtKind::Let {
                    name: name.clone(),
                    mutable: *mutable,
                    init: e,
                }
            }
            StmtKind::Return(e) => {
                let (e2, _) = self.transform_expr(e, subst, env)?;
                StmtKind::Return(e2)
            }
            StmtKind::Expr(e) => {
                let (e2, _) = self.transform_expr(e, subst, env)?;
                StmtKind::Expr(e2)
            }
            StmtKind::Block(b) => {
                let (inner, _) = self.transform_block(b, subst, env)?;
                StmtKind::Block(inner)
            }
            StmtKind::While { cond, body } => {
                let (c, _) = self.transform_expr(cond, subst, env)?;
                let (b, _) = self.transform_block(body, subst, env)?;
                StmtKind::While { cond: c, body: b }
            }
            StmtKind::For {
                var,
                start,
                end,
                inclusive,
                body,
            } => {
                let (s, sty) = self.transform_expr(start, subst, env)?;
                let _ = sty;
                let (e, _ety) = match end {
                    Some(e) => {
                        let (e2, _) = self.transform_expr(e, subst, env)?;
                        (Some(e2), ())
                    }
                    None => (None, ()),
                };
                // In a range `a..b` the loop variable is always Int.
                let var_type = TypeExpr::Named("Int".to_string());
                env.last_mut().unwrap().insert(var.clone(), var_type);
                let (b, _) = self.transform_block(body, subst, env)?;
                StmtKind::For {
                    var: var.clone(),
                    start: s,
                    end: e,
                    inclusive: *inclusive,
                    body: b,
                }
            }
            StmtKind::Break => StmtKind::Break,
            StmtKind::Continue => StmtKind::Continue,
        };
        Ok(Stmt { pos, kind })
    }

    /// Walks an expression, transforming sub-expressions and trait method
    /// calls. Returns the transformed expression and its inferred type.
    fn transform_expr(
        &mut self,
        e: &Expr,
        subst: &Substitution,
        env: &mut Vec<HashMap<String, TypeExpr>>,
    ) -> Result<(Expr, TypeExpr), LowerError> {
        let pos = e.pos;
        let (kind, ty) = match &e.kind {
            ExprKind::Int(_) => (e.kind.clone(), TypeExpr::Named("Int".to_string())),
            ExprKind::Float(_) => (e.kind.clone(), TypeExpr::Named("Float".to_string())),
            ExprKind::Bool(_) => (e.kind.clone(), TypeExpr::Named("Bool".to_string())),
            ExprKind::Char(_) => (e.kind.clone(), TypeExpr::Named("Char".to_string())),
            ExprKind::Str(_) => (e.kind.clone(), TypeExpr::Named("Str".to_string())),
            ExprKind::Ident(n) => {
                let t = self.lookup_type(n, pos, env)?;
                (e.kind.clone(), t)
            }
            ExprKind::Call { callee, args } => {
                return self.transform_call(callee, args, subst, env, pos);
            }
            ExprKind::VariantCall {
                ty: ty_name,
                variant,
                args,
            } => {
                let mut new_args = Vec::with_capacity(args.len());
                for a in args {
                    let (e2, _) = self.transform_expr(a, subst, env)?;
                    new_args.push(e2);
                }
                (
                    ExprKind::VariantCall {
                        ty: ty_name.clone(),
                        variant: variant.clone(),
                        args: new_args,
                    },
                    TypeExpr::Named(ty_name.clone()),
                )
            }
            ExprKind::VariantRecord {
                ty: ty_name,
                variant,
                fields,
            } => {
                let mut new_fields = Vec::with_capacity(fields.len());
                for (name, f) in fields {
                    let (e2, _) = self.transform_expr(f, subst, env)?;
                    new_fields.push((name.clone(), e2));
                }
                (
                    ExprKind::VariantRecord {
                        ty: ty_name.clone(),
                        variant: variant.clone(),
                        fields: new_fields,
                    },
                    TypeExpr::Named(ty_name.clone()),
                )
            }
            ExprKind::Record {
                ty: ty_name,
                fields,
            } => {
                let mut new_fields = Vec::with_capacity(fields.len());
                for (name, f) in fields {
                    let (e2, _) = self.transform_expr(f, subst, env)?;
                    new_fields.push((name.clone(), e2));
                }
                (
                    ExprKind::Record {
                        ty: ty_name.clone(),
                        fields: new_fields,
                    },
                    TypeExpr::Named(ty_name.clone()),
                )
            }
            ExprKind::Variant { ty: _, variant: _ } => {
                // A bare variant constructor value (e.g. `Shape.circle`). Its
                // type is the enum type, but we only need it for method
                // resolution, which does not apply here.
                (e.kind.clone(), TypeExpr::Named("Unit".to_string()))
            }
            ExprKind::Field(base, name) => {
                let (b, bty) = self.transform_expr(base, subst, env)?;
                let field_ty = self.field_type(&bty, name);
                (ExprKind::Field(Box::new(b), name.clone()), field_ty)
            }
            ExprKind::Index(arr, idx) => {
                let (a, _) = self.transform_expr(arr, subst, env)?;
                let (i, _) = self.transform_expr(idx, subst, env)?;
                (
                    ExprKind::Index(Box::new(a), Box::new(i)),
                    TypeExpr::Named("Unit".to_string()),
                )
            }
            ExprKind::Tuple(items) => {
                let mut new_items = Vec::with_capacity(items.len());
                for i in items {
                    let (e2, _) = self.transform_expr(i, subst, env)?;
                    new_items.push(e2);
                }
                (
                    ExprKind::Tuple(new_items),
                    TypeExpr::Named("Unit".to_string()),
                )
            }
            ExprKind::Paren(inner) => {
                let (e2, ty) = self.transform_expr(inner, subst, env)?;
                (ExprKind::Paren(Box::new(e2)), ty)
            }
            ExprKind::Block(b) => {
                let (block, bt) = self.transform_block(b, subst, env)?;
                (ExprKind::Block(block), bt)
            }
            ExprKind::Try(inner) => {
                let (e2, ty) = self.transform_expr(inner, subst, env)?;
                (ExprKind::Try(Box::new(e2)), ty)
            }
            ExprKind::Array(items) => {
                let mut new_items = Vec::with_capacity(items.len());
                for i in items {
                    let (e2, _) = self.transform_expr(i, subst, env)?;
                    new_items.push(e2);
                }
                (
                    ExprKind::Array(new_items),
                    TypeExpr::Named("Unit".to_string()),
                )
            }
            ExprKind::Repeat { value, count } => {
                let (v, _) = self.transform_expr(value, subst, env)?;
                let (c, _) = self.transform_expr(count, subst, env)?;
                (
                    ExprKind::Repeat {
                        value: Box::new(v),
                        count: Box::new(c),
                    },
                    TypeExpr::Named("Unit".to_string()),
                )
            }
            ExprKind::Neg(inner) => {
                let (e2, ty) = self.transform_expr(inner, subst, env)?;
                (ExprKind::Neg(Box::new(e2)), ty)
            }
            ExprKind::Not(inner) => {
                let (e2, _) = self.transform_expr(inner, subst, env)?;
                (
                    ExprKind::Not(Box::new(e2)),
                    TypeExpr::Named("Bool".to_string()),
                )
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let (l, lt) = self.transform_expr(lhs, subst, env)?;
                let (r, rt) = self.transform_expr(rhs, subst, env)?;
                let result_ty = binary_type(op, &lt, &rt);
                (
                    ExprKind::Binary {
                        op: *op,
                        lhs: Box::new(l),
                        rhs: Box::new(r),
                    },
                    result_ty,
                )
            }
            ExprKind::Cast(inner, ty) => {
                let (e2, _) = self.transform_expr(inner, subst, env)?;
                (ExprKind::Cast(Box::new(e2), ty.clone()), ty.clone())
            }
            ExprKind::Assign { name, index, value } => {
                let (v, _) = self.transform_expr(value, subst, env)?;
                (
                    ExprKind::Assign {
                        name: name.clone(),
                        index: index.clone(),
                        value: Box::new(v),
                    },
                    TypeExpr::Named("Unit".to_string()),
                )
            }
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                let (c, _) = self.transform_expr(cond, subst, env)?;
                let (t_block, then_ty) = self.transform_block(then, subst, env)?;
                let (o_block, _else_ty) = match otherwise {
                    Some(o) => {
                        let (inner, ty) = self.transform_else(o, subst, env)?;
                        (Some(Box::new(inner)), ty)
                    }
                    None => (None, TypeExpr::Named("Unit".to_string())),
                };
                // The result type of an if is the then-type when there is no
                // else, or the common type of then and else when there is.
                let result_ty = if o_block.is_none() {
                    TypeExpr::Named("Unit".to_string())
                } else {
                    then_ty
                };
                (
                    ExprKind::If {
                        cond: Box::new(c),
                        then: t_block,
                        otherwise: o_block,
                    },
                    result_ty,
                )
            }
            ExprKind::Match { scrutinee, arms } => {
                let (s, _sty) = self.transform_expr(scrutinee, subst, env)?;
                let mut new_arms = Vec::with_capacity(arms.len());
                let mut arm_ty = TypeExpr::Named("Unit".to_string());
                for arm in arms {
                    let (new_arm, ty) = self.transform_arm(arm, subst, env)?;
                    new_arms.push(new_arm);
                    arm_ty = ty;
                }
                (
                    ExprKind::Match {
                        scrutinee: Box::new(s),
                        arms: new_arms,
                    },
                    arm_ty,
                )
            }
        };
        Ok((Expr::new(pos, kind), ty))
    }

    fn transform_else(
        &mut self,
        e: &Else,
        subst: &Substitution,
        env: &mut Vec<HashMap<String, TypeExpr>>,
    ) -> Result<(Else, TypeExpr), LowerError> {
        match e {
            Else::Block(b) => {
                let (block, ty) = self.transform_block(b, subst, env)?;
                Ok((Else::Block(block), ty))
            }
            Else::If(spanned) => {
                let (e2, ty) = self.transform_expr(&spanned.node, subst, env)?;
                Ok((Else::If(Spanned::new(e2, spanned.pos)), ty))
            }
        }
    }

    fn transform_arm(
        &mut self,
        arm: &Arm,
        subst: &Substitution,
        env: &mut Vec<HashMap<String, TypeExpr>>,
    ) -> Result<(Arm, TypeExpr), LowerError> {
        env.push(HashMap::new());
        let mut bindings = Vec::new();
        for p in &arm.patterns {
            for name in pattern_bindings(p) {
                bindings.push(name.clone());
                env.last_mut()
                    .unwrap()
                    .insert(name, TypeExpr::Named("Unit".to_string()));
            }
        }
        let (body, ty) = self.transform_expr(&arm.body, subst, env)?;
        env.pop();
        Ok((
            Arm {
                pos: arm.pos,
                patterns: arm.patterns.clone(),
                body,
            },
            ty,
        ))
    }

    /// Looks up `name` in the inference environment (innermost scope first).
    fn lookup_type(
        &self,
        name: &str,
        pos: Pos,
        env: &[HashMap<String, TypeExpr>],
    ) -> Result<TypeExpr, LowerError> {
        for scope in env.iter().rev() {
            if let Some(t) = scope.get(name) {
                return Ok(t.clone());
            }
        }
        // PascalCase identifiers are type names (struct, enum, or builtin),
        // not variables. Return the name so the lowerer can handle variant
        // or struct access.
        if name.chars().next().map_or(false, |c| c.is_uppercase()) {
            return Ok(TypeExpr::Named(name.to_string()));
        }
        Err(LowerError {
            message: format!("undefined name `{}`", name),
            pos,
        })
    }

    /// Resolves the field type of `name` on `base_type`.
    ///
    /// Returns a `Unit` fallback for types we cannot resolve. The checker
    /// produces the real error for unknown fields; the mono pass only needs
    /// types for trait-method receiver resolution.
    fn field_type(&self, base_type: &TypeExpr, name: &str) -> TypeExpr {
        let type_name = base_type.name();
        self.ctx
            .structs
            .get(type_name)
            .and_then(|fields| fields.iter().find(|f| f.name == name))
            .map(|f| f.ty.clone())
            .unwrap_or_else(|| TypeExpr::Named("Unit".to_string()))
    }
}

/// The return type of a builtin function, given the argument types.
/// `abs`, `min`, and `max` return the type of their first argument.
fn builtin_return_type(callee: &str, arg_types: &[TypeExpr]) -> TypeExpr {
    match callee {
        "print" | "println" => TypeExpr::Named("Unit".to_string()),
        "int_to_string" | "float_to_string" => TypeExpr::Named("Str".to_string()),
        "string_to_int" => TypeExpr::Named("Int".to_string()),
        "sqrt" => TypeExpr::Named("Float".to_string()),
        "min" | "max" | "abs" => arg_types
            .first()
            .cloned()
            .unwrap_or_else(|| TypeExpr::Named("Unit".to_string())),
        "now_ns" => TypeExpr::Named("Int".to_string()),
        _ => TypeExpr::Named("Unit".to_string()),
    }
}

/// Infers the result type of a binary operation from the operand types.
fn binary_type(op: &BinOp, lhs: &TypeExpr, _rhs: &TypeExpr) -> TypeExpr {
    use BinOp::*;
    match op {
        Add | Sub | Mul | Div | Rem => {
            // Arithmetic returns the type of the left operand, which is the
            // authoritative type after lowering when both operands agree.
            lhs.clone()
        }
        Eq | Ne => TypeExpr::Named("Bool".to_string()),
        Lt | Le | Gt | Ge => TypeExpr::Named("Bool".to_string()),
        And | Or => TypeExpr::Named("Bool".to_string()),
    }
}

/// Collects the names bound by a pattern, in order.
fn pattern_bindings(p: &Pattern) -> Vec<String> {
    match &p.kind {
        PatternKind::Wildcard => Vec::new(),
        PatternKind::Tuple(items) => items.iter().flat_map(pattern_bindings).collect(),
        PatternKind::Literal(_) => Vec::new(),
        PatternKind::Variant { bindings, .. } => bindings.clone(),
        PatternKind::Binding(n) => vec![n.clone()],
    }
}
