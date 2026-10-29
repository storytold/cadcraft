//! The clap definition is consistent, and `examples/man-and-completions.rs` writes every shell's
//! completions and every man page under the names `packaging/linux/package.sh` installs.

// The example itself, so the test runs its code; its `main` is unused here.
#[allow(dead_code)]
#[path = "../examples/man-and-completions.rs"]
mod example;

#[test]
fn command_is_consistent() {
    cadcraft_cli::command().debug_assert();
}

#[test]
fn the_example_writes_the_files_the_packages_install() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("man-and-completions-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    example::generate(&dir).unwrap();

    let mut written: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    written.sort();
    let completions = ["cadcraft-cli.bash", "_cadcraft-cli", "cadcraft-cli.fish", "cadcraft-cli.elv", "_cadcraft-cli.ps1"];
    let subcommands = ["info", "convert", "run", "commands", "mcp", "perf", "sample"];
    let mut expected: Vec<String> = completions.map(String::from).into();
    expected.push("cadcraft-cli.1".into());
    expected.extend(subcommands.iter().map(|s| format!("cadcraft-cli-{s}.1")));
    expected.sort();
    assert_eq!(written, expected);
    std::fs::remove_dir_all(&dir).unwrap();
}
