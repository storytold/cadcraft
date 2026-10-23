//! MIRROR with MIRRTEXT 0: a TEXT or MTEXT stays readable but takes the box its mirror image
//! would occupy, measured with its style's font (#1169). Observed black-box in AutoCAD 2027 for
//! Mac for a left-justified TEXT mirrored about a vertical line: the copy's insertion point lands
//! at −(x + width), the far end of the mirrored box.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, HAlign, Handle};
use cadcraft_engine::geom::{Bounds2, Mat3, Vec2};
use cadcraft_engine::render;
use serde_json::{Value, json};

fn handle(v: &Value) -> Handle {
    Handle::parse_hex(v.as_str().unwrap()).unwrap()
}

/// MIRROR `h` about the line `p1`–`p2`, keeping the source; the copy's handle.
fn mirror(s: &mut Session, h: Handle, p1: [f64; 2], p2: [f64; 2]) -> Handle {
    let r = s.execute("mirror", &json!({ "handles": [h.hex()], "p1": p1, "p2": p2 })).unwrap();
    handle(&r["handles"][0])
}

/// The box a text draws in, measured like the renderer draws it.
fn drawn_box(s: &Session, h: Handle) -> Bounds2 {
    let d = s.doc().unwrap();
    match &d.entity(h).unwrap().kind {
        EntityKind::Text(t) => render::place_text_entity(d, t, &t.value).1,
        EntityKind::MText(t) => render::layout_mtext_entity(d, t).bounds,
        k => panic!("not a text: {k:?}"),
    }
}

fn mirrored(b: &Bounds2, p1: [f64; 2], p2: [f64; 2]) -> Bounds2 {
    let m = Mat3::mirror(Vec2::new(p1[0], p1[1]), Vec2::new(p2[0], p2[1]));
    Bounds2::from_points(b.corners().map(|c| m.apply(c)))
}

fn same_box(a: &Bounds2, b: &Bounds2) -> bool {
    a.min.near(b.min, 1e-6) && a.max.near(b.max, 1e-6)
}

fn text_of(s: &Session, h: Handle) -> cadcraft_engine::doc::Text {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Text(t) => t.clone(),
        k => panic!("not a TEXT: {k:?}"),
    }
}

#[test]
fn text_mirrored_about_a_vertical_line_takes_its_mirrored_extent() {
    let mut s = Session::new();
    let r = s.execute("text", &json!({ "at": [10, 10], "text": "ABC", "height": 5 })).unwrap();
    let src = handle(&r["handle"]);
    let width = drawn_box(&s, src).width();
    assert!(width > 1.0, "the text has a width: {width}");
    let copy = mirror(&mut s, src, [0.0, 0.0], [0.0, 100.0]);
    let t = text_of(&s, copy);
    // Readable (rotation 0), still left-justified, starting at the far end of the mirrored box.
    assert!(t.rotation.abs() < 1e-9, "rotation {}", t.rotation);
    assert_eq!(t.halign, HAlign::Left);
    assert!(t.insert.xy().near(Vec2::new(-(10.0 + width), 10.0), 1e-6), "insertion {:?}, width {width}", t.insert);
    assert!(same_box(&drawn_box(&s, copy), &mirrored(&drawn_box(&s, src), [0.0, 0.0], [0.0, 100.0])));
    // The source is untouched.
    assert!(text_of(&s, src).insert.xy().near(Vec2::new(10.0, 10.0), 1e-9));
}

#[test]
fn justified_text_mirrored_about_a_vertical_line_keeps_its_box() {
    for j in ["R", "C", "TL", "BR", "M"] {
        let mut s = Session::new();
        let r = s.execute("text", &json!({ "at": [10, 10], "text": "ABC", "height": 5, "justify": j })).unwrap();
        let src = handle(&r["handle"]);
        let before = text_of(&s, src);
        let copy = mirror(&mut s, src, [0.0, 0.0], [0.0, 100.0]);
        let t = text_of(&s, copy);
        assert!(t.rotation.abs() < 1e-9, "{j}: rotation {}", t.rotation);
        assert_eq!((t.halign, t.valign), (before.halign, before.valign), "{j}: justification kept");
        let want = mirrored(&drawn_box(&s, src), [0.0, 0.0], [0.0, 100.0]);
        let got = drawn_box(&s, copy);
        assert!(same_box(&got, &want), "{j}: drawn {got:?}, mirrored box {want:?}");
    }
}

#[test]
fn text_mirrored_about_a_horizontal_line_keeps_its_box() {
    // The readable text sits in the mirrored box, below the line: its baseline moves down by the
    // box height, its rotation stays 0.
    let mut s = Session::new();
    let r = s.execute("text", &json!({ "at": [10, 10], "text": "ABC", "height": 5 })).unwrap();
    let src = handle(&r["handle"]);
    let copy = mirror(&mut s, src, [0.0, 0.0], [100.0, 0.0]);
    let t = text_of(&s, copy);
    assert!(t.rotation.abs() < 1e-9, "rotation {}", t.rotation);
    assert!(t.insert.y < -10.0, "the baseline moves below the mirrored insertion point: {:?}", t.insert);
    assert!((t.insert.x - 10.0).abs() < 1e-9);
    assert!(same_box(&drawn_box(&s, copy), &mirrored(&drawn_box(&s, src), [0.0, 0.0], [100.0, 0.0])));
}

#[test]
fn text_mirrored_about_a_slanted_line_turns_with_its_box_and_reads_upward() {
    // Mirrored about the 45° line through the origin, the box of a horizontal text stands
    // upright; the text reads from the bottom (rotation 90°), never upside down.
    let mut s = Session::new();
    let r = s.execute("text", &json!({ "at": [10, 10], "text": "ABC", "height": 5 })).unwrap();
    let src = handle(&r["handle"]);
    let copy = mirror(&mut s, src, [0.0, 0.0], [100.0, 100.0]);
    let t = text_of(&s, copy);
    assert!((t.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "rotation {}", t.rotation.to_degrees());
    assert!(same_box(&drawn_box(&s, copy), &mirrored(&drawn_box(&s, src), [0.0, 0.0], [100.0, 100.0])));
}

#[test]
fn mtext_mirrored_about_a_vertical_line_stays_readable_in_its_mirrored_box() {
    let mut s = Session::new();
    let r = s.execute("mtext", &json!({ "at": [10, 30], "text": "MTEXT", "height": 5, "width": 50 })).unwrap();
    let src = handle(&r["handle"]);
    let copy = mirror(&mut s, src, [0.0, 0.0], [0.0, 100.0]);
    let d = s.doc().unwrap();
    let EntityKind::MText(t) = &d.entity(copy).unwrap().kind else { panic!("expected an MTEXT") };
    assert!(t.rotation.abs() < 1e-9, "readable, not upside down: rotation {}", t.rotation.to_degrees());
    assert_eq!(t.attach, 1, "attachment kept");
    // Top-left of the mirrored 50-wide frame: x from −60 to −10.
    assert!(t.insert.xy().near(Vec2::new(-60.0, 30.0), 1e-6), "insertion {:?}", t.insert);
    assert!(same_box(&drawn_box(&s, copy), &mirrored(&drawn_box(&s, src), [0.0, 0.0], [0.0, 100.0])));
}

#[test]
fn mirror_preview_shows_the_text_where_it_lands() {
    // The drag preview transforms texts one by one (the GPU mirrors the rest): it must place
    // them exactly where the command does.
    let mut s = Session::new();
    let r = s.execute("text", &json!({ "at": [10, 10], "text": "ABC", "height": 5 })).unwrap();
    let src = handle(&r["handle"]);
    for t in ["mirror", "all", "", "0,0"] {
        s.cmdline(t).unwrap();
    }
    let (_, extra) = s.drag(Vec2::new(0.0, 100.0)).expect("MIRROR drags its selection");
    let EntityKind::Text(shown) = &extra[1].kind else { panic!("expected the text") };
    let shown = shown.clone();
    s.cmdline("0,100").unwrap();
    s.cmdline("").unwrap();
    let copy = s.doc().unwrap().model.iter().map(|e| e.handle).filter(|h| *h != src).last().unwrap();
    let t = text_of(&s, copy);
    assert!(shown.insert.xy().near(t.insert.xy(), 1e-9), "preview {:?}, result {:?}", shown.insert, t.insert);
    assert!(shown.insert.x < -10.0, "the preview takes the mirrored extent: {:?}", shown.insert);
}

/// An INSERT (block missing, only its attribute matters) with one multiline attribute at (10, 10).
fn insert_with_multiline_attrib(s: &mut Session) -> Handle {
    use cadcraft_engine::doc::{Attrib, Insert, MText, MultilineAttrib, Text};
    use cadcraft_engine::geom::Vec3;
    let t: Text = serde_json::from_value(json!({ "insert": [10, 10, 0], "height": 5, "value": "LABEL", "rotation": 0.0 })).unwrap();
    let mt = MText {
        insert: Vec3::new(10.0, 10.0, 0.0),
        height: 5.0,
        width: 0.0,
        attach: 1,
        rotation: 0.0,
        style: "Standard".into(),
        contents: "LABEL".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    let a = Attrib {
        tag: "T".into(),
        text: t,
        invisible: false,
        constant: false,
        prompt: String::new(),
        props: Default::default(),
        multiline: Some(MultilineAttrib { mtext: mt, read_as: "LABEL".into() }),
        modes: Default::default(),
    };
    let ins = Insert {
        block: "NOPE".into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: vec![a],
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    s.add_entity(EntityKind::Insert(ins)).unwrap()
}

#[test]
fn multiline_attribute_of_a_mirrored_insert_follows_the_block_like_its_text() {
    // An attribute follows its block as before: its multiline text lands where the attribute's
    // own text lands (mirrored about x = 0: (−10, 10), turned half a turn), not in a mirrored box.
    let mut s = Session::new();
    let h = insert_with_multiline_attrib(&mut s);
    let copy = mirror(&mut s, h, [0.0, 0.0], [0.0, 100.0]);
    let EntityKind::Insert(i) = &s.doc().unwrap().entity(copy).unwrap().kind else { panic!("expected an INSERT") };
    let a = i.attribs.first().unwrap();
    let m = &a.multiline.as_ref().unwrap().mtext;
    assert!(a.text.insert.xy().near(Vec2::new(-10.0, 10.0), 1e-9), "text {:?}", a.text.insert);
    assert!(m.insert.xy().near(a.text.insert.xy(), 1e-9), "multiline {:?}, text {:?}", m.insert, a.text.insert);
    assert!((m.rotation - a.text.rotation).abs() < 1e-9, "multiline {}, text {}", m.rotation, a.text.rotation);
}

#[test]
fn attribute_of_a_mirrored_insert_does_not_depend_on_the_definition_default() {
    // INSERT with a negative X scale and ATTSYNC place the attribute from the block's matrix
    // alone: the length of the default value the definition happens to carry plays no part.
    let mut s = Session::new();
    for (name, default) in [("B1", "x"), ("B2", "a much longer default value")] {
        s.execute("attdef", &json!({ "tag": "T", "default": default, "at": [10, 0], "height": 5 })).unwrap();
        let hs: Vec<Value> = s.doc().unwrap().model.iter().map(|e| json!(e.handle.hex())).collect();
        s.execute("block", &json!({ "name": name, "base": [0, 0], "handles": hs, "keep": "delete" })).unwrap();
        let r = s.execute("insert", &json!({ "name": name, "at": [0, 0], "scale": [-1, 1], "attribs": { "T": "LONG VALUE HERE" } })).unwrap();
        let ins = handle(&r["handle"]);
        let attrib_insert = |s: &Session| match &s.doc().unwrap().entity(ins).unwrap().kind {
            EntityKind::Insert(i) => i.attribs.first().unwrap().text.insert.xy(),
            k => panic!("expected an INSERT: {k:?}"),
        };
        assert!(attrib_insert(&s).near(Vec2::new(-10.0, 0.0), 1e-9), "{name} at insert: {:?}", attrib_insert(&s));
        s.execute("attsync", &json!({ "name": name })).unwrap();
        assert!(attrib_insert(&s).near(Vec2::new(-10.0, 0.0), 1e-9), "{name} after ATTSYNC: {:?}", attrib_insert(&s));
    }
}

// ---- block attributes: only the ATTDEF is mirrored into a box; what is placed from it follows the
// ---- block's matrix exactly as before (rotation unchanged when the block mirrors, never measured)

use cadcraft_engine::doc::{Attrib, MText, MultilineAttrib, Text};
use cadcraft_engine::geom::Vec3;

fn first_attrib(s: &Session, ins: Handle) -> Attrib {
    match &s.doc().unwrap().entity(ins).unwrap().kind {
        EntityKind::Insert(i) => i.attribs.first().unwrap().clone(),
        k => panic!("expected an INSERT: {k:?}"),
    }
}

/// An ATTDEF `T` whose single-line text sits at (10, 10) and whose multiline text sits at
/// `(10 + dx, 10)`, both rotated by `rot`.
fn multiline_attdef(dx: f64, rot: f64) -> Attrib {
    let t: Text = serde_json::from_value(json!({ "insert": [10, 10, 0], "height": 5, "value": "TAG VALUE", "rotation": rot })).unwrap();
    let mt = MText {
        insert: Vec3::new(10.0 + dx, 10.0, 0.0),
        height: 5.0,
        width: 0.0,
        attach: 1,
        rotation: rot,
        style: "Standard".into(),
        contents: "A MUCH LONGER MULTILINE DEFAULT".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    Attrib {
        tag: "T".into(),
        text: t,
        invisible: false,
        constant: false,
        prompt: String::new(),
        props: Default::default(),
        multiline: Some(MultilineAttrib { mtext: mt, read_as: "A MUCH LONGER MULTILINE DEFAULT".into() }),
        modes: Default::default(),
    }
}

fn make_block(s: &mut Session, name: &str, hs: &[Handle]) {
    let hs: Vec<Value> = hs.iter().map(|h| json!(h.hex())).collect();
    s.execute("block", &json!({ "name": name, "base": [0, 0], "handles": hs, "keep": "delete" })).unwrap();
}

#[test]
fn attribute_of_a_block_inserted_with_a_negative_scale_keeps_its_rotation() {
    // INSERT (interactive INSERT and XREF attach go through the same code) and ATTSYNC place the
    // attribute from the block's matrix: when the block mirrors the text keeps its rotation, when
    // it only turns the text turns with it.
    let mut s = Session::new();
    s.execute("attdef", &json!({ "tag": "T", "default": "x", "at": [10, 0], "height": 5 })).unwrap();
    let hs: Vec<Handle> = s.doc().unwrap().model.iter().map(|e| e.handle).collect();
    make_block(&mut s, "B", &hs);
    for (sc, at, rot) in [([-1.0, 1.0], -10.0, 0.0), ([1.0, -1.0], 10.0, 0.0), ([-1.0, -1.0], -10.0, std::f64::consts::PI)] {
        let r = s.execute("insert", &json!({ "name": "B", "at": [0, 0], "scale": sc, "attribs": { "T": "VALUE" } })).unwrap();
        let ins = handle(&r["handle"]);
        for stage in ["INSERT", "ATTSYNC"] {
            if stage == "ATTSYNC" {
                s.execute("attsync", &json!({ "name": "B" })).unwrap();
            }
            let a = first_attrib(&s, ins);
            assert!(a.text.insert.xy().near(Vec2::new(at, 0.0), 1e-9), "{sc:?} {stage}: insert {:?}", a.text.insert);
            assert!((a.text.rotation - rot).abs() < 1e-9, "{sc:?} {stage}: rotation {}", a.text.rotation);
            assert!((a.text.height - 5.0).abs() < 1e-9, "{sc:?} {stage}: height {}", a.text.height);
            assert_eq!(a.text.value, "VALUE");
        }
    }
}

#[test]
fn multiline_attribute_of_a_mirrored_block_takes_the_rotation_of_its_text() {
    // A definition with a multiline text two units right of its single-line text: after INSERT
    // with scale [-1, 1] and after ATTSYNC the multiline text sits two units left of the text and
    // turns exactly as the text does (not upside down while the text reads forward).
    let mut s = Session::new();
    let h = s.add_entity(EntityKind::AttDef(multiline_attdef(2.0, 0.0))).unwrap();
    make_block(&mut s, "MB", &[h]);
    let r = s.execute("insert", &json!({ "name": "MB", "at": [0, 0], "scale": [-1, 1] })).unwrap();
    let ins = handle(&r["handle"]);
    for stage in ["INSERT", "ATTSYNC"] {
        if stage == "ATTSYNC" {
            s.execute("attsync", &json!({ "name": "MB" })).unwrap();
        }
        let a = first_attrib(&s, ins);
        let m = &a.multiline.as_ref().unwrap().mtext;
        assert!(a.text.insert.xy().near(Vec2::new(-10.0, 10.0), 1e-9), "{stage}: text {:?}", a.text.insert);
        assert!(m.insert.xy().near(Vec2::new(-12.0, 10.0), 1e-9), "{stage}: multiline {:?}", m.insert);
        assert!(a.text.rotation.abs() < 1e-9, "{stage}: text rotation {}", a.text.rotation);
        assert!((m.rotation - a.text.rotation).abs() < 1e-9, "{stage}: multiline {}, text {}", m.rotation, a.text.rotation);
    }
}

#[test]
fn attdef_text_and_its_multiline_keep_their_place_under_mirror() {
    // The multiline text of an ATTDEF is carried by the mapping of the ATTDEF's text, not measured
    // on its own: its offset in the text's frame and its turn relative to the text survive MIRROR
    // about any line.
    for (rot, dx, p1, p2) in [
        (0.0, 2.0, [0.0, 0.0], [0.0, 100.0]),
        (0.0, 2.0, [0.0, 0.0], [100.0, 0.0]),
        (0.5, 3.0, [0.0, 0.0], [100.0, 100.0]),
        (0.5, 3.0, [5.0, 0.0], [5.0, 100.0]),
    ] {
        let mut s = Session::new();
        let src = multiline_attdef(dx, rot);
        let h = s.add_entity(EntityKind::AttDef(src.clone())).unwrap();
        let c = mirror(&mut s, h, p1, p2);
        let EntityKind::AttDef(a) = &s.doc().unwrap().entity(c).unwrap().kind else { panic!("expected an ATTDEF") };
        let got = a.multiline.as_ref().unwrap().mtext.clone();
        let want_off = (src.multiline.as_ref().unwrap().mtext.insert.xy() - src.text.insert.xy()).rotate(-rot);
        let got_off = (got.insert.xy() - a.text.insert.xy()).rotate(-a.text.rotation);
        assert!(got_off.near(want_off, 1e-6), "{p1:?}-{p2:?}: offset in the text's frame {got_off:?}, was {want_off:?}");
        let turn = |x: f64| cadcraft_engine::geom::norm_angle(x);
        assert!(
            (turn(got.rotation - a.text.rotation)).min(turn(a.text.rotation - got.rotation)).abs() < 1e-6,
            "{p1:?}-{p2:?}: multiline {} against text {}",
            got.rotation,
            a.text.rotation
        );
        // An attribute made from the mirrored definition keeps them together too.
        make_block(&mut s, "MB", &[c]);
        let r = s.execute("insert", &json!({ "name": "MB", "at": [0, 0] })).unwrap();
        let at = first_attrib(&s, handle(&r["handle"]));
        let m = at.multiline.as_ref().unwrap().mtext.clone();
        assert!(((m.insert.xy() - at.text.insert.xy()).rotate(-at.text.rotation)).near(want_off, 1e-6), "{p1:?}-{p2:?}: inserted attribute");
    }
}
