//! An OLE2FRAME is window-selectable (it counts in the extents, #2422): MOVE and COPY carry it by
//! the offset, and the transforms that would not keep it axis aligned are refused before anything
//! reports success (review on #2587).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Common, Drawing, EntityKind, RawTag, Space, Unknown};
use serde_json::json;

fn session() -> (Session, String) {
    let tag = |code: i32, v: &str| RawTag { code, value: v.into() };
    let mut d = Drawing::new_imperial();
    let kind =
        EntityKind::Unknown(Unknown { dxf_type: "OLE2FRAME".into(), tags: vec![tag(10, "30"), tag(20, "-19"), tag(11, "35"), tag(21, "-22")] });
    let h = d.add(&Space::Model, Common::default(), kind).unwrap();
    let mut s = Session::empty();
    s.open_drawing(d, "ole.dxf", None);
    (s, h.hex())
}

fn corners(s: &Session) -> Vec<(i32, String)> {
    let d = s.doc().unwrap();
    d.model
        .iter()
        .filter_map(|e| {
            if let EntityKind::Unknown(u) = &e.kind { Some(u.tags.iter().map(|t| (t.code, t.value.clone())).collect::<Vec<_>>()) } else { None }
        })
        .flatten()
        .collect()
}

#[test]
fn move_and_copy_carry_an_ole_frame() {
    let (mut s, h) = session();
    s.execute("move", &json!({ "handles": [h], "delta": [10, 5] })).unwrap();
    let want: Vec<(i32, String)> = [(10, "40"), (20, "-14"), (11, "45"), (21, "-17")].iter().map(|(c, v)| (*c, v.to_string())).collect();
    assert_eq!(corners(&s), want);
    s.execute("copy", &json!({ "handles": [h], "delta": [10, 5] })).unwrap();
    let all = corners(&s);
    assert_eq!(all.len(), 8);
    assert_eq!(all[4..].iter().map(|(_, v)| v.as_str()).collect::<Vec<_>>(), ["50", "-9", "55", "-12"]);
}

#[test]
fn rotate_and_mirror_refuse_an_ole_frame() {
    let (mut s, h) = session();
    let before = corners(&s);
    for (cmd, p) in
        [("rotate", json!({ "handles": [h], "base": [0, 0], "angle": 30 })), ("mirror", json!({ "handles": [h], "p1": [0, 0], "p2": [1, 0] }))]
    {
        let e = s.execute(cmd, &p).expect_err(cmd);
        assert!(e.to_string().contains("OLE object"), "{cmd}: {e}");
    }
    assert_eq!(corners(&s), before);
    assert_eq!(s.doc().unwrap().model.len(), 1, "nothing was copied");
}

/// The OLE data's marker and four corners (x, y, z doubles each), as hex split into chunks.
fn data_session(chunks: &[usize], hex_override: Option<&str>) -> (Session, String) {
    let tag = |code: i32, v: &str| RawTag { code, value: v.into() };
    let mut blob = vec![1u8, 0];
    for (x, y) in [(30.0f64, -19.0f64), (35.0, -19.0), (35.0, -22.0), (30.0, -22.0)] {
        for v in [x, y, 7.0] {
            blob.extend_from_slice(&v.to_le_bytes());
        }
    }
    let hex: String = hex_override.map(str::to_string).unwrap_or_else(|| blob.iter().map(|b| format!("{b:02X}")).collect());
    let mut tags = vec![tag(10, "30"), tag(20, "-19"), tag(11, "35"), tag(21, "-22")];
    let mut at = 0;
    for &n in chunks {
        let end = (at + n).min(hex.len());
        tags.push(tag(310, &hex[at..end]));
        at = end;
    }
    if at < hex.len() {
        tags.push(tag(310, &hex[at..]));
    }
    let mut d = Drawing::new_imperial();
    let h = d.add(&Space::Model, Common::default(), EntityKind::Unknown(Unknown { dxf_type: "OLE2FRAME".into(), tags })).unwrap();
    let mut s = Session::empty();
    s.open_drawing(d, "ole.dxf", None);
    (s, h.hex())
}

fn data_tags(s: &Session) -> Vec<String> {
    let d = s.doc().unwrap();
    d.model
        .iter()
        .filter_map(|e| {
            if let EntityKind::Unknown(u) = &e.kind {
                Some(u.tags.iter().filter(|t| t.code == 310).map(|t| t.value.clone()).collect::<Vec<_>>())
            } else {
                None
            }
        })
        .flatten()
        .collect()
}

#[test]
fn move_also_moves_the_corners_in_the_ole_data() {
    let (mut s, h) = data_session(&[10, 100, 40], None);
    let before = data_tags(&s);
    s.execute("move", &json!({ "handles": [h], "delta": [10, 5] })).unwrap();
    let after = data_tags(&s);
    assert_eq!(after.iter().map(String::len).collect::<Vec<_>>(), before.iter().map(String::len).collect::<Vec<_>>(), "chunk lengths are kept");
    let hex: String = after.concat();
    let bytes: Vec<u8> = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap()).collect();
    let f = |o: usize| f64::from_le_bytes(bytes[o..o + 8].try_into().unwrap());
    let want = [(40.0, -14.0), (45.0, -14.0), (45.0, -17.0), (40.0, -17.0)];
    for (c, (x, y)) in want.iter().enumerate() {
        assert_eq!((f(2 + c * 24), f(10 + c * 24), f(18 + c * 24)), (*x, *y, 7.0), "corner {c}");
    }
}

#[test]
fn move_leaves_malformed_or_short_ole_data_alone() {
    for (chunks, hex) in [(vec![4], "0100"), (vec![3, 3], "ABCDE"), (vec![10], "ZZ0100ZZ0100"), (vec![2], "é")] {
        let (mut s, h) = data_session(&chunks, Some(hex));
        let before = data_tags(&s);
        s.execute("move", &json!({ "handles": [h], "delta": [10, 5] })).unwrap();
        assert_eq!(data_tags(&s), before, "{hex}");
        assert_eq!(corners(&s)[0].1, "40", "the corner groups still move");
    }
}

/// OLE data whose bytes are not this frame's corners (an unknown header, so the layout does not
/// apply) is not rewritten by MOVE; the frame's own corner groups still move (review on #2587).
#[test]
fn move_leaves_ole_data_that_is_not_the_frames_corners_alone() {
    let mut blob = vec![9u8, 9];
    for (x, y) in [(1.0f64, 2.0f64), (3.0, 2.0), (3.0, 4.0), (1.0, 4.0)] {
        for v in [x, y, 7.0] {
            blob.extend_from_slice(&v.to_le_bytes());
        }
    }
    let hex: String = blob.iter().map(|b| format!("{b:02X}")).collect();
    let (mut s, h) = data_session(&[40, 100], Some(&hex));
    let before = data_tags(&s);
    s.execute("move", &json!({ "handles": [h], "delta": [10, 5] })).unwrap();
    assert_eq!(data_tags(&s), before);
    assert_eq!(corners(&s)[0].1, "40", "the corner groups still move");
}

#[test]
fn mocoro_refuses_before_it_changes_anything() {
    let (mut s, h) = session();
    let before = corners(&s);
    let undo = s.state().unwrap().undo.len();
    let ops = json!([{ "move": [10, 5] }, { "copy": [20, 0] }, { "rotate": 30 }]);
    let e = s.execute("mocoro", &json!({ "handles": [h], "base": [0, 0], "ops": ops })).expect_err("rotate");
    assert!(e.to_string().contains("OLE object"), "{e}");
    assert_eq!(corners(&s), before, "the earlier move and copy were not applied");
    assert_eq!(s.doc().unwrap().model.len(), 1);
    assert_eq!(s.state().unwrap().undo.len(), undo);
}

#[test]
fn a_locked_ole_frame_does_not_block_an_in_place_rotation() {
    let tag = |code: i32, v: &str| RawTag { code, value: v.into() };
    let mut d = Drawing::new_imperial();
    d.ensure_layer("L");
    let common = Common { layer: "L".into(), ..Common::default() };
    let kind =
        EntityKind::Unknown(Unknown { dxf_type: "OLE2FRAME".into(), tags: vec![tag(10, "30"), tag(20, "-19"), tag(11, "35"), tag(21, "-22")] });
    let ole = d.add(&Space::Model, common, kind).unwrap();
    if let Some(l) = d.layer_mut("L") {
        l.locked = true;
    }
    let mut s = Session::empty();
    s.open_drawing(d, "ole.dxf", None);
    let ln = s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let ln = ln["handle"].as_str().or_else(|| ln["handles"][0].as_str()).unwrap().to_string();
    let before = corners(&s);
    s.execute("rotate", &json!({ "handles": [ln.clone(), ole.hex()], "base": [0, 0], "angle": 90 })).unwrap();
    assert_eq!(corners(&s), before, "the locked frame is left alone");
    let d = s.doc().unwrap();
    let e = d.entity(cadcraft_engine::doc::Handle::parse_hex(&ln).unwrap()).unwrap();
    let cadcraft_engine::doc::EntityKind::Line(l) = &e.kind else { panic!("line") };
    assert!((l.b.y - 10.0).abs() < 1e-9 && l.b.x.abs() < 1e-9, "{l:?}");
}

/// The four corners (x, y) in the OLE data header, read back from the 310 groups.
fn data_corners(s: &Session) -> Vec<(f64, f64)> {
    let hex: String = data_tags(s).concat();
    let bytes: Vec<u8> = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap()).collect();
    let f = |o: usize| f64::from_le_bytes(bytes[o..o + 8].try_into().unwrap());
    (0..4).map(|c| (f(2 + c * 24), f(10 + c * 24))).collect()
}

fn line_in(s: &mut Session) -> String {
    let ln = s.execute("line", &json!({ "points": [[1, 2], [4, 6]] })).unwrap();
    ln["handle"].as_str().or_else(|| ln["handles"][0].as_str()).unwrap().to_string()
}

fn line_ends(s: &Session, h: &str) -> ((f64, f64), (f64, f64)) {
    let e = s.doc().unwrap().entity(cadcraft_engine::doc::Handle::parse_hex(h).unwrap()).unwrap();
    let EntityKind::Line(l) = &e.kind else { panic!("line") };
    ((l.a.x, l.a.y), (l.b.x, l.b.y))
}

/// SCALE with a uniform positive factor carries an OLE frame with the rest of the selection: the
/// corner groups and the corners in the OLE data are both mapped (review on #2587).
#[test]
fn scale_carries_an_ole_frame_with_the_rest_of_the_selection() {
    let (mut s, h) = data_session(&[10, 100, 40], None);
    let ln = line_in(&mut s);
    s.execute("scale", &json!({ "handles": [ln.clone(), h], "base": [0, 0], "factor": 2 })).unwrap();
    assert_eq!(line_ends(&s, &ln), ((2.0, 4.0), (8.0, 12.0)));
    let want: Vec<(i32, String)> = [(10, "60"), (20, "-38"), (11, "70"), (21, "-44")].iter().map(|(c, v)| (*c, v.to_string())).collect();
    let tags: Vec<(i32, String)> = corners(&s).into_iter().filter(|(c, _)| *c != 310).collect();
    assert_eq!(tags, want);
    assert_eq!(data_corners(&s), vec![(60.0, -38.0), (70.0, -38.0), (70.0, -44.0), (60.0, -44.0)]);
}

/// MOCORO with a scale step carries the frame too, like SCALE.
#[test]
fn mocoro_scales_an_ole_frame() {
    let (mut s, h) = data_session(&[10, 100, 40], None);
    s.execute("mocoro", &json!({ "handles": [h], "base": [0, 0], "ops": [{ "scale": 3 }] })).unwrap();
    assert_eq!(data_corners(&s)[0], (90.0, -57.0));
    assert_eq!(corners(&s)[0].1, "90");
}

/// ARRAYPATH with align rotates each copy, which an OLE frame refuses. The refusal comes before
/// the first copy is added: an error does not roll the drawing back (review on #2587).
#[test]
fn arraypath_refuses_before_adding_any_copy() {
    let (mut s, h) = session();
    // Straight for 30 units, then a right-angle bend: the first copies only move, a later one turns.
    let path = s.execute("pline", &json!({ "points": [[0, 0], [30, 0], [30, 30]] })).unwrap();
    let path = path["handle"].as_str().or_else(|| path["handles"][0].as_str()).unwrap().to_string();
    let count = s.doc().unwrap().model.len();
    let before = corners(&s);
    let e = s.execute("arraypath", &json!({ "handles": [h], "path": path, "count": 7, "align": true })).expect_err("align");
    assert!(e.to_string().contains("OLE object"), "{e}");
    assert_eq!(s.doc().unwrap().model.len(), count, "no copy was added");
    assert_eq!(corners(&s), before);
}

/// A refused command leaves the drawing and its undo history alone: UNDO afterwards takes back the
/// earlier MOVE, not a phantom step (review on #2587).
#[test]
fn a_refused_rotation_adds_no_undo_step() {
    let (mut s, h) = session();
    let original = corners(&s);
    s.execute("move", &json!({ "handles": [h.clone()], "delta": [10, 5] })).unwrap();
    let undo = s.state().unwrap().undo.len();
    s.execute("rotate", &json!({ "handles": [h], "base": [0, 0], "angle": 30 })).expect_err("rotate");
    assert_eq!(s.state().unwrap().undo.len(), undo);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(corners(&s), original);
}

/// The classic -ARRAY makes separate copies: a polar array that turns its items refuses an OLE
/// frame before the first copy, and one that keeps the items upright carries it.
#[test]
fn classic_polar_array_refuses_a_turned_frame_but_not_an_upright_one() {
    let (mut s, h) = session();
    let before = corners(&s);
    let turned = json!({ "handles": [h.clone()], "type": "polar", "center": [0, 0], "count": 4, "rotate": true });
    let e = s.execute("-array", &turned).expect_err("turned items");
    assert!(e.to_string().contains("OLE object"), "{e}");
    assert_eq!((s.doc().unwrap().model.len(), corners(&s)), (1, before));
    let upright = json!({ "handles": [h], "type": "polar", "center": [0, 0], "count": 4, "rotate": false });
    s.execute("-array", &upright).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 4);
}

/// The frame is picked by its outline but is not geometry for other commands: a line across it is
/// not cut at its edges by TRIM, and object snap near its corner offers no frame point (review on
/// #2587).
#[test]
fn ole_frame_is_not_a_trim_edge_or_snap_target() {
    use cadcraft_engine::snap::{mode, osnap};
    let (mut s, _frame) = session();
    let ln = s.execute("line", &json!({ "points": [[28, -20.5], [37, -20.5]] })).unwrap();
    let ln = ln["handle"].as_str().or_else(|| ln["handles"][0].as_str()).unwrap().to_string();
    // No cutting edge is named, so every object but the line itself is one.
    let _ = s.execute("trim", &json!({ "handle": ln.clone(), "pick": [32, -20.5] }));
    assert_eq!(line_ends(&s, &ln), ((28.0, -20.5), (37.0, -20.5)), "the line is not cut at the frame");
    assert_eq!(s.doc().unwrap().model.len(), 2);
    let all = mode::END | mode::MID | mode::INT | mode::PER | mode::CEN | mode::QUA | mode::NOD;
    let hit = osnap(s.doc().unwrap(), &Space::Model, cadcraft_engine::geom::Vec2::new(30.0, -19.0), 0.5, all, None, false);
    assert!(hit.is_none(), "no snap point on the frame: {hit:?}");
}
