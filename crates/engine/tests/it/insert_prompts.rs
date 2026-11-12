//! Command-line INSERT: X/Y/Z scale prompts and the Basepoint/Scale/X/Y/Z/Rotate options (#245).

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

/// (x, y, x scale, y scale, z scale, rotation in degrees) of the last block reference.
fn last_insert(s: &Session) -> (f64, f64, f64, f64, f64, f64) {
    let r = |v: f64| (v * 1e9).round() / 1e9;
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Insert(i) => Some((r(i.insert.x), r(i.insert.y), r(i.scale.x), r(i.scale.y), r(i.scale.z), r(i.rotation.to_degrees()))),
            _ => None,
        })
        .last()
        .unwrap()
}

/// The options of AutoCAD 2024's -INSERT, each run as AutoLISP's `command` gives it (accoreconsole,
/// the block reference read back with `entget`): X/Y/Z presets set that axis alone and skip the
/// scale prompts, XYZ asks Z too, a Z factor not asked is the X factor's size, and a Basepoint is
/// picked on the block dropped at the cursor (the origin in a script).
#[test]
fn insert_options_match_autocad() {
    // A line (0,0)-(10,0) and a circle at (5,5) r 3, around `base`, as block `name`.
    let block = |s: &mut Session, name: &str, base: [f64; 2], unit: f64| {
        let at = |x: f64, y: f64| [base[0] + x * unit, base[1] + y * unit];
        s.execute("line", &json!({ "points": [at(0.0, 0.0), at(10.0, 0.0)] })).unwrap();
        s.execute("circle", &json!({ "center": at(5.0, 5.0), "radius": 3.0 * unit })).unwrap();
        let hs: Vec<String> = s.doc().unwrap().model.handles().iter().map(|h| h.hex()).collect();
        s.execute("block", &json!({ "name": name, "base": base, "keep": "delete", "handles": hs })).unwrap();
    };
    let cases: [(&str, [f64; 2], f64, &str, (f64, f64, f64, f64, f64, f64)); 17] = [
        // X -1, Y as X: Z +1.
        ("B3", [612345.678, 2398765.432], 1.0, "\"B3\" '(612365.678 2398775.432) -1 \"\" 0", (612365.678, 2398775.432, -1.0, -1.0, 1.0, 0.0)),
        ("B4", [0.0, 0.0], 1.0, "\"B4\" \"_S\" 2.5 '(20.0 10.0) 45", (20.0, 10.0, 2.5, 2.5, 2.5, 45.0)),
        ("B5", [0.0, 0.0], 1.0, "\"B5\" \"_R\" 60 '(20.0 10.0) 2 \"\"", (20.0, 10.0, 2.0, 2.0, 2.0, 60.0)),
        // X alone: the next number is the rotation.
        ("B6", [0.5, 0.25], 0.001, "\"B6\" \"_X\" 2 '(0.52 0.26) 3", (0.52, 0.26, 2.0, 1.0, 1.0, 3.0)),
        (
            "B7",
            [612345.678, 2398765.432],
            1.0,
            "\"B7\" \"_B\" '(612350.678 2398770.432) '(612365.678 2398775.432) 1 \"\" 0",
            (15.0, 5.0, 1.0, 1.0, 1.0, 0.0),
        ),
        ("B8", [0.0, 0.0], 1.0, "\"B8\" '(20.0 10.0) \"_XYZ\" 2 3 4 0", (20.0, 10.0, 2.0, 3.0, 4.0, 0.0)),
        ("B9", [0.0, 0.0], 1.0, "\"B9\" '(20.0 10.0) \"_C\" '(26.0 14.0) 0", (20.0, 10.0, 6.0, 4.0, 6.0, 0.0)),
        // Z is stored as a size unless typed at XYZ's Z prompt: Scale -2, XYZ -2 with Enter at Y
        // and Z, and a Z preset of -3 all give +; XYZ's -4 stays.
        ("C1", [0.0, 0.0], 1.0, "\"C1\" \"_S\" -2 '(20.0 10.0) 0", (20.0, 10.0, -2.0, -2.0, 2.0, 0.0)),
        ("C2", [0.0, 0.0], 1.0, "\"C2\" '(20.0 10.0) \"_XYZ\" -2 \"\" \"\" 0", (20.0, 10.0, -2.0, -2.0, 2.0, 0.0)),
        ("C5", [0.0, 0.0], 1.0, "\"C5\" \"_Z\" -3 '(20.0 10.0) 0", (20.0, 10.0, 1.0, 1.0, 3.0, 0.0)),
        ("C6", [0.0, 0.0], 1.0, "\"C6\" '(20.0 10.0) \"_XYZ\" 2 3 -4 0", (20.0, 10.0, 2.0, 3.0, -4.0, 0.0)),
        ("C7", [0.0, 0.0], 1.0, "\"C7\" \"_X\" -2 '(20.0 10.0) 0", (20.0, 10.0, -2.0, 1.0, 1.0, 0.0)),
        // Scale and the axis presets in turn: the later one wins for its axes.
        ("C8", [0.0, 0.0], 1.0, "\"C8\" \"_S\" -3 \"_X\" 2 '(20.0 10.0) 0", (20.0, 10.0, 2.0, -3.0, 3.0, 0.0)),
        ("C9", [0.0, 0.0], 1.0, "\"C9\" \"_X\" 2 \"_S\" -3 '(20.0 10.0) 0", (20.0, 10.0, -3.0, -3.0, 3.0, 0.0)),
        ("CA", [0.0, 0.0], 1.0, "\"CA\" \"_S\" 2 \"_Z\" -5 '(20.0 10.0) 0", (20.0, 10.0, 2.0, 2.0, 5.0, 0.0)),
        // XYZ then a corner (by Corner or straight away) still asks Z, which defaults to X's size.
        ("C3", [0.0, 0.0], 1.0, "\"C3\" '(20.0 10.0) \"_XYZ\" \"_C\" '(26.0 14.0) \"\" 0", (20.0, 10.0, 6.0, 4.0, 6.0, 0.0)),
        ("C4", [0.0, 0.0], 1.0, "\"C4\" '(20.0 10.0) \"_XYZ\" '(14.0 14.0) \"\" 0", (20.0, 10.0, -6.0, 4.0, 6.0, 0.0)),
    ];
    for (name, base, unit, args, want) in cases {
        let mut s = Session::new();
        block(&mut s, name, base, unit);
        s.cmdline(&format!("(command \"_.-INSERT\" {args})")).unwrap();
        assert!(s.running.is_none(), "{name}: {}", s.prompt_text());
        assert_eq!(last_insert(&s), want, "{name}");
    }
    // Basepoint is picked on the block dropped at the cursor: with the cursor at (100, 50)
    // instead of the origin, B7's call inserts 100 and 50 further on.
    let mut s = Session::new();
    block(&mut s, "B7", [612345.678, 2398765.432], 1.0);
    s.cursor = cadcraft_engine::geom::Vec2::new(100.0, 50.0);
    s.cmdline("(command \"_.-INSERT\" \"B7\" \"_B\" '(612350.678 2398770.432) '(612365.678 2398775.432) 1 \"\" 0)").unwrap();
    assert_eq!(last_insert(&s), (115.0, 55.0, 1.0, 1.0, 1.0, 0.0));
}
