//! `hyper-use` command line. Locate, observe, act, diff, and verify.

#![forbid(unsafe_code)]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match hyper_use_cli::execute(&args) {
        Ok(stdout) => print!("{stdout}"),
        Err(err) => {
            eprintln!("hyper-use: {err}");
            std::process::exit(1);
        }
    }
}
