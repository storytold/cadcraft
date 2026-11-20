//! -STYLE at the command line (#3129): the prompt sequence that scripts and
//! `(command "-STYLE" ...)` answer, ending in the same code as the JSON `style` command.

use cadcraft_engine::Session;

fn run(s: &mut Session, lines: &[&str]) {
    for line in lines {
        s.cmdline(line).unwrap();
    }
}

#[test]
fn dash_style_creates_a_style_from_the_prompts() {
    let mut s = Session::new();
    run(&mut s, &["-STYLE", "ZTEST", "isocp.shx", "0", "1", "0", "N", "N", "N"]);
    assert!(s.running.is_none(), "{:?}", s.current_prompt());
    let d = s.doc().unwrap();
    let st = d.text_style("ZTEST").expect("style created");
    assert_eq!(st.font, "isocp.shx");
    assert_eq!(st.width_factor, 1.0);
    assert!(!st.backwards && !st.upside_down && !st.vertical);
    assert_eq!(d.header.str("TEXTSTYLE", ""), "ZTEST");
}

#[test]
fn dash_style_changes_an_existing_style_and_keeps_defaults_on_enter() {
    let mut s = Session::new();
    run(&mut s, &["-style", "Z2", "txt", "2.5", "0.8", "15", "Y", "N", "N"]);
    let st = s.doc().unwrap().text_style("Z2").unwrap().clone();
    assert_eq!((st.height, st.width_factor, st.backwards), (2.5, 0.8, true));
    assert!((st.oblique - 15f64.to_radians()).abs() < 1e-9);
    // Enter keeps every current value.
    run(&mut s, &["-style", "z2", "", "", "", "", "", "", ""]);
    assert!(s.running.is_none());
    let st2 = s.doc().unwrap().text_style("Z2").unwrap().clone();
    assert_eq!((st2.height, st2.width_factor, st2.backwards, st2.font.as_str()), (2.5, 0.8, true, "txt"));
}

#[test]
fn dash_style_reasks_on_a_bad_number() {
    let mut s = Session::new();
    run(&mut s, &["-style", "Z3", "txt", "abc"]);
    assert!(s.current_prompt().unwrap().message.contains("height"), "{:?}", s.current_prompt());
}
