//! `hyper-use` command line. Locate, observe, act, diff, verify, and `mcp`.

#![forbid(unsafe_code)]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "mcp") {
        if args.len() != 1 {
            eprintln!("hyper-use: mcp takes no arguments");
            std::process::exit(2);
        }
        if let Err(err) = hyper_use_mcp::serve_stdio() {
            eprintln!("hyper-use: {err}");
            std::process::exit(1);
        }
        return;
    }
    match hyper_use_cli::execute(&args) {
        Ok(stdout) => print!("{stdout}"),
        Err(err) => {
            eprintln!("hyper-use: {err}");
            std::process::exit(1);
        }
    }
}
