//! MINSERT, one block reference repeated in rows and columns, as AutoCAD 2024 makes and edits it
//! (accoreconsole: the prompts, and `entget` of the reference after MINSERT, EXPLODE, MIRROR,
//! SCALE and ROTATE).

use std::f64::consts::PI;

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Insert};
use serde_json::{Value, json};

fn handle(v: &Value) -> String {
    v.get("handle").unwrap_or(&v["handles"][0]).as_str().unwrap().to_owned()
}

/// A session with block B (a line from (0, 0) to (10, 0) and a circle of radius 3 round (5, 5),
/// base point (0, 0)), after the AutoLISP `code`.
fn run(code: &str) -> Session {
    let mut s = Session::new();
    let a = handle(&s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap());
    let c = handle(&s.execute("circle", &json!({"center": [5, 5], "radius": 3})).unwrap());
    s.execute("-block", &json!({"name": "B", "base": [0, 0], "handles": [a, c]})).unwrap();
    s.cmdline(code).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    s
}

fn inserts(s: &Session) -> Vec<Insert> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Insert(i) => Some(i.clone()),
            _ => None,
        })
        .collect()
}

/// The grid as `entget` gives it: 70 columns, 71 rows, 44 between columns, 45 between rows (a
/// plain reference, one row and one column, as 1/1/0/0).
fn grid(i: &Insert) -> (u32, u32, f64, f64) {
    (i.cols, i.rows, i.col_spacing, i.row_spacing)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn the_grid_is_stored_as_autocad_2024_stores_it() {
    // (MINSERT's answers after the block name and insertion point, the grid AutoCAD stored.)
    let cases = [
        // One row: no distance between rows asked, and 0 stored for it.
        ("1 \"\" 0 1 3 20.0", (3, 1, 20.0, 0.0)),
        ("1 \"\" 0 3 1 15.0", (1, 3, 0.0, 15.0)),
        // One of each: a plain reference.
        ("1 \"\" 0 1 1", (1, 1, 0.0, 0.0)),
        // A unit cell: its width between columns, its height between rows, signed.
        ("1 \"\" 0 2 3 '(0.0 0.0) '(25.0 12.0)", (3, 2, 25.0, 12.0)),
        ("1 \"\" 0 2 3 '(25.0 12.0) '(0.0 0.0)", (3, 2, -25.0, -12.0)),
        // Its sides along the drawing's axes, the reference rotated or not.
        ("1 \"\" 30 2 3 '(0.0 0.0) '(25.0 12.0)", (3, 2, 25.0, 12.0)),
        // Typed distances are kept as typed, negative or 0 too.
        ("1 \"\" 0 2 2 -15.0 -20.0", (2, 2, -20.0, -15.0)),
        ("1 \"\" 0 2 2 0.0 20.0", (2, 2, 20.0, 0.0)),
        // The distances are not scaled with the reference.
        ("2 \"\" 30 2 2 15.0 20.0", (2, 2, 20.0, 15.0)),
    ];
    for (answers, want) in cases {
        let s = run(&format!("(command \"_.MINSERT\" \"B\" '(20.0 10.0) {answers})"));
        let [i] = inserts(&s).try_into().unwrap();
        let got = grid(&i);
        assert!(got.0 == want.0 && got.1 == want.1 && near(got.2, want.2) && near(got.3, want.3), "{answers}: {got:?}");
        assert!(near(i.insert.x, 20.0) && near(i.insert.y, 10.0));
    }
    let s = run("(command \"_.MINSERT\" \"B\" '(20.0 10.0) 2 \"\" 30 2 2 15.0 20.0)");
    let [i] = inserts(&s).try_into().unwrap();
    assert!(near(i.scale.x, 2.0) && near(i.scale.y, 2.0) && near(i.rotation, PI / 6.0));
}

#[test]
fn the_prompts_follow_autocad_2024() {
    let mut s = Session::new();
    let l = handle(&s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap());
    s.execute("-block", &json!({"name": "B", "base": [0, 0], "handles": [l]})).unwrap();
    // Each prompt, and what was written to the command line on the way to it.
    let mut asked = Vec::new();
    for input in ["minsert", "B", "20,10", "1", "", "0", "0", "2", "3", "x", "15", "20"] {
        let logged = s.log.len();
        s.cmdline(input).unwrap();
        asked.extend(s.log.iter().skip(logged).filter(|l| l.starts_with("Requires")).cloned());
        if let Some(p) = s.current_prompt() {
            asked.push(p.display());
        }
    }
    // The grid's prompts and messages are AutoCAD 2024's; the ones before are INSERT's.
    assert_eq!(
        asked,
        [
            "Enter block name or [?]:",
            "Specify insertion point or [Basepoint/Scale/X/Y/Z/Rotate]:",
            "Enter X scale factor, specify opposite corner or [Corner/XYZ] <1>:",
            "Enter Y scale factor <use X scale factor>:",
            "Specify rotation angle <0>:",
            "Enter number of rows (---) <1>:",
            "Requires an integer between 1 and 32767.",
            "Enter number of rows (---) <1>:",
            "Enter number of columns (|||) <1>:",
            "Enter distance between rows or specify unit cell (---):",
            "Requires a distance.",
            "Enter distance between rows or specify unit cell (---):",
            "Specify distance between columns (|||):",
        ]
    );
    assert!(s.running.is_none());
    let [i] = inserts(&s).try_into().unwrap();
    assert_eq!(grid(&i), (3, 2, 20.0, 15.0));
}

#[test]
fn counts_run_from_1_to_32767() {
    // AutoCAD 2024: "Requires an integer between 1 and 32767."
    let mut s = Session::new();
    let l = handle(&s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap());
    s.execute("-block", &json!({"name": "B", "base": [0, 0], "handles": [l]})).unwrap();
    for input in ["minsert", "B", "0,0", "1", "", "0", "32768", "32767", "1", "2"] {
        s.cmdline(input).unwrap();
    }
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(s.log.iter().filter(|l| l.starts_with("Requires an integer between 1 and 32767.")).count(), 1, "{:?}", s.log);
    let [i] = inserts(&s).try_into().unwrap();
    assert_eq!(grid(&i), (1, 32767, 0.0, 2.0));
}

#[test]
fn attribute_values_are_asked_once_after_the_grid() {
    let mut s = Session::new();
    let l = handle(&s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap());
    let a = handle(&s.execute("attdef", &json!({"tag": "TAG", "prompt": "Value", "at": [0, 2], "height": 1})).unwrap());
    s.execute("-block", &json!({"name": "BA", "base": [0, 0], "handles": [l, a]})).unwrap();
    for input in ["minsert", "BA", "0,700", "1", "", "0", "5", "4", "15", "20"] {
        s.cmdline(input).unwrap();
    }
    // After the grid, with no "<>" for an empty default (AutoCAD 2024: "Value: hello").
    assert_eq!(s.current_prompt().unwrap().display(), "Value:");
    s.cmdline("hello").unwrap();
    assert!(s.running.is_none());
    let [i] = inserts(&s).try_into().unwrap();
    assert_eq!(grid(&i), (4, 5, 20.0, 15.0));
    assert_eq!(i.attribs.iter().map(|a| a.text.value.as_str()).collect::<Vec<_>>(), ["hello"]);
}

#[test]
fn explode_leaves_a_grid_whole() {
    let s = run("(progn (command \"_.MINSERT\" \"B\" '(20.0 10.0) 1 \"\" 0 2 2 15.0 20.0) (command \"_.EXPLODE\" (entlast) \"\"))");
    let [i] = inserts(&s).try_into().unwrap();
    assert_eq!(grid(&i), (2, 2, 20.0, 15.0));
    assert_eq!(s.doc().unwrap().model.iter().count(), 1);
    assert!(s.log.iter().any(|l| l.contains("The object is minserted.")), "{:?}", s.log);
}

#[test]
fn edits_carry_the_grid_as_in_autocad_2024() {
    let made = "(command \"_.MINSERT\" \"B\" '(20.0 10.0) 1 \"\" 0 2 2 15.0 20.0)";
    // (the edit, the copy it leaves last: insertion point, X and Y scale, rotation, grid)
    let cases = [
        ("(command \"_.SCALE\" (entlast) \"\" '(20.0 10.0) 2)", ([20.0, 10.0], [2.0, 2.0], 0.0, (2, 2, 40.0, 30.0))),
        ("(command \"_.ROTATE\" (entlast) \"\" '(20.0 10.0) 30)", ([20.0, 10.0], [1.0, 1.0], PI / 6.0, (2, 2, 20.0, 15.0))),
        ("(command \"_.MOVE\" (entlast) \"\" '(20.0 10.0) '(25.0 13.0))", ([25.0, 13.0], [1.0, 1.0], 0.0, (2, 2, 20.0, 15.0))),
        // Mirrored about the vertical through the insertion point, and about the horizontal: X
        // scale -1, and the columns run the other way.
        ("(command \"_.MIRROR\" (entlast) \"\" '(20.0 10.0) '(20.0 30.0) \"_N\")", ([20.0, 10.0], [-1.0, 1.0], 0.0, (2, 2, -20.0, 15.0))),
        ("(command \"_.MIRROR\" (entlast) \"\" '(20.0 10.0) '(40.0 10.0) \"_N\")", ([20.0, 10.0], [-1.0, 1.0], PI, (2, 2, -20.0, 15.0))),
    ];
    for (edit, (at, scale, rot, want)) in cases {
        let s = run(&format!("(progn {made} {edit})"));
        let i = inserts(&s).pop().unwrap();
        let got = grid(&i);
        assert!(
            near(i.insert.x, at[0])
                && near(i.insert.y, at[1])
                && near(i.scale.x, scale[0])
                && near(i.scale.y, scale[1])
                && near(i.rotation, rot)
                && got.0 == want.0
                && got.1 == want.1
                && near(got.2, want.2)
                && near(got.3, want.3),
            "{edit}: {i:?}"
        );
    }
}

#[test]
fn the_json_form_makes_the_same_grid() {
    let mut s = Session::new();
    let l = handle(&s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap());
    s.execute("-block", &json!({"name": "B", "base": [0, 0], "handles": [l]})).unwrap();
    s.execute("minsert", &json!({"name": "B", "at": [20, 10], "rows": 2, "columns": 3, "rowSpacing": 15, "columnSpacing": 20})).unwrap();
    // One row: its distance is not kept.
    s.execute("minsert", &json!({"name": "B", "at": [20, 100], "rows": 1, "columns": 3, "rowSpacing": 15, "columnSpacing": 20})).unwrap();
    let got: Vec<_> = inserts(&s).iter().map(grid).collect();
    assert_eq!(got, [(3, 2, 20.0, 15.0), (3, 1, 20.0, 0.0)]);
    for bad in [
        json!({"name": "B", "at": [0, 0], "rows": 0}),
        json!({"name": "B", "at": [0, 0], "columns": 32768}),
        json!({"name": "B", "at": [0, 0], "rows": 2.5}),
        // A whole number written as a float is refused, as by the array commands' JSON forms.
        json!({"name": "B", "at": [0, 0], "rows": 2.0}),
        json!({"name": "B", "at": [0, 0], "columns": 1e2}),
        json!({"name": "B", "at": [0, 0], "rows": "2"}),
        json!({"name": "B", "at": [0, 0], "rows": -2}),
        // A distance that is not a number is refused, not taken as 0.
        json!({"name": "B", "at": [0, 0], "rows": 2, "rowSpacing": "15"}),
        json!({"name": "B", "at": [0, 0], "columns": 2, "columnSpacing": true}),
    ] {
        assert!(s.execute("minsert", &bad).is_err(), "{bad}");
    }
    // Nothing was made by the refused calls.
    assert_eq!(inserts(&s).len(), 2);
    // `explode` is INSERT's and leaves the grid made; 32767 is the largest count.
    let r = s.execute("minsert", &json!({"name": "B", "at": [0, 200], "rows": 2, "rowSpacing": 15, "explode": true})).unwrap();
    assert!(r["handle"].is_string(), "{r}");
    assert_eq!(inserts(&s).last().map(grid), Some((1, 2, 0.0, 15.0)));
    // A null count is 1 and a null distance 0, as missing ones are: each count on its own, then
    // both distances with two rows and two columns (one row or column keeps no distance anyway).
    s.execute("minsert", &json!({"name": "B", "at": [0, 250], "rows": null, "columns": 2, "columnSpacing": 20})).unwrap();
    assert_eq!(inserts(&s).last().map(grid), Some((2, 1, 20.0, 0.0)));
    s.execute("minsert", &json!({"name": "B", "at": [0, 270], "rows": 2, "columns": null, "rowSpacing": 15})).unwrap();
    assert_eq!(inserts(&s).last().map(grid), Some((1, 2, 0.0, 15.0)));
    s.execute("minsert", &json!({"name": "B", "at": [0, 290], "rows": 2, "columns": 2, "rowSpacing": null, "columnSpacing": null})).unwrap();
    assert_eq!(inserts(&s).last().map(grid), Some((2, 2, 0.0, 0.0)));
    s.execute("minsert", &json!({"name": "B", "at": [0, 300], "columns": 32767, "columnSpacing": 1})).unwrap();
    assert_eq!(inserts(&s).last().map(grid), Some((32767, 1, 1.0, 0.0)));
}
