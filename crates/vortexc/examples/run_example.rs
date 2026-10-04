use std::io::Write;

fn main() {
    let path = std::env::args().nth(1).expect("usage: run_example <file.vx>");
    let src = std::fs::read_to_string(&path).expect("cannot read the example");
    let mut out = Vec::new();
    match vortexc::run_source(&src, &mut out) {
        Ok(_) => {
            print!("{}", String::from_utf8_lossy(&out));
            let _ = std::io::stdout().flush();
        }
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}
