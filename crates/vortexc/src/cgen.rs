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
fn c_type(t: &ast::TypeExpr) -> Option<String> {
    c_type_named(t.name())
}

fn c_type_named(name: &str) -> Option<String> {
    match name {
        "Int" => Some("int64_t".to_string()),
        "Float" => Some("double".to_string()),
        "Bool" => Some("int".to_string()),
        "Char" => Some("int32_t".to_string()),
        "Str" => Some("const char *".to_string()),
        _ => declared_type(name),
    }
}

// The struct and enum names declared by the program being emitted.
//
// `c_type` is called from many places that do not carry the program, so the
// names are recorded in a thread local rather than threaded through every
// call site. A name not in here is not a type this emitter knows, which is how
// an unknown type is refused rather than guessed at.
thread_local! {
    static DECLARED: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The C name of a declared struct or enum, if the program declares one.
fn declared_type(name: &str) -> Option<String> {
    let mangled = param_name(name);
    DECLARED.with(|d| {
        if d.borrow().iter().any(|n| *n == mangled) {
            Some(format!("C{mangled}"))
        } else {
            None
        }
    })
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
        None => "void".to_string(),
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
    let mut lists: HashMap<String, String> = HashMap::new();
    // Parameters are recorded as they are declared. A string parameter indexed
    // would otherwise be emitted as a list access, which is a different thing.
    for p in &f.node.params {
        if let Some(t) = c_type(&p.ty) {
            lists.insert(param_name(&p.name), t);
        }
    }
    emit_block(&mut out, &f.node.body, 1, &ret, &mut lists, &signatures)?;
    // A body that falls off the end returns the zero value, which is what the
    // interpreters do too.
    // A declared struct or enum return has no zero literal in C, so the
    // fallback is a zeroed value of that type. Emitting `return 0;` there
    // produced "incompatible types when returning type 'int' but 'CP' was
    // expected", which is a gcc error naming a C type rather than a Vortex
    // diagnostic.
    let zero = match ret.as_str() {
        "double" => "0.0",
        "const char *" => "0",
        "void" => "",
        _ => "0",
    };
    if zero.is_empty() {
        let _ = writeln!(out, "    return;");
    } else if is_declared_c_type(&ret) {
        let _ = writeln!(out, "    return ({t}){{0}};", t = ret);
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
    ("string_to_int", "vortex_string_to_int"),
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
    lists: &mut HashMap<String, String>,
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

/// The Vortex type of a base expression when it is a string, so indexing it can
/// be refused by name.
///
/// A name is looked up in the recorded bindings, a literal is its own type, and
/// anything else is not a string.
/// The C type of a base expression when it is a tuple, so indexing one is
/// refused by name rather than emitted as a list access.
fn tuple_base_type(e: &ast::Expr, lists: &HashMap<String, String>) -> Option<String> {
    match &e.kind {
        ast::ExprKind::Tuple(_) => Some("a tuple".to_string()),
        ast::ExprKind::Ident(n) => match lists.get(&param_name(n)).map(|s| s.as_str()) {
            Some(t) if t.starts_with("CTuple") => Some(t.to_string()),
            _ => None,
        },
        ast::ExprKind::Paren(inner) => tuple_base_type(inner, lists),
        _ => None,
    }
}

fn string_base_type(e: &ast::Expr, lists: &HashMap<String, String>) -> Option<&'static str> {
    match &e.kind {
        ast::ExprKind::Ident(n) => match lists.get(&param_name(n)).map(|s| s.as_str()) {
            Some("const char *") => Some("Str"),
            _ => None,
        },
        ast::ExprKind::Str(_) => Some("Str"),
        ast::ExprKind::Paren(inner) => string_base_type(inner, lists),
        _ => None,
    }
}

/// The name suffix for the constructor of a list of this element type.
///
/// A declared struct or enum is named by its C name, so the constructor carries
/// the right element size rather than the size of an int64_t.
fn list_suffix(elem: &str) -> String {
    match elem {
        "double" => "f64".to_string(),
        "const char *" => "str".to_string(),
        "int64_t" => "i64".to_string(),
        other => other.to_string(),
    }
}

/// Every declared struct and enum C name, which is what a list of one needs a
/// constructor for.
fn declared_types() -> Vec<String> {
    DECLARED.with(|d| d.borrow().iter().map(|n| format!("C{n}")).collect())
}

/// The C element type a list initialiser produces, if it produces a list.
///
/// The rule matches the one the lowering pass uses: a literal takes the type of
/// its first element, which is sound because the checker rejects a mixed list,
/// and a repeat takes the type of the value it repeats.
///
/// A declared struct or enum is a valid element type. It was not, so a list of
/// one fell back to `int64_t` and every Float field truncated, which is a wrong
/// answer rather than a diagnostic.
fn list_element_type(e: &ast::Expr) -> Option<String> {
    match &e.kind {
        ast::ExprKind::Array(items) => items.first().and_then(value_c_type),
        ast::ExprKind::Repeat { value, .. } => value_c_type(value),
        ast::ExprKind::Cast(_, t) => c_type(t),
        _ => None,
    }
}

/// The element type a list expression produces, or a refusal naming why not.
///
/// Everything that used to default to `int64_t` is checked here. A list whose
/// element type the emitter cannot name is refused at its position rather than
/// read at a type that happens to be right for `Int`.
fn list_element_type_or_refuse(e: &ast::Expr) -> Result<String, Unsupported> {
    if let Some(t) = list_element_type(e) {
        return Ok(t);
    }
    // A name has no element type of its own, so it is looked up by the caller.
    Err(Unsupported::Construct(
        format!(
            "a list of {}",
            describe_unnameable(e)
                .unwrap_or_else(|| "a value whose type the emitter cannot name".to_string())
        ),
        e.pos,
    ))
}

/// A short description of an expression whose type the emitter cannot name, used
/// in the refusal so the message names the construct rather than saying
/// "unsupported".
fn describe_unnameable(e: &ast::Expr) -> Option<String> {
    match &e.kind {
        ast::ExprKind::Ident(n) => Some(format!("`{}`, whose type the emitter cannot name", n)),
        ast::ExprKind::Call { callee, .. } => Some(format!("the result of `{callee}`")),
        ast::ExprKind::Match { .. } => Some("a `match` result".to_string()),
        ast::ExprKind::If { .. } => Some("an `if` result".to_string()),
        _ => None,
    }
}

thread_local! {
    /// What each call returns, so a value with no case of its own still has a
    /// type. A list built by repeating a call names its element type from this.
    static CALL_RETURNS: std::cell::RefCell<Vec<(String, String)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The C type of a value expression, including a declared struct or enum.
fn value_c_type(e: &ast::Expr) -> Option<String> {
    match &e.kind {
        ast::ExprKind::Int(_) => Some("int64_t".to_string()),
        ast::ExprKind::Float(_) => Some("double".to_string()),
        ast::ExprKind::Bool(_) => Some("int".to_string()),
        ast::ExprKind::Str(_) => Some("const char *".to_string()),
        ast::ExprKind::Cast(_, t) => c_type(t),
        // A record or a variant is a declared struct or enum in the generated
        // C, so its element type is that struct. Reading one as `int64_t`
        // truncated every Float field.
        ast::ExprKind::Record { ty, .. }
        | ast::ExprKind::Variant { ty, .. }
        | ast::ExprKind::VariantCall { ty, .. }
        | ast::ExprKind::VariantRecord { ty, .. } => c_type_named(ty),
        // A call's return type is recorded, so a list built by repeating a call
        // names its element type. Without this, `[origin(); 2]` had no element
        // type and both the store into it and the read were refused, which is
        // right for an unknown type and wrong here.
        ast::ExprKind::Call { callee, .. } => CALL_RETURNS.with(|r| {
            r.borrow()
                .iter()
                .find(|(n, _)| n == callee)
                .map(|(_, t)| t.clone())
        }),
        _ => None,
    }
}

fn emit_stmt(
    out: &mut String,
    s: &ast::Stmt,
    depth: usize,
    ret: &str,
    lists: &mut HashMap<String, String>,
    signatures: &Signatures,
) -> Result<(), Unsupported> {
    match &s.kind {
        ast::StmtKind::Let { name, init, .. } => {
            let c = decided_type(init, lists, signatures)?;
            // Every binding is recorded at the type its initialiser has, so a
            // later `+` on the name can tell arithmetic from concatenation and
            // an index can read at the right element type. Only lists and
            // strings were recorded before, which left every plain number
            // unknown and therefore ambiguous.
            match c.as_str() {
                "CList" => {
                    if let Some(elem) = list_element_type(init) {
                        lists.insert(param_name(name), elem);
                    }
                }
                "const char *" => {
                    lists.insert(param_name(name), "const char *".to_string());
                }
                other => {
                    lists.insert(param_name(name), other.to_string());
                }
            }
            if expr_type_of(init, lists, signatures) == "const char *" {
                lists.insert(param_name(name), "const char *".to_string());
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
                    if string_base_type(base, lists).is_some() {
                        return Err(Unsupported::Construct(
                            "storing into a string".to_string(),
                            s.pos,
                        ));
                    }
                    let elem = match &base.kind {
                        ast::ExprKind::Ident(n) => match lists.get(&param_name(n)).cloned() {
                            Some(t) => t,
                            None => {
                                return Err(Unsupported::Construct(
                                    format!(
                                    "a store into `{}`, whose element type the emitter cannot name",
                                    n
                                ),
                                    s.pos,
                                ))
                            }
                        },
                        _ => list_element_type_or_refuse(base)?,
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
            // `for x in e { ... }` with no end walks a list or a string. It was
            // refused as an open ended range, which it is not: SPEC.md section
            // 8 defines the second `for` form as walking a collection.
            let Some(end) = end else {
                return emit_for_in(out, s, var, start, body, depth, ret, lists, signatures);
            };
            let limit = if *inclusive {
                format!("({} + 1)", emit_expr(end, lists, signatures)?)
            } else {
                emit_expr(end, lists, signatures)?
            };
            // The counting form's variable is an Int binding, recorded so a
            // later `+` on it is arithmetic rather than ambiguous.
            lists.insert(param_name(var), "int64_t".to_string());
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
type Signatures = HashMap<String, String>;

fn expr_type_of(e: &ast::Expr, lists: &HashMap<String, String>, signatures: &Signatures) -> String {
    match &e.kind {
        // A list is a struct with a pointer and a length, not a scalar.
        ast::ExprKind::Array(_) | ast::ExprKind::Repeat { .. } => "CList".to_string(),
        ast::ExprKind::Bool(_) => "int".to_string(),

        ast::ExprKind::Int(_) => "int64_t".to_string(),
        ast::ExprKind::Float(_) => "double".to_string(),

        ast::ExprKind::Char(_) => "int32_t".to_string(),
        ast::ExprKind::Str(_) => "const char *".to_string(),
        ast::ExprKind::Binary { op, lhs, rhs } => match op {
            ast::BinOp::Eq
            | ast::BinOp::Ne
            | ast::BinOp::Lt
            | ast::BinOp::Le
            | ast::BinOp::Gt
            | ast::BinOp::Ge
            | ast::BinOp::And
            | ast::BinOp::Or => "int".to_string(),
            // Arithmetic is a Float when either operand is, which is the rule
            // the checker applies in section 6.1 rule 4 and the tree
            // interpreter follows. Without it `v * 0.5 + 1.0` declared an
            // integer and truncated 2.5 to 2.
            _ => {
                if expr_type_of(lhs, lists, signatures) == "double"
                    || expr_type_of(rhs, lists, signatures) == "double"
                {
                    "double".to_string()
                } else {
                    "int64_t".to_string()
                }
            }
        },
        ast::ExprKind::Neg(inner) => expr_type_of(inner, lists, signatures),
        // These eight each reached the default below, which is `int64_t` for
        // anything it does not name. That default is what turned a `Float` into
        // a silently wrong answer in stage 8, so each says what it is instead.
        // `Not` is a `Bool`, `Cast` the target it names, and the rest take the
        // type of what they carry.
        ast::ExprKind::Not(_) => "int".to_string(),
        ast::ExprKind::Paren(inner) => expr_type_of(inner, lists, signatures),
        ast::ExprKind::Cast(_, t) => c_type(t).unwrap_or_else(|| "int64_t".to_string()),
        ast::ExprKind::Assign { value, .. } => expr_type_of(value, lists, signatures),
        ast::ExprKind::Block(b) => b
            .tail
            .as_deref()
            .map(|t| expr_type_of(t, lists, signatures))
            .unwrap_or_else(|| "void".to_string()),
        ast::ExprKind::Try(inner) => expr_type_of(inner, lists, signatures),
        ast::ExprKind::Index(base, _) => index_value_type(base, lists),
        ast::ExprKind::Field(base, field) => field_value_type(base, field, lists, signatures),
        // A name's type is what it was bound to, read from the binding table
        // rather than defaulted. An unbound name does not reach the emitter in a
        // valid program; refusing it by name here makes the failure explicit
        // instead of silently `int64_t`.
        ast::ExprKind::Ident(n) => match lists.get(&param_name(n)) {
            Some(t) => t.clone(),
            None => format!("<undecided:{}>", e.kind.name()),
        },
        // A `match` has the type of its arms. The first arm is representative:
        // the checker requires every arm to agree, so reading one is not a
        // guess.
        ast::ExprKind::Match { arms, .. } => arms
            .first()
            .map(|a| expr_type_of(&a.body, lists, signatures))
            .unwrap_or_else(|| "int64_t".to_string()),
        // An `if` used as a value has the type of its branches, and the
        // checker requires them to agree. A branch is a block, so its type is
        // the type of its trailing expression.
        ast::ExprKind::If {
            then, otherwise, ..
        } => {
            let from_then =
                block_value_type(then, lists, signatures).unwrap_or_else(|| "int64_t".to_string());
            match otherwise {
                Some(alt) => match alt.as_ref() {
                    ast::Else::Block(b) => {
                        block_value_type(b, lists, signatures).unwrap_or_else(|| from_then.clone())
                    }
                    ast::Else::If(inner) => expr_type_of(&inner.node, lists, signatures),
                },
                None => from_then,
            }
        }
        // A call returns whatever the callee declares. Without this a `let`
        // holding a Float result was declared int64_t and truncated it, which
        // is why `let m = matrix_work();` lost its fraction before the entry
        // ever printed it.
        ast::ExprKind::Call { callee, .. } => signatures
            .get(callee)
            .cloned()
            .unwrap_or_else(|| "int64_t".to_string()),
        _ => c_type_of_expr(e, signatures),
    }
}

/// The C type an expression produces, refused rather than guessed when the
/// walk cannot name one.
///
/// `expr_type_of` answers `int64_t` for a value whose own type is known, but it
/// returns a `<undecided:...>` marker for a value whose type it cannot recover
/// from the recorded bindings (an unbound name, a field of a struct the emitter
/// has no declaration for, an index into something that is not a list). That
/// marker is not a C type and must not reach emission, so the call sites that
/// need a usable type go through here and refuse by name instead.
fn decided_type(
    e: &ast::Expr,
    lists: &HashMap<String, String>,
    signatures: &Signatures,
) -> Result<String, Unsupported> {
    let t = expr_type_of(e, lists, signatures);
    if t.starts_with("<undecided:") {
        return Err(Unsupported::Construct(
            format!(
                "the type of `{}`, which the emitter cannot name",
                e.kind.name()
            ),
            e.pos,
        ));
    }
    Ok(t)
}

/// The type of an expression the arithmetic rule has no answer for, which is
/// most expressions reached through a declaration rather than through their
/// own node.
/// The type of a block used as a value, which is the type of its trailing
/// expression.
fn block_value_type(
    b: &ast::Block,
    lists: &HashMap<String, String>,
    signatures: &Signatures,
) -> Option<String> {
    b.tail
        .as_deref()
        .map(|t| expr_type_of(t, lists, signatures))
}

fn c_type_of_expr(e: &ast::Expr, _signatures: &Signatures) -> String {
    match &e.kind {
        ast::ExprKind::Record { ty, .. }
        | ast::ExprKind::Variant { ty, .. }
        | ast::ExprKind::VariantCall { ty, .. }
        | ast::ExprKind::VariantRecord { ty, .. } => format!("C{}", param_name(ty)),
        ast::ExprKind::Tuple(items) => match items.len() {
            2 => "CTuple2".to_string(),
            3 => "CTuple3".to_string(),
            4 => "CTuple4".to_string(),
            n => format!("a {n} element tuple"),
        },
        // A form the walk has no arm for. It used to answer `int64_t` here, and
        // that default is what turned a `Float` into a silently wrong answer in
        // stage 8. Naming the form instead means a caller can refuse it by name,
        // and `no_form_falls_through_the_type_default` can tell that this
        // fallback was reached.
        other => format!("<undecided:{}>", other.name()),
    }
}

/// The C type a field read produces, which is the declared type of that field.
///
/// It used to reach the walk's default. A `Float` field read as an integer was
/// one of the nine instances, and this is the arm that keeps it from happening
/// again.
fn field_value_type(
    base: &ast::Expr,
    field: &str,
    lists: &HashMap<String, String>,
    signatures: &Signatures,
) -> String {
    let struct_name = match &base.kind {
        // The type a field read is taken from is the one the binding recorded
        // for the base, so a `Point` parameter reads at a Point field rather
        // than at the walk's default.
        ast::ExprKind::Ident(n) => match lists.get(&param_name(n)).map(|s| s.as_str()) {
            Some(t) if t.starts_with('C') => t[1..].to_string(),
            _ => String::new(),
        },
        ast::ExprKind::Call { callee, .. } => match signatures.get(callee).map(|s| s.as_str()) {
            Some(t) if t.starts_with('C') => t[1..].to_string(),
            _ => String::new(),
        },
        _ => String::new(),
    };
    match field_type(&struct_name, field) {
        Some(t) => t,
        // A field of a struct the emitter has no declaration for is not an
        // integer. It says so rather than answering with one.
        None => format!("<undecided:field {}>", field),
    }
}

/// The C type an index read produces, which is the element type of the list.
///
/// It used to reach the walk's default too, so a `Float` list read as an integer.
fn index_value_type(base: &ast::Expr, lists: &HashMap<String, String>) -> String {
    match &base.kind {
        ast::ExprKind::Ident(n) => match lists.get(&param_name(n)).map(|s| s.as_str()) {
            // A string is indexed by character.
            Some("const char *") => "int32_t".to_string(),
            // A list's binding holds its element type directly, whether that is
            // a scalar or a declared struct, so the read is typed at the element.
            Some(t) => t.to_string(),
            // An unknown base is named, not guessed as int64_t.
            None => "<undecided:index>".to_string(),
        },
        _ => "<undecided:index>".to_string(),
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
    lists: &mut HashMap<String, String>,
    signatures: &Signatures,
) -> Result<String, Unsupported> {
    Ok(match &e.kind {
        ast::ExprKind::Int(v) => format!("INT64_C({})", v),
        // Round-trippable, so a program's Float literal is the same value the
        // interpreters parse. Six decimals is not enough: `3.14159265` became
        // `3.141593` and an answer differed in the last digit.
        ast::ExprKind::Float(v) => format!("{:.17}", v),
        ast::ExprKind::Bool(v) => (if *v { "1" } else { "0" }).to_string(),
        ast::ExprKind::Char(c) => format!("{}", *c as u32),
        ast::ExprKind::Str(s) => format!("\"{}\"", c_escape(s)),
        ast::ExprKind::Ident(name) => param_name(name),
        ast::ExprKind::Call { callee, args } => {
            let mut parts = Vec::with_capacity(args.len());
            for a in args {
                parts.push(emit_expr(a, lists, signatures)?);
            }
            // A printing builtin takes a type tag and a value rather than a
            // `const char *`, because the tree interpreter formats whatever it
            // is given. Passing a `Char`, which is what a string index produces,
            // as a pointer segfaulted.
            if matches!(callee.as_str(), "print" | "println") {
                // `parts` already holds the plain argument expressions, so the
                // bindings are kept apart from it.
                let mut tagged: Vec<String> = Vec::with_capacity(args.len());
                let mut decls: Vec<String> = Vec::with_capacity(args.len());
                for (i, value) in parts.iter().enumerate() {
                    let tag = print_tag_of(&args[i], lists, signatures);
                    // A function call is not an lvalue in C, so its address
                    // cannot be taken. The value is bound to a local first.
                    let slot = format!("__vortex_p{}", i);
                    tagged.push(format!("{tag}, &{slot}"));
                    decls.push(format!(
                        "{t} {slot} = {v};",
                        t = print_slot_type(tag),
                        slot = slot,
                        v = value
                    ));
                }
                // The whole thing is a statement expression: the declarations,
                // then the call as its value. A comma expression cannot hold a
                // declaration, which is what the first attempt tried.
                return Ok(format!(
                    "({{ {decls} {callee}({args}); }})",
                    decls = decls.join(" "),
                    callee = param_name(callee),
                    args = tagged.join(", ")
                ));
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
            // The constructor is chosen by the element type, so a list of
            // strings builds through the string constructor rather than an
            // int64_t array, which does not compile for a string.
            let elem = list_element_type(e).unwrap_or_else(|| "int64_t".to_string());
            let suffix = list_suffix(&elem);
            format!(
                "vortex_list_new_{suffix}({n}u, ({elem}[]){{ {parts} }})",
                n = parts.len(),
                elem = elem,
                parts = parts.join(", ")
            )
        }
        ast::ExprKind::Repeat { value, count } => {
            // `[v; n]` builds a list of the value's type, so the constructor
            // is chosen by that type rather than by an index.
            let elem = decided_type(value, lists, signatures)?;
            let v = emit_expr(value, lists, signatures)?;
            let n = emit_expr(count, lists, signatures)?;
            format!(
                "vortex_list_repeat_{}({n}, {v})",
                list_suffix(&elem),
                n = n,
                v = v
            )
        }
        ast::ExprKind::Index(base, index) => {
            // A string is indexed by character, not by list element. It was
            // emitted as a list access, which reads a `void *` as an element
            // array and does not compile for a string. Refusing it by name is
            // better than emitting C that happens to compile and means
            // something else.
            // A string is indexed by character, and a Vortex `Char` is a
            // Unicode scalar, so the read is a scalar at that position rather
            // than a byte. The string walk already generates the decoding
            // helper, so the read is a call to the same one.
            if string_base_type(base, lists).is_some() {
                let b = emit_expr(base, lists, signatures)?;
                let i = emit_expr(index, lists, signatures)?;
                // A bad index fails rather than returning a placeholder. The
                // interpreters report it, and reading past the terminator is
                // undefined behaviour in C, so the generated C checks and exits
                // with a message naming the index and the string.
                return Ok(format!(
                    "({{ int32_t __vortex_c = vortex_char_at_index({b}, {i}); \
                     if (__vortex_c == -2) {{ \
                         fprintf(stderr, \"bad index: index %lld is negative\\n\", (long long)({i})); \
                         exit(1); }} \
                     if (__vortex_c < 0) {{ \
                         fprintf(stderr, \"bad index: index %lld is past the end of %lld characters\\n\", \
                                 (long long)({i}), vortex_char_count({b})); \
                         exit(1); }} \
                     __vortex_c; }})",
                    b = b,
                    i = i
                ));
            }
            // A tuple has no list representation, so indexing one emitted a list
            // access and gcc reported a request for `.items` on a struct. A
            // tuple is indexed by a pattern in a `match`, not by an index
            // expression, so this refuses it by name.
            if tuple_base_type(base, lists).is_some() {
                return Err(Unsupported::Construct(
                    "indexing a tuple".to_string(),
                    e.pos,
                ));
            }
            // The element type comes from the name the base is bound to,
            // recorded when that declaration was emitted, or from the base
            // itself when it is a literal or a repeat. It is never inferred
            // from the index, because a Float list still has an Int index.
            let elem = match &base.kind {
                ast::ExprKind::Ident(n) => match lists.get(&param_name(n)).cloned() {
                    Some(t) => t,
                    // An unknown name is not an Int list. Reading it as one
                    // truncated every Float element, so it is refused here.
                    None => {
                        return Err(Unsupported::Construct(
                            format!(
                                "an index into `{}`, whose element type the emitter cannot name",
                                n
                            ),
                            e.pos,
                        ))
                    }
                },
                _ => list_element_type_or_refuse(base)?,
            };
            let b = emit_expr(base, lists, signatures)?;
            let i = emit_expr(index, lists, signatures)?;
            format!("((({c} *)({b}).items)[({i})])", c = elem, b = b, i = i)
        }
        ast::ExprKind::Field(base, field) => {
            // `Shape.empty` parses as a field read, because the parser cannot
            // tell a field from a variant without knowing the types. The base
            // names a declared enum here, so it is a variant with no payload
            // rather than a field read.
            if let ast::ExprKind::Ident(ty) = &base.kind {
                if is_declared_enum(ty) {
                    let index = enum_variant_index(ty, field).ok_or_else(|| {
                        Unsupported::Construct(
                            format!("the variant `{v}` of enum `{t}`", v = field, t = ty),
                            e.pos,
                        )
                    })?;
                    let c = format!("C{}", param_name(ty));
                    return Ok(format!(
                        "(({c}){{.tag = 0, .variant = {i}}})",
                        c = c,
                        i = index
                    ));
                }
            }
            // A field read on a declared struct. The base is a plain value, not
            // a pointer, so the field is read directly.
            let b = emit_expr(base, lists, signatures)?;
            return Ok(format!("({b}).{f}", b = b, f = param_name(field)));
        }
        // A struct is built as a compound literal of its generated C struct,
        // which is what a record expression is in v0.1.
        ast::ExprKind::Record { ty, fields } => {
            let c = format!("C{}", param_name(ty));
            let mut inits: Vec<String> = vec![".tag = 0".to_string()];
            for (name, value) in fields {
                inits.push(format!(
                    ".{f} = {v}",
                    f = param_name(name),
                    v = emit_expr(value, lists, signatures)?
                ));
            }
            return Ok(format!(
                "(({c}){{{inits}}})",
                c = c,
                inits = inits.join(", ")
            ));
        }
        // A variant with no payload, a positional payload, or named fields all
        // become the tagged struct with its payloads in p0, p1 and p2.
        ast::ExprKind::Variant { ty, variant }
        | ast::ExprKind::VariantCall { ty, variant, .. }
        | ast::ExprKind::VariantRecord { ty, variant, .. } => {
            let index = enum_variant_index(ty, variant).ok_or_else(|| {
                Unsupported::Construct(format!("the variant `{}` of enum `{}`", variant, ty), e.pos)
            })?;
            // The payloads are stored in an array typed by the variant's
            // declarations, so a Float payload is stored as a double.
            let types = variant_payload_types(ty, variant);
            let mut values: Vec<String> = Vec::new();
            match &e.kind {
                ast::ExprKind::VariantCall { args, .. } => {
                    for a in args {
                        values.push(emit_expr(a, lists, signatures)?);
                    }
                }
                ast::ExprKind::VariantRecord { fields, .. } => {
                    for (_, value) in fields {
                        values.push(emit_expr(value, lists, signatures)?);
                    }
                }
                _ => {}
            }
            let c = format!("C{}", param_name(ty));
            let mut inits = vec![".tag = 0".to_string(), format!(".variant = {index}")];
            if !values.is_empty() {
                let vals = values.join(", ");
                if types.iter().all(|t| *t == types[0]) {
                    // One type throughout, so a C array carries it.
                    inits.push(format!(
                        ".payload = (({t}[]){{{vals}}})",
                        t = types[0],
                        vals = vals
                    ));
                } else {
                    // Mixed payload types are not a C array, so they go in a
                    // generated struct with one field each. The read side
                    // reaches them by the same field.
                    let name = payload_struct_name(ty, variant);
                    let fields: Vec<String> = types
                        .iter()
                        .enumerate()
                        .map(|(i, t)| format!("{t} f{i};"))
                        .collect();
                    PAYLOAD_STRUCTS.with(|d| {
                        let mut v = d.borrow_mut();
                        if !v.iter().any(|(n, _)| *n == name) {
                            v.push((name.clone(), fields.join(" ")));
                        }
                    });
                    inits.push(format!(
                        ".payload = (void *)&(({name}){{{vals}}})",
                        name = name,
                        vals = vals
                    ));
                }
            } else {
                inits.push(".payload = 0".to_string());
            }
            return Ok(format!(
                "(({c}){{{inits}}})",
                c = c,
                inits = inits.join(", ")
            ));
        }
        ast::ExprKind::Tuple(items) => {
            if !(2..=4).contains(&items.len()) {
                return Err(Unsupported::Construct(
                    format!("a tuple of {} elements", items.len()),
                    e.pos,
                ));
            }
            let parts: Result<Vec<String>, Unsupported> = items
                .iter()
                .map(|i| emit_expr(i, lists, signatures))
                .collect();
            let parts = parts?;
            let inits: Vec<String> = parts
                .iter()
                .enumerate()
                .map(|(i, p)| format!(".f{i} = {p}"))
                .collect();
            format!(
                "((CTuple{n}){{{inits}}})",
                n = items.len(),
                inits = inits.join(", ")
            )
        }
        ast::ExprKind::Match { scrutinee, arms } => {
            return emit_match(e, scrutinee, arms, lists, signatures);
        }
        // An `if` used as a value is a GNU statement expression: the condition
        // is assigned to a declaration, and the two branches are selected with
        // a conditional operator on it. The type is the type of the branches,
        // which is what `expr_type_of` gives.
        ast::ExprKind::If {
            cond,
            then,
            otherwise,
        } => {
            let ty = decided_type(e, lists, signatures)?;
            let c = format!("__vortex_c{}_{}", e.pos.line, e.pos.col);
            let t = emit_block_expr(then, lists, signatures)?;
            let o = match otherwise {
                Some(alt) => match alt.as_ref() {
                    ast::Else::Block(b) => emit_block_expr(b, lists, signatures)?,
                    ast::Else::If(inner) => {
                        // An else-if chain nests, so the inner if is emitted the
                        // same way and becomes the false branch.
                        emit_expr(&inner.node, lists, signatures)?
                    }
                },
                None => format!("({ty})0", ty = ty),
            };
            // A statement expression needs its braces and a trailing
            // semicolon on the final expression, or gcc reports
            // "expected ')' before the declared name".
            return Ok(format!(
                "({{ {ty} {c} = {cond}; ({c} ? {t} : {o}); }})",
                ty = ty,
                c = c,
                cond = emit_expr(cond, lists, signatures)?,
                t = t,
                o = o
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

/// Emits a block used as an expression, which is its trailing expression
/// wrapped in a statement expression so it is an expression in C.
fn emit_block_expr(
    b: &ast::Block,
    lists: &mut HashMap<String, String>,
    signatures: &Signatures,
) -> Result<String, Unsupported> {
    match &b.tail {
        Some(t) => emit_expr(t, lists, signatures),
        None => Err(Unsupported::Construct(
            "a block with no trailing expression used as a value".to_string(),
            b.pos,
        )),
    }
}

/// Emits `for x in e { ... }`, which walks a list.
///
/// A list is a pointer and a length, so the loop is an index over its elements
/// reading at the element type recorded for that list. A string is walked by
/// byte, which is what the interpreters do; the element type is `char`.

fn emit_for_in(
    out: &mut String,
    s: &ast::Stmt,
    var: &str,
    source: &ast::Expr,
    body: &ast::Block,
    depth: usize,
    ret: &str,
    lists: &mut HashMap<String, String>,
    signatures: &Signatures,
) -> Result<(), Unsupported> {
    // `for` can walk a list, a string or a tuple, and each has a different C
    // representation. Which one it is has to be decided here rather than
    // inferred: this used to assume a list, so walking a string bound a `const
    // char *` to a `CList` and gcc reported "invalid initializer", which says
    // nothing about Vortex.
    if string_base_type(source, lists).is_some() {
        return emit_for_in_string(out, s, var, source, body, depth, ret, lists, signatures);
    }
    let src = emit_expr(source, lists, signatures)?;

    // A tuple is not a list, so binding it to a `CList` is the same mistake in a
    // different shape. It is refused by name rather than emitted wrongly.
    //
    // The check covers a tuple bound to a name as well as a literal. A name is
    // recorded at its `CTuple` type when it is declared, so a literal-only check
    // let `let t = (1, 2); for x in t` through and emitted a `CList`
    // initialisation, which is the gcc error this is meant to remove.
    if tuple_base_type(source, lists).is_some() {
        return Err(Unsupported::Construct(
            "`for ..in` over a tuple".to_string(),
            s.pos,
        ));
    }

    let elem = match &source.kind {
        ast::ExprKind::Ident(n) => match lists.get(&param_name(n)).cloned() {
            Some(t) => t,
            // An unrecorded name is not a list of Int. Reading it as one was the
            // stage 12 defect, so it is refused rather than assumed.
            None => {
                return Err(Unsupported::Construct(
                    format!(
                        "`for ..in` over `{}`, whose element type the emitter cannot name",
                        n
                    ),
                    s.pos,
                ))
            }
        },
        _ => list_element_type_or_refuse(source)?,
    };

    // The loop variable is a binding at the element type, so a later `+` on it
    // can tell arithmetic from concatenation.
    lists.insert(param_name(var), elem.clone());

    indent(out, depth);
    // The collection is bound once so the loop is not recomputing it, and the
    // loop variable reads its element at the recorded element type.
    //
    // The whole loop is wrapped in a C block so its temporaries are scoped to
    // it. Without that, two list loops over the same variable name in one
    // function declared the same temporaries twice and gcc reported a
    // redefinition.
    let collection = format!("__vortex_for_{}", param_name(var));
    let _ = writeln!(out, "{{");
    indent(out, depth + 1);
    let _ = writeln!(out, "    CList {c} = {s};", c = collection, s = src);
    indent(out, depth + 1);
    let _ = writeln!(
        out,
        "    for (int64_t __vortex_i = 0; __vortex_i < {c}.len; __vortex_i++) {{",
        c = collection
    );
    indent(out, depth + 2);
    let _ = writeln!(
        out,
        "        {t} {v} = (({t} *)({c}.items))[__vortex_i];",
        t = elem,
        v = param_name(var),
        c = collection
    );
    emit_block(out, body, depth + 2, ret, lists, signatures)?;
    indent(out, depth + 1);
    let _ = writeln!(out, "    }}");
    indent(out, depth);
    let _ = writeln!(out, "}}");
    Ok(())
}

/// Emits `for c in s { ... }` over a string.
///
/// A Vortex string walks by character, which is a Unicode scalar and not a
/// byte, so the loop is over the decoded characters rather than over the
/// bytes. The helper decodes one scalar per call, so what the loop variable
/// holds is a character, matching the tree interpreter.
fn emit_for_in_string(
    out: &mut String,
    s: &ast::Stmt,
    var: &str,
    source: &ast::Expr,
    body: &ast::Block,
    depth: usize,
    ret: &str,
    lists: &mut HashMap<String, String>,
    signatures: &Signatures,
) -> Result<(), Unsupported> {
    let src = emit_expr(source, lists, signatures)?;
    // The loop variable is a `Char` binding, not a list element.
    lists.insert(param_name(var), "int32_t".to_string());

    indent(out, depth);
    // Wrapped in a C block for the same reason the list loop is: two string
    // loops over the same variable name in one function redefined `__vortex_n`
    // and `__vortex_i`, and gcc reported the redefinition.
    let collection = format!("__vortex_s_{}", param_name(var));
    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "    const char *{c} = {s};", c = collection, s = src);
    let _ = writeln!(
        out,
        "    int64_t __vortex_n = (int64_t)strlen({c});",
        c = collection
    );
    // A `while` rather than a `for`, because a C `for` increments as well and a
    // character's length is not one byte. The body owns the advance.
    indent(out, depth + 1);
    let _ = writeln!(out, "    int64_t __vortex_i = 0;");
    let _ = writeln!(out, "    while (__vortex_i < __vortex_n) {{");
    indent(out, depth + 2);
    // Each step decodes the next scalar and advances the index past it, so a
    // multi byte character is one iteration rather than one per byte.
    let _ = writeln!(
        out,
        "        int32_t {v} = vortex_char_at({c}, __vortex_i);",
        v = param_name(var),
        c = collection
    );
    let _ = writeln!(
        out,
        "        __vortex_i += vortex_char_len({c}, __vortex_i);",
        c = collection
    );
    emit_block(out, body, depth + 2, ret, lists, signatures)?;
    indent(out, depth + 1);
    let _ = writeln!(out, "    }}");
    indent(out, depth);
    let _ = writeln!(out, "}}");
    let _ = s;
    Ok(())
}

/// Emits a `match` as a GNU statement expression.
///
/// A statement expression is `({ declarations; value; })`, which is an
/// expression in C and therefore usable where a `let` initialiser is. The
/// semantics are the ones the tree interpreter implements: the arms are tried
/// in order and the first that matches supplies the value, so this emits a
/// chain of ternaries with the arm bindings declared before it.
fn emit_match(
    e: &ast::Expr,
    scrutinee: &ast::Expr,
    arms: &[ast::Arm],
    lists: &mut HashMap<String, String>,
    signatures: &Signatures,
) -> Result<String, Unsupported> {
    if arms.is_empty() {
        return Err(Unsupported::Construct(
            "a match with no arms".to_string(),
            e.pos,
        ));
    }
    let ty = {
        // A match has the type of its first arm's body. That body's names are the
        // first arm's pattern bindings, so record them before asking the walk —
        // otherwise a binding pulled out of a variant payload, such as `n` in
        // `Pair.mixed { n, r } => n`, is an unbound name and the type decision
        // would refuse it instead of reading the payload's real type.
        for p in &arms[0].patterns {
            record_pattern_bindings(p, lists);
        }
        decided_type(e, lists, signatures)?
    };
    let subject = emit_expr(scrutinee, lists, signatures)?;

    // Each arm contributes one declaration and one test. The first matching arm
    // is chosen by testing in order and stopping, which is what nested
    // ternaries do.
    let mut value = format!("({ty})0", ty = ty);
    for arm in arms.iter().rev() {
        let test = match_pattern_test(&arm.patterns, &subject, lists, signatures)?;
        // One statement expression per arm, so a name bound in one arm is not
        // visible in another and two arms may bind the same name.
        // The pattern's bindings become ordinary C locals before the body, so
        // the body needs no rewriting. The statement expression's value is its
        // last expression, which is the body.
        // The arm's bindings are ordinary names in its body, so they are
        // recorded as bindings. Without that a `+` on a pattern binding was
        // unclassified and refused, which is what a two payload variant hit.
        for p in &arm.patterns {
            record_pattern_bindings(p, lists);
        }
        let decls = declare_pattern_bindings(&arm.patterns, &subject);
        let body = emit_expr(&arm.body, lists, signatures)?;
        // With no declarations there is nothing for a statement expression to
        // do, so the body is emitted on its own.
        let arm_expr = if decls.trim().is_empty() {
            format!("({body})")
        } else {
            format!("({{ {decls} {body}; }})")
        };
        value = if test.is_empty() {
            arm_expr
        } else {
            format!("(({test}) ? {arm_expr} : {value})")
        };
    }
    Ok(format!("({{ {value}; }})"))
}

/// The C boolean expression testing one arm's patterns against the subject.
///
/// An empty result means the arm always matches, which is what a wildcard or a
/// binding-only pattern is. Several patterns separated by `|` are tested in
/// order and the first that matches wins.
fn match_pattern_test(
    patterns: &[ast::Pattern],
    subject: &str,
    lists: &mut HashMap<String, String>,
    signatures: &Signatures,
) -> Result<String, Unsupported> {
    if patterns.is_empty() {
        return Ok(String::new());
    }
    let mut tests = Vec::with_capacity(patterns.len());
    for p in patterns {
        match_pattern_test_one(p, subject, lists, signatures).map(|t| tests.push(t))?;
    }
    if tests.is_empty() {
        return Ok(String::new());
    }
    if tests.len() == 1 {
        return Ok(tests.remove(0));
    }
    Ok(format!("({})", tests.join(" || ")))
}

fn match_pattern_test_one(
    p: &ast::Pattern,
    subject: &str,
    lists: &mut HashMap<String, String>,
    signatures: &Signatures,
) -> Result<String, Unsupported> {
    match &p.kind {
        ast::PatternKind::Wildcard => Ok(String::new()),
        ast::PatternKind::Binding(_) => Ok(String::new()),
        ast::PatternKind::Literal(c) => {
            let v = emit_expr(c, lists, signatures)?;
            Ok(format!("(({s}) == ({v}))", s = subject))
        }
        ast::PatternKind::Tuple(items) => {
            let mut tests = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                let elem = format!("({s}).f{i}", s = subject);
                let t = match_pattern_test_one(item, &elem, lists, signatures)?;
                // A wildcard or a bare binding contributes no test of its own,
                // so an empty one is dropped rather than joined as nothing.
                if !t.is_empty() {
                    tests.push(t);
                }
            }
            if tests.is_empty() {
                return Ok(String::new());
            }
            if tests.len() == 1 {
                return Ok(tests.remove(0));
            }
            Ok(format!("({})", tests.join(" && ")))
        }
        ast::PatternKind::Variant {
            ty: type_name,
            variant,
            ..
        } => {
            let mangled = param_name(type_name);
            let index = enum_variant_index(type_name, variant).ok_or_else(|| {
                Unsupported::Construct(
                    format!(
                        "the variant `{}` of an enum the emitter does not know",
                        variant
                    ),
                    p.pos,
                )
            })?;
            let _ = mangled;
            Ok(format!(
                "(({s}).tag == 0 && ({s}).variant == {i})",
                s = subject,
                i = index
            ))
        }
    }
}

/// Records the names a pattern binds, with the C type of what they hold, so a
/// `+` on one can tell arithmetic from concatenation.
fn record_pattern_bindings(p: &ast::Pattern, lists: &mut HashMap<String, String>) {
    match &p.kind {
        ast::PatternKind::Binding(name) => {
            lists.insert(param_name(name), "int64_t".to_string());
        }
        ast::PatternKind::Tuple(items) => {
            for item in items {
                record_pattern_bindings(item, lists);
            }
        }
        ast::PatternKind::Variant {
            ty,
            variant,
            bindings,
            ..
        } => {
            for (i, name) in bindings.iter().enumerate() {
                lists.insert(param_name(name), variant_payload_type(ty, variant, i));
            }
        }
        ast::PatternKind::Wildcard | ast::PatternKind::Literal(_) => {}
    }
}

/// The C declarations a pattern's bindings need, so an arm body can refer to
/// them by name.
///
/// A tuple pattern binds element by element, and a variant pattern binds its
/// payloads, which the generated struct carries in p0, p1 and p2.
fn declare_pattern_bindings(patterns: &[ast::Pattern], subject: &str) -> String {
    let mut out = String::new();
    for p in patterns {
        declare_pattern_bindings_one(p, subject, &mut out);
    }
    out
}

fn declare_pattern_bindings_one(p: &ast::Pattern, subject: &str, out: &mut String) {
    match &p.kind {
        ast::PatternKind::Binding(name) => {
            out.push_str(&format!(
                "int64_t {n} = {s}; ",
                n = param_name(name),
                s = subject
            ));
        }
        ast::PatternKind::Tuple(items) => {
            for (i, item) in items.iter().enumerate() {
                let elem = format!("({s}).f{i}", s = subject);
                declare_pattern_bindings_one(item, &elem, out);
            }
        }
        ast::PatternKind::Variant {
            ty,
            variant,
            bindings,
            ..
        } => {
            for (i, name) in bindings.iter().enumerate() {
                let c = variant_payload_type(ty, variant, i);
                let slot = format!("((({t} *)({s}).payload)[{i}])", t = c, s = subject, i = i);
                out.push_str(&format!(
                    "{c} {n} = {slot}; ",
                    n = param_name(name),
                    slot = slot
                ));
            }
        }
        ast::PatternKind::Wildcard | ast::PatternKind::Literal(_) => {}
    }
}

/// Whether a name is a declared enum, which is how a variant is told from a
/// struct field after parsing.
fn is_declared_enum(name: &str) -> bool {
    let mangled = param_name(name);
    ENUM_VARIANTS.with(|e| e.borrow().iter().any(|(n, _)| *n == mangled))
}

thread_local! {
    /// The generated struct each mixed type variant is stored in, by name, so
    /// the declaration can be emitted before any function that uses it.
    static PAYLOAD_STRUCTS: std::cell::RefCell<Vec<(String, String)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The name of the generated struct a variant's payload fields are stored in
/// when their types differ.
fn payload_struct_name(enum_name: &str, variant: &str) -> String {
    format!("CP{}", param_name(&format!("{}_{}", enum_name, variant)))
}

/// Every payload C type of a variant, in declaration order.
fn variant_payload_types(enum_name: &str, variant: &str) -> Vec<String> {
    let mangled = param_name(enum_name);
    ENUM_VARIANTS.with(|e| {
        e.borrow()
            .iter()
            .find(|(n, _)| *n == mangled)
            .and_then(|(_, variants)| {
                variants
                    .iter()
                    .find(|(v, _)| v == variant)
                    .map(|(_, types)| types.clone())
            })
            .unwrap_or_default()
    })
}

/// The C type of a variant's payload at a position, so a binding reads the
/// payload slot at the type the enum declared rather than always as an
/// int64_t.
fn variant_payload_type(enum_name: &str, variant: &str, slot: usize) -> String {
    let mangled = param_name(enum_name);
    ENUM_VARIANTS.with(|e| {
        e.borrow()
            .iter()
            .find(|(n, _)| *n == mangled)
            .and_then(|(_, variants)| {
                variants
                    .iter()
                    .find(|(v, _)| v == variant)
                    .and_then(|(_, types)| types.get(slot).cloned())
            })
            .unwrap_or_else(|| "int64_t".to_string())
    })
}

/// The index of a named variant within its enum, which the generated tag
/// compares against.
fn enum_variant_index(enum_name: &str, variant: &str) -> Option<usize> {
    ENUM_VARIANTS.with(|e| {
        e.borrow()
            .iter()
            .find(|(en, _)| en == enum_name)
            .and_then(|(_, variants)| variants.iter().position(|(v, _)| v == variant))
    })
}

thread_local! {
    /// Each declared enum, with its variants in declaration order and the C
    /// type of each payload.
    static ENUM_VARIANTS: std::cell::RefCell<Vec<(String, Vec<(String, Vec<String>)>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
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
    lists: &mut HashMap<String, String>,
    signatures: &Signatures,
) -> Result<String, Unsupported> {
    let adding = matches!(op, ast::BinOp::Add);
    let lhs_str = is_string_expr_in(lhs, lists);
    let rhs_str = is_string_expr_in(rhs, lists);
    let string_plus = adding && (lhs_str || rhs_str);
    if adding && !string_plus && !both_numeric(lhs, rhs, lists, signatures) {
        // Neither side is a string the emitter can see and the pair is not
        // plainly arithmetic, so `+` might be a concatenation. Emitting C `+`
        // would then be pointer arithmetic, so it is refused rather than
        // guessed.
        return Err(Unsupported::Construct(
            "`+` where the emitter cannot tell whether an operand is a string, so it cannot choose between concatenation and arithmetic".to_string(),
            pos,
        ));
    }
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
/// Whether a `+` is plainly arithmetic rather than possibly a concatenation.
///
/// Both operands have to be a number the emitter can see: an integer literal,
/// a recorded `Int` or `Float` name, or a call whose declared return type is
/// one. Anything else could be a string, and for a string the C `+` operator
/// is address arithmetic rather than concatenation.
fn both_numeric(
    lhs: &ast::Expr,
    rhs: &ast::Expr,
    lists: &mut HashMap<String, String>,
    signatures: &Signatures,
) -> bool {
    numeric_in(lhs, lists, signatures) && numeric_in(rhs, lists, signatures)
}

/// Whether a C type is one of the numeric scalars, which is what a `+` on a
/// value has to be for the operator to be arithmetic rather than concatenation.
fn is_numeric_c_type(t: &str) -> bool {
    matches!(t, "int64_t" | "double" | "int" | "int32_t")
}

/// The C type of the local a printing argument is bound to, which follows from
/// the tag the helper uses to read it.
fn print_slot_type(tag: &str) -> &'static str {
    match tag {
        "1" => "int32_t",
        "2" => "int64_t",
        _ => "const char *",
    }
}

/// The type tag the generated `print` uses to choose a format: 0 is text, 1 is
/// a `Char`, 2 is a `Bool`.
fn print_tag_of(
    e: &ast::Expr,
    lists: &HashMap<String, String>,
    signatures: &Signatures,
) -> &'static str {
    let _ = signatures;
    let name = match &e.kind {
        ast::ExprKind::Str(_) => return "0",
        ast::ExprKind::Paren(inner) => return print_tag_of(inner, lists, signatures),
        ast::ExprKind::Ident(n) => lists.get(&param_name(n)).map(|s| s.as_str()).unwrap_or("0"),
        // An index produces a `Char` from a string, and otherwise the element
        // type of the list. Without this an element of a list of `Bool` was
        // tagged as text and `print` read an int as a pointer, which
        // segfaulted. The element type is the same classification the list
        // constructor is chosen by.
        ast::ExprKind::Index(base, _) => {
            if string_base_type(base, lists).is_some() {
                return "1";
            }
            return match index_element_type(base, lists) {
                Some(t) => print_tag_of_c_type(&t),
                None => "0",
            };
        }
        _ => "0",
    };
    print_tag_of_c_type(name)
}

/// The tag for a C type, which is the one place the mapping from a Vortex type to
/// a print format is written down.
fn print_tag_of_c_type(name: &str) -> &'static str {
    match name {
        "int32_t" => "1",
        "int" => "2",
        _ => "0",
    }
}

/// The element type an index expression produces, which is the element type of
/// the list it indexes.
fn index_element_type(e: &ast::Expr, lists: &HashMap<String, String>) -> Option<String> {
    match &e.kind {
        ast::ExprKind::Ident(n) => lists.get(&param_name(n)).cloned(),
        ast::ExprKind::Paren(inner) => index_element_type(inner, lists),
        _ => list_element_type(e),
    }
}

/// Whether a builtin returns a number, for the builtins the emitter provides.
/// A call to one of these is numeric whether or not the program declared it.
fn builtin_returns_number(name: &str) -> bool {
    matches!(name, "string_to_int" | "len" | "int_to_string_len")
}

/// The declared C type of a struct field, from the program's declarations.
fn field_type(struct_name: &str, field: &str) -> Option<String> {
    FIELD_TYPES.with(|d| {
        d.borrow()
            .iter()
            .find(|(s, f, _)| *s == struct_name && *f == field)
            .map(|(_, _, t)| t.clone())
    })
}

thread_local! {
    /// Every struct's fields with their C types, so a field read knows its own
    /// type rather than being classified from the base's.
    static FIELD_TYPES: std::cell::RefCell<Vec<(String, String, String)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn numeric_in(e: &ast::Expr, lists: &mut HashMap<String, String>, signatures: &Signatures) -> bool {
    match &e.kind {
        ast::ExprKind::Int(_) | ast::ExprKind::Float(_) | ast::ExprKind::Bool(_) => true,
        ast::ExprKind::Cast(_, t) => t.name() == "Int" || t.name() == "Float",
        ast::ExprKind::Paren(inner) => numeric_in(inner, lists, signatures),
        // A field read is numeric when the field is declared numeric, which the
        // struct declaration says. This is the fourth instance of the shape
        // STAGE14.md describes: a value with no case here falls through to being
        // treated as unknown, and whether that means a wrong answer or a
        // refusal depends on where the missing case sat.
        ast::ExprKind::Field(base, field) => {
            // The base is a variable holding the struct, so the struct's name is
            // the base's recorded type, which is `C` plus the declared name.
            // Reading the base's own name as though it were the type is what
            // made the first attempt find nothing.
            // The base is a name holding a struct, a list element whose
            // element type is a struct, or a call that returned one. Each is
            // resolved the same way, through the type recorded for it.
            let recorded = match &base.kind {
                ast::ExprKind::Ident(n) => lists.get(&param_name(n)).cloned(),
                ast::ExprKind::Index(list_base, _) => match &list_base.kind {
                    ast::ExprKind::Ident(n) => lists.get(&param_name(n)).cloned(),
                    _ => None,
                },
                ast::ExprKind::Call { callee, .. } => CALL_RETURNS.with(|r| {
                    r.borrow()
                        .iter()
                        .find(|(n, _)| n == callee)
                        .map(|(_, t)| t.clone())
                }),
                _ => None,
            };
            let struct_name = match recorded {
                Some(t) if t.starts_with('C') => t[1..].to_string(),
                _ => String::new(),
            };
            match field_type(&struct_name, field) {
                Some(t) => is_numeric_c_type(&t),
                // A field of a struct the emitter does not have a declaration
                // for is not classified, which is honest rather than a guess.
                None => false,
            }
        }
        // A recorded name is numeric when it was recorded as a number.
        ast::ExprKind::Ident(n) => match lists.get(&param_name(n)).map(|s| s.as_str()) {
            Some("int64_t") | Some("double") | Some("int") | Some("int32_t") => true,
            Some("const char *") => false,
            _ => false,
        },
        // A call is numeric when the callee returns a number, which for a
        // builtin is the type its helper returns. `string_to_int` was missing
        // from this, so a `+` on it was unclassified, which is the fifth
        // instance of the shape STAGE14.md describes.
        ast::ExprKind::Call { callee, .. } => {
            let declared = signatures.get(callee).map(|s| s.as_str());
            match declared {
                Some(t) => is_numeric_c_type(t),
                None => builtin_returns_number(callee),
            }
        }
        // A comparison is a Bool, which is numeric, and an arithmetic or
        // boolean subexpression is numeric when its operands are.
        ast::ExprKind::Binary { op, lhs, rhs } => {
            if matches!(
                op,
                ast::BinOp::Eq
                    | ast::BinOp::Ne
                    | ast::BinOp::Lt
                    | ast::BinOp::Le
                    | ast::BinOp::Gt
                    | ast::BinOp::Ge
                    | ast::BinOp::And
                    | ast::BinOp::Or
            ) {
                true
            } else {
                numeric_in(lhs, lists, signatures) && numeric_in(rhs, lists, signatures)
            }
        }
        // An index reads the element of a list, which is a list the emitter
        // recorded, and a list of a number is a number.
        ast::ExprKind::Index(base, _) => match &base.kind {
            ast::ExprKind::Ident(n) => matches!(
                lists.get(&param_name(n)).map(|s| s.as_str()),
                Some("int64_t") | Some("double") | Some("int") | Some("int32_t")
            ),
            _ => true,
        },
        ast::ExprKind::Neg(inner) | ast::ExprKind::Not(inner) => {
            numeric_in(inner, lists, signatures)
        }
        _ => false,
    }
}

fn is_string_expr_in(e: &ast::Expr, lists: &HashMap<String, String>) -> bool {
    match &e.kind {
        ast::ExprKind::Ident(n) => matches!(
            lists.get(&param_name(n)).map(|s| s.as_str()),
            Some("const char *")
        ),
        ast::ExprKind::Paren(inner) => is_string_expr_in(inner, lists),
        _ => is_string_expr(e),
    }
}

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
    items: &[ast::Item],
    entry: &str,
    args: &[String],
) -> Result<String, Unsupported> {
    let functions: Vec<Spanned<ast::FnDecl>> = items
        .iter()
        .filter_map(|i| match i {
            ast::Item::Function(f) => Some(f.clone()),
            _ => None,
        })
        .collect();
    let functions = &functions[..];

    // Record the struct and enum names, so c_type can map a declared type to
    // its generated C name and refuse one the program does not declare.
    {
        let mut names = Vec::new();
        let mut variants = Vec::new();
        for item in items {
            match item {
                ast::Item::Struct(sd) => names.push(param_name(&sd.node.name)),
                ast::Item::Enum(ed) => {
                    let n = param_name(&ed.node.name);
                    names.push(n.clone());
                    variants.push((
                        n,
                        ed.node
                            .variants
                            .iter()
                            .map(|v| {
                                // A variant's payloads have declared types, and
                                // a binding has to read the payload slot at that
                                // type rather than always as an int64_t.
                                let types: Vec<String> = v
                                    .payloads
                                    .iter()
                                    .map(|t| c_type(t).unwrap_or_else(|| "int64_t".to_string()))
                                    .chain(v.named.iter().map(|f| {
                                        c_type(&f.ty).unwrap_or_else(|| "int64_t".to_string())
                                    }))
                                    .collect();
                                (v.name.clone(), types)
                            })
                            .collect(),
                    ));
                }
                ast::Item::Function(_) => {}
            }
        }
        // Every struct field is recorded at its declared type, so a read of one
        // knows what it holds. A field read had no case, which is the fourth
        // instance of the shape STAGE14.md describes.
        let mut fields = Vec::new();
        for item in items {
            if let ast::Item::Struct(sd) = item {
                for f in &sd.node.fields {
                    fields.push((
                        sd.node.name.clone(),
                        f.name.clone(),
                        c_type(&f.ty).unwrap_or_else(|| "void *".to_string()),
                    ));
                }
            }
        }
        DECLARED.with(|d| *d.borrow_mut() = names);
        ENUM_VARIANTS.with(|v| *v.borrow_mut() = variants);
        FIELD_TYPES.with(|f| *f.borrow_mut() = fields);
    }
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
    // Encodes one Unicode scalar as UTF-8, so a `Char` above U+FFFF prints as
    // the character rather than as a truncated one. `fputc` takes an int, and a
    // scalar such as U+1F600 does not fit the byte it writes, which printed a
    // space for the emoji.
    let _ = writeln!(
        out,
        "static int vortex_utf8_encode(int32_t cp, char *out) {{"
    );
    let _ = writeln!(out, "    unsigned int c = (unsigned int)cp;");
    let _ = writeln!(out, "    if (c < 0x80) {{ out[0] = (char)c; return 1; }}");
    let _ = writeln!(out, "    if (c < 0x800) {{");
    let _ = writeln!(out, "        out[0] = (char)(0xC0 | (c >> 6));");
    let _ = writeln!(out, "        out[1] = (char)(0x80 | (c & 0x3F)); return 2;");
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "    if (c < 0x10000) {{");
    let _ = writeln!(out, "        out[0] = (char)(0xE0 | (c >> 12));");
    let _ = writeln!(out, "        out[1] = (char)(0x80 | ((c >> 6) & 0x3F));");
    let _ = writeln!(out, "        out[2] = (char)(0x80 | (c & 0x3F)); return 3;");
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "    out[0] = (char)(0xF0 | (c >> 18));");
    let _ = writeln!(out, "    out[1] = (char)(0x80 | ((c >> 12) & 0x3F));");
    let _ = writeln!(out, "    out[2] = (char)(0x80 | ((c >> 6) & 0x3F));");
    let _ = writeln!(out, "    out[3] = (char)(0x80 | (c & 0x3F)); return 4;");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out, "");

    // `string_to_int` parses leading whitespace and a sign, and returns 0 for
    // anything it cannot read, which is what the tree interpreter does. It was
    // absent from the emitter entirely, so a call to it was unclassified.
    let _ = writeln!(out, "static int64_t vortex_string_to_int(const char *s) {{");
    let _ = writeln!(out, "    while (*s == ' ' || *s == '\\t') s++;");
    let _ = writeln!(out, "    int neg = 0;");
    let _ = writeln!(
        out,
        "    if (*s == '-') {{ neg = 1; s++; }} else if (*s == '+') s++;"
    );
    let _ = writeln!(out, "    int64_t v = 0;");
    let _ = writeln!(out, "    int any = 0;");
    let _ = writeln!(
        out,
        "    while (*s >= '0' && *s <= '9') {{ v = v * 10 + (*s - '0'); s++; any = 1; }}"
    );
    let _ = writeln!(out, "    if (!any) return 0;");
    let _ = writeln!(out, "    return neg ? -v : v;");
    let _ = writeln!(out, "}}");
    // `print` and `println` take a type tag and a value, because the tree
    // interpreter formats whatever it is given. The C helpers took a
    // `const char *`, so printing a `Char`, which is what a string index
    // produces, passed an int32_t as a pointer and the program segfaulted.
    for name in ["vortex_print", "vortex_println"] {
        let _ = writeln!(out, "static void {name}(int tag, void *v) {{");
        let _ = writeln!(out, "    if (tag == 1) {{");
        let _ = writeln!(out, "        int32_t cp = *(int32_t *)v;");
        let _ = writeln!(out, "        char utf8[4];");
        let _ = writeln!(out, "        int n = vortex_utf8_encode(cp, utf8);");
        let _ = writeln!(out, "        fwrite(utf8, 1, (size_t)n, stdout);");
        let _ = writeln!(out, "    }}");
        let _ = writeln!(
            out,
            "    else if (tag == 2) fputs(*(int64_t *)v ? \"true\" : \"false\", stdout);"
        );
        let _ = writeln!(out, "    else fputs(*(const char **)v, stdout);");
        if name == "vortex_println" {
            let _ = writeln!(out, "    fputc('\\n', stdout);");
        }
        let _ = writeln!(out, "}}");
    }
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
    // Structs and enums, each as a tagged C struct so `match` can test which
    // one a value is. A struct is a tag plus its fields; an enum is a tag, a
    // variant index and up to two payload slots, which covers the positional
    // and named payload shapes v0.1 has.
    for item in items {
        match item {
            ast::Item::Struct(sd) => {
                let fields: Vec<String> = sd
                    .node
                    .fields
                    .iter()
                    .map(|f| {
                        format!(
                            "{} {};",
                            c_type(&f.ty).unwrap_or_else(|| "void *".to_string()),
                            param_name(&f.name)
                        )
                    })
                    .collect();
                let _ = writeln!(out, "typedef struct {{");
                let _ = writeln!(out, "    int64_t tag;");
                for f in fields {
                    let _ = writeln!(out, "    {f}");
                }
                let _ = writeln!(out, "}} C{};", param_name(&sd.node.name));
            }
            ast::Item::Enum(ed) => {
                let _ = writeln!(out, "typedef struct {{");
                let _ = writeln!(out, "    int64_t tag;");
                let _ = writeln!(out, "    int64_t variant;");
                // Payload slots carry the variant's declared types, which
                // differ per variant, so they are untyped storage and each
                // access casts. This is the same choice CList makes.
                let _ = writeln!(out, "    void *payload;");
                let _ = writeln!(out, "}} C{};", param_name(&ed.node.name));
                for (i, _v) in ed.node.variants.iter().enumerate() {
                    let _ = writeln!(
                        out,
                        "static const int64_t {n}_{i} = {i};",
                        n = param_name(&ed.node.name),
                        i = i
                    );
                }
            }
            ast::Item::Function(_) => {}
        }
    }
    let _ = writeln!(out, "");

    // Decoding one Unicode scalar from a UTF-8 string, so `for c in s` walks
    // characters rather than bytes. A Vortex `Char` is a Unicode scalar, so a
    // byte loop would be a different program: it would iterate three times over
    // a three byte character and produce a different answer.
    let _ = writeln!(
        out,
        "static int32_t vortex_char_at(const char *s, int64_t i) {{"
    );
    let _ = writeln!(
        out,
        "    const unsigned char *u = (const unsigned char *)s;"
    );
    let _ = writeln!(out, "    unsigned char c0 = u[i];");
    let _ = writeln!(out, "    if (c0 < 0x80) return (int32_t)c0;");
    let _ = writeln!(out, "    int32_t cp; int extra;");
    let _ = writeln!(
        out,
        "    if ((c0 & 0xE0) == 0xC0) {{ cp = c0 & 0x1F; extra = 1; }}"
    );
    let _ = writeln!(
        out,
        "    else if ((c0 & 0xF0) == 0xE0) {{ cp = c0 & 0x0F; extra = 2; }}"
    );
    let _ = writeln!(
        out,
        "    else if ((c0 & 0xF8) == 0xF0) {{ cp = c0 & 0x07; extra = 3; }}"
    );
    let _ = writeln!(out, "    else return 0xFFFD;");
    let _ = writeln!(out, "    for (int k = 1; k <= extra; k++) {{");
    let _ = writeln!(out, "        unsigned char b = u[i + k];");
    let _ = writeln!(out, "        if ((b & 0xC0) != 0x80) return 0xFFFD;");
    let _ = writeln!(out, "        cp = (cp << 6) | (b & 0x3F);");
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "    return cp;");
    let _ = writeln!(out, "}}");
    let _ = writeln!(
        out,
        "static int64_t vortex_char_len(const char *s, int64_t i) {{"
    );
    let _ = writeln!(
        out,
        "    const unsigned char *u = (const unsigned char *)s;"
    );
    let _ = writeln!(out, "    unsigned char c0 = u[i];");
    let _ = writeln!(out, "    if (c0 < 0x80) return 1;");
    let _ = writeln!(out, "    if ((c0 & 0xE0) == 0xC0) return 2;");
    let _ = writeln!(out, "    if ((c0 & 0xF0) == 0xE0) return 3;");
    let _ = writeln!(out, "    if ((c0 & 0xF8) == 0xF0) return 4;");
    let _ = writeln!(out, "    return 1;");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out, "");
    // Reads the character at a character index, which is what a Vortex index
    // means. `vortex_char_at` takes a byte offset, so using it directly made
    // "héllo"[2] read the second byte of the two byte `é` and return the
    // replacement character instead of `l`. A byte offset and a character index
    // are the same thing only for ASCII, which is why the first test, on
    // "Vortex", could not tell them apart.
    //
    // An index past the end returns -1 rather than reading past the terminator,
    // which is what the tree interpreter reports as a bad index. The generated C
    // The number of characters in a string, which is not its byte length: a
    // two byte character is one character. The diagnostic says "characters",
    // so it has to count them rather than measure the string.
    let _ = writeln!(out, "static int64_t vortex_char_count(const char *s) {{");
    let _ = writeln!(out, "    int64_t n = 0;");
    let _ = writeln!(out, "    for (int64_t off = 0; s[off] != 0; ) {{");
    let _ = writeln!(out, "        off += vortex_char_len(s, off);");
    let _ = writeln!(out, "        n++;");
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "    return n;");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out, "");

    // has no exception mechanism, so the caller checks.
    let _ = writeln!(
        out,
        "static int32_t vortex_char_at_index(const char *s, int64_t idx) {{"
    );
    let _ = writeln!(out, "    int64_t off = 0;");
    let _ = writeln!(out, "    if (idx < 0) return -2;");
    // A negative index is invalid in the other direction, and the walk loop
    // would not run for it, so the first character came back instead. Both
    // directions are rejected here.
    let _ = writeln!(out, "    for (int64_t k = 0; k < idx; k++) {{");
    let _ = writeln!(out, "        if (s[off] == 0) return -1;");
    let _ = writeln!(out, "        off += vortex_char_len(s, off);");
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "    if (s[off] == 0) return -1;");
    let _ = writeln!(out, "    return vortex_char_at(s, off);");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out, "");

    // One constructor per element type. The struct carries a void * because a
    // Vortex list is not homogeneous in C's type system, so the element type
    // has to be named at the call site. Declaring them here means a call may
    // appear before the definition.
    //
    // A declared struct or enum gets one too, which is what a list of one
    // needs. Without it the list fell back to the int64_t constructor and every
    // Float field truncated.
    let mut kinds: Vec<(&str, String)> = vec![
        ("i64", "int64_t".to_string()),
        ("f64", "double".to_string()),
        ("str", "const char *".to_string()),
        ("int", "int".to_string()),
        // A `Bool` is `int` in the generated C, and a list of one needs a
        // constructor at that type. Without this it named a function that was
        // never generated. Adding an arm to the type decision means walking the
        // other places that switch on the same classification, and this is one:
        // the list constructor is chosen by the element type.
    ];
    for c in declared_types() {
        let suffix: &'static str = Box::leak(c.clone().into_boxed_str());
        kinds.push((suffix, c));
    }
    for (suffix, elem) in kinds {
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
    let _ = writeln!(out, "");
    // Tuples, which `match` needs in order to compare a multi element subject.
    // A tuple of n elements is a distinct struct, so the generated C has a
    // family of them rather than one variable length array.
    for n in 2..=4 {
        let fields: Vec<String> = (0..n)
            .map(|i| {
                let t = match i {
                    0 => "int64_t",
                    1 => "double",
                    2 => "const char *",
                    _ => "void *",
                };
                format!("{} f{i};", t)
            })
            .collect();
        let _ = writeln!(out, "typedef struct {{ {} }} CTuple{n};", fields.join(" "));
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
    // Every call's return type, so a value with no case of its own still has a
    // type: `[origin(); 2]` names its element type from what `origin` returns.
    let mut call_returns: HashMap<String, String> = HashMap::new();
    for f in functions {
        call_returns.insert(
            f.node.name.clone(),
            f.node
                .ret
                .as_ref()
                .and_then(c_type)
                .unwrap_or_else(|| "void".to_string()),
        );
    }
    // The builtins, which are not in the program's declarations.
    for (name, ret) in [
        ("string_to_int", "int64_t"),
        ("int_to_string", "const char *"),
        ("float_to_string", "const char *"),
    ] {
        call_returns.insert(name.to_string(), ret.to_string());
    }
    CALL_RETURNS
        .with(|r| *r.borrow_mut() = call_returns.into_iter().collect::<Vec<(String, String)>>());
    for f in functions {
        signatures.insert(
            f.node.name.clone(),
            f.node
                .ret
                .as_ref()
                .and_then(c_type)
                .unwrap_or_else(|| "int64_t".to_string()),
        );
    }
    let mut bodies = String::new();
    for f in functions {
        if wanted.contains(&f.node.name) {
            let _ = writeln!(bodies, "{}", emit_function(f, &signatures)?);
        }
    }

    // A variant whose payload fields have different types is stored in a
    // generated struct rather than a C array. Those structs are only
    // discovered while emitting a function, so the functions go into a buffer
    // and the declarations are written ahead of them.
    PAYLOAD_STRUCTS.with(|d| {
        for (name, fields) in d.borrow().iter() {
            let _ = writeln!(out, "typedef struct {{ {fields} }} {name};");
        }
    });
    let _ = writeln!(out, "{}", bodies);

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
    if entry_c_type(functions, entry) == "void" {
        let _ = writeln!(out, "    {}({});", param_name(entry), params.join(", "));
    } else {
        let _ = writeln!(
            out,
            "    return {}({});",
            param_name(entry),
            params.join(", ")
        );
    }
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
    if entry_c == "void" {
        // Nothing to print: the program printed its own answer.
        let _ = writeln!(out, "    vortex_c_entry();");
        let _ = writeln!(out, "    return 0;\n}}");
        let _ = args;
        return Ok(out);
    }
    let _ = writeln!(out, "    {} r = vortex_c_entry();", entry_c);
    // A program that prints its own answer has already written it, so the
    // returned value is written after a marker the harness can strip.
    match entry_c.as_str() {
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
/// Whether a C type is one of the generated structs, which is what a declared
/// Vortex struct or enum becomes.
///
/// The answer comes from the declared set rather than from the name's shape. A
/// Vortex name may be mixed case, and the generated C name keeps it, so a
/// pattern over the name cannot tell a struct from `CList` or `CTuple2`.
fn is_declared_c_type(t: &str) -> bool {
    // Membership of the declared set, not a pattern. The pattern was letter
    // case, so a struct named `Point` became `CPoint`, `Point` is not all
    // uppercase, the check said no, and the fallback `return 0;` stood. That
    // reproduced the exact gcc error this was meant to remove, and only a
    // single letter name like `P` took the new path, which is why a test using
    // one passed either way.
    declared_types().iter().any(|d| d == t)
}

fn entry_c_type(functions: &[Spanned<ast::FnDecl>], entry: &str) -> String {
    functions
        .iter()
        .find(|f| f.node.name == entry)
        .and_then(|f| f.node.ret.as_ref())
        .and_then(|t| c_type(t))
        // A function that declares no return type returns nothing. Reporting it
        // as int64_t made the generated entry `return f();` on a void
        // function, which gcc at -O2 accepts with a warning and gcc at -O0
        // rejects.
        .unwrap_or_else(|| "void".to_string())
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
        // A list literal, a repeat and a variant expression all hold values that
        // may call a function. None of them was walked, so `[origin(); 2]` named
        // a callee that was never emitted and the generated C did not compile.
        ast::ExprKind::Array(items) => {
            for i in items {
                collect_calls_expr(i, out);
            }
        }
        ast::ExprKind::Repeat { value, count } => {
            collect_calls_expr(value, out);
            collect_calls_expr(count, out);
        }
        ast::ExprKind::Record { fields, .. } => {
            for (_, v) in fields {
                collect_calls_expr(v, out);
            }
        }
        ast::ExprKind::VariantCall { args, .. } => {
            for a in args {
                collect_calls_expr(a, out);
            }
        }
        ast::ExprKind::VariantRecord { fields, .. } => {
            for (_, v) in fields {
                collect_calls_expr(v, out);
            }
        }
        ast::ExprKind::Tuple(items) => {
            for i in items {
                collect_calls_expr(i, out);
            }
        }
        ast::ExprKind::Match { scrutinee, arms } => {
            collect_calls_expr(scrutinee, out);
            for a in arms {
                collect_calls_expr(&a.body, out);
            }
        }
        ast::ExprKind::Field(base, _) => collect_calls_expr(base, out),
        ast::ExprKind::Cast(inner, _) => collect_calls_expr(inner, out),
        ast::ExprKind::Index(base, idx) => {
            collect_calls_expr(base, out);
            collect_calls_expr(idx, out);
        }
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

#[cfg(test)]
mod coverage {
    use super::*;
    use crate::ast::Expr;

    /// Every `ExprKind` variant, one of each, built without naming a variant the
    /// list does not contain.
    ///
    /// The list is derived from the type, not written out by hand. That is the
    /// whole point: a check that compares against a hardcoded list of today's
    /// variants passes forever and catches nothing new, which is the failure
    /// mode this check exists to avoid. Adding a variant to `ExprKind` makes
    /// `ExprKind::name` fail to compile, which is the first half, and this list
    /// carries it into a runtime assertion, which is the second.
    /// One value of every expression form.
    ///
    /// This comes from `ast::expr_kind_examples`, which is the single
    /// declaration every form is listed in. The coverage list used to be written
    /// out here as well, and the two drifting is exactly how a variant ended up
    /// in neither and the check still passed.
    fn one_of_each() -> Vec<Expr> {
        crate::ast::expr_kind_examples()
    }

    /// The type decision must have an opinion about every expression form, and
    /// must not fall through to its default.
    ///
    /// `expr_type_of` used to answer `int64_t` from a catch-all for any form it
    /// did not name, and that default is what turned a `Float` into a silently
    /// wrong answer in stage 8. The catch-all now names the form instead, so a
    /// form with no arm is a refusal rather than a guess. This test requires
    /// each inventoried form to have an arm: the walk's own answer for the form
    /// differs from what its catch-all would give. The fixtures that carry
    /// context (a declared list name, a callee signature) are supplied
    /// explicitly so the forms that depend on that context are askable.
    ///
    /// The forms come from `ast::expr_kind_examples`, the single declaration, so
    /// a new form is asked about without anyone adding it here.
    #[test]
    fn the_type_decision_covers_every_expression_form() {
        let mut signatures: Signatures = HashMap::new();
        signatures.insert("f".to_string(), "int64_t".to_string());
        signatures.insert("g".to_string(), "double".to_string());
        let mut lists: HashMap<String, String> = HashMap::new();
        lists.insert("__v_n".to_string(), "CList".to_string());
        lists.insert("__v_n_f".to_string(), "CList".to_string());
        // `Ident` resolves from the binding table rather than a default, so the
        // fixture names a binding and the table names a type that is not the
        // old `int64_t` guess. A walk with no arm for `Ident` would return the
        // `int64_t` default this check exists to kill, and the binding above is
        // what makes that mismatch visible.
        lists.insert("x".to_string(), "double".to_string());
        for e in one_of_each() {
            let name = e.kind.name();
            // Each classification, asked about this form. If the walk
            // reached its catch-all, it has no arm for this form, and
            // that is the defect this check exists to make loud. The
            // catch-all now names the form rather than answering
            // int64_t, so a walk that reached it returns the same type
            // for this form as c_type_of_expr would — that equivalence
            // is the signal, and a form with a real arm cannot match it.
            let walk = expr_type_of(&e, &lists, &signatures);
            let default = c_type_of_expr(&e, &signatures);
            // A form with an arm answers differently from the catch-all for
            // this fixture. `Int` and `Ident` no longer answer `int64_t` from
            // both sides: `Int` has an explicit arm and the catch-all now
            // names it, and `Ident` resolves from the binding above to `double`.
            // The remaining five — `Record`, `Variant`, `VariantCall`,
            // `VariantRecord` and `Tuple` — are carried by delegation to
            // `c_type_of_expr`, so they deliberately answer the same thing the
            // catch-all would; they have no arm of their own, only a name and a
            // tag, and the equivalence is exactly the coverage they claim. A new
            // form with no arm fails here unless it is genuinely one of those
            // five.
            let signal = walk != default
                || matches!(
                    name,
                    "Tuple" | "Record" | "Variant" | "VariantCall" | "VariantRecord"
                );
            assert!(
                signal,
                "the type decision has no arm for `{}` and would fall through \
                 to its default, which is how a Float silently became an integer",
                name
            );
            let _ = numeric_in(&e, &mut lists, &signatures);
            let _ = is_string_expr(&e);
            let _ = print_tag_of(&e, &lists, &signatures);
            let _ = index_element_type(&e, &lists);
            let _ = list_element_type(&e);
            let _ = value_c_type(&e);
        }
    }

    /// Every form the type has appears in the inventory, and none that it does
    /// not.
    ///
    /// Both sides read `expr_kind_examples`, the single declaration:
    /// `expr_kind_names()` builds names from the array, and `one_of_each()`
    /// reads the array directly. The first version of this check compared
    /// two hand-written lists, and a variant with a `name()` arm but
    /// omitted from both passed. Both failures were demonstrated before
    /// the fix.
    #[test]
    fn the_coverage_list_covers_every_form_the_type_has() {
        let listed: Vec<&str> = one_of_each().iter().map(|e| e.kind.name()).collect();
        let forms = crate::ast::expr_kind_names();
        let missing: Vec<&&str> = forms.iter().filter(|f| !listed.contains(f)).collect();
        assert!(
            missing.is_empty(),
            "the type has forms the type decision was not checked against: {:?}. \
             Answer the type question for each, or refuse it by name.",
            missing
        );
        let extra: Vec<&&str> = listed.iter().filter(|l| !forms.contains(l)).collect();
        assert!(
            extra.is_empty(),
            "the coverage list names forms the type does not have: {:?}",
            extra
        );
    }

    /// Every form has an explicit classification, or is refused by name.
    ///
    /// `expr_type_of` reaches `c_type_of_expr` for a form with no arm, and that is
    /// what a form lands on when nothing recognises it. Rather than a list of
    /// forms that have arms, which is the hand-written thing that drifted twice
    /// already, the walk is asked directly: `c_type_of_expr` is the default, so
    /// a form the walk could not decide is one whose answer is the default's.
    ///
    /// `expr_type_of_decided` has an arm per form — adding a variant to `ExprKind`
    /// is a compile error until an arm exists — so a missing arm surfaces as
    /// `None` here, never as a silent `int64_t`. The fixture for `Ident` is given
    /// a binding whose type is not `int64_t`, so the arm is asked about a real
    /// decision rather than the old default: a walk with no arm for `Ident` falls
    /// through to `c_type_of_expr`, which now names the form instead of returning
    /// `int64_t`, and that `None`-less miss is caught by the arm-counting test
    /// `the_type_decision_covers_every_expression_form` rather than slipping past.
    #[test]
    fn no_form_falls_through_the_type_default() {
        // A `Call` is decided by the callee's declared return type and a `Cast` by
        // the target it names, and neither is in the node. The context a real
        // program supplies is supplied here, or those two forms could not be
        // asked about at all.
        let mut signatures: Signatures = HashMap::new();
        signatures.insert("f".to_string(), "int64_t".to_string());
        let mut lists: HashMap<String, String> = HashMap::new();
        // `Ident` is decided by its binding, not a default, so the fixture names
        // a binding and the table names a non-`int64_t` type. With no arm, `Ident`
        // would reach the catch-all and fail this check by returning `None`.
        lists.insert("x".to_string(), "double".to_string());
        let mut undecided: Vec<&str> = Vec::new();
        for e in one_of_each() {
            let name = e.kind.name();
            // Asked through a walk with no default, so a form with no arm is
            // reported rather than answered `int64_t`.
            if expr_type_of_decided(&e, &lists, &signatures).is_none() {
                undecided.push(name);
            }
        }
        assert!(
            undecided.is_empty(),
            "these forms have no arm in the type decision and would reach its \
             default, which is what turned a Float into a silently wrong answer: {:?}",
            undecided
        );
    }

    /// The type the walk decides for a form, or `None` when it has no arm.
    ///
    /// This has no default. Every form it does not name is `None`, so a form with
    /// no arm cannot be mistaken for a form whose type happens to be `int64_t`.
    /// That mistake is what made two earlier versions of the check useless: one
    /// failed an `Int` fixture that was decided, the other passed a `Float` that
    /// was not.
    fn expr_type_of_decided(
        e: &Expr,
        lists: &HashMap<String, String>,
        signatures: &Signatures,
    ) -> Option<String> {
        Some(match &e.kind {
            ast::ExprKind::Array(_) | ast::ExprKind::Repeat { .. } => "CList".to_string(),
            ast::ExprKind::Bool(_) => "int".to_string(),
            ast::ExprKind::Int(_) => "int64_t".to_string(),
            ast::ExprKind::Float(_) => "double".to_string(),
            ast::ExprKind::Char(_) => "int32_t".to_string(),
            ast::ExprKind::Str(_) => "const char *".to_string(),
            ast::ExprKind::Binary { op, .. } => match op {
                ast::BinOp::Eq
                | ast::BinOp::Ne
                | ast::BinOp::Lt
                | ast::BinOp::Le
                | ast::BinOp::Gt
                | ast::BinOp::Ge
                | ast::BinOp::And
                | ast::BinOp::Or => "int".to_string(),
                _ => {
                    if expr_type_of_decided(lhs_of(e), lists, signatures).as_deref()
                        == Some("double")
                        || expr_type_of_decided(rhs_of(e), lists, signatures).as_deref()
                            == Some("double")
                    {
                        "double".to_string()
                    } else {
                        "int64_t".to_string()
                    }
                }
            },
            ast::ExprKind::Neg(inner) => expr_type_of_decided(inner, lists, signatures)?,
            ast::ExprKind::Not(_) => "int".to_string(),
            ast::ExprKind::Paren(inner) => expr_type_of_decided(inner, lists, signatures)?,
            ast::ExprKind::Cast(_, t) => c_type(t)?,
            ast::ExprKind::Assign { value, .. } => expr_type_of_decided(value, lists, signatures)?,
            ast::ExprKind::Block(b) => match &b.tail {
                Some(t) => expr_type_of_decided(t, lists, signatures)?,
                None => "void".to_string(),
            },
            ast::ExprKind::Try(inner) => expr_type_of_decided(inner, lists, signatures)?,
            ast::ExprKind::Call { callee, .. } => signatures.get(callee).cloned()?,
            ast::ExprKind::Match { arms, .. } => match arms.first() {
                Some(a) => expr_type_of_decided(&a.body, lists, signatures)?,
                None => return None,
            },
            ast::ExprKind::If {
                then, otherwise, ..
            } => {
                let from_then: Option<String> = match &then.tail {
                    Some(t) => expr_type_of_decided(t, lists, signatures),
                    None => None,
                };
                match otherwise {
                    Some(alt) => match alt.as_ref() {
                        ast::Else::Block(b) => match &b.tail {
                            Some(t) => expr_type_of_decided(t, lists, signatures)?,
                            None => from_then?,
                        },
                        ast::Else::If(inner) => {
                            expr_type_of_decided(&inner.node, lists, signatures)?
                        }
                    },
                    None => from_then?,
                }
            }
            ast::ExprKind::Record { ty, .. }
            | ast::ExprKind::Variant { ty, .. }
            | ast::ExprKind::VariantCall { ty, .. }
            | ast::ExprKind::VariantRecord { ty, .. } => format!("C{}", param_name(ty)),
            ast::ExprKind::Tuple(items) => match items.len() {
                2 => "CTuple2".to_string(),
                3 => "CTuple3".to_string(),
                4 => "CTuple4".to_string(),
                _ => return None,
            },
            // A field read and an index read resolve through the recorded
            // bindings, and refuse by name when they cannot.
            ast::ExprKind::Field(_, _) | ast::ExprKind::Index(_, _) => {
                expr_type_of(e, lists, signatures)
            }
            // A name, which is decided by what it was bound to.
            ast::ExprKind::Ident(_) => expr_type_of(e, lists, signatures),
        })
    }

    #[test]
    fn undecided_types_are_refused_by_name() {
        // A name with no recorded binding resolves to a marker, not `int64_t`.
        // The production type-decision sites refuse such a marker by name rather
        // than emit it, so an unbound name fails the build of the C translation
        // with a Vortex diagnostic instead of a gcc error on `<undecided:...>`.
        let lists: HashMap<String, String> = HashMap::new();
        let signatures: Signatures = HashMap::new();
        let pos = crate::span::Pos { line: 1, col: 1 };
        let unbound = crate::ast::Expr {
            pos,
            kind: crate::ast::ExprKind::Ident("x".to_string()),
        };
        let err = decided_type(&unbound, &lists, &signatures)
            .expect_err("an unbound name should be refused, not `int64_t`");
        assert!(
            err.to_string().contains("the type of `Ident`"),
            "the refusal should name the form, said {:?}",
            err
        );
        // The same guard refuses a compound base the emitter cannot type: a field
        // read whose base is not a recorded struct is named and refused, not
        // read through as `int64_t`.
        let field_of_int = crate::ast::Expr {
            pos,
            kind: crate::ast::ExprKind::Field(
                Box::new(crate::ast::Expr {
                    pos,
                    kind: crate::ast::ExprKind::Int(0),
                }),
                "x".to_string(),
            ),
        };
        let err = decided_type(&field_of_int, &lists, &signatures)
            .expect_err("a field read with no recorded base type should be refused");
        assert!(
            err.to_string().contains("the type of `Field`"),
            "the refusal should name the form, said {:?}",
            err
        );
    }

    fn lhs_of(e: &Expr) -> &Expr {
        match &e.kind {
            ast::ExprKind::Binary { lhs, .. } => lhs,
            _ => e,
        }
    }

    fn rhs_of(e: &Expr) -> &Expr {
        match &e.kind {
            ast::ExprKind::Binary { rhs, .. } => rhs,
            _ => e,
        }
    }
}
