//! Command-line INSERT: X/Y scale prompts and the Basepoint/Scale/Rotate options (#245).

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

/// (x, y, x scale, y scale, rotation in degrees) of every block reference, in drawing order.
fn inserts(s: &Session) -> Vec<(f64, f64, f64, f64, f64)> {
    let r = |v: f64| (v * 1e6).round() / 1e6;
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Insert(i) => Some((r(i.insert.x), r(i.insert.y), r(i.scale.x), r(i.scale.y), r(i.rotation.to_degrees()))),
            _ => None,
        })
        .collect()
}

#[test]
fn script_insert_reads_x_and_y_scale_then_rotation_and_honours_its_options() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    s.execute("block", &json!({ "name": "B1", "base": [0, 0], "keep": "delete", "handles": ["100"] })).unwrap();
    s.script(
        "-INSERT B1 20,20 1 1 0\n\
         -INSERT B1 40,20 2 3 45\n\
         -INSERT B1 S 2 60,20 0\n\
         -INSERT B1 R 30 80,20 1 1\n\
         -INSERT B1 100,20 C 105,22 0\n\
         -INSERT B1 B 5,0 200,0 1 1 0\n",
    )
    .unwrap();
    assert!(s.running.is_none(), "every INSERT finished: {}", s.prompt_text());
    assert_eq!(
        inserts(&s),
        vec![
            (20.0, 20.0, 1.0, 1.0, 0.0),
            (40.0, 20.0, 2.0, 3.0, 45.0),
            (60.0, 20.0, 2.0, 2.0, 0.0),
            (80.0, 20.0, 1.0, 1.0, 30.0),
            (100.0, 20.0, 5.0, 2.0, 0.0),
            // Base point 5,0 of the block lands on 200,0.
            (195.0, 0.0, 1.0, 1.0, 0.0),
        ]
    );
    // Enter at the Y scale prompt uses the X scale factor.
    s.script("-INSERT B1 0,50 4\n\n0\n").unwrap();
    assert_eq!(inserts(&s).last(), Some(&(0.0, 50.0, 4.0, 4.0, 0.0)));

    // The JSON form takes [x, y] scale factors and a base point too.
    s.execute("insert", &json!({ "name": "B1", "at": [0, 80], "scale": [2, 3], "basePoint": [10, 0] })).unwrap();
    assert_eq!(inserts(&s).last(), Some(&(-20.0, 80.0, 2.0, 3.0, 0.0)));
}
