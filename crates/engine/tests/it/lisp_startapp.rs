//! LISP `startapp` starts a program without waiting and answers its process id (#2999).

use cadcraft_engine::Session;
use serde_json::json;

/// The value of `code` as LISP prints it, or the error message.
fn lisp(code: &str) -> Result<String, String> {
    let mut s = Session::new();
    match s.execute("lisp", &json!({ "code": code })) {
        Ok(r) => match r["error"].as_str() {
            Some(e) => Err(e.to_string()),
            None => Ok(r["value"].as_str().unwrap_or_default().to_string()),
        },
        Err(e) => Err(e.to_string()),
    }
}

/// The test binary itself: it exists on every OS and `--list` prints the tests and exits.
fn this_exe() -> String {
    std::env::current_exe().unwrap().to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/")
}

#[test]
fn startapp_is_a_builtin() {
    assert_eq!(lisp("(type startapp)").unwrap(), "SUBR");
}

#[test]
fn startapp_starts_a_program_and_returns_a_positive_process_id() {
    let exe = this_exe();
    assert_eq!(lisp(&format!("(> (startapp \"{exe}\" \"--list\") 0)")).unwrap(), "T");
    assert_eq!(lisp(&format!("(type (startapp \"{exe}\" \"--list\"))")).unwrap(), "INT");
}

#[test]
fn startapp_is_nil_when_the_program_cannot_be_started() {
    assert_eq!(lisp("(startapp \"/no/such/dir/no-such-program-2999\")").unwrap(), "nil");
    assert_eq!(lisp("(startapp \"\")").unwrap(), "nil");
}

#[test]
fn startapp_checks_its_arguments() {
    assert!(lisp("(startapp)").is_err());
    assert!(lisp("(startapp 1)").is_err());
    assert!(lisp("(startapp \"x\" 1)").is_err());
    assert!(lisp("(startapp \"x\" \"y\" \"z\")").is_err());
}
