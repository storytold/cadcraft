//! `cadcraft-cli` against a stdout nobody reads any more (#537).

use std::io::Read;
use std::process::{Command, Output, Stdio};

/// `cadcraft-cli commands | head` panicked with "failed printing to stdout: Broken pipe (os error
/// 32)" and exit status 101: `commands` prints a few thousand lines, which no pipe buffer holds,
/// so the reader is gone before the last write.
#[test]
fn closed_stdout_ends_quietly() {
    for args in [&["commands"][..], &["commands", "ellipse"], &["--version"]] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_cadcraft-cli")).args(args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let mut first = [0u8; 10];
        child.stdout.as_mut().unwrap().read_exact(&mut first).unwrap();
        drop(child.stdout.take());
        let out = child.wait_with_output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success() && err.is_empty(), "{args:?}: {:?}: {err}", out.status);
    }
}

/// The CLI keeps working once stdout is gone: `run` still executes its commands and still saves,
/// so `cadcraft-cli run … --save out.dxf | head` keeps the drawing.
fn run_with_closed_stdout(args: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cadcraft-cli")).args(args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    drop(child.stdout.take());
    child.wait_with_output().unwrap()
}

#[test]
fn closed_stdout_still_runs_the_commands_and_saves() {
    let dir = std::env::temp_dir().join(format!("cc-cli-pipe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let drawing = dir.join("piped.dxf");
    let out = run_with_closed_stdout(&[
        "run",
        "--metric",
        "--cmd",
        "ellipse {\"center\":[0,0],\"major\":[10,0],\"ratio\":0.5}",
        "--cmd",
        "drawing.inspect {}",
        "--save",
        drawing.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{:?}: {}", out.status, String::from_utf8_lossy(&out.stderr));
    let saved = std::fs::read_to_string(&drawing).unwrap();
    assert!(saved.contains("ELLIPSE"), "the drawing is saved all the same: {}", &saved[..saved.len().min(200)]);

    // A failing command is still a failure, whether or not anyone reads stdout.
    let out = run_with_closed_stdout(&["run", "--metric", "--cmd", "no.such.command {}"]);
    assert_eq!(out.status.code(), Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}
