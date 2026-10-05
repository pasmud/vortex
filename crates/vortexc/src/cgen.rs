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

use std::collections::HashMap;
use std::fmt::Write as _;

use crate::ast::{self, Spanned};
use crate::span::Pos;

/// The C representation of a Vortex list.
///
/// A list is a struct with a pointer and a length, so an index is a pointer
/// offset. `items` is never null because an empty list gets a one element array
/// with a length of zero, which keeps every index expression free of a branch.
#[allow(dead_code)]
pub type CList = *mut i64;

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
/// Emits one function with no knowledge of the rest of the program.
///
/// A call inside it is then declared Int, because nothing says what the callee
/// returns. Use this only when testing a single function in isolation.
pub fn emit_function_alone(f: &Spanned<ast::FnDecl>) -> Result<String, Unsupported> {
    let signatures: Signatures = HashMap::new();
    emit_function(f, &signatures)
}

pub fn emit_function(
    f: &Spanned<ast::FnDecl>,
    signatures: &Signatures,
) -> Result<String, Unsupported> {
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
    let _ = writeln!(
        out,
        "{} {}({}) {{",
        ret,
        param_name(&f.node.name),
        param_list
    );
    let mut lists = HashMap::new();
    emit_block(&mut out, &f.node.body, 1, ret, &mut lists, &signatures)?;
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
        // `main` is reserved as well. A Vortex program may name its entry
        // function `main`, and the generated translation unit has its own.
        "main",
    ];
    if RESERVED.contains(&name) {
        format!("vortex_{}", name)
    } else if BUILTIN_RENAMES.iter().any(|(v, _)| *v == name) {
        BUILTIN_RENAMES
            .iter()
            .find(|(v, _)| *v == name)
            .map(|(_, c)| c.to_string())
            .unwrap_or_else(|| name.to_string())
    } else {
        name.to_string()
    }
}

/// The builtins the generated C provides under a prefixed name, so a Vortex
/// builtin call and the generated helper are the same thing.
const BUILTIN_RENAMES: &[(&str, &str)] = &[
    ("int_to_string", "vortex_int_to_string"),
    ("float_to_string", "vortex_float_to_string"),
    ("print", "vortex_print"),
    ("println", "vortex_println"),
];

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
    lists: &mut HashMap<String, &'static str>,
    signatures: &Signatures,
) -> Result<(), Unsupported> {
    for stmt in &b.stmts {
        emit_stmt(out, stmt, depth, ret, lists, signatures)?;
    }
    if let Some(tail) = &b.tail {
        // A body ending in an expression returns it. SPEC.md section 8 makes a
        // trailing expression the value of its block, and the interpreters treat
        // it that way, so the emitted C has to as well.
        indent(out, depth);
        if ret == "void" {
            let _ = writeln!(out, "{}", emit_expr(tail, lists, signatures)?);
        } else {
            let _ = writeln!(out, "return {};", emit_expr(tail, lists, signatures)?);
        }
    }
    Ok(())
}

/// The name suffix for the constructor of a list of this element type.
fn list_suffix(elem: &str) -> &'static str {
    match elem {
        "double" => "f64",
        "const char *" => "str",
        _ => "i64",
    }
}

/// The C element type a list initialiser produces, if it produces a list.
///
/// The rule matches the one the lowering pass uses: a literal takes the type of
/// its first element, which is sound because the checker rejects a mixed list,
/// and a repeat takes the type of the value it repeats.
fn list_element_type(e: &ast::Expr) -> Option<&'static str> {
    match &e.kind {
        ast::ExprKind::Array(items) => items.first().and_then(value_c_type),
        ast::ExprKind::Repeat { value, .. } => value_c_type(value),
        ast::ExprKind::Cast(_, t) => c_type(t),
        _ => None,
    }
}

/// The element type a base expression produces, when the base is itself a list
/// expression rather than a name.
fn list_element_type_index(base: &ast::Expr, _other: &ast::ExprKind) -> Option<&'static str> {
    list_element_type(base)
}

/// The C type of a scalar value expression.
fn value_c_type(e: &ast::Expr) -> Option<&'static str> {
    match &e.kind {
        ast::ExprKind::Int(_) => Some("int64_t"),
        ast::ExprKind::Float(_) => Some("double"),
        ast::ExprKind::Str(_) => Some("const char *"),
        ast::ExprKind::Cast(_, t) => c_type(t),
        _ => None,
    }
}

fn emit_stmt(
    out: &mut String,
    s: &ast::Stmt,
    depth: usize,
    ret: &str,
    lists: &mut HashMap<String, &'static str>,
    signatures: &Signatures,
) -> Result<(), Unsupported> {
    match &s.kind {
        ast::StmtKind::Let { name, init, .. } => {
            let c = expr_type_of(init, signatures);
            // A list binding records its element type, so an index into this
            // name reads through the right cast later in the same walk.
            if let Some(elem) = list_element_type(init) {
                lists.insert(param_name(name), elem);
            }
            indent(out, depth);
            let _ = writeln!(
                out,
                "{} {} = {};",
                c,
                param_name(name),
                emit_expr(init, lists, signatures)?
            );
        }
        ast::StmtKind::Return(e) => {
            let value = emit_expr(e, lists, signatures)?;
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
                    // The element type comes from the name the base is bound
                    // to, recorded when that declaration was emitted.
                    let elem = match &base.kind {
                        ast::ExprKind::Ident(n) => {
                            lists.get(&param_name(n)).copied().unwrap_or("int64_t")
                        }
                        other => list_element_type_index(base, other)
                            .or_else(|| list_element_type(base))
                            .unwrap_or("int64_t"),
                    };
                    let _ = writeln!(
                        out,
                        "((({c} *)({b}).items)[({i})]) = {v};",
                        c = elem,
                        b = emit_expr(base, lists, signatures)?,
                        i = emit_expr(i, lists, signatures)?,
                        v = emit_expr(value, lists, signatures)?
                    );
                }
                None => {
                    indent(out, depth);
                    let _ = writeln!(
                        out,
                        "{} = {};",
                        param_name(name),
                        emit_expr(value, lists, signatures)?
                    );
                }
            },
            // An if used as a statement. This is the shape the sieve and
            // every loop in it use, and it is why an if statement is emitted
            // rather than refused.
            ast::ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                indent(out, depth);
                let _ = writeln!(out, "if ({}) {{", emit_expr(cond, lists, signatures)?);
                emit_block(out, then, depth + 1, ret, lists, signatures)?;
                match otherwise {
                    Some(else_box) => {
                        indent(out, depth);
                        let _ = writeln!(out, "}} else {{");
                        if let ast::Else::Block(b) = else_box.as_ref() {
                            emit_block(out, b, depth + 1, ret, lists, signatures)?;
                        } else {
                            return Err(Unsupported::Construct(
                                "an else if chain in a statement".to_string(),
                                e.pos,
                            ));
                        }
                        indent(out, depth);
                        let _ = writeln!(out, "}}");
                    }
                    None => {
                        indent(out, depth);
                        let _ = writeln!(out, "}}");
                    }
                }
            }
            // Any other expression statement is evaluated for its effect,
            // which for a call means the call runs.
            ast::ExprKind::Call { .. } => {
                indent(out, depth);
                let _ = writeln!(out, "{};", emit_expr(e, lists, signatures)?);
            }
            _ => {
                let _ = emit_expr(e, lists, signatures)?;
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
        // An if used as a statement. The condition and both arms are emitted
        // inline, which is what the sieve needs and what a block value would
        // require before it could be handled more generally.
        ast::StmtKind::Block(inner) => {
            let _ = writeln!(out, "{{");
            emit_block(out, inner, depth + 1, ret, lists, signatures)?;
            indent(out, depth);
            let _ = writeln!(out, "}}");
        }
        ast::StmtKind::While { cond, body, .. } => {
            indent(out, depth);
            let _ = writeln!(out, "while ({}) {{", emit_expr(cond, lists, signatures)?);
            emit_block(out, body, depth + 1, ret, lists, signatures)?;
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
                format!("({} + 1)", emit_expr(end, lists, signatures)?)
            } else {
                emit_expr(end, lists, signatures)?
            };
            indent(out, depth);
            let _ = writeln!(
                out,
                "for (int64_t {} = {}; {} {} {}; {}++) {{",
                param_name(var),
                emit_expr(start, lists, signatures)?,
                param_name(var),
                "<",
                limit,
                param_name(var)
            );
            emit_block(out, body, depth + 1, ret, lists, signatures)?;
            indent(out, depth);
            let _ = writeln!(out, "}}");
        }
    }
    Ok(())
}

/// The C type an expression produces, which decides what a `let` declares.
/// The declared return type of each function in the program, by name.
///
/// The emitter needs it because a `let` holding a call has to be declared at
/// what the callee returns. Inferring it from the call expression would always
/// say Int, which truncated a Float result.
type Signatures = HashMap<String, &'static str>;

fn expr_type_of(e: &ast::Expr, signatures: &Signatures) -> &'static str {
    match &e.kind {
        // A list is a struct with a pointer and a length, not a scalar.
        ast::ExprKind::Array(_) | ast::ExprKind::Repeat { .. } => "CList",
        ast::ExprKind::Int(_) => "int64_t",
        ast::ExprKind::Float(_) => "double",
        ast::ExprKind::Bool(_) => "int",
        ast::ExprKind::Char(_) => "int32_t",
        ast::ExprKind::Str(_) => "const char *",
        ast::ExprKind::Binary { op, lhs, rhs } => match op {
            ast::BinOp::Eq
            | ast::BinOp::Ne
            | ast::BinOp::Lt
            | ast::BinOp::Le
            | ast::BinOp::Gt
            | ast::BinOp::Ge
            | ast::BinOp::And
            | ast::BinOp::Or => "int",
            // Arithmetic is a Float when either operand is, which is the rule
            // the checker applies in section 6.1 rule 4 and the tree
            // interpreter follows. Without it `v * 0.5 + 1.0` declared an
            // integer and truncated 2.5 to 2.
            _ => {
                if expr_type_of(lhs, signatures) == "double"
                    || expr_type_of(rhs, signatures) == "double"
                {
                    "double"
                } else {
                    "int64_t"
                }
            }
        },
        ast::ExprKind::Neg(inner) => expr_type_of(inner, signatures),
        // A call returns whatever the callee declares. Without this a `let`
        // holding a Float result was declared int64_t and truncated it, which
        // is why `let m = matrix_work();` lost its fraction before the entry
        // ever printed it.
        ast::ExprKind::Call { callee, .. } => signatures.get(callee).copied().unwrap_or("int64_t"),
        _ => "int64_t",
    }
}

/// Emits an expression as a C expression.
///
/// Every form the compiled path needs is handled. Anything else names itself
/// rather than emitting something that will not compile.
/// Emits a block as a single C expression.
///
fn emit_expr(
    e: &ast::Expr,
    lists: &HashMap<String, &'static str>,
    signatures: &Signatures,
) -> Result<String, Unsupported> {
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
                parts.push(emit_expr(a, lists, signatures)?);
            }
            format!("{}({})", param_name(callee), parts.join(", "))
        }
        ast::ExprKind::Paren(inner) => format!("({})", emit_expr(inner, lists, signatures)?),
        ast::ExprKind::Cast(inner, to) => {
            let target = c_type(to).ok_or_else(|| {
                Unsupported::Construct(format!("a cast to `{}`", to.name()), e.pos)
            })?;
            format!("(({})({}))", target, emit_expr(inner, lists, signatures)?)
        }
        ast::ExprKind::Neg(inner) => format!("-({})", emit_expr(inner, lists, signatures)?),
        ast::ExprKind::Not(inner) => format!("!({})", emit_expr(inner, lists, signatures)?),
        ast::ExprKind::Binary { op, lhs, rhs } => {
            emit_binary(op, lhs, rhs, e.pos, lists, signatures)?
        }
        ast::ExprKind::Array(items) => {
            // A list literal is a call to the generated helper, because a C
            // compound literal is not valid where a list is built.
            let mut parts = Vec::with_capacity(items.len());
            for i in items {
                parts.push(emit_expr(i, lists, signatures)?);
            }
            format!(
                "vortex_list_new({}u, (int64_t[]){{ {} }})",
                parts.len(),
                parts.join(", ")
            )
        }
        ast::ExprKind::Repeat { value, count } => {
            // `[v; n]` builds a list of the value's type, so the constructor
            // is chosen by that type rather than by an index.
            let elem = expr_type_of(value, signatures);
            let v = emit_expr(value, lists, signatures)?;
            let n = emit_expr(count, lists, signatures)?;
            format!(
                "vortex_list_repeat_{}({n}, {v})",
                list_suffix(elem),
                n = n,
                v = v
            )
        }
        ast::ExprKind::Index(base, index) => {
            // The element type comes from the name the base is bound to,
            // recorded when that declaration was emitted, or from the base
            // itself when it is a literal or a repeat. It is never inferred
            // from the index, because a Float list still has an Int index.
            let elem = match &base.kind {
                ast::ExprKind::Ident(n) => lists
                    .get(&param_name(n))
                    .copied()
                    .or_else(|| list_element_type(base))
                    .unwrap_or("int64_t"),
                _ => list_element_type(base).unwrap_or("int64_t"),
            };
            let b = emit_expr(base, lists, signatures)?;
            let i = emit_expr(index, lists, signatures)?;
            format!("((({c} *)({b}).items)[({i})])", c = elem, b = b, i = i)
        }
        ast::ExprKind::Field(base, _) => {
            // A field read on a struct the emitter emitted inline.
            let _ = base;
            return Err(Unsupported::Construct(
                "a field read on a named struct".to_string(),
                e.pos,
            ));
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

/// Emits a binary operation, choosing between the arithmetic form and the
/// string concatenation form.
///
/// Vortex `+` on two strings is concatenation, and C `+` on two pointers is
/// address arithmetic, so the string form is a call rather than the operator.
fn emit_binary(
    op: &ast::BinOp,
    lhs: &ast::Expr,
    rhs: &ast::Expr,
    pos: crate::span::Pos,
    lists: &HashMap<String, &'static str>,
    signatures: &Signatures,
) -> Result<String, Unsupported> {
    let string_plus = matches!(op, ast::BinOp::Add) && (is_string_expr(lhs) || is_string_expr(rhs));
    if string_plus {
        return Ok(format!(
            "vortex_str_concat({}, {})",
            emit_expr(lhs, lists, signatures)?,
            emit_expr(rhs, lists, signatures)?
        ));
    }
    let o = c_operator(op)?;
    let _ = pos;
    Ok(format!(
        "({} {} {})",
        emit_expr(lhs, lists, signatures)?,
        o,
        emit_expr(rhs, lists, signatures)?
    ))
}

/// Whether an expression is known to be a Vortex string.
///
/// `int_to_string` and `float_to_string` return a string, and that has to be
/// known before a `+` is emitted, because C `+` on two pointers is address
/// arithmetic rather than concatenation. An expression whose type is not known
/// here is treated as a scalar, which is right for every builtin that returns
/// one.
fn is_string_expr(e: &ast::Expr) -> bool {
    match &e.kind {
        ast::ExprKind::Str(_) => true,
        ast::ExprKind::Cast(inner, t) => t.name() == "Str" || is_string_expr(inner),
        ast::ExprKind::Binary { op, lhs, rhs } if matches!(op, ast::BinOp::Add) => {
            is_string_expr(lhs) || is_string_expr(rhs)
        }
        ast::ExprKind::Call { callee, .. } => {
            matches!(callee.as_str(), "int_to_string" | "float_to_string")
        }
        ast::ExprKind::Paren(inner) => is_string_expr(inner),
        _ => false,
    }
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
    let _ = writeln!(out, "#include <stdio.h>");
    let _ = writeln!(out, "#include <stdlib.h>");
    let _ = writeln!(out, "#include <string.h>");
    // A Vortex list is a pointer and a length, which is what SPEC.md section 7
    // describes: one owner and a known size.
    let _ = writeln!(
        out,
        "typedef struct {{ int64_t *items; int64_t len; }} CList;"
    );
    // The builtins a Vortex program calls. Each matches what the tree
    // interpreter and the VM do, so a program using one gets the same answer on
    // all three paths.
    let _ = writeln!(out, "static const char *vortex_int_to_string(int64_t n) {{");
    let _ = writeln!(out, "    char buf[32];");
    let _ = writeln!(
        out,
        "    snprintf(buf, sizeof(buf), \"%lld\", (long long)n);"
    );
    let _ = writeln!(out, "    char *r = (char *)malloc(strlen(buf) + 1);");
    let _ = writeln!(out, "    strcpy(r, buf);");
    let _ = writeln!(out, "    return r;");
    let _ = writeln!(out, "}}");
    let _ = writeln!(
        out,
        "static const char *vortex_float_to_string(double f) {{"
    );
    let _ = writeln!(out, "    char buf[64];");
    let _ = writeln!(out, "    snprintf(buf, sizeof(buf), \"%.6f\", f);");
    let _ = writeln!(out, "    char *r = (char *)malloc(strlen(buf) + 1);");
    let _ = writeln!(out, "    strcpy(r, buf);");
    let _ = writeln!(out, "    return r;");
    let _ = writeln!(out, "}}");
    let _ = writeln!(
        out,
        "static void vortex_print(const char *s) {{ fputs(s, stdout); }}"
    );
    let _ = writeln!(out, "static void vortex_println(const char *s) {{");
    let _ = writeln!(out, "    fputs(s, stdout);");
    let _ = writeln!(out, "    fputc('\\n', stdout);");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out, "");
    let _ = writeln!(
        out,
        "/* A list is allocated once and owned by its binding. */"
    );
    let _ = writeln!(
        out,
        "static CList vortex_list_new(int64_t len, int64_t *values) {{"
    );
    let _ = writeln!(out, "    CList l;");
    let _ = writeln!(out, "    l.len = len;");
    let _ = writeln!(
        out,
        "    l.items = (int64_t *)malloc(sizeof(int64_t) * (size_t)(len > 0 ? len : 1));"
    );
    let _ = writeln!(
        out,
        "    for (int64_t k = 0; k < len; k++) l.items[k] = values[k];"
    );
    let _ = writeln!(out, "    return l;");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out, "");
    // One constructor per element type. The struct carries a void * because a
    // Vortex list is not homogeneous in C's type system, so the element type
    // has to be named at the call site. Declaring them here means a call may
    // appear before the definition.
    for (suffix, elem) in [
        ("i64", "int64_t"),
        ("f64", "double"),
        ("str", "const char *"),
    ] {
        let _ = writeln!(
            out,
            "static CList vortex_list_new_{suffix}(int64_t len, const {elem} *values) {{"
        );
        let _ = writeln!(out, "    CList l;");
        let _ = writeln!(out, "    l.len = len;");
        let _ = writeln!(
            out,
            "    l.items = malloc(sizeof({elem}) * (size_t)(len > 0 ? len : 1));"
        );
        let _ = writeln!(
            out,
            "    for (int64_t k = 0; k < len; k++) (({elem} *)l.items)[k] = values[k];"
        );
        let _ = writeln!(out, "    return l;");
        let _ = writeln!(out, "}}");
        let _ = writeln!(out, "");
        let _ = writeln!(
            out,
            "static CList vortex_list_repeat_{suffix}(int64_t len, {elem} value) {{"
        );
        let _ = writeln!(out, "    CList l;");
        let _ = writeln!(out, "    l.len = len;");
        let _ = writeln!(
            out,
            "    l.items = malloc(sizeof({elem}) * (size_t)(len > 0 ? len : 1));"
        );
        let _ = writeln!(
            out,
            "    for (int64_t k = 0; k < len; k++) (({elem} *)l.items)[k] = value;"
        );
        let _ = writeln!(out, "    return l;");
        let _ = writeln!(out, "}}");
        let _ = writeln!(out, "");
    }
    let _ = writeln!(out, "");
    let _ = writeln!(
        out,
        "/* Vortex string concatenation, which owns its result. */"
    );
    let _ = writeln!(
        out,
        "static const char *vortex_str_concat(const char *a, const char *b) {{"
    );
    let _ = writeln!(out, "    size_t la = strlen(a), lb = strlen(b);");
    let _ = writeln!(out, "    char *r = (char *)malloc(la + lb + 1);");
    let _ = writeln!(out, "    memcpy(r, a, la);");
    let _ = writeln!(out, "    memcpy(r + la, b, lb);");
    let _ = writeln!(out, "    r[la + lb] = '\\0';");
    let _ = writeln!(out, "    return r;");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out, "");
    let _ = writeln!(
        out,
        "static CList vortex_list_repeat(int64_t len, int64_t value) {{"
    );
    let _ = writeln!(out, "    CList l;");
    let _ = writeln!(out, "    l.len = len;");
    let _ = writeln!(
        out,
        "    l.items = (int64_t *)malloc(sizeof(int64_t) * (size_t)(len > 0 ? len : 1));"
    );
    let _ = writeln!(
        out,
        "    for (int64_t k = 0; k < len; k++) l.items[k] = value;"
    );
    let _ = writeln!(out, "    return l;");
    let _ = writeln!(out, "}}");

    // Only the function under test and its callees are emitted. The Vortex
    // `main` is not among them, because emitting it alongside the C entry point
    // gives the translation unit two `main` definitions and C rejects it.
    let wanted = reachable_from(functions, entry);
    // The declared return type of every function, so a `let` holding a call
    // can be declared at what the callee actually returns.
    let mut signatures: Signatures = HashMap::new();
    for f in functions {
        signatures.insert(
            f.node.name.clone(),
            f.node.ret.as_ref().and_then(c_type).unwrap_or("int64_t"),
        );
    }
    for f in functions {
        if wanted.contains(&f.node.name) {
            let _ = writeln!(out, "{}", emit_function(f, &signatures)?);
        }
    }

    // The C entry point is named apart from the Vortex one, because a Vortex
    // program has its own `main` and two C functions of that name collide.
    let _ = writeln!(
        out,
        "{} vortex_c_entry(void) {{",
        entry_c_type(functions, entry)
    );
    // The Vortex entry takes its declared parameters, so the call passes one
    // value per parameter. v0.1 has no list type, so every parameter is a
    // scalar and a literal is enough to exercise the compiled path.
    // One value per declared parameter. A Vortex entry taking no parameters
    // gets none, rather than an argument that does not exist.
    let declared: Vec<String> = functions
        .iter()
        .find(|f| f.node.name == entry)
        .map(|f| f.node.params.iter().map(|_| String::new()).collect())
        .unwrap_or_default();
    let params: Vec<String> = declared
        .iter()
        .enumerate()
        .map(|(i, _)| args.get(i).cloned().unwrap_or_else(|| "0".to_string()))
        .collect();
    let _ = writeln!(
        out,
        "    return {}({});",
        param_name(entry),
        params.join(", ")
    );
    let _ = writeln!(out, "}}\n");

    // The Vortex entry is run and whatever it printed goes to standard output,
    // which is how the compiled answer is compared with the two engines. Its
    // return value is printed only when the program produced no output of its
    // own, so a program that prints is not printed twice.
    // Printed at the type the entry declares. Printing it as a long long
    // truncated a Float, which is one of the three causes of the compiled path
    // disagreeing with the two engines on the matrix half.
    let entry_c = entry_c_type(functions, entry);
    let _ = writeln!(out, "int main(void) {{");
    let _ = writeln!(out, "    {} r = vortex_c_entry();", entry_c);
    // A program that prints its own answer has already written it, so the
    // returned value is written after a marker the harness can strip.
    match entry_c {
        "double" => {
            let _ = writeln!(out, "    fprintf(stderr, \"vortex_returned %.6f\\n\", r);");
        }
        "const char *" => {
            let _ = writeln!(out, "    fprintf(stderr, \"vortex_returned %s\\n\", r);");
        }
        _ => {
            let _ = writeln!(
                out,
                "    fprintf(stderr, \"vortex_returned %lld\\n\", (long long)r);"
            );
        }
    }
    let _ = writeln!(out, "    return 0;\n}}");
    let _ = args;
    Ok(out)
}

/// The C type the entry point carries its result in.
///
/// It is the entry function's declared return type, so a 64 bit `Int` is not
/// truncated on the way out. Casting it through `int` gave a wrong answer that
/// agreed with nothing.
fn entry_c_type(functions: &[Spanned<ast::FnDecl>], entry: &str) -> &'static str {
    functions
        .iter()
        .find(|f| f.node.name == entry)
        .and_then(|f| f.node.ret.as_ref())
        .and_then(|t| c_type(t))
        .unwrap_or("int64_t")
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
