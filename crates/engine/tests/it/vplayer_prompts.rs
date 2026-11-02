//! VPLAYER typed at the command line asks for an option, layer names and the viewports to change
//! instead of running the next inputs as commands.

use cadcraft_doc::{EntityKind, Handle, Viewport};
use cadcraft_engine::Session;
use serde_json::json;

fn vp(s: &Session, h: &str) -> Viewport {
    let h = Handle::parse_hex(h).unwrap();
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Viewport(v) => v.clone(),
        _ => panic!("not a viewport"),
    }
}

fn frozen(s: &Session, h: &str) -> Vec<String> {
    let mut f = vp(s, h).frozen_layers;
    f.sort();
    f
}

#[test]
fn typed_vplayer_changes_layers_in_chosen_viewports() {
    let mut s = Session::new();
    s.cmdline("line 0,0 100,0").unwrap();
    s.cmdline("").unwrap();
    s.execute("layer.new", &json!({ "name": "Walls" })).unwrap();
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    let r = s.execute("mview", &json!({ "count": 2, "p1": [20, 20], "p2": [220, 120] })).unwrap();
    let (a, b) = (r["viewports"][0].as_str().unwrap().to_string(), r["viewports"][1].as_str().unwrap().to_string());
    s.execute("mspace", &json!({ "handle": a })).unwrap();
    let undo_depth = s.state().unwrap().undo.len();

    // Freeze in the current viewport (the default inside a viewport).
    s.cmdline("VPLAYER").unwrap();
    assert_eq!(s.current_prompt().unwrap().display(), "Enter an option or [?/Color/Freeze/Thaw/Reset/Newfrz/Vpvisdflt]:");
    s.script("F\n0\n").unwrap();
    assert_eq!(s.current_prompt().unwrap().display(), "Enter an option or [All/Select/Current/Except current] <Current>:");
    s.script("\n").unwrap();
    assert_eq!(frozen(&s, &a), ["0"]);
    assert!(frozen(&s, &b).is_empty());
    // Still at the option prompt: Thaw everything everywhere, colour Walls in the others.
    s.script("T * A\nC 1 Walls E\n").unwrap();
    assert!(frozen(&s, &a).is_empty());
    assert_eq!(vp(&s, &b).layer_colors.len(), 1);
    assert!(vp(&s, &a).layer_colors.is_empty());
    // Default visibility, Reset to it, and a new layer frozen in all viewports.
    s.script("V Walls F\nR Wal* A\nN Doors\n\n").unwrap();
    assert!(s.current_prompt().is_none(), "Enter ends VPLAYER");
    assert!(s.doc().unwrap().layer("Walls").unwrap().vp_freeze_new);
    assert_eq!(frozen(&s, &a), ["Doors", "Walls"]);
    assert_eq!(frozen(&s, &b), ["Doors", "Walls"]);
    // One undo step for the whole command.
    assert_eq!(s.state().unwrap().undo.len(), undo_depth + 1);

    // Paper space: Select (the default there) picks viewports by a point on the sheet; ? lists.
    s.cmdline("pspace").unwrap();
    s.script("VPLAYER F 0\n\n170,70\n\n?\n170,70\n\n").unwrap();
    assert_eq!(frozen(&s, &b), ["0", "Doors", "Walls"]);
    assert_eq!(frozen(&s, &a), ["Doors", "Walls"]);
    assert!(s.log.iter().any(|l| l == "  0"), "? lists the frozen layers");

    // The Model tab refuses it without taking the next input.
    s.execute("layout.set", &json!({ "name": "Model" })).unwrap();
    s.cmdline("VPLAYER").unwrap();
    assert!(s.current_prompt().is_none());
}
