//! OPTIONS: the JSON form reads and sets the user-profile variables without a dialog.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::options::{PROFILE_VARS, profile, restore};
use serde_json::json;

#[test]
fn options_json_reports_sets_and_resets_the_profile() {
    let mut s = Session::new();
    let all = s.execute("options", &json!({})).unwrap();
    for name in PROFILE_VARS {
        assert!(all.get(*name).is_some(), "{name} is reported");
    }
    assert_eq!(all["PICKBOX"], json!(3.0));

    // Names in any case; the reply is the whole profile.
    let r = s.execute("options", &json!({"pickbox": 6, "GRIPSIZE": 9, "PICKFIRST": 0, "CursorSize": 25})).unwrap();
    assert_eq!(
        (r["PICKBOX"].clone(), r["GRIPSIZE"].clone(), r["PICKFIRST"].clone(), r["CURSORSIZE"].clone()),
        (json!(6.0), json!(9.0), json!(0), json!(25.0))
    );
    assert_eq!(s.execute("getvar", &json!({"name": "PICKBOX"})).unwrap()["value"], json!(6.0));

    // All or nothing: a bad name or value changes nothing.
    assert!(s.execute("options", &json!({"PICKBOX": 2, "LUNITS": 4})).is_err(), "drawing variables are not profile settings");
    assert!(s.execute("options", &json!({"APERTURE": 20, "SNAPUNIT": "wide"})).is_err());
    assert!(s.execute("options", &json!({"reset": "yes"})).is_err());
    assert!(s.execute("options", &json!([1, 2])).is_err());
    let now = s.execute("options", &json!({})).unwrap();
    assert_eq!((now["PICKBOX"].clone(), now["APERTURE"].clone()), (json!(6.0), json!(10.0)));

    // Reset, then apply in the same call.
    let r = s.execute("options", &json!({"reset": true, "APERTURE": 15})).unwrap();
    assert_eq!((r["PICKBOX"].clone(), r["GRIPSIZE"].clone(), r["APERTURE"].clone()), (json!(3.0), json!(5.0), json!(15.0)));
}

#[test]
fn saved_profile_restores_and_hostile_entries_are_skipped() {
    let mut a = Session::new();
    a.execute("options", &json!({"OSMODE": 515, "ORTHOMODE": 1, "POLARANG": 15, "GRIDUNIT": [2, 2], "DYNMODE": 0, "QPMODE": 1})).unwrap();
    let saved = profile(&a);
    let mut b = Session::new();
    restore(&mut b, &saved);
    assert_eq!(profile(&b), saved);

    // Unknown names, drawing variables, bad values and non-objects are ignored, never panic.
    let mut c = Session::new();
    restore(&mut c, &json!({"PICKBOX": "huge", "LUNITS": 4, "NOPE": 1, "POLARANG": -5, "SNAPUNIT": ["x"], "GRIPSIZE": 8}));
    restore(&mut c, &json!("not an object"));
    restore(&mut c, &json!(null));
    let p = profile(&c);
    assert_eq!((p["PICKBOX"].clone(), p["GRIPSIZE"].clone()), (json!(3.0), json!(8.0)));
    assert_ne!(c.execute("getvar", &json!({"name": "LUNITS"})).ok().map(|v| v["value"].clone()), Some(json!(4)));
}

#[test]
fn typed_options_without_a_dialog_reports_how_to_change_settings() {
    let mut s = Session::new();
    s.cmdline("OP").unwrap();
    assert!(s.log.iter().any(|l| l.contains("SETVAR")), "{:?}", s.log);
}

fn getvar(s: &mut Session, name: &str) -> serde_json::Value {
    s.execute("getvar", &json!({"name": name})).unwrap()["value"].clone()
}

#[test]
fn selection_highlight_variables() {
    let mut s = Session::new();
    // AutoCAD's defaults: highlighted, the colour effect, preview with and without a command,
    // the preview dashed and thicker.
    for (name, v) in [("HIGHLIGHT", 1), ("SELECTIONEFFECT", 1), ("SELECTIONPREVIEW", 3), ("PREVIEWEFFECT", 2)] {
        assert_eq!(getvar(&mut s, name), json!(v), "{name}");
    }
    s.execute("setvar", &json!({"name": "HIGHLIGHT", "value": 0})).unwrap();
    s.execute("options", &json!({"SELECTIONEFFECT": 0, "SELECTIONPREVIEW": 2, "PREVIEWEFFECT": 1})).unwrap();
    let saved = profile(&s);
    assert_eq!(saved["SELECTIONEFFECT"], json!(0));
    // HIGHLIGHT lasts the session only (AutoCAD doesn't save it either): not in the profile, and
    // OPTIONS, which sets profile variables, refuses it.
    assert!(saved.get("HIGHLIGHT").is_none());
    assert!(s.execute("options", &json!({"HIGHLIGHT": 0})).is_err());
    for (name, value) in [("SELECTIONEFFECT", json!(2)), ("SELECTIONPREVIEW", json!(4)), ("PREVIEWEFFECT", json!(-1)), ("PREVIEWEFFECT", json!("x"))]
    {
        assert!(s.execute("setvar", &json!({"name": name, "value": value})).is_err(), "{name} {value}");
    }
    assert_eq!(getvar(&mut s, "SELECTIONPREVIEW"), json!(2), "a refused value changes nothing");
    // A new session restores the profile and starts highlighted, even from a profile saved
    // with HIGHLIGHT 0 by a build that kept it.
    let mut t = Session::new();
    let mut old = saved.clone();
    old["HIGHLIGHT"] = json!(0);
    restore(&mut t, &old);
    assert_eq!((getvar(&mut t, "HIGHLIGHT"), getvar(&mut t, "SELECTIONPREVIEW"), getvar(&mut t, "PREVIEWEFFECT")), (json!(1), json!(2), json!(1)));
}

#[test]
fn selection_preview_follows_its_bits() {
    let mut s = Session::new();
    s.settings.pickfirst = true;
    s.settings.selectionpreview = 3;
    assert!(s.previews_selection(), "no command, bit 1");
    s.settings.selectionpreview = 2;
    assert!(!s.previews_selection(), "bit 1 off");
    s.start("erase").unwrap();
    assert!(s.picking_objects());
    assert!(s.previews_selection(), "ERASE asks for objects, bit 2");
    s.settings.selectionpreview = 1;
    assert!(!s.previews_selection(), "bit 2 off");
}

#[test]
fn selection_tab_variables_have_autocads_defaults_ranges_and_are_saved() {
    let mut s = Session::new();
    for (name, v) in [
        ("PREVIEWFILTER", json!(3)),
        ("GRIPS", json!(1)),
        ("GRIPOBJLIMIT", json!(100)),
        ("SELECTIONAREA", json!(1)),
        ("WINDOWAREACOLOR", json!(150)),
        ("CROSSINGAREACOLOR", json!(100)),
        ("SELECTIONAREAOPACITY", json!(25)),
    ] {
        assert_eq!(getvar(&mut s, name), v, "{name}");
    }
    s.execute("options", &json!({"PREVIEWFILTER": 20, "GRIPS": 0, "GRIPOBJLIMIT": 0, "SELECTIONAREA": 0, "WINDOWAREACOLOR": 5, "CROSSINGAREACOLOR": 3, "SELECTIONAREAOPACITY": 80, "PICKSTYLE": 0})).unwrap();
    for (name, value) in [
        ("PREVIEWFILTER", json!(64)),
        ("GRIPS", json!(3)),
        ("GRIPOBJLIMIT", json!(40_000)),
        ("WINDOWAREACOLOR", json!(0)),
        ("CROSSINGAREACOLOR", json!(256)),
        ("SELECTIONAREAOPACITY", json!(101)),
    ] {
        assert!(s.execute("setvar", &json!({"name": name, "value": value})).is_err(), "{name} {value}");
    }
    let mut t = Session::new();
    restore(&mut t, &profile(&s));
    assert_eq!(
        (
            getvar(&mut t, "PREVIEWFILTER"),
            getvar(&mut t, "GRIPS"),
            getvar(&mut t, "SELECTIONAREA"),
            getvar(&mut t, "WINDOWAREACOLOR"),
            getvar(&mut t, "PICKSTYLE")
        ),
        (json!(20), json!(0), json!(0), json!(5), json!(0)),
        "saved with the profile"
    );
}

#[test]
fn grips_show_up_to_the_object_limit() {
    let mut s = Session::new();
    assert!(s.shows_grips(100) && !s.shows_grips(101), "GRIPOBJLIMIT 100");
    s.settings.gripobjlimit = 0;
    assert!(s.shows_grips(100_000), "0: no limit");
    s.settings.grips = 0;
    assert!(!s.shows_grips(1), "GRIPS 0");
}

#[test]
fn preview_filter_excludes_each_kind_it_names() {
    use cadcraft_engine::select::preview_filtered;
    let mut s = Session::new();
    s.execute("mtext", &json!({"at": [0, 0], "text": "NOTE", "height": 2})).unwrap();
    s.execute("rectang", &json!({"p1": [10, 0], "p2": [20, 10]})).unwrap();
    s.execute("hatch", &json!({"points": [[15, 5]], "pattern": "SOLID"})).unwrap();
    s.execute("line", &json!({"points": [[30, 0], [40, 0]]})).unwrap();
    let d = s.doc().unwrap();
    let find = |k: &str| d.model.iter().find(|e| e.kind.type_name() == k).unwrap().clone();
    let (mtext, hatch, line) = (find("MText"), find("Hatch"), find("Line"));
    assert!(preview_filtered(d, &mtext, 8) && !preview_filtered(d, &mtext, 16));
    assert!(preview_filtered(d, &hatch, 16) && !preview_filtered(d, &hatch, 8));
    assert!(!preview_filtered(d, &line, 63), "a line on an unlocked layer, in no group");
    // Locked layers (1) and groups (32).
    let d = s.doc_mut().unwrap();
    d.ensure_layer("LOCKED");
    if let Some(l) = d.layers.iter_mut().find(|l| l.name == "LOCKED") {
        l.locked = true;
    }
    d.model.modify(line.handle, |e| e.common.layer = "LOCKED".into());
    d.groups.push(cadcraft_engine::doc::Group { name: "G".into(), description: String::new(), selectable: true, members: vec![mtext.handle] });
    let d = s.doc().unwrap();
    let line = d.entity(line.handle).unwrap();
    assert!(preview_filtered(d, line, 1) && !preview_filtered(d, line, 62));
    assert!(preview_filtered(d, &mtext, 32));
}
