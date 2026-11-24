//! JUSTIFYTEXT keeps the text where it is drawn; SCALETEXT scales its height about a base point (#3033).

use cadcraft_doc::{Drawing, EntityKind, HAlign, Handle, MText, Text, VAlign};
use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

const ALL: [&str; 15] = ["Left", "Align", "Fit", "Center", "Middle", "Right", "TL", "TC", "TR", "ML", "MC", "MR", "BL", "BC", "BR"];

/// The justification TEXT's option of that name stands for.
fn expect(name: &str) -> (HAlign, VAlign) {
    match name {
        "Left" => (HAlign::Left, VAlign::Baseline),
        "Align" => (HAlign::Aligned, VAlign::Baseline),
        "Fit" => (HAlign::Fit, VAlign::Baseline),
        "Center" => (HAlign::Center, VAlign::Baseline),
        "Middle" => (HAlign::Middle, VAlign::Baseline),
        "Right" => (HAlign::Right, VAlign::Baseline),
        n => {
            let h = match &n[1..] {
                "L" => HAlign::Left,
                "C" => HAlign::Center,
                _ => HAlign::Right,
            };
            let v = match &n[..1] {
                "T" => VAlign::Top,
                "M" => VAlign::Middle,
                _ => VAlign::Bottom,
            };
            (h, v)
        }
    }
}

fn handle(v: &Value) -> Handle {
    let h = v.get("handle").or_else(|| v.get("handles").and_then(|a| a.get(0))).unwrap_or_else(|| panic!("no handle in {v}"));
    Handle::parse_hex(h.as_str().unwrap()).unwrap()
}

fn text_of(s: &Session, h: Handle) -> Text {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Text(t) => t.clone(),
        EntityKind::AttDef(a) => a.text.clone(),
        k => panic!("not text: {k:?}"),
    }
}

fn mtext_of(s: &Session, h: Handle) -> MText {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::MText(m) => m.clone(),
        k => panic!("not mtext: {k:?}"),
    }
}

/// Every point of what the TEXT draws.
fn ink(d: &Drawing, t: &Text) -> Vec<Vec2> {
    cadcraft_render::place_text_entity(d, t, &t.value).0.strokes.into_iter().flatten().collect()
}

fn mink(d: &Drawing, m: &MText) -> Vec<Vec2> {
    cadcraft_render::layout_mtext_entity(d, m).strokes.into_iter().flatten().collect()
}

fn same(a: &[Vec2], b: &[Vec2], what: &str) {
    assert!(!a.is_empty(), "{what}: draws nothing");
    assert_eq!(a.len(), b.len(), "{what}");
    for (p, q) in a.iter().zip(b) {
        assert!(p.dist(*q) < 1e-6, "{what}: {p:?} moved to {q:?}");
    }
}

fn session_with_text(p: Value) -> (Session, Handle) {
    let mut s = Session::new();
    let h = handle(&s.execute("text", &p).unwrap());
    (s, h)
}

#[test]
fn justifytext_keeps_text_drawn_where_it_is_for_every_justification() {
    let (mut s, h) = session_with_text(json!({"at": [10, 5], "text": "Hello, World", "height": 2.5, "rotation": 30}));
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let EntityKind::Text(t) = &mut e.kind {
                t.width_factor = 0.8;
                t.oblique = 0.2;
            }
        })
        .unwrap();
    let before = ink(s.doc().unwrap(), &text_of(&s, h));
    // Round and round through every justification, each from the one before.
    for name in ALL.iter().chain(ALL.iter().rev()) {
        s.execute("justifytext", &json!({"handles": [h.hex()], "justify": name})).unwrap();
        let t = text_of(&s, h);
        same(&before, &ink(s.doc().unwrap(), &t), name);
        let (ha, va) = expect(name);
        assert_eq!((t.halign, t.valign), (ha, if matches!(ha, HAlign::Aligned | HAlign::Fit) { VAlign::Baseline } else { va }), "{name}");
    }
}

#[test]
fn justifytext_moves_the_alignment_point_not_the_text() {
    let (mut s, h) = session_with_text(json!({"at": [10, 5], "text": "ABCD", "height": 2}));
    let w = {
        let t = text_of(&s, h);
        cadcraft_render::place_text_entity(s.doc().unwrap(), &t, &t.value).0.width
    };
    let r = s.execute("justifytext", &json!({"handles": [h.hex()], "justify": "TR"})).unwrap();
    assert_eq!(r["changed"], 1);
    let t = text_of(&s, h);
    assert_eq!((t.halign, t.valign), (HAlign::Right, VAlign::Top));
    // Top right: the end of the baseline, a height above it.
    let at = t.justify_point();
    assert!((at.x - (10.0 + w)).abs() < 1e-9 && (at.y - 7.0).abs() < 1e-9, "{at:?}");
}

#[test]
fn justifytext_leaves_fit_text_drawn_as_it_was() {
    let (mut s, h) = session_with_text(json!({"at": [0, 0], "text": "Fit me", "height": 2, "justify": "F"}));
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let EntityKind::Text(t) = &mut e.kind {
                t.align_pt = Some(cadcraft_geom::Vec3::new(30.0, 40.0, 0.0));
            }
        })
        .unwrap();
    let before = ink(s.doc().unwrap(), &text_of(&s, h));
    for name in ["MC", "Align", "BR", "Fit"] {
        s.execute("justifytext", &json!({"handles": [h.hex()], "justify": name})).unwrap();
        same(&before, &ink(s.doc().unwrap(), &text_of(&s, h)), name);
    }
}

#[test]
fn justifytext_keeps_mtext_where_it_is() {
    let mut s = Session::new();
    let h = handle(&s.execute("mtext", &json!({"at": [4, 9], "text": "Single line note", "height": 2, "rotation": 40})).unwrap());
    let before = mink(s.doc().unwrap(), &mtext_of(&s, h));
    for (name, attach) in [
        ("BR", 9),
        ("MC", 5),
        ("TL", 1),
        ("TR", 3),
        ("BL", 7),
        ("Middle", 5),
        ("Center", 8),
        ("Right", 9),
        ("Left", 7),
        ("TC", 2),
        ("Align", 7),
        ("ML", 4),
        ("Fit", 7),
    ] {
        s.execute("justifytext", &json!({"handles": [h.hex()], "justify": name})).unwrap();
        let m = mtext_of(&s, h);
        assert_eq!(m.attach, attach, "{name}");
        same(&before, &mink(s.doc().unwrap(), &m), name);
    }
}

#[test]
fn justifytext_keeps_wrapped_mtext_lines_where_they_are() {
    let mut s = Session::new();
    let h = handle(&s.execute("mtext", &json!({"at": [0, 0], "text": "one two three four five six seven", "height": 1, "width": 12})).unwrap());
    let before = mink(s.doc().unwrap(), &mtext_of(&s, h));
    // The lines stay left aligned in the left column: nothing but the attachment point changes.
    for name in ["BL", "ML", "TL"] {
        s.execute("justifytext", &json!({"handles": [h.hex()], "justify": name})).unwrap();
        same(&before, &mink(s.doc().unwrap(), &mtext_of(&s, h)), name);
    }
}

#[test]
fn justifytext_skips_what_it_cannot_justify_and_counts_it() {
    let mut s = Session::new();
    let m = handle(&s.execute("mtext", &json!({"at": [0, 0], "text": "M", "height": 1})).unwrap());
    let line = handle(&s.execute("line", &json!({"points": [[0, 0], [1, 1]]})).unwrap());
    // Fit is bottom left for MTEXT, as in AutoCAD; the line is not text.
    let r = s.execute("justifytext", &json!({"handles": [m.hex(), line.hex()], "justify": "Fit"})).unwrap();
    assert_eq!((r["changed"].clone(), r["ignored"].clone()), (json!(1), json!(1)));
    assert_eq!(mtext_of(&s, m).attach, 7);
    // Existing leaves the justification as it is.
    let r = s.execute("justifytext", &json!({"handles": [m.hex()], "justify": "Existing"})).unwrap();
    assert_eq!(r["changed"], 0);
    assert_eq!(mtext_of(&s, m).attach, 7);
    assert!(s.execute("justifytext", &json!({"handles": [m.hex()], "justify": "Sideways"})).is_err());
    assert!(s.execute("justifytext", &json!({"handles": [m.hex()]})).is_err());
}

#[test]
fn justifytext_leaves_locked_layers_alone() {
    let mut s = Session::new();
    s.execute("layer.new", &json!({"name": "L", "current": true})).unwrap();
    let h = handle(&s.execute("text", &json!({"at": [0, 0], "text": "Locked", "height": 1})).unwrap());
    s.execute("layer.set", &json!({"name": "L", "locked": true})).unwrap();
    assert!(s.doc().unwrap().layer("L").unwrap().locked);
    let before = text_of(&s, h);
    let r = s.execute("justifytext", &json!({"handles": [h.hex()], "justify": "MC"})).unwrap();
    assert_eq!((r["changed"].clone(), r["locked"].clone()), (json!(0), json!(1)));
    assert_eq!(text_of(&s, h), before);
}

#[test]
fn justifytext_changes_attribute_definitions_and_block_attributes() {
    let mut s = Session::new();
    s.execute("attdef", &json!({"tag": "T", "default": "ADAPTER 01", "at": [0, 15], "height": 3})).unwrap();
    let def = s.doc().unwrap().model.last().unwrap().handle;
    let before_def = ink(s.doc().unwrap(), &text_of(&s, def));
    s.execute("justifytext", &json!({"handles": [def.hex()], "justify": "MC"})).unwrap();
    let t = text_of(&s, def);
    assert_eq!((t.halign, t.valign), (HAlign::Center, VAlign::Middle));
    same(&before_def, &ink(s.doc().unwrap(), &t), "attdef");

    let handles: Vec<_> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("block", &json!({"name": "Eq", "handles": handles, "base": [0, 0], "keep": "delete"})).unwrap();
    let ins = handle(&s.execute("insert", &json!({"name": "Eq", "at": [100, 200], "scale": 2, "rotation": 30})).unwrap());
    let attrib = |s: &Session| match &s.doc().unwrap().entity(ins).unwrap().kind {
        EntityKind::Insert(i) => i.attribs[0].text.clone(),
        k => panic!("{k:?}"),
    };
    let before = ink(s.doc().unwrap(), &attrib(&s));
    let r = s.execute("justifytext", &json!({"handles": [ins.hex()], "justify": "BR"})).unwrap();
    assert_eq!(r["changed"], 1);
    let t = attrib(&s);
    assert_eq!((t.halign, t.valign), (HAlign::Right, VAlign::Bottom));
    same(&before, &ink(s.doc().unwrap(), &t), "attribute");
}

#[test]
fn justifytext_at_the_command_line_is_one_undo_step() {
    let (mut s, h) = session_with_text(json!({"at": [10, 5], "text": "Line", "height": 2}));
    let before = text_of(&s, h);
    s.set_selection(vec![h]);
    s.script("JUSTIFYTEXT\nMC\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!((text_of(&s, h).halign, text_of(&s, h).valign), (HAlign::Center, VAlign::Middle));
    s.undo().unwrap();
    assert_eq!(text_of(&s, h), before);
}

// ---------------- SCALETEXT ----------------

fn about(p: Vec2, base: Vec2, k: f64) -> Vec2 {
    base + (p - base) * k
}

#[test]
fn scaletext_existing_scales_about_the_alignment_point() {
    let (mut s, h) = session_with_text(json!({"at": [10, 5], "text": "ABCD", "height": 2, "justify": "MC"}));
    let before = ink(s.doc().unwrap(), &text_of(&s, h));
    s.execute("scaletext", &json!({"handles": [h.hex()], "height": 6})).unwrap();
    let t = text_of(&s, h);
    assert_eq!((t.height, t.insert.x, t.insert.y), (6.0, 10.0, 5.0));
    let want: Vec<_> = before.iter().map(|p| about(*p, Vec2::new(10.0, 5.0), 3.0)).collect();
    same(&want, &ink(s.doc().unwrap(), &t), "existing");
}

#[test]
fn scaletext_named_base_point_stays_put() {
    let (mut s, h) = session_with_text(json!({"at": [10, 5], "text": "ABCD", "height": 2}));
    let t0 = text_of(&s, h);
    let before = ink(s.doc().unwrap(), &t0);
    let w = cadcraft_render::place_text_entity(s.doc().unwrap(), &t0, &t0.value).0.width;
    let centre = Vec2::new(10.0 + w / 2.0, 5.0);
    let r = s.execute("scaletext", &json!({"handles": [h.hex()], "base": "Center", "factor": 2.5})).unwrap();
    assert_eq!(r["changed"], 1);
    let t = text_of(&s, h);
    assert_eq!((t.height, t.halign, t.valign), (5.0, HAlign::Left, VAlign::Baseline), "justification kept");
    let want: Vec<_> = before.iter().map(|p| about(*p, centre, 2.5)).collect();
    same(&want, &ink(s.doc().unwrap(), &t), "centre");
    s.undo().unwrap();
    assert_eq!(text_of(&s, h), t0);
}

#[test]
fn scaletext_match_object_takes_the_height_of_the_picked_text() {
    let mut s = Session::new();
    let a = handle(&s.execute("text", &json!({"at": [0, 0], "text": "A", "height": 1})).unwrap());
    let b = handle(&s.execute("text", &json!({"at": [0, 10], "text": "B", "height": 4})).unwrap());
    let m = handle(&s.execute("mtext", &json!({"at": [0, 20], "text": "C", "height": 0.5})).unwrap());
    s.execute("scaletext", &json!({"handles": [a.hex(), m.hex()], "match": b.hex()})).unwrap();
    assert_eq!(text_of(&s, a).height, 4.0);
    assert_eq!(mtext_of(&s, m).height, 4.0);
    assert_eq!(text_of(&s, b).height, 4.0);
    let line = handle(&s.execute("line", &json!({"points": [[0, 0], [1, 1]]})).unwrap());
    assert!(s.execute("scaletext", &json!({"handles": [a.hex()], "match": line.hex()})).is_err());
}

#[test]
fn scaletext_scales_mtext_about_its_attachment_point_and_keeps_its_wrapping() {
    let mut s = Session::new();
    let h = handle(&s.execute("mtext", &json!({"at": [5, 5], "text": "one two three four", "height": 1, "width": 6})).unwrap());
    let before = mink(s.doc().unwrap(), &mtext_of(&s, h));
    s.execute("scaletext", &json!({"handles": [h.hex()], "base": "TL", "factor": 2})).unwrap();
    let m = mtext_of(&s, h);
    assert_eq!((m.height, m.width, m.insert.x, m.insert.y), (2.0, 12.0, 5.0, 5.0));
    let want: Vec<_> = before.iter().map(|p| about(*p, Vec2::new(5.0, 5.0), 2.0)).collect();
    same(&want, &mink(s.doc().unwrap(), &m), "mtext");
    // About the bottom left corner instead: the corner stays, the insertion point moves up.
    s.execute("scaletext", &json!({"handles": [h.hex()], "base": "BL", "factor": 0.5})).unwrap();
    let m2 = mtext_of(&s, h);
    assert_eq!((m2.height, m2.width), (1.0, 6.0));
    assert!(m2.insert.y < 5.0 && m2.insert.x == 5.0, "{:?}", m2.insert);
}

#[test]
fn scaletext_checks_its_arguments() {
    let (mut s, h) = session_with_text(json!({"at": [0, 0], "text": "A", "height": 1}));
    let hs = json!([h.hex()]);
    assert!(s.execute("scaletext", &json!({"handles": hs})).is_err(), "no size");
    assert!(s.execute("scaletext", &json!({"handles": hs, "height": 2, "factor": 2})).is_err(), "two sizes");
    assert!(s.execute("scaletext", &json!({"handles": hs, "height": 0})).is_err());
    assert!(s.execute("scaletext", &json!({"handles": hs, "factor": -1})).is_err());
    assert!(s.execute("scaletext", &json!({"handles": hs, "height": 2, "base": "Fit"})).is_err());
    assert!(s.execute("scaletext", &json!({"handles": hs, "height": 2, "base": "Nowhere"})).is_err());
    assert_eq!(text_of(&s, h).height, 1.0);
}

#[test]
fn scaletext_at_the_command_line_asks_base_then_height() {
    let (mut s, h) = session_with_text(json!({"at": [10, 5], "text": "ABCD", "height": 2}));
    s.set_selection(vec![h]);
    s.script("SCALETEXT\nExisting\n3\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(text_of(&s, h).height, 3.0);
    s.set_selection(vec![h]);
    s.script("SCALETEXT\nExisting\nScale factor\n2\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(text_of(&s, h).height, 6.0);
}
