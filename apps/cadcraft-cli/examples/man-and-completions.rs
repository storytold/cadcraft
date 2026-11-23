//! Writes cadcraft-cli's shell completions ([`clap_complete`]: bash, zsh, fish, elvish,
//! PowerShell) and man pages ([`clap_mangen`]: `cadcraft-cli.1` and one per subcommand, such as
//! `cadcraft-cli-convert.1`) into a directory. The Linux packages ship them
//! (`packaging/linux/package.sh`):
//!
//! ```sh
//! cargo run -p cadcraft-cli --example man-and-completions -- <out-dir>
//! ```

use std::path::Path;
use std::process::ExitCode;

use clap::ValueEnum;
use clap_complete::Shell;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let (Some(dir), None) = (args.next(), args.next()) else {
        eprintln!("usage: man-and-completions <out-dir>");
        return ExitCode::from(2);
    };
    let dir = Path::new(&dir);
    match generate(dir) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {}: {e}", dir.display());
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn generate(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut cmd = cadcraft_cli::command();
    for &shell in Shell::value_variants() {
        clap_complete::generate_to(shell, &mut cmd, "cadcraft-cli", dir)?;
    }
    // A fresh command: `clap_complete::generate_to` built `cmd`, adding the `help` subcommand,
    // which `clap_mangen` would otherwise give pages of its own (`cadcraft-cli-help-convert.1`).
    clap_mangen::generate_to(cadcraft_cli::command(), dir)
}
