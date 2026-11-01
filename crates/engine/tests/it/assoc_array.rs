//! Associative arrays: ARRAYRECT and ARRAYPOLAR make one block reference whose block keeps the
//! array parameters (ASsociative, Levels, polar ROWs); EXPLODE gives plain objects; MOVE and COPY
//! keep the array.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{ArrayKind, ArrayParams, Drawing, Entity, EntityKind, Handle};
use cadcraft_engine::geom::{Mat3, Vec2};
use serde_json::{Value, json};

fn typed(s: &mut Session, inputs: &[&str]) {
    for t in inputs {
        s.cmdline(t).unwrap();
    }
}

fn circle(s: &mut Session, x: f64, y: f64) -> Value {
    s.execute("circle", &json!({"center": [x, y], "radius": 1})).unwrap()["handle"].clone()
}

fn model(s: &Session) -> Vec<Entity> {
    s.doc().unwrap().model.iter().map(|e| (**e).clone()).collect()
}

/// The only object in model space, which must be an associative array, and its parameters.
fn the_array(s: &Session) -> (Entity, ArrayParams) {
    let m = model(s);
    assert_eq!(m.len(), 1, "{m:?}");
    let e = m[0].clone();
    let p = s.doc().unwrap().array_params(&e).expect("an associative array");
    (e, p)
}

/// World positions of the circles drawn by `e` (through nested block references).
fn circles(d: &Drawing, e: &Entity) -> Vec<(Vec2, f64)> {
    fn walk(d: &Drawing, e: &Entity, m: Mat3, z: f64, depth: usize, out: &mut Vec<(Vec2, f64)>) {
        match &e.kind {
            EntityKind::Circle(c) => out.push((m.apply(c.center.xy()), z + c.center.z)),
            EntityKind::Insert(i) if depth < 8 => {
                let b = d.block(&i.block).unwrap();
                let im = m.then_before(i.transform(b.base.xy()));
                for be in b.entities.iter() {
                    walk(d, be, im, z + i.insert.z, depth + 1, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(d, e, Mat3::IDENTITY, 0.0, 0, &mut out);
    out
}

fn has(pts: &[(Vec2, f64)], x: f64, y: f64, z: f64) -> bool {
    pts.iter().any(|(p, pz)| p.near(Vec2::new(x, y), 1e-6) && (pz - z).abs() < 1e-6)
}

#[test]
fn json_arrays_are_associative_by_default() {
    let mut s = Session::new();
    let h = circle(&mut s, 0.0, 0.0);
    let r = s
        .execute(
            "arrayrect",
            &json!({"handles": [h], "rows": 2, "cols": 3, "rowSpacing": 4, "colSpacing": 2, "levels": 2, "levelSpacing": 10, "rowElevation": 1}),
        )
        .unwrap();
    assert_eq!(r["associative"], true);
    assert_eq!(r["created"], 11);
    let (e, p) = the_array(&s);
    assert_eq!(r["handle"], json!(e.handle.hex()));
    assert_eq!(p.levels, 2);
    assert!(matches!(&p.kind, ArrayKind::Rect(k) if k.cols == 3 && k.rows == 2 && k.row_elevation == 1.0));
    // The source circle moved into the array; the items are where the copies would be.
    let c = circles(s.doc().unwrap(), &e);
    assert_eq!(c.len(), 12);
    assert!(has(&c, 0.0, 0.0, 0.0) && has(&c, 4.0, 4.0, 1.0) && has(&c, 4.0, 4.0, 11.0), "{c:?}");

    // associative: false makes separate copies, as before.
    let mut s = Session::new();
    let h = circle(&mut s, 0.0, 0.0);
    let r = s.execute("arraypolar", &json!({"handles": [h], "center": [10, 0], "count": 4, "associative": false})).unwrap();
    assert_eq!(r["created"], 3);
    assert!(r.get("handle").is_none());
    assert_eq!(model(&s).len(), 4);
}

#[test]
fn typed_levels_and_polar_rows() {
    // ARRAYRECT Levels: count, then spacing.
    let mut s = Session::new();
    circle(&mut s, 0.0, 0.0);
    typed(&mut s, &["arrayrect", "l", "", "cou", "2", "1", "s", "5", "5", "l"]);
    assert!(s.prompt_text().contains("Enter the number of levels"), "{}", s.prompt_text());
    typed(&mut s, &["3", "7", "x"]);
    assert!(s.current_prompt().is_none(), "{}", s.prompt_text());
    let (e, p) = the_array(&s);
    assert_eq!((p.levels, p.level_spacing), (3, 7.0));
    let c = circles(s.doc().unwrap(), &e);
    assert_eq!(c.len(), 6);
    assert!(has(&c, 5.0, 0.0, 14.0), "{c:?}");

    // ARRAYPOLAR ROWs: rings further out from the centre, with an elevation per row.
    let mut s = Session::new();
    circle(&mut s, 10.0, 0.0);
    typed(&mut s, &["arraypolar", "l", "", "0,0", "i", "4", "row"]);
    assert!(s.prompt_text().contains("Enter the number of rows"), "{}", s.prompt_text());
    typed(&mut s, &["2", "3", "0.5", "x"]);
    assert!(s.current_prompt().is_none(), "{}", s.prompt_text());
    let (e, p) = the_array(&s);
    assert!(matches!(&p.kind, ArrayKind::Polar(k) if k.items == 4 && k.rows == 2 && k.row_spacing == 3.0));
    let c = circles(s.doc().unwrap(), &e);
    assert_eq!(c.len(), 8);
    assert!(has(&c, 0.0, 10.0, 0.0) && has(&c, 13.0, 0.0, 0.5) && has(&c, 0.0, -13.0, 0.5), "{c:?}");

    // ASsociative No (or ARRAYASSOCIATIVITY 0) makes copies; ROWs work there too.
    let mut s = Session::new();
    s.execute("setvar", &json!({"name": "ARRAYASSOCIATIVITY", "value": 0})).unwrap();
    circle(&mut s, 10.0, 0.0);
    typed(&mut s, &["arraypolar", "l", "", "0,0"]);
    assert!(s.log.iter().any(|l| l.contains("Associative = No")), "{:?}", s.log);
    typed(&mut s, &["i", "4", "row", "2", "3", "", "x"]);
    assert_eq!(model(&s).len(), 8);
    assert!(model(&s).iter().all(|e| matches!(e.kind, EntityKind::Circle(_))));
}

#[test]
fn explode_gives_plain_objects_and_edits_move_the_array() {
    let mut s = Session::new();
    let h = circle(&mut s, 10.0, 0.0);
    let r = s.execute("arraypolar", &json!({"handles": [h], "center": [0, 0], "count": 4})).unwrap();
    let arr = r["handle"].clone();

    // MOVE and COPY transform the block reference; the copy is an array too.
    s.execute("move", &json!({"handles": [arr], "delta": [100, 0]})).unwrap();
    let r = s.execute("copy", &json!({"handles": [arr], "delta": [0, 100]})).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.model.len(), 2, "{r}");
    for e in d.model.iter() {
        assert!(d.array_params(e).is_some());
    }
    let e = d.entity(Handle::parse_hex(arr.as_str().unwrap()).unwrap()).unwrap().as_ref().clone();
    let c = circles(d, &e);
    assert!(has(&c, 100.0, 10.0, 0.0) && has(&c, 90.0, 0.0, 0.0), "{c:?}");

    // EXPLODE: the items become circles, not block references.
    let r = s.execute("explode", &json!({"handles": [arr]})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 4);
    let d = s.doc().unwrap();
    let plain: Vec<Vec2> = d
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Circle(c) => Some(c.center.xy()),
            _ => None,
        })
        .collect();
    assert_eq!(plain.len(), 4);
    assert!(plain.iter().any(|p| p.near(Vec2::new(100.0, -10.0), 1e-6)), "{plain:?}");

    // UNDO brings the array back.
    s.execute("undo", &json!({})).unwrap();
    assert!(s.doc().unwrap().entity(Handle::parse_hex(arr.as_str().unwrap()).unwrap()).is_some());
}

#[test]
fn arrays_of_arrays_and_limits() {
    // An array of an array: exploding the outer one gives the inner arrays.
    let mut s = Session::new();
    let h = circle(&mut s, 0.0, 0.0);
    let inner = s.execute("arrayrect", &json!({"handles": [h], "rows": 1, "cols": 3, "colSpacing": 2})).unwrap()["handle"].clone();
    let outer = s.execute("arrayrect", &json!({"handles": [inner], "rows": 2, "cols": 1, "rowSpacing": 5})).unwrap()["handle"].clone();
    let (e, _) = the_array(&s);
    assert_eq!(circles(s.doc().unwrap(), &e).len(), 6);
    let r = s.execute("explode", &json!({"handles": [outer]})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 2);
    let d = s.doc().unwrap();
    assert!(d.model.iter().all(|e| d.array_params(e).is_some()));

    // Levels count toward MAXARRAY; nothing is created when refused.
    let mut s = Session::new();
    let h = circle(&mut s, 0.0, 0.0);
    let e = s.execute("arrayrect", &json!({"handles": [h], "rows": 1000, "cols": 1000, "levels": 1000})).unwrap_err();
    assert!(e.to_string().contains("MAXARRAY"), "{e}");
    let e = s.execute("arraypolar", &json!({"handles": [h], "center": [5, 5], "count": 1000, "rows": 1000})).unwrap_err();
    assert!(e.to_string().contains("MAXARRAY"), "{e}");
    assert_eq!(model(&s).len(), 1);
}

#[test]
fn pasted_arrays_get_their_own_blocks() {
    let mut s = Session::new();
    let h = circle(&mut s, 0.0, 0.0);
    let arr = s.execute("arrayrect", &json!({"handles": [h], "rows": 1, "cols": 3, "colSpacing": 2})).unwrap()["handle"].clone();
    s.execute("copyclip", &json!({"handles": [arr]})).unwrap();
    s.execute("pasteclip", &json!({"at": [0, 50]})).unwrap();
    let d = s.doc().unwrap();
    let params: Vec<ArrayParams> = d.model.iter().filter_map(|e| d.array_params(e)).collect();
    assert_eq!(params.len(), 2);
    // The pasted array's blocks were renamed (the names were taken); its parameters follow.
    assert_ne!(params[0].source, params[1].source);
    assert!(params.iter().all(|p| d.block(&p.source).is_some()));
}

/// Circle centres drawn by the array `h`, sorted.
fn centres(s: &Session, h: &Value) -> Vec<(i64, i64)> {
    let d = s.doc().unwrap();
    let e = d.entity(Handle::parse_hex(h.as_str().unwrap()).unwrap()).unwrap().as_ref().clone();
    let mut v: Vec<(i64, i64)> = circles(d, &e).iter().map(|(p, _)| ((p.x * 1000.0).round() as i64, (p.y * 1000.0).round() as i64)).collect();
    v.sort();
    v
}

fn lines_in(s: &Session, h: &Value) -> usize {
    fn walk(d: &Drawing, e: &Entity, depth: usize) -> usize {
        match &e.kind {
            EntityKind::Line(_) => 1,
            EntityKind::Insert(i) if depth < 8 => d.block(&i.block).unwrap().entities.iter().map(|b| walk(d, b, depth + 1)).sum(),
            _ => 0,
        }
    }
    let d = s.doc().unwrap();
    walk(d, d.entity(Handle::parse_hex(h.as_str().unwrap()).unwrap()).unwrap(), 0)
}

fn rect_array(s: &mut Session) -> Value {
    let h = circle(s, 0.0, 0.0);
    s.execute("arrayrect", &json!({"handles": [h], "rows": 1, "cols": 3, "colSpacing": 2})).unwrap()["handle"].clone()
}

#[test]
fn arrayedit_changes_parameters_of_one_array() {
    let mut s = Session::new();
    let a = rect_array(&mut s);
    let b = s.execute("copy", &json!({"handles": [a], "delta": [0, 100]})).unwrap();
    let b = s.doc().unwrap().model.iter().map(|e| json!(e.handle.hex())).find(|x| *x != a).unwrap_or(b);
    s.execute("arrayedit", &json!({"handle": a, "cols": 2, "rows": 2, "rowSpacing": 5, "colSpacing": 3})).unwrap();
    assert_eq!(centres(&s, &a), vec![(0, 0), (0, 5000), (3000, 0), (3000, 5000)]);
    // The copy that shared the array's block is unchanged.
    assert_eq!(centres(&s, &b), vec![(0, 100_000), (2000, 100_000), (4000, 100_000)]);
    // Blocks nothing uses are gone: two arrays, each with an array block and a shared source.
    let anon = s.doc().unwrap().blocks.keys().filter(|k| k.starts_with('*')).count();
    assert_eq!(anon, 3);

    // Polar: items, fill, rotation, centre.
    let mut s = Session::new();
    let h = circle(&mut s, 10.0, 0.0);
    let p = s.execute("arraypolar", &json!({"handles": [h], "center": [0, 0], "count": 4})).unwrap()["handle"].clone();
    s.execute("arrayedit", &json!({"handle": p, "items": 3, "angle": 180})).unwrap();
    assert_eq!(centres(&s, &p), vec![(-10000, 0), (0, 10000), (10000, 0)]);
    s.execute("arrayedit", &json!({"handle": p, "angleBetween": 45, "center": [0, 10]})).unwrap();
    assert_eq!(centres(&s, &p).len(), 3);
    assert!(centres(&s, &p).contains(&(10000, 0)));

    // Bad values are refused and change nothing.
    let before = centres(&s, &p);
    assert!(s.execute("arrayedit", &json!({"handle": p, "items": 0})).is_err());
    assert!(s.execute("arrayedit", &json!({"handle": p, "items": 10_000_000})).unwrap_err().to_string().contains("MAXARRAY"));
    assert!(s.execute("arrayedit", &json!({"handle": p, "angle": 400})).is_err());
    assert_eq!(centres(&s, &p), before);
    let c = circle(&mut s, 50.0, 50.0);
    assert!(s.execute("arrayedit", &json!({"handle": c, "items": 3})).is_err());
}

#[test]
fn arrayedit_base_replace_and_reset() {
    let mut s = Session::new();
    let a = rect_array(&mut s);
    // A new base point moves the reference, not the items.
    s.execute("arrayedit", &json!({"handle": a, "base": [7, 7]})).unwrap();
    assert_eq!(centres(&s, &a), vec![(0, 0), (2000, 0), (4000, 0)]);
    let d = s.doc().unwrap();
    let e = d.entity(Handle::parse_hex(a.as_str().unwrap()).unwrap()).unwrap();
    assert!(matches!(&e.kind, EntityKind::Insert(i) if i.insert.xy().near(Vec2::new(7.0, 7.0), 1e-9)));
    // Spacing still counts from the items.
    s.execute("arrayedit", &json!({"handle": a, "colSpacing": 4})).unwrap();
    assert_eq!(centres(&s, &a), vec![(0, 0), (4000, 0), (8000, 0)]);

    // REPlace item 1 with a line; the line stays in the drawing.
    let l = s.execute("line", &json!({"points": [[0, 20], [1, 20]]})).unwrap()["handles"][0].clone();
    s.execute("arrayedit", &json!({"handle": a, "replace": {"handles": [l], "base": [0, 20], "items": [[1, 0, 0]]}})).unwrap();
    assert_eq!(centres(&s, &a), vec![(0, 0), (8000, 0)]);
    assert_eq!(lines_in(&s, &a), 1);
    assert_eq!(s.doc().unwrap().model.len(), 2);
    // Replacing with the array itself is refused.
    assert!(s.execute("arrayedit", &json!({"handle": a, "replace": {"handles": [a], "items": [[0, 0, 0]]}})).is_err());
    // RESet restores the item.
    s.execute("arrayedit", &json!({"handle": a, "reset": true})).unwrap();
    assert_eq!(centres(&s, &a).len(), 3);
    assert_eq!(lines_in(&s, &a), 0);
    // REPlace Source objects: every item becomes the line.
    s.execute("arrayedit", &json!({"handle": a, "replace": {"handles": [l], "items": "source"}})).unwrap();
    assert_eq!(centres(&s, &a).len(), 0);
    assert_eq!(lines_in(&s, &a), 3);
    // EXPLODE gives three lines (plus the original one).
    s.execute("explode", &json!({"handles": [a]})).unwrap();
    assert_eq!(s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count(), 4);
}

#[test]
fn arrayedit_source_then_arrayclose() {
    let mut s = Session::new();
    let a = rect_array(&mut s);
    // Source places the circle of item 2 (at x = 4) in the drawing.
    let r = s.execute("arrayedit", &json!({"handle": a, "source": {"item": [2, 0, 0]}})).unwrap();
    let placed = r["source"].as_array().unwrap().clone();
    assert_eq!(placed.len(), 1);
    // Only one array at a time; other edits wait for ARRAYCLOSE.
    assert!(s.execute("arrayedit", &json!({"handle": a, "cols": 5})).is_err());
    // Move the circle up by 1 and draw a line next to it: both become the source.
    s.execute("move", &json!({"handles": placed, "delta": [0, 1]})).unwrap();
    s.execute("line", &json!({"points": [[4, 3], [5, 3]]})).unwrap();
    s.execute("arrayclose", &json!({})).unwrap();
    assert_eq!(centres(&s, &a), vec![(0, 1000), (2000, 1000), (4000, 1000)]);
    assert_eq!(lines_in(&s, &a), 3);
    assert_eq!(s.doc().unwrap().model.len(), 1);

    // Discard: the placed objects go, the array stays as it was.
    s.execute("arrayedit", &json!({"handle": a, "source": {}})).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 3);
    s.execute("arrayclose", &json!({"save": false})).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 1);
    assert_eq!(centres(&s, &a).len(), 3);
    assert!(s.execute("arrayclose", &json!({})).is_err());

    // Saving an empty source is refused; the edit stays open.
    let r = s.execute("arrayedit", &json!({"handle": a, "source": {}})).unwrap();
    s.execute("erase", &json!({"handles": r["source"]})).unwrap();
    assert!(s.execute("arrayclose", &json!({})).is_err());
    s.execute("arrayclose", &json!({"save": false})).unwrap();
    assert_eq!(centres(&s, &a).len(), 3);
}

#[test]
fn typed_arrayedit() {
    let mut s = Session::new();
    let a = rect_array(&mut s);
    typed(&mut s, &["arrayedit", "l"]);
    let p = s.prompt_text();
    assert!(p.contains("Enter an option or [Source/REPlace/Base point/Rows/Columns/Levels/RESet/eXit] <eXit>"), "{p}");
    typed(&mut s, &["c", "4", "t", "9", "r", "2", "5", ""]);
    assert!(s.prompt_text().contains("Enter an option"), "{}", s.prompt_text());
    assert_eq!(centres(&s, &a).len(), 8);
    assert!(centres(&s, &a).contains(&(9000, 5000)), "{:?}", centres(&s, &a));
    typed(&mut s, &["x"]);
    assert!(s.current_prompt().is_none(), "{}", s.prompt_text());
    // One undo step takes the whole edit back.
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(centres(&s, &a).len(), 3);

    // REPlace with a typed pick of item 2, then Source on item 0 and ARRAYCLOSE.
    s.execute("line", &json!({"points": [[0, 20], [1, 20]]})).unwrap();
    s.set_selection(Vec::new());
    typed(&mut s, &["arrayedit", "1,0"]);
    typed(&mut s, &["rep", "l", "", ""]);
    assert!(s.prompt_text().contains("Select an item in array to replace or [Source objects]"), "{}", s.prompt_text());
    typed(&mut s, &["4,0", "", "x"]);
    assert_eq!(centres(&s, &a).len(), 2);
    assert_eq!(lines_in(&s, &a), 1);
    typed(&mut s, &["arrayedit", "1,0", "s", "0,0"]);
    assert!(s.current_prompt().is_none(), "{}", s.prompt_text());
    typed(&mut s, &["arrayclose"]);
    assert!(s.prompt_text().contains("Save changes to array? [Yes/No] <Yes>"), "{}", s.prompt_text());
    typed(&mut s, &[""]);
    assert!(s.current_prompt().is_none(), "{}", s.prompt_text());
    assert_eq!(centres(&s, &a).len(), 2);
}

#[test]
fn json_arrays_follow_arrayassociativity() {
    // With ARRAYASSOCIATIVITY 0 the JSON form makes copies unless `associative` asks otherwise.
    let mut s = Session::new();
    s.execute("setvar", &json!({"name": "ARRAYASSOCIATIVITY", "value": 0})).unwrap();
    let h = circle(&mut s, 0.0, 0.0);
    let r = s.execute("arrayrect", &json!({"handles": [h], "rows": 1, "cols": 3, "rowSpacing": 4, "colSpacing": 2})).unwrap();
    assert_ne!(r["associative"], true);
    assert_eq!(model(&s).iter().filter(|e| matches!(e.kind, EntityKind::Circle(_))).count(), 3);
    let r = s.execute("arraypolar", &json!({"handles": [h], "center": [10, 0], "count": 4, "associative": true})).unwrap();
    assert_eq!(r["associative"], true);
}
