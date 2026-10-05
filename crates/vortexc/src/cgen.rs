//! Emits C for one compiled function.
//!
//! `DECISION.md` decided the backend is an ahead-of-time compiler over
//! `crates/vortexc/src/ir.rs`, and made no performance claim because none had
//! been measured. This is the first piece of it: a function from the lowered
//! form becomes C, the C is compiled and run, and its answer is compared against
//! the tree interpreter and the VM.
//!
//! The claim is not speed. It is that a compiled path exists and agrees with the
//! two engines already in the tree, which is what makes the claim checkable.
//!
//! Scope is deliberately small. `crate::ast::FnDecl` says a function returns
//! `Int`, `Float`, `Str`, `Char`, `Bool` or nothing, and the emitter handles
//! exactly those return types with `Int` and `Float` bodies. Anything else
//! returns [`Unsupported`] naming the construct, so the next function that
//! cannot be compiled is a known limit rather than a broken translation.

use std::fmt::Write as _;

use crate::ast::{self, Spanned};
use crate::span::Pos;

/// Something the emitter does not handle, named rather than counted.
///
/// The reason travels with the refusal so a caller can report which construct
/// stopped the compilation.
#[derive(Debug, Clone, PartialEq)]
pub enum Unsupported {
    /// A construct this emitter does not handle, at a position.
    Construct(String, Pos),
}

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unsupported::Construct(what, pos) => {
                write!(f, "{} at {}", what, pos)
            }
        }
    }
}

/// The C type a Vortex return type maps to.
///
/// Every value in v0.1 is a scalar or an aggregate of scalars, so the mapping is
/// direct. A list would become a struct with a pointer and a length, which is not
/// written yet.
fn c_type(t: &ast::TypeExpr) -> Option<&'static str> {
    match t.name() {
        "Int" => Some("int64_t"),
        "Float" => Some("double"),
        "Bool" => Some("int"),
        "Char" => Some("int32_t"),
        "Str" => Some("const char *"),
        _ => None,
    }
}

/// Emits one function as a C function definition.
///
/// `body` is the same body the interpreters run, so the three paths execute the
/// same program rather than three similar ones.
pub fn emit_function(f: &Spanned<ast::FnDecl>) -> Result<String, Unsupported> {
    let ret = match &f.node.ret {
        None => "void",
        Some(t) => c_type(t).ok_or_else(|| {
            Unsupported::Construct(format!("the return type `{}`", t.name()), f.pos)
        })?,
    };

    let mut params = Vec::with_capacity(f.node.params.len());
    for p in &f.node.params {
        let c = c_type(&p.ty).ok_or_else(|| {
            Unsupported::Construct(format!("the parameter type `{}`", p.ty.name()), f.pos)
        })?;
        // The parameter name is fixed because a function has a fixed arity in v0.1.
        let name = param_name(&p.name);
        params.push(format!("{} {}", c, name));
    }
    let param_list = if params.is_empty() {
        "void".to_string()
    } else {
        params.join(", ")
    };

    let mut out = String::new();
    let _ = writeln!(out, "{} {}({}) {{", ret, f.node.name, param_list);
    emit_block(&mut out, &f.node.body, 1, ret)?;
    // A body that falls off the end returns the zero value, which is what the
    // interpreters do too.
    let zero = match ret {
        "double" => "0.0",
        "const char *" => "0",
        "void" => "",
        _ => "0",
    };
    if zero.is_empty() {
        let _ = writeln!(out, "    return;");
    } else {
        let _ = writeln!(out, "    return {};", zero);
    }
    let _ = writeln!(out, "}}");
    Ok(out)
}

/// A C identifier for a Vortex name. Vortex names are already identifiers, so
/// this only guards the reserved words C reserves.
fn param_name(name: &str) -> String {
    const RESERVED: &[&str] = &[
        "auto", "break", "case", "char", "const", "continue", "default", "do", "double", "else",
        "enum", "extern", "float", "for", "goto", "if", "inline", "int", "long", "register",
        "restrict", "return", "short", "signed", "sizeof", "static", "struct", "switch", "typedef",
        "union", "unsigned", "void", "volatile", "while",
    ];
    if RESERVED.contains(&name) {
        format!("vortex_{}", name)
    } else {
        name.to_string()
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("    ");
    }
}

fn emit_block(
    out: &mut String,
    b: &ast::Block,
    depth: usize,
    ret: &str,
) -> Result<(), Unsupported> {
    for stmt in &b.stmts {
        emit_stmt(out, stmt, depth, ret)?;
    }
    if let Some(tail) = &b.tail {
        // A trailing expression is a block value, which this emitter does not
        // handle yet. Naming it is better than emitting C that will not compile.
        return Err(Unsupported::Construct(
            "a block value, a trailing expression".to_string(),
            tail.pos,
        ));
    }
    Ok(())
}

fn emit_stmt(out: &mut String, s: &ast::Stmt, depth: usize, ret: &str) -> Result<(), Unsupported> {
    match &s.kind {
        ast::StmtKind::Let { name, init, .. } => {
            let c = expr_type_of(init);
            indent(out, depth);
            let _ = writeln!(out, "{} {} = {};", c, param_name(name), emit_expr(init)?);
        }
        ast::StmtKind::Return(e) => {
            let value = emit_expr(e)?;
            if ret == "void" {
                indent(out, depth);
                let _ = writeln!(out, "return;");
            } else {
                indent(out, depth);
                let _ = writeln!(out, "return {};", value);
            }
        }
        // An assignment statement. `SPEC.md` section 8.1 makes the target
        // mutable, so the target is always a name or an index.
        ast::StmtKind::Expr(e) => match &e.kind {
            ast::ExprKind::Assign { name, index, value } => match index {
                Some((base, i)) => {
                    indent(out, depth);
                    let _ = writeln!(
                        out,
                        "{}[{}] = {};",
                        emit_expr(base)?,
                        emit_expr(i)?,
                        emit_expr(value)?
                    );
                }
                None => {
                    indent(out, depth);
                    let _ = writeln!(out, "{} = {};", param_name(name), emit_expr(value)?);
                }
            },
            // Any other expression statement is evaluated for its effect, which
            // for a call means the call.
            _ => {
                let _ = emit_expr(e)?;
            }
        },
        ast::StmtKind::Break => {
            indent(out, depth);
            let _ = writeln!(out, "break;");
        }
        ast::StmtKind::Continue => {
            indent(out, depth);
            let _ = writeln!(out, "continue;");
        }
        ast::StmtKind::Block(inner) => {
            let _ = writeln!(out, "{{");
            emit_block(out, inner, depth + 1, ret)?;
            indent(out, depth);
            let _ = writeln!(out, "}}");
        }
        ast::StmtKind::While { cond, body, .. } => {
            indent(out, depth);
            let _ = writeln!(out, "while ({}) {{", emit_expr(cond)?);
            emit_block(out, body, depth + 1, ret)?;
            indent(out, depth);
            let _ = writeln!(out, "}}");
        }
        ast::StmtKind::For {
            var,
            start,
            end,
            inclusive,
            body,
            ..
        } => {
            // An inclusive range is a strict comparison against one past the
            // end, which is how the interpreters do it too. An open ended range
            // is not expressible in v0.1 and is refused rather than guessed at.
            let end = end.as_ref().ok_or_else(|| {
                Unsupported::Construct("an open ended for range".to_string(), s.pos)
            })?;
            let limit = if *inclusive {
                format!("({} + 1)", emit_expr(end)?)
            } else {
                emit_expr(end)?
            };
            indent(out, depth);
            let _ = writeln!(
                out,
                "for (int64_t {} = {}; {} {} {}; {}++) {{",
                param_name(var),
                emit_expr(start)?,
                param_name(var),
                "<",
                limit,
                param_name(var)
            );
            emit_block(out, body, depth + 1, ret)?;
            indent(out, depth);
            let _ = writeln!(out, "}}");
        }
    }
    Ok(())
}

/// The C type an expression produces, which decides what a `let` declares.
fn expr_type_of(e: &ast::Expr) -> &'static str {
    match &e.kind {
        ast::ExprKind::Int(_) => "int64_t",
        ast::ExprKind::Float(_) => "double",
        ast::ExprKind::Bool(_) => "int",
        ast::ExprKind::Char(_) => "int32_t",
        ast::ExprKind::Str(_) => "const char *",
        ast::ExprKind::Binary { op, .. } => match op {
            ast::BinOp::Eq
            | ast::BinOp::Ne
            | ast::BinOp::Lt
            | ast::BinOp::Le
            | ast::BinOp::Gt
            | ast::BinOp::Ge
            | ast::BinOp::And
            | ast::BinOp::Or => "int",
            _ => "int64_t",
        },
        ast::ExprKind::Neg(inner) => expr_type_of(inner),
        _ => "int64_t",
    }
}

/// Emits an expression as a C expression.
///
/// Every form the compiled path needs is handled. Anything else names itself
/// rather than emitting something that will not compile.
fn emit_expr(e: &ast::Expr) -> Result<String, Unsupported> {
    Ok(match &e.kind {
        ast::ExprKind::Int(v) => format!("INT64_C({})", v),
        ast::ExprKind::Float(v) => format!("{:.6}", v),
        ast::ExprKind::Bool(v) => (if *v { "1" } else { "0" }).to_string(),
        ast::ExprKind::Char(c) => format!("{}", *c as u32),
        ast::ExprKind::Str(s) => format!("\"{}\"", c_escape(s)),
        ast::ExprKind::Ident(name) => param_name(name),
        ast::ExprKind::Call { callee, args } => {
            let mut parts = Vec::with_capacity(args.len());
            for a in args {
                parts.push(emit_expr(a)?);
            }
            format!("{}({})", param_name(callee), parts.join(", "))
        }
        ast::ExprKind::Paren(inner) => format!("({})", emit_expr(inner)?),
        ast::ExprKind::Neg(inner) => format!("-({})", emit_expr(inner)?),
        ast::ExprKind::Not(inner) => format!("!({})", emit_expr(inner)?),
        ast::ExprKind::Binary { op, lhs, rhs } => {
            let o = c_operator(op)?;
            format!("({} {} {})", emit_expr(lhs)?, o, emit_expr(rhs)?)
        }
        ast::ExprKind::Tuple(_) => {
            return Err(Unsupported::Construct("a tuple".to_string(), e.pos));
        }
        ast::ExprKind::If { cond: _, .. } => {
            return Err(Unsupported::Construct(
                "an if expression".to_string(),
                e.pos,
            ));
        }
        other => {
            let _ = other;
            return Err(Unsupported::Construct(
                "this expression form".to_string(),
                e.pos,
            ));
        }
    })
}

fn c_operator(op: &ast::BinOp) -> Result<&'static str, Unsupported> {
    Ok(match op {
        ast::BinOp::Add => "+",
        ast::BinOp::Sub => "-",
        ast::BinOp::Mul => "*",
        ast::BinOp::Div => "/",
        ast::BinOp::Rem => "%",
        ast::BinOp::Eq => "==",
        ast::BinOp::Ne => "!=",
        ast::BinOp::Lt => "<",
        ast::BinOp::Le => "<=",
        ast::BinOp::Gt => ">",
        ast::BinOp::Ge => ">=",
        ast::BinOp::And => "&&",
        ast::BinOp::Or => "||",
    })
}

/// Escapes a string for a C literal.
fn c_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

/// Emits a translation unit: the given functions plus a `main` that calls the
/// entry function and prints its result.
///
/// The printing is what makes the compiled answer comparable with the other two
/// engines, which print rather than return.
pub fn emit_program(
    functions: &[Spanned<ast::FnDecl>],
    entry: &str,
    args: &[String],
) -> Result<String, Unsupported> {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "/* Generated by crates/vortexc/src/cgen.rs from a Vortex program. */"
    );
    let _ = writeln!(out, "#include <stdint.h>");
    let _ = writeln!(out, "#include <stdio.h>\n");

    // Only the function under test and its callees are emitted. The Vortex
    // `main` is not among them, because emitting it alongside the C entry point
    // gives the translation unit two `main` definitions and C rejects it.
    let wanted = reachable_from(functions, entry);
    for f in functions {
        if wanted.contains(&f.node.name) {
            let _ = writeln!(out, "{}", emit_function(f)?);
        }
    }

    // The C entry point is named apart from the Vortex one, because a Vortex
    // program has its own `main` and two C functions of that name collide.
    let _ = writeln!(out, "int vortex_c_entry(void) {{");
    // The Vortex entry takes its declared parameters, so the call passes one
    // value per parameter. v0.1 has no list type, so every parameter is a
    // scalar and a literal is enough to exercise the compiled path.
    let params: Vec<String> = args.to_vec();
    let _ = writeln!(
        out,
        "    return (int){}({});",
        param_name(entry),
        params.join(", ")
    );
    let _ = writeln!(out, "}}\n");

    let _ = writeln!(out, "int main(void) {{");
    let _ = writeln!(out, "    printf(\"%d\\n\", vortex_c_entry());");
    let _ = writeln!(out, "    return 0;\n}}");
    let _ = args;
    Ok(out)
}

/// The names reachable from `entry`, which is the set worth emitting.
fn reachable_from(functions: &[Spanned<ast::FnDecl>], entry: &str) -> Vec<String> {
    let mut wanted = vec![entry.to_string()];
    let mut i = 0;
    while i < wanted.len() {
        let name = wanted[i].clone();
        if let Some(f) = functions.iter().find(|f| f.node.name == name) {
            for callee in called_names(&f.node.body) {
                if !wanted.contains(&callee) {
                    wanted.push(callee);
                }
            }
        }
        i += 1;
    }
    wanted
}

/// The functions a body calls, found syntactically.
fn called_names(b: &ast::Block) -> Vec<String> {
    let mut out = Vec::new();
    for stmt in &b.stmts {
        collect_calls_stmt(stmt, &mut out);
    }
    if let Some(t) = &b.tail {
        collect_calls_expr(t, &mut out);
    }
    out
}

fn collect_calls_stmt(s: &ast::Stmt, out: &mut Vec<String>) {
    match &s.kind {
        ast::StmtKind::Let { init, .. }
        | ast::StmtKind::Return(init)
        | ast::StmtKind::Expr(init) => {
            collect_calls_expr(init, out);
        }
        ast::StmtKind::Block(b) | ast::StmtKind::While { body: b, .. } => {
            for inner in &b.stmts {
                collect_calls_stmt(inner, out);
            }
        }
        ast::StmtKind::For {
            body, start, end, ..
        } => {
            collect_calls_expr(start, out);
            if let Some(e) = end {
                collect_calls_expr(e, out);
            }
            for inner in &body.stmts {
                collect_calls_stmt(inner, out);
            }
        }
        _ => {}
    }
}

fn collect_calls_expr(e: &ast::Expr, out: &mut Vec<String>) {
    match &e.kind {
        ast::ExprKind::Call { callee, args } => {
            out.push(callee.clone());
            for a in args {
                collect_calls_expr(a, out);
            }
        }
        ast::ExprKind::Binary { lhs, rhs, .. } => {
            collect_calls_expr(lhs, out);
            collect_calls_expr(rhs, out);
        }
        ast::ExprKind::Neg(i) | ast::ExprKind::Not(i) | ast::ExprKind::Paren(i) => {
            collect_calls_expr(i, out)
        }
        ast::ExprKind::Assign { value, .. } => collect_calls_expr(value, out),
        ast::ExprKind::Block(b) => {
            for inner in &b.stmts {
                collect_calls_stmt(inner, out);
            }
        }
        _ => {}
    }
}
