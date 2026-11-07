//! -LAYER stAte: AutoCAD 2024's layer state prompts and messages (measured with accoreconsole on a
//! drawing with layers A, red, and B), and the `layerstate.*` commands they run.

use cadcraft_engine::doc::LayerState;
use cadcraft_engine::{Input, Session};
use serde_json::json;

const LABELS: [&str; 10] = ["On", "Frozen", "Lock", "Plot", "Newvpfreeze", "Color", "lineType", "lineWeight", "Transparency", "plotStyle"];

/// The toggles AutoCAD lists before "Enter states to change", with `no` the ones set to No.
fn table(no: &[&str]) -> Vec<String> {
    let mut v = vec!["States".to_string()];
    v.extend(LABELS.iter().map(|l| format!("{l:<20}{}", if no.contains(l) { "No" } else { "Yes" })));
    v
}

/// Layers A (red) and B; the current layer stays 0.
fn drawing() -> Session {
    let mut s = Session::new();
    s.script("-LAYER N A,B C 1 A\n\n").unwrap();
    s
}

/// Run `script` to its end and return what it wrote besides the commands' names and -LAYER's
/// "Current layer" line.
fn run(s: &mut Session, script: &str) -> Vec<String> {
    let at = s.log.len();
    s.script(script).unwrap();
    assert!(s.running.is_none(), "{script:?} ends at: {}", s.prompt_text());
    s.log[at..].iter().filter(|l| !l.starts_with("Command: ") && !l.starts_with("Current layer:")).cloned().collect()
}

fn states(s: &Session) -> Vec<(String, u32)> {
    s.doc().unwrap().layer_states.iter().map(|st| (st.name.clone(), st.mask)).collect()
}

fn state(name: &str, mask: u32) -> (String, u32) {
    (name.to_string(), mask)
}

#[test]
fn save_asks_the_states_and_remembers_them_for_the_next_save() {
    let mut s = drawing();
    // Each toggle lists the states again; On off is AutoCAD's 2046.
    let out = run(&mut s, "-LAYER A S S6 ON\n\n\n\nLINE 0,0 1,0\n\n");
    assert_eq!(out, [table(&[]), table(&["On"])].concat());
    assert_eq!(states(&s), [state("S6", 2046)]);
    assert_eq!(s.doc().unwrap().model.len(), 1, "the next command runs");
    // The next Save starts from the toggles the last one left: On alone is 513, then every
    // toggle once more is 2046 (AutoCAD's S7 and S8).
    let out = run(&mut s, "-LAYER A S S7 ON F L P N C T W TR S\n\n\n\n");
    assert_eq!(out[..11], table(&["On"]));
    assert_eq!(
        out[out.len() - 11..],
        table(&["Frozen", "Lock", "Plot", "Newvpfreeze", "Color", "lineType", "lineWeight", "Transparency", "plotStyle"])
    );
    run(&mut s, "-LAYER A S S8 ON F L P N C T W TR S\n\n\n\n");
    run(&mut s, "-LAYER A S S9 ON\n\n\n\n");
    assert_eq!(states(&s)[1..], [state("S7", 513), state("S8", 2046), state("S9", LayerState::ALL)]);
    // Edit lists and changes the state's own mask (found in any case), not the next Save's
    // toggles (AutoCAD: Edit S1 to 2015, then S2 is 2047).
    let out = run(&mut s, "-LAYER A E s6 ON C\n\n\n\n");
    assert_eq!(out, [table(&["On"]), table(&[]), table(&["Color"])].concat());
    run(&mut s, "-LAYER A S S10\n\n\n\n");
    assert_eq!(states(&s)[0], state("S6", 2015));
    assert_eq!(states(&s)[4], state("S10", LayerState::ALL));
    // Each -LAYER is one undo step.
    s.undo().unwrap();
    assert_eq!(states(&s).len(), 4);
    s.undo().unwrap();
    assert_eq!(states(&s)[0], state("S6", 2046));
    // A state from AutoCAD after a restore (mask 67583 = 2047 + 65536) named as AutoCAD names a
    // state renamed to "*X": `?` lists it, and Edit keeps the bit no toggle reaches.
    s.doc_mut().unwrap().layer_states.push(LayerState { name: "*A1".into(), mask: 67583, ..LayerState::default() });
    assert_eq!(
        run(
            &mut s,
            "-LAYER A ?


"
        )[2],
        "*A1"
    );
    run(
        &mut s,
        "-LAYER A E *a1 C



",
    );
    assert_eq!(states(&s)[4], state("*A1", 67583 - 32));
}

#[test]
fn names_follow_autocads_rules_and_messages() {
    let mut s = drawing();
    run(&mut s, "-LAYER A S S1\n\n\n\n");
    // A saved name in another case is refused and asked again.
    let out = run(&mut s, "-LAYER A S s1 S2\n\n\n\n");
    assert_eq!(out, [vec!["Layer state \"s1\" already exists.".to_string()], table(&[])].concat());
    // The layer name characters, and more than 255 characters, are refused; Enter goes back.
    let long = "y".repeat(256);
    let out = run(&mut s, &format!("-LAYER A S *X a<b a,b {long}\n\n\n\n"));
    assert_eq!(
        out,
        [
            "\"*X\" is an invalid layer state name.",
            "\"a<b\" is an invalid layer state name.",
            "\"a,b\" is an invalid layer state name.",
            &format!("\"{long}\" is an invalid layer state name.")
        ]
    );
    run(&mut s, &format!("-LAYER A S {}\n\n\n\n", "x".repeat(255)));
    assert_eq!(states(&s).len(), 3);
    run(&mut s, &format!("-LAYER A D {}\n\n\n", "x".repeat(255)));
    // `?` lists by name without regard to case.
    run(&mut s, "-LAYER A S b\n\n\n\n");
    let list = ["Layer state", "-----------------------------", "b", "S1", "S2"];
    assert_eq!(run(&mut s, "-LAYER A ?\n\n\n"), list);
    // Each name prompt lists on `?`, says an unknown name and asks again, and Enter goes back.
    let out = run(&mut s, "-LAYER A R ? NOSUCH\n\nE NOSUCH\n\nN NOSUCH\n\nD NOSUCH\n\n\n\n");
    assert_eq!(out[..5], list);
    assert_eq!(out[5..], ["Unknown layer state name."; 4]);
    // Unknown options, Import and EXport.
    let out = run(&mut s, "-LAYER A XYZ I EX\n\n\n");
    assert_eq!(out, ["Invalid option keyword.", "-LAYER stAte Import is not available yet.", "-LAYER stAte EXport is not available yet."]);
    let out = run(&mut s, "-LAYER A S T1 XYZ\n\n\n\n");
    assert_eq!(out, [table(&[]), vec!["Invalid option keyword.".to_string()]].concat());
    assert_eq!(states(&s).len(), 4, "T1 is saved after the invalid toggle");
    run(&mut s, "-LAYER A D T1\n\n\n");
    // Name: a name some state has in any case, the state's own too, is refused with two lines.
    assert_eq!(run(&mut s, "-LAYER A N S1 S2\n\n\n"), ["Layer state could not be renamed.", "S2 already exists."]);
    assert_eq!(run(&mut s, "-LAYER A N S1 s1\n\n\n"), ["Layer state could not be renamed.", "s1 already exists."]);
    // A name with a refused character leaves the state as it is, with no message; Enter too.
    assert!(run(&mut s, "-LAYER A N S1 a,b\n\n\n").is_empty());
    assert!(run(&mut s, "-LAYER A N S1\n\n\n\n").is_empty());
    assert_eq!(states(&s).iter().map(|st| st.0.as_str()).collect::<Vec<_>>(), ["S1", "S2", "b"]);
    // The old name in another case.
    assert!(run(&mut s, "-LAYER A N s1 S9\n\n\n").is_empty());
    assert_eq!(states(&s).iter().map(|st| st.0.as_str()).collect::<Vec<_>>(), ["S9", "S2", "b"]);
    assert!(run(&mut s, "-LAYER A D s9\n\n\n").is_empty());
    assert_eq!(states(&s).iter().map(|st| st.0.as_str()).collect::<Vec<_>>(), ["S2", "b"]);
}

#[test]
fn esc_drops_the_state_being_saved_and_undo_takes_back_name_and_delete() {
    let mut s = drawing();
    run(
        &mut s,
        "-LAYER A S S1



",
    );
    // Esc at the toggles: that state isn't saved and the next Save's toggles stay as they were;
    // what the same -LAYER did before Esc stays, as for the other -LAYER options.
    s.script(
        "-LAYER A S S2

S S3 C ON
",
    )
    .unwrap();
    s.input(Input::Cancel).unwrap();
    assert!(s.running.is_none());
    assert_eq!(states(&s), [state("S1", LayerState::ALL), state("S2", LayerState::ALL)]);
    assert_eq!(s.settings.layer_state_mask, LayerState::ALL);
    // Name and Delete are one undo step each, like Save.
    run(
        &mut s,
        "-LAYER A N S1 S9


",
    );
    run(
        &mut s,
        "-LAYER A D S2


",
    );
    assert_eq!(states(&s), [state("S9", LayerState::ALL)]);
    s.undo().unwrap();
    assert_eq!(states(&s), [state("S9", LayerState::ALL), state("S2", LayerState::ALL)]);
    s.undo().unwrap();
    assert_eq!(states(&s), [state("S1", LayerState::ALL), state("S2", LayerState::ALL)]);
}

#[test]
fn restore_finds_the_state_in_any_case() {
    let mut s = drawing();
    run(&mut s, "-LAYER A S S1\n\n\n\n");
    run(&mut s, "-LAYER C 3 A M C\n\n");
    let d = s.doc().unwrap();
    assert_eq!((d.layer("A").unwrap().color, d.header.str("CLAYER", "0")), (cadcraft_color::Color::Index(3), "C".to_string()));
    assert!(run(&mut s, "-LAYER A R s1\n\n\n").is_empty());
    let d = s.doc().unwrap();
    assert_eq!((d.layer("A").unwrap().color, d.header.str("CLAYER", "0")), (cadcraft_color::Color::Index(1), "0".to_string()));
}

#[test]
fn json_layer_states_take_a_mask_and_find_names_in_any_case() {
    let mut s = drawing();
    s.execute("layerstate.save", &json!({ "name": "M" })).unwrap();
    s.execute("layerstate.save", &json!({ "name": "On", "mask": LayerState::ON })).unwrap();
    for bad in [json!(4096), json!(-1), json!(1.5), json!("1")] {
        assert!(s.execute("layerstate.save", &json!({ "name": "X", "mask": bad })).is_err(), "{bad}");
    }
    assert_eq!(states(&s), [state("M", LayerState::ALL), state("On", 513)]);
    // Restore, Rename and Delete find a name in another case; a name another state has in any
    // case is refused, the state's own in another case too.
    s.execute("layer.set", &json!({ "name": "A", "color": "3" })).unwrap();
    s.execute("layerstate.restore", &json!({ "name": "m" })).unwrap();
    assert_eq!(s.doc().unwrap().layer("A").unwrap().color, cadcraft_color::Color::Index(1));
    assert!(s.execute("layerstate.rename", &json!({ "from": "m", "to": "ON" })).is_err());
    assert!(s.execute("layerstate.rename", &json!({ "from": "m", "to": "m" })).is_err());
    s.execute("layerstate.rename", &json!({ "from": "m", "to": "M2" })).unwrap();
    s.execute("layerstate.delete", &json!({ "name": "on" })).unwrap();
    assert_eq!(states(&s), [state("M2", LayerState::ALL)]);
    // Names that differ only in case (CADCraft's own record can hold both): the one spelled so.
    s.execute("layerstate.save", &json!({ "name": "m2", "mask": LayerState::ON })).unwrap();
    s.execute("layerstate.delete", &json!({ "name": "m2" })).unwrap();
    assert_eq!(states(&s), [state("M2", LayerState::ALL)]);
}
