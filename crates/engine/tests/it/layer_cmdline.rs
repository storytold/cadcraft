//! -LAYER: the command-line option loop runs the layer commands and ends on Enter.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Lineweight};
use serde_json::json;

#[test]
fn make_and_color_then_the_next_command_runs() {
    let mut s = Session::new();
    s.script("-LAYER M Walls C 1 Walls\n\nLINE 0,0 10,0\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let d = s.doc().unwrap();
    let walls = d.layer("Walls").expect("Walls created");
    assert_eq!(walls.color, cadcraft_color::Color::Index(1));
    assert_eq!(d.header.str("CLAYER", "0"), "Walls");
    let lines: Vec<_> = d.model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).collect();
    assert_eq!(lines.len(), 1, "{:?}", s.log);
    assert_eq!(lines[0].common.layer, "Walls");
    // The whole -LAYER session is one undo step.
    assert_eq!(s.state().unwrap().undo.len(), 2);
    s.undo().unwrap();
    s.undo().unwrap();
    assert!(s.doc().unwrap().layer("Walls").is_none());
}

#[test]
fn options_reach_every_layer_property() {
    let mut s = Session::new();
    s.cmdline("linetype {\"load\": \"DASHED\"}").unwrap();
    s.cmdline("-LAYER").unwrap();
    assert!(
        s.prompt_text().contains(
            "[?/Make/Set/New/Rename/ON/OFF/Color/Ltype/LWeight/TRansparency/MATerial/Plot/Freeze/Thaw/LOck/Unlock/stAte/Description/rEconcile]"
        ),
        "{}",
        s.prompt_text()
    );
    // MATerial is not available yet and must not swallow the next option.
    let script =
        "N A,B\nMAT\nS A\nF A\nF B\nLO B\nOFF A\nY\nTR 50 B\nLW 0.3 B\nP N B\nL NOPE\nL DASHED B\nC T 10,20,30 B\nD\nnote here\nB\nR B C\n?\n\n\n";
    s.script(script).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let d = s.doc().unwrap();
    assert_eq!(d.header.str("CLAYER", "0"), "A");
    let a = d.layer("A").unwrap();
    assert!(!a.on, "the current layer is turned off after Yes");
    assert!(!a.frozen, "the current layer cannot be frozen");
    assert!(d.layer("B").is_none(), "B was renamed");
    let c = d.layer("C").unwrap();
    assert!(c.frozen && c.locked && !c.plot);
    assert_eq!(c.transparency, 50);
    assert_eq!(c.lineweight, Lineweight::Mm100(30));
    assert_eq!(c.linetype, "DASHED", "NOPE is refused and DASHED applied: {:?}", s.log);
    assert_eq!(c.color, cadcraft_color::Color::True(cadcraft_color::Rgb(10, 20, 30)));
    assert_eq!(c.description, "note here");
    assert!(s.log.iter().any(|l| l.contains("not available yet")));
    assert!(s.log.iter().any(|l| l.contains("NOPE")));
}

#[test]
fn layer_commands_reject_unloaded_linetypes_and_duplicate_renames() {
    let mut s = Session::new();
    assert!(s.execute("layer.new", &json!({ "name": "A", "linetype": "NOPE" })).is_err());
    assert!(s.doc().unwrap().layer("A").is_none());
    s.execute("layer.new", &json!({ "name": "A" })).unwrap();
    s.execute("layer.new", &json!({ "name": "B" })).unwrap();
    assert!(s.execute("layer.set", &json!({ "name": "A", "linetype": "ByLayer" })).is_err());
    assert!(s.execute("layer.set", &json!({ "name": "A", "newName": "b" })).is_err());
    assert_eq!(s.doc().unwrap().layers.iter().filter(|l| l.name.eq_ignore_ascii_case("b")).count(), 1);
    // A case-only rename of the same layer is fine.
    s.execute("layer.set", &json!({ "name": "A", "newName": "a", "linetype": "continuous" })).unwrap();
    assert_eq!(s.doc().unwrap().layer("a").unwrap().linetype, "Continuous");
}
