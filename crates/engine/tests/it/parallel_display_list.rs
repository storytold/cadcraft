//! Model space is built on all cores for the screen: the chunks are joined in drawing order, so
//! the display list is exactly what one thread builds (vertex and triangle indices included).

use cadcraft_engine::Session;
use cadcraft_engine::doc::Space;
use cadcraft_engine::render::{DisplayList, Options, build, build_entities};
use serde_json::json;

fn same(a: &DisplayList, b: &DisplayList) {
    assert_eq!(a.prims.len(), b.prims.len());
    assert!(a.prims == b.prims, "primitives differ");
    assert!(a.verts == b.verts, "vertices differ");
    assert!(a.tris == b.tris, "triangles differ");
    assert_eq!((a.bounds.min, a.bounds.max), (b.bounds.min, b.bounds.max));
    assert_eq!(a.expanded, b.expanded);
}

#[test]
fn parallel_model_space_matches_one_thread() {
    let mut s = Session::new();
    // A block with a hatch in it, inserted many times: block expansion in every chunk.
    let r = s.execute("rectang", &json!({"p1": [0, 0], "p2": [4, 3]})).unwrap();
    let h = r["handle"].clone();
    s.execute("hatch", &json!({ "handles": [h.clone()] })).unwrap();
    s.execute("block", &json!({"name": "B", "base": [0, 0], "handles": [h], "keep": "delete"})).unwrap();
    // Well over two chunks of mixed entities: lines, circles (curves), text, inserts, points,
    // solid fills (triangles), so both vertex and triangle offsets are exercised.
    for i in 0..700 {
        let x = f64::from(i) * 10.0;
        match i % 7 {
            0 => s.execute("line", &json!({"points": [[x, 0], [x + 5.0, 5]]})),
            1 => s.execute("circle", &json!({"center": [x, 20], "radius": 3})),
            2 => s.execute("text", &json!({"at": [x, 40], "height": 2, "text": format!("T{i}")})),
            3 => s.execute("insert", &json!({"name": "B", "at": [x, 60]})),
            4 => s.execute("point", &json!({"at": [x, 80]})),
            5 => {
                let r = s.execute("rectang", &json!({"p1": [x, 100], "p2": [x + 4.0, 104]})).unwrap();
                s.execute("hatch", &json!({ "handles": [r["handle"].clone()], "pattern": "SOLID" }))
            }
            _ => s.execute("line", &json!({"points": [[x, 120], [x, 125]]})),
        }
        .unwrap();
    }
    let st = s.state().unwrap();
    let d = &st.doc;
    let n = d.space(&Space::Model).map(|m| m.len()).unwrap_or(0);
    assert!(n > 600, "{n} entities: enough for several parallel chunks");
    let one_thread = build_entities(d, d.space(&Space::Model).unwrap().iter().map(|e| e.as_ref()), &Options::default());
    let parallel = build(d, &Space::Model, &Options::default());
    assert!(!parallel.tris.is_empty() && !parallel.verts.is_empty());
    same(&parallel, &one_thread);
}
