//! `hyper-use` command line. Phase 1 loads a fixture and ranks a locate query.

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
