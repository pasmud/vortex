//! Compiles one Vortex function to C, builds it, and runs it.
//!
//! Usage:
//!
//!     cargo run --release --example run_compiled -- <file.vx> <function>
//!
//! The point is the comparison rather than the speed. The same function is run
//! on the tree interpreter, on the VM, and as compiled C, and all three have to
//! print the same thing. A compiled path that disagrees with the two engines is
//! worse than no compiled path, so the disagreement is what this reports.

use std::io::Write;
use std::process::Command;

use vortexc::ast::Item;

/// The three engines, run on the same function.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: run_compiled <file.vx> <function>");
        std::process::exit(2);
    }
    let path = &args[0];
    let func = &args[1];

    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {}: {}", path, e);
            std::process::exit(1);
        }
    };

    let ast = match vortexc::parse(&src) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    let lowered = match vortexc::lower(&ast) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    if let Err(e) = vortexc::check(&lowered) {
        eprintln!("{}", e);
        std::process::exit(1);
    }

    // The functions the emitter is asked to translate.
    let functions: Vec<_> = ast
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Function(f) => Some(f),
            _ => None,
        })
        .cloned()
        .collect();

    if !functions.iter().any(|f| f.node.name == *func) {
        eprintln!("no function named `{}` in {}", func, path);
        std::process::exit(1);
    }

    // The compiled path.
    // The argument the Vortex program passes to the function, so the compiled
    // path computes the same answer as the engines rather than a trivial one.
    let arg = arg_for(&functions, func);

    let c = match vortexc::cgen::emit_program(&functions, func, &[arg]) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("not compiled: {}", e);
            std::process::exit(1);
        }
    };
    println!("--- generated C ---");
    println!("{}", c);

    let dir = std::env::temp_dir().join("vortex-cgen");
    let _ = std::fs::create_dir_all(&dir);
    let cfile = dir.join("program.c");
    let bin = dir.join("program");
    if let Err(e) = std::fs::write(&cfile, &c) {
        eprintln!("cannot write the generated C: {}", e);
        std::process::exit(1);
    }

    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let build = Command::new(&cc)
        .args(["-O2", "-o"])
        .arg(&bin)
        .arg(&cfile)
        .output()
        .expect("the C compiler should run");
    if !build.status.success() {
        eprintln!("the C compiler rejected the generated source:");
        eprintln!("{}", String::from_utf8_lossy(&build.stderr));
        std::process::exit(1);
    }

    // The compiled binary writes the value main returned to standard error, so
    // the comparison works whether the program printed an answer or returned
    // one.
    let compiled = {
        let out = std::process::Command::new(&bin)
            .output()
            .expect("the compiled program should run");
        let returned = String::from_utf8_lossy(&out.stderr)
            .lines()
            .find(|l| l.starts_with("vortex_returned "))
            .map(|l| l.trim_start_matches("vortex_returned ").to_string())
            .unwrap_or_default();
        if returned.is_empty() {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        } else {
            returned
        }
    };
    let tree = value_of(vortexc::run_source(&src, &mut std::io::sink()));
    let vm = value_of(vortexc::run_on_vm(&src, &mut std::io::sink()));

    println!("--- results ---");
    println!("compiled C : {}", compiled);
    println!("tree        : {}", tree);
    println!("vm          : {}", vm);

    let _ = std::io::stdout().flush();
}

/// The argument to pass, taken from the Vortex `main` so the compiled path
/// runs on the same input as the two engines.
fn arg_for(functions: &[vortexc::ast::Spanned<vortexc::ast::FnDecl>], func: &str) -> String {
    // The single literal argument the entry function's caller supplies, which
    // for the examples in this repository is a small integer literal.
    for f in functions {
        if f.node.name != "main" {
            continue;
        }
        for stmt in &f.node.body.stmts {
            // The call may be a bare statement or the initialiser of a `let`,
            // and the argument is the caller's value, not a literal chosen here.
            let e = match &stmt.kind {
                vortexc::ast::StmtKind::Expr(e) => e,
                vortexc::ast::StmtKind::Let { init, .. } => init,
                _ => continue,
            };
            if let vortexc::ast::ExprKind::Call { callee, args } = &e.kind {
                if callee == func {
                    if let Some(a) = args.first() {
                        if let vortexc::ast::ExprKind::Int(v) = &a.kind {
                            return format!("INT64_C({})", v);
                        }
                    }
                }
            }
        }
    }
    "INT64_C(0)".to_string()
}

/// What an engine returned, as text.
fn value_of(r: Result<vortexc::Value, vortexc::Error>) -> String {
    match r {
        Ok(v) => vortexc::interp::display(&v),
        Err(e) => format!("error: {}", e),
    }
}
