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
fn rotate_scale_and_mirror_refuse_an_ole_frame() {
    let (mut s, h) = session();
    let before = corners(&s);
    for (cmd, p) in [
        ("rotate", json!({ "handles": [h], "base": [0, 0], "angle": 30 })),
        ("scale", json!({ "handles": [h], "base": [0, 0], "factor": 2 })),
        ("mirror", json!({ "handles": [h], "p1": [0, 0], "p2": [1, 0] })),
    ] {
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
