//! Prints the bytecode of a Vortex program.
//!
//! This is what makes the virtual machine inspectable: it shows exactly what
//! the frontend lowered a function into, so a slow program can be read rather
//! than guessed at.
//!
//! Usage:
//!
//!     cargo run --release --example disasm -- <file.vx>
//!     cargo run --release --example disasm -- <file.vx> --fn <name>
//!
//! The output is the instruction index, the instruction, and the source
//! position it came from, so a jump target can be read against the line it
//! belongs to.

use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: disasm <file.vx> [--fn <name>]");
        std::process::exit(2);
    }

    let path = &args[0];
    // An optional second argument names one function, so the output for a large
    // program stays readable.
    let only = args
        .iter()
        .position(|a| a == "--fn")
        .and_then(|i| args.get(i + 1))
        .cloned();

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
    // Type checking runs before compilation, so disassembly never shows code
    // for a program that could not run.
    if let Err(e) = vortexc::check(&lowered) {
        eprintln!("{}", e);
        std::process::exit(1);
    }

    let compiled = match vortexc::bytecode::compile(&lowered) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    let text = match &only {
        Some(name) => {
            let mut found = false;
            let mut out = String::new();
            for code in &compiled.code {
                if &code.name == name {
                    out.push_str(&vortexc::bytecode::disassemble(code));
                    found = true;
                }
            }
            if !found {
                eprintln!("no function named `{}` in {}", name, path);
                std::process::exit(1);
            }
            out
        }
        None => vortexc::bytecode::disassemble_program(&compiled),
    };

    print!("{}", text);
    let _ = std::io::stdout().flush();
}
