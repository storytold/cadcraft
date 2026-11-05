//! A script that ends with a command still waiting for input says so (#2711). In a script, as in
//! AutoCAD's, a command that keeps prompting (LINE, PLINE) ends only at an empty input, so without
//! one the next line answers its prompt and the drawing comes out different with no error.

/// `cadcraft-cli` with `args`: its exit code and standard error.
fn run(args: &[&str]) -> (i32, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = cadcraft_cli::run(args, &mut out, &mut err);
    (code, String::from_utf8_lossy(&err).into_owned())
}

#[test]
fn a_command_left_waiting_at_the_end_is_reported() {
    let (code, err) = run(&["run", "--metric", "--script", "line 0,0 @30,0"]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("note: the script ended with a command still waiting for input: LINE"), "{err}");
    assert!(err.contains("an empty line is Enter in a script"), "{err}");
}

#[test]
fn a_script_that_ends_its_commands_gives_no_note() {
    // An empty line, or a space at the end of the line, is the Enter that ends LINE.
    for script in ["line 0,0 @30,0\n\ncircle 15,15 6", "line 0,0 @30,0 \ncircle 15,15 6"] {
        let (code, err) = run(&["run", "--metric", "--script", script]);
        assert_eq!(code, 0, "{err}");
        assert!(!err.contains("note:"), "{script:?}: {err}");
    }
}
