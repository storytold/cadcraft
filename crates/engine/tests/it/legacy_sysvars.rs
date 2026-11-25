//! Legacy system variables a LISP routine saves and restores around its work: DIMASO (obsolete in
//! AutoCAD, which reports and sets DIMASSOC through it), TEXTEVAL, REGENMODE (kept in the drawing
//! and its DXF header) and POPUPS (read-only). Before them `(getvar "DIMASO")` read nil, so
//! `(setq o (getvar "DIMASO")) … (setvar "DIMASO" o)` failed and the routine left the drawing's
//! associativity changed.

use cadcraft_engine::Session;
use cadcraft_engine::lisp::value::{Value as LispValue, to_text};
use serde_json::{Value, json};

fn getvar(s: &mut Session, name: &str) -> Value {
    s.execute("getvar", &json!({ "name": name })).unwrap()["value"].clone()
}

fn setvar(s: &mut Session, name: &str, value: Value) -> bool {
    s.execute("setvar", &json!({ "name": name, "value": value })).is_ok()
}

/// The value of a LISP expression, or the error it left.
fn eval(s: &mut Session, src: &str) -> Result<LispValue, String> {
    match s.lisp_eval(src, false, true, Vec::new()) {
        Ok(Some(v)) => v,
        Ok(None) => Err("no value".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// `(getvar name)` as the integer a routine receives.
fn lisp_getvar(s: &mut Session, name: &str) -> i64 {
    let v = eval(s, &format!("(getvar \"{name}\")")).unwrap_or_else(|e| panic!("{name}: {e}"));
    let n = v.as_number().unwrap_or_else(|| panic!("{name} is not a number"));
    assert_eq!(n.fract(), 0.0, "{name} is a whole number");
    n as i64
}

/// `(setvar name value)` from LISP: whether it was accepted.
fn lisp_setvar(s: &mut Session, name: &str, value: &str) -> bool {
    eval(s, &format!("(setvar \"{name}\" {value})")).is_ok()
}

/// The DXF pairs `(group code, value)` of a file, both trimmed.
fn pairs(text: &str) -> Vec<(i32, String)> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out = Vec::new();
    for chunk in lines.chunks(2) {
        let [code, value] = chunk else { break };
        let Ok(c) = code.parse::<i32>() else { continue };
        out.push((c, (*value).to_string()));
    }
    out
}

/// The HEADER variable names a DXF text writes (the value after each group code 9).
fn header_names(text: &str) -> Vec<String> {
    pairs(text).iter().filter(|(c, _)| *c == 9).map(|(_, v)| v.trim_start_matches('$').to_string()).collect()
}

/// The integer a header variable is written with, as `(group code, value)` after its name.
fn header_int(text: &str, name: &str) -> Option<(i32, String)> {
    let ps = pairs(text);
    let i = ps.iter().position(|(c, v)| *c == 9 && v.trim_start_matches('$') == name)?;
    ps.get(i + 1).cloned()
}

#[test]
fn fresh_values_are_integers_a_routine_reads() {
    let mut s = Session::new();
    for (name, want) in [("DIMASO", 1), ("TEXTEVAL", 0), ("REGENMODE", 1), ("POPUPS", 1)] {
        assert_eq!(getvar(&mut s, name), json!(want), "{name}");
        assert_eq!(lisp_getvar(&mut s, name), want, "{name} through getvar");
        let kind = eval(&mut s, &format!("(type (getvar \"{name}\"))")).unwrap();
        assert_eq!(to_text(&kind, true), "INT", "{name} is an integer to LISP");
    }
    // The list a routine sees (`SETVAR ?`) carries them too.
    let all = cadcraft_engine::sysvars::list(&s);
    for name in ["DIMASO", "TEXTEVAL", "REGENMODE", "POPUPS"] {
        assert_eq!(all.get(name), Some(&getvar(&mut s, name)), "{name} listed");
    }
}

#[test]
fn a_routine_saving_and_restoring_each_one_leaves_it_as_it_was() {
    let mut s = Session::new();
    for (name, changed) in [("DIMASO", "0"), ("TEXTEVAL", "1"), ("REGENMODE", "0")] {
        let before = lisp_getvar(&mut s, name);
        // (setq o (getvar X)) … (setvar X o) must give back exactly what it read.
        assert!(eval(&mut s, &format!("(setq o (getvar \"{name}\"))")).is_ok(), "{name} readable");
        assert!(lisp_setvar(&mut s, name, changed), "{name} takes {changed}");
        assert_ne!(lisp_getvar(&mut s, name), before, "{name} changed by {changed}");
        assert!(eval(&mut s, &format!("(setvar \"{name}\" o)")).is_ok(), "{name} restored");
        assert_eq!(lisp_getvar(&mut s, name), before, "{name} back to {before}");
    }
    // REGENMODE is the drawing's, kept in its header; TEXTEVAL only the session's.
    assert_eq!(s.doc().unwrap().header.i64("REGENMODE", 1), 1);
}

#[test]
fn dimaso_restores_the_dimassoc_mode_it_turned_off() {
    // The routine's save/restore through DIMASO, for each associative mode.
    for mode in [1, 2] {
        let mut s = Session::new();
        assert!(setvar(&mut s, "DIMASSOC", json!(mode)));
        assert_eq!(getvar(&mut s, "DIMASO"), json!(1), "DIMASO follows DIMASSOC {mode}");
        let o = eval(&mut s, "(getvar \"DIMASSOC\")").unwrap().as_number().unwrap();
        assert!(lisp_setvar(&mut s, "DIMASO", "0"));
        assert_eq!(getvar(&mut s, "DIMASO"), json!(0));
        assert_eq!(getvar(&mut s, "DIMASSOC"), json!(0), "DIMASO 0 turns associativity off");
        assert!(lisp_setvar(&mut s, "DIMASO", "1"));
        assert_eq!(getvar(&mut s, "DIMASO"), json!(1));
        assert_eq!(eval(&mut s, "(getvar \"DIMASSOC\")").unwrap().as_number(), Some(o), "DIMASO 1 puts back the DIMASSOC {mode} the routine read");
    }
    // Set directly, without ever switching it off first, DIMASO 1 leaves the mode alone.
    let mut s = Session::new();
    assert!(setvar(&mut s, "DIMASSOC", json!(1)));
    assert!(setvar(&mut s, "DIMASO", json!(1)));
    assert_eq!(getvar(&mut s, "DIMASSOC"), json!(1));
    // No non-zero mode was seen: turning associativity back on gives AutoCAD's 2.
    let mut s = Session::new();
    assert!(setvar(&mut s, "DIMASSOC", json!(0)));
    assert!(setvar(&mut s, "DIMASO", json!(0)));
    assert!(setvar(&mut s, "DIMASO", json!(1)));
    assert_eq!(getvar(&mut s, "DIMASO"), json!(1));
    assert_eq!(getvar(&mut s, "DIMASSOC"), json!(2), "none was seen: the default 2");
    // A drawing whose saved DIMASSOC is no mode DIMASO can restore (a hostile file) gives 2.
    let mut s = Session::new();
    s.doc_mut().unwrap().header.set_i64("DIMASSOC", 5);
    assert!(setvar(&mut s, "DIMASO", json!(0)));
    assert!(setvar(&mut s, "DIMASO", json!(1)));
    assert_eq!(getvar(&mut s, "DIMASO"), json!(1));
    assert_eq!(getvar(&mut s, "DIMASSOC"), json!(2));
}

#[test]
fn hostile_values_are_refused_and_change_nothing() {
    let mut s = Session::new();
    for (name, v) in [
        ("REGENMODE", json!(2)),
        ("REGENMODE", json!(-1)),
        ("REGENMODE", json!(1.5)),
        ("REGENMODE", json!("two")),
        ("REGENMODE", Value::Null),
        ("TEXTEVAL", json!(-1)),
        ("TEXTEVAL", json!(2)),
        ("TEXTEVAL", json!("two")),
        ("TEXTEVAL", Value::Null),
        ("DIMASO", json!(3)),
        ("DIMASO", json!(-1)),
        ("DIMASO", json!(0.5)),
        ("DIMASO", json!("two")),
        ("DIMASO", Value::Null),
    ] {
        let before = getvar(&mut s, name);
        assert!(!setvar(&mut s, name, v.clone()), "{name} = {v} refused");
        assert_eq!(getvar(&mut s, name), before, "{name} unchanged by {v}");
    }
    // The drawing's associativity is untouched by a refused DIMASO.
    assert!(setvar(&mut s, "DIMASSOC", json!(1)));
    assert!(!setvar(&mut s, "DIMASO", json!(3)));
    assert_eq!(getvar(&mut s, "DIMASSOC"), json!(1));
    // The same through LISP, where nil and a NaN real arrive as values a variable can't take.
    assert!(!lisp_setvar(&mut s, "REGENMODE", "nil"), "nil refused");
    assert!(!lisp_setvar(&mut s, "DIMASO", "3"), "DIMASO 3 refused");
    assert!(!lisp_setvar(&mut s, "DIMASO", "\"two\""), "DIMASO text refused");
    assert!(!lisp_setvar(&mut s, "TEXTEVAL", "-1"), "TEXTEVAL -1 refused");
    // (- (exp 1000) (exp 1000)) is NaN: not a number JSON can hold, so it is refused as one.
    assert!(!lisp_setvar(&mut s, "TEXTEVAL", "(- (exp 1000) (exp 1000))"), "NaN refused");
    assert_eq!(getvar(&mut s, "TEXTEVAL"), json!(0));
}

#[test]
fn popups_is_read_only() {
    let mut s = Session::new();
    assert!(cadcraft_engine::sysvars::is_read_only("popups"));
    for v in [json!(1), json!(0), json!("1")] {
        let e = s.execute("setvar", &json!({ "name": "POPUPS", "value": v })).unwrap_err().to_string();
        assert!(e.contains("read-only"), "{v}: {e}");
    }
    assert!(!lisp_setvar(&mut s, "POPUPS", "0"));
    assert_eq!(getvar(&mut s, "POPUPS"), json!(1));
}

#[test]
fn regenmode_is_saved_in_the_dxf_header_and_read_back() {
    let mut s = Session::new();
    assert!(setvar(&mut s, "REGENMODE", json!(0)));
    let text = cadcraft_io::write_dxf(s.doc().unwrap());
    assert!(header_names(&text).contains(&"REGENMODE".to_string()), "$REGENMODE is in the header (has {:?})", header_names(&text));
    assert_eq!(header_int(&text, "REGENMODE"), Some((70, "0".to_string())), "$REGENMODE is group 70, 0");
    // A reopened drawing has the value it was saved with, and saves it back.
    let mut back = Session::new();
    back.open_drawing(cadcraft_io::read_dxf(text.as_bytes()).unwrap(), "regenmode.dxf", None);
    assert_eq!(getvar(&mut back, "REGENMODE"), json!(0));
    let again = cadcraft_io::write_dxf(back.doc().unwrap());
    assert_eq!(header_int(&again, "REGENMODE"), Some((70, "0".to_string())));
}

#[test]
fn texteval_dimaso_and_popups_are_not_saved() {
    let mut s = Session::new();
    assert!(setvar(&mut s, "TEXTEVAL", json!(1)));
    let text = cadcraft_io::write_dxf(s.doc().unwrap());
    let names = header_names(&text);
    for name in ["TEXTEVAL", "DIMASO", "POPUPS"] {
        assert!(!names.contains(&name.to_string()), "{name} is not a header variable (has {names:?})");
    }
    // TEXTEVAL is the session's, so a reopened drawing starts at 0 again.
    let mut back = Session::new();
    back.open_drawing(cadcraft_io::read_dxf(text.as_bytes()).unwrap(), "no_legacy_vars.dxf", None);
    assert_eq!(getvar(&mut back, "TEXTEVAL"), json!(0));
}

#[test]
fn dimaso_remembers_the_mode_of_each_open_drawing() {
    // The mode DIMASO turns off is the drawing's, not the session's: two open drawings switched
    // between each put back the DIMASSOC mode they had.
    let mut s = Session::new();
    let first = s.active;
    assert!(setvar(&mut s, "DIMASSOC", json!(1)));
    assert!(setvar(&mut s, "DIMASO", json!(0)));
    let second = s.new_drawing(true);
    assert_eq!(second, first + 1);
    assert!(setvar(&mut s, "DIMASSOC", json!(2)));
    assert!(setvar(&mut s, "DIMASO", json!(0)));
    s.execute("document.switch", &json!({ "index": first })).unwrap();
    assert!(setvar(&mut s, "DIMASO", json!(1)));
    assert_eq!(getvar(&mut s, "DIMASSOC"), json!(1), "the first drawing restores its own mode");
    s.execute("document.switch", &json!({ "index": second })).unwrap();
    assert!(setvar(&mut s, "DIMASO", json!(1)));
    assert_eq!(getvar(&mut s, "DIMASSOC"), json!(2), "the second drawing restores its own mode");
}

#[test]
fn the_dollar_forms_are_the_same_variables() {
    // The DXF header writes these names with a `$` ($DIMASO, $TEXTEVAL, $REGENMODE, $POPUPS), so
    // the command line takes them that way too, in any case and with spaces around them.
    let mut s = Session::new();
    for name in ["DIMASO", "TEXTEVAL", "REGENMODE", "POPUPS"] {
        let plain = getvar(&mut s, name);
        let dollar = format!("${name}");
        assert_eq!(getvar(&mut s, &dollar), plain, "{dollar} reads like {name}");
        let typed = format!("  ${}  ", name.to_ascii_lowercase());
        assert_eq!(getvar(&mut s, &typed), plain, "{typed:?} reads like {name}");
        assert_eq!(cadcraft_engine::sysvars::is_read_only(&dollar), cadcraft_engine::sysvars::is_read_only(name), "{dollar} read-only as {name}");
    }
    // DIMASO through its DXF name still turns DIMASSOC off and puts its mode back.
    assert!(setvar(&mut s, "DIMASSOC", json!(1)));
    assert!(setvar(&mut s, "$DIMASO", json!(0)));
    assert_eq!(getvar(&mut s, "DIMASSOC"), json!(0), "$DIMASO 0 turns associativity off");
    assert!(setvar(&mut s, " $dimaso ", json!(1)));
    assert_eq!(getvar(&mut s, "DIMASSOC"), json!(1), "$DIMASO 1 puts the mode back");
    // TEXTEVAL: set under one name, read under the other.
    assert!(setvar(&mut s, "$TEXTEVAL", json!(1)));
    assert_eq!(getvar(&mut s, "TEXTEVAL"), json!(1), "$TEXTEVAL set TEXTEVAL");
    assert!(setvar(&mut s, "TEXTEVAL", json!(0)));
    assert_eq!(getvar(&mut s, "$TEXTEVAL"), json!(0));
    // REGENMODE takes only the values its plain name takes, and stores the ones it takes.
    assert!(!setvar(&mut s, "$REGENMODE", json!(2)), "$REGENMODE 2 is out of range");
    assert_eq!(getvar(&mut s, "REGENMODE"), json!(1), "a refused $REGENMODE changed nothing");
    assert!(!setvar(&mut s, "$REGENMODE", json!(-1)), "$REGENMODE -1 is out of range");
    assert!(setvar(&mut s, "$REGENMODE", json!(0)));
    assert_eq!(getvar(&mut s, "$REGENMODE"), json!(0));
    assert_eq!(getvar(&mut s, "REGENMODE"), json!(0));
    assert_eq!(s.doc().unwrap().header.i64("REGENMODE", 1), 0, "$REGENMODE stored the drawing's value");
    // POPUPS is read-only under its DXF name too.
    for v in [json!(0), json!("1")] {
        let e = s.execute("setvar", &json!({ "name": "$POPUPS", "value": v })).unwrap_err().to_string();
        assert!(e.contains("read-only"), "$POPUPS {v}: {e}");
    }
    assert!(!lisp_setvar(&mut s, "$POPUPS", "0"));
    assert_eq!(getvar(&mut s, "$POPUPS"), json!(1));
}

#[test]
fn other_dollar_names_keep_their_own_handling() {
    // Only those four take a `$`: LISP's angle variables convert radians for the plain name, and
    // `$OSMODE` stays a name of its own rather than becoming the session's OSMODE.
    let mut s = Session::new();
    assert!(eval(&mut s, "(setvar \"$ANGBASE\" (/ pi 2))").is_ok());
    let base = eval(&mut s, "(getvar \"$ANGBASE\")").unwrap().as_number().unwrap_or(f64::NAN);
    assert!((base - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "(getvar \"$ANGBASE\") gives pi/2, not {base}");
    let osmode = eval(&mut s, "(setvar \"$OSMODE\" 9)").unwrap();
    assert_eq!(to_text(&osmode, true), "9", "(setvar \"$OSMODE\" 9) returns 9");
    assert_eq!(getvar(&mut s, "OSMODE"), json!(4133), "$OSMODE is not the session's OSMODE");
    assert_eq!(getvar(&mut s, "$OSMODE"), json!(9), "$OSMODE still reads back what it was set to");
}
