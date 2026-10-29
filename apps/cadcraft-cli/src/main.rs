#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

fn main() {
    // `args_os`, not `args`: a non-Unicode argument (a Latin-1 file name on Linux) must be
    // reported as an error, not panic before anything is printed.
    let code = cadcraft_cli::run(std::env::args_os().skip(1), &mut std::io::stdout(), &mut std::io::stderr());
    std::process::exit(code);
}
