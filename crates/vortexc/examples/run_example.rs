//! Runs a Vortex program on either execution engine.
//!
//! The tree interpreter and the virtual machine are two consumers of the same
//! lowered form, which is what `SPEC.md` section 10.1 asks for. This chooses
//! between them so the stage 2 and 3 tests can run against both without being
//! edited: the engine is a choice here, not a difference in the expectations.
//!
//! Usage:
//!
//!     cargo run --release --example run_example -- <file.vx> [--vm]
//!
//! With no flag the tree interpreter runs, so existing behaviour is unchanged.
//! `--vm` selects the virtual machine.

use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = match args.iter().find(|a| !a.starts_with("--")) {
        Some(p) => p.clone(),
        None => {
            eprintln!("usage: run_example <file.vx> [--vm]");
            std::process::exit(2);
        }
    };
    let use_vm = args.iter().any(|a| a == "--vm");

    let src = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {}: {}", path, e);
            std::process::exit(1);
        }
    };

    if use_vm {
        match vortexc::run_on_vm(&src, &mut std::io::stdout()) {
            Ok(_) => {
                let _ = std::io::stdout().flush();
            }
            Err(e) => {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    match vortexc::run_source(&src, &mut std::io::stdout()) {
        Ok(_) => {
            let _ = std::io::stdout().flush();
        }
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}
