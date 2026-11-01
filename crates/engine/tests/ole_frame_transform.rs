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
