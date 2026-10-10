use cadcraft_doc::{AssocSnap, DimKind, Dimension, EntityKind, Handle};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use crate::Session;

fn h(v: &Value) -> Handle {
    Handle::parse_hex(v["handle"].as_str().unwrap()).unwrap()
}

fn dim(s: &Session, hd: Handle) -> Dimension {
    match &s.doc().unwrap().entity(hd).unwrap().kind {
        EntityKind::Dimension(d) => d.clone(),
        other => panic!("not a dimension: {other:?}"),
    }
}

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Handle {
    s.execute("line", &json!({ "points": [a, b] })).unwrap();
    s.doc().unwrap().model.last().unwrap().handle
}

fn near(a: Vec2, b: Vec2) -> bool {
    a.dist(b) < 1e-9
}

#[test]
fn linear_dimension_follows_line_endpoint() {
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let d = h(&s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, -2] })).unwrap());
    let dm = dim(&s, d);
    assert_eq!(dm.assoc.len(), 2, "both origins attach to the line: {:?}", dm.assoc);
    assert!(dm.assoc.iter().all(|a| a.handle == l));
    // Move the line's end point: the dimension's second origin follows.
    s.execute("properties.set", &json!({ "handles": [l.hex()], "end": [12, 0] })).unwrap();
    let dm = dim(&s, d);
    assert!(near(dm.p14.xy(), Vec2::new(12.0, 0.0)), "{:?}", dm.p14);
    assert!(near(dm.p13.xy(), Vec2::ZERO));
    // The dimension line stays where it was.
    assert!(near(dm.defpt.xy(), Vec2::new(5.0, -2.0)));
    let g = cadcraft_render::dimension_in(s.doc().unwrap(), &dm);
    assert_eq!(g.value, "12.0000");
    // Undo restores both in one step.
    s.undo().unwrap();
    assert!(near(dim(&s, d).p14.xy(), Vec2::new(10.0, 0.0)));
}

#[test]
fn moving_the_object_carries_the_dimension() {
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let d = h(&s.execute("dimaligned", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, -2] })).unwrap());
    s.execute("move", &json!({ "handles": [l.hex()], "from": [0, 0], "to": [3, 4] })).unwrap();
    let dm = dim(&s, d);
    assert!(near(dm.p13.xy(), Vec2::new(3.0, 4.0)) && near(dm.p14.xy(), Vec2::new(13.0, 4.0)), "{dm:?}");
    assert!(near(dm.defpt.xy(), Vec2::new(8.0, 2.0)), "rigid move carries the dimension line: {:?}", dm.defpt);
    // Moving both together keeps them attached and unchanged relative to each other.
    s.execute("move", &json!({ "handles": [l.hex(), d.hex()], "from": [0, 0], "to": [1, 0] })).unwrap();
    let dm = dim(&s, d);
    assert_eq!(dm.assoc.len(), 2);
    assert!(near(dm.p13.xy(), Vec2::new(4.0, 4.0)) && near(dm.defpt.xy(), Vec2::new(9.0, 2.0)), "{dm:?}");
}

#[test]
fn moving_the_object_keeps_the_ordinate_datum() {
    let mut s = Session::new();
    let l = line(&mut s, [5.0, 0.0], [5.0, 5.0]);
    let d = h(&s.execute("dimordinate", &json!({ "feature": [5, 5], "leader": [5, 8] })).unwrap());
    s.execute("move", &json!({ "handles": [l.hex()], "from": [5, 5], "to": [8, 5] })).unwrap();
    let dm = dim(&s, d);
    assert!(near(dm.p13.xy(), Vec2::new(8.0, 5.0)) && near(dm.p14.xy(), Vec2::new(8.0, 8.0)), "{dm:?}");
    assert!(near(dm.defpt.xy(), Vec2::ZERO), "datum must stay at the origin: {:?}", dm.defpt);
    let g = cadcraft_render::dimension_in(s.doc().unwrap(), &dm);
    assert_eq!(g.value, "8.0000");
}

#[test]
fn moving_the_line_keeps_a_free_continued_origin() {
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, -2] })).unwrap();
    s.execute("dimcontinue", &json!({ "points": [[25, 0]] })).unwrap();
    let d = s.doc().unwrap().model.last().unwrap().handle;
    s.execute("move", &json!({ "handles": [l.hex()], "delta": [3, 0] })).unwrap();
    let dm = dim(&s, d);
    assert!(near(dm.p13.xy(), Vec2::new(13.0, 0.0)), "attached origin follows: {dm:?}");
    assert!(near(dm.p14.xy(), Vec2::new(25.0, 0.0)), "free origin stays: {dm:?}");
    assert_eq!(cadcraft_render::dimension_in(s.doc().unwrap(), &dm).value, "12.0000");
    s.execute("move", &json!({ "handles": [l.hex()], "delta": [3, 0] })).unwrap();
    let dm = dim(&s, d);
    assert!(near(dm.p13.xy(), Vec2::new(16.0, 0.0)) && near(dm.p14.xy(), Vec2::new(25.0, 0.0)), "{dm:?}");
    assert_eq!(cadcraft_render::dimension_in(s.doc().unwrap(), &dm).value, "9.0000");
}

#[test]
fn interactive_dimlinear_select_object_is_associative() {
    let mut s = Session::new();
    s.cmdline("line 0,0 6,0").unwrap();
    s.cmdline("").unwrap();
    let l = s.doc().unwrap().model.last().unwrap().handle;
    s.cmdline("dimlinear").unwrap();
    s.cmdline("").unwrap(); // <select object>
    s.cmdline("3,0").unwrap();
    s.cmdline("3,-1").unwrap();
    assert!(s.running.is_none());
    let d = s.last_dim.unwrap();
    assert_eq!(dim(&s, d).assoc.len(), 2);
    // A grip-style edit through properties.
    s.execute("properties.set", &json!({ "handles": [l.hex()], "start": [-1, 0] })).unwrap();
    assert!(near(dim(&s, d).p13.xy(), Vec2::new(-1.0, 0.0)));
}

#[test]
fn radius_and_diameter_follow_circle() {
    let mut s = Session::new();
    s.execute("circle", &json!({ "center": [0, 0], "radius": 2 })).unwrap();
    let c = s.doc().unwrap().model.last().unwrap().handle;
    let r = h(&s.execute("dimradius", &json!({ "handle": c.hex(), "at": [5, 0] })).unwrap());
    let dd = h(&s.execute("dimdiameter", &json!({ "handle": c.hex(), "at": [0, 5] })).unwrap());
    assert_eq!(dim(&s, r).assoc.len(), 2);
    assert_eq!(dim(&s, dd).assoc.len(), 2);
    s.execute("properties.set", &json!({ "handles": [c.hex()], "radius": 3 })).unwrap();
    let g = cadcraft_render::dimension_in(s.doc().unwrap(), &dim(&s, r));
    assert_eq!(g.value, "R3.0000");
    let g = cadcraft_render::dimension_in(s.doc().unwrap(), &dim(&s, dd));
    assert_eq!(g.value, "⌀6.0000");
    s.execute("move", &json!({ "handles": [c.hex()], "from": [0, 0], "to": [10, 0] })).unwrap();
    assert!(near(dim(&s, r).defpt.xy(), Vec2::new(10.0, 0.0)));
}

#[test]
fn angular_between_lines_follows_rotation() {
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [0.0, 0.0], [0.0, 10.0]);
    let d = h(&s.execute("dimangular", &json!({ "lines": [a.hex(), b.hex()], "at": [3, 3] })).unwrap());
    let dm = dim(&s, d);
    assert!(dm.assoc.iter().any(|x| matches!(x.snap, AssocSnap::Intersection { .. })));
    assert_eq!(cadcraft_render::dimension_in(s.doc().unwrap(), &dm).value, "90°");
    // Swing the second line to 45°.
    s.execute("properties.set", &json!({ "handles": [b.hex()], "end": [10, 10] })).unwrap();
    assert_eq!(cadcraft_render::dimension_in(s.doc().unwrap(), &dim(&s, d)).value, "45°");
}

#[test]
fn interactive_angular_picks_polyline_segments() {
    // Rectangle sides are polyline segments, not lines (issue #10).
    let mut s = Session::new();
    s.cmdline("rectang 0,0 10,5").unwrap();
    s.cmdline("dimangular").unwrap();
    s.cmdline("5,0").unwrap();
    s.cmdline("0,2").unwrap();
    s.cmdline("2,2").unwrap();
    assert!(s.running.is_none());
    let d = s.last_dim.unwrap();
    assert_eq!(cadcraft_render::dimension_in(s.doc().unwrap(), &dim(&s, d)).value, "90°");
    // A polyline arc segment works like an arc.
    s.cmdline("pline 20,0 30,0 a 30,10").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("dimangular").unwrap();
    let (cx, cy) = (30.0 + 5.0 * std::f64::consts::FRAC_1_SQRT_2, 5.0 - 5.0 * std::f64::consts::FRAC_1_SQRT_2);
    s.cmdline(&format!("{cx},{cy}")).unwrap();
    s.cmdline("40,5").unwrap();
    assert!(s.running.is_none());
    let d2 = s.last_dim.unwrap();
    assert_ne!(d, d2);
    assert_eq!(cadcraft_render::dimension_in(s.doc().unwrap(), &dim(&s, d2)).value, "180°");
}

#[test]
fn erase_disassociate_and_reassociate() {
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [4.0, 0.0]);
    let d = h(&s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [4, 0], "at": [2, 1] })).unwrap());
    s.execute("dimdisassociate", &json!({ "handles": [d.hex()] })).unwrap();
    assert!(dim(&s, d).assoc.is_empty());
    s.execute("properties.set", &json!({ "handles": [l.hex()], "end": [5, 0] })).unwrap();
    assert!(near(dim(&s, d).p14.xy(), Vec2::new(4.0, 0.0)), "not associative any more");
    // Put the line back and reassociate.
    s.execute("properties.set", &json!({ "handles": [l.hex()], "end": [4, 0] })).unwrap();
    let r = s.execute("dimreassociate", &json!({ "handles": [d.hex()] })).unwrap();
    assert_eq!(r["associated"], 1);
    assert_eq!(dim(&s, d).assoc.len(), 2);
    // Erasing the object leaves the dimension where it is (and no longer attached).
    s.execute("erase", &json!({ "handles": [l.hex()] })).unwrap();
    let dm = dim(&s, d);
    assert!(dm.assoc.is_empty());
    assert!(near(dm.p14.xy(), Vec2::new(4.0, 0.0)));
}

#[test]
fn dimassoc_zero_makes_plain_dimensions() {
    let mut s = Session::new();
    line(&mut s, [0.0, 0.0], [4.0, 0.0]);
    s.doc_mut().unwrap().header.set_i64("DIMASSOC", 0);
    let d = h(&s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [4, 0], "at": [2, 1] })).unwrap());
    assert!(dim(&s, d).assoc.is_empty());
}

#[test]
fn dimstyle_commands() {
    let mut s = Session::new();
    let r = s.execute("dimstyle", &json!({ "name": "Arch", "DIMTSZ": 0.1, "DIMBLK": "_ARCHTICK", "DIMLUNIT": 4, "DIMTAD": 1, "bogus": 3 })).unwrap();
    assert_eq!(r["ignored"], json!(["bogus"]));
    let st = s.doc().unwrap().dim_style("Arch").unwrap().clone();
    assert_eq!(st.tick_size, 0.1);
    assert_eq!(st.arrow_block, "_ARCHTICK");
    assert_eq!(st.linear_unit, 4);
    assert_eq!(st.text_above, 1);
    let l = s.execute("dimstyle.list", &json!({ "name": "Arch" })).unwrap();
    assert_eq!(l["variables"]["DIMTSZ"], json!(0.1));
    assert_eq!(l["current"], "Arch");
    let d = h(&s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [18.5, 0], "at": [5, 2] })).unwrap());
    assert_eq!(cadcraft_render::dimension_in(s.doc().unwrap(), &dim(&s, d)).value, "1'-6 1/2\"");
    // In use: cannot delete; rename follows into dimensions.
    s.execute("dimstyle.current", &json!({ "name": "Standard" })).unwrap();
    assert!(s.execute("dimstyle.delete", &json!({ "name": "Arch" })).is_err());
    s.execute("dimstyle.rename", &json!({ "from": "Arch", "to": "Architectural" })).unwrap();
    assert_eq!(dim(&s, d).style, "Architectural");
    s.execute("erase", &json!({ "handles": [d.hex()] })).unwrap();
    s.execute("dimstyle.delete", &json!({ "name": "Architectural" })).unwrap();
    assert!(s.doc().unwrap().dim_style("Architectural").is_none());
    assert!(s.execute("dimstyle.delete", &json!({ "name": "Standard" })).is_err());
}

#[test]
fn dimension_overrides() {
    let mut s = Session::new();
    let d = h(&s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 2] })).unwrap());
    s.execute("dimoverride", &json!({ "handles": [d.hex()], "DIMDEC": 1, "DIMPOST": "<> mm" })).unwrap();
    let dm = dim(&s, d);
    assert_eq!(dm.overrides.get("decimals"), Some(&json!(1)));
    assert_eq!(cadcraft_render::dimension_in(s.doc().unwrap(), &dm).value, "10.0 mm");
    // The interactive form.
    s.cmdline("dimoverride").unwrap();
    s.cmdline("DIMDEC").unwrap();
    s.cmdline("2").unwrap();
    s.cmdline("").unwrap();
    s.input(crate::Input::Pick(vec![d])).unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(cadcraft_render::dimension_in(s.doc().unwrap(), &dim(&s, d)).value, "10.00 mm");
    s.execute("dimstyle.override", &json!({ "handles": [d.hex()], "clear": true })).unwrap();
    assert!(dim(&s, d).overrides.is_empty());
    assert!(s.execute("dimoverride", &json!({ "handles": [d.hex()], "nope": 1 })).is_err());
    // Plain text override keeps working.
    s.execute("dimoverride", &json!({ "handles": [d.hex()], "text": "<> TYP" })).unwrap();
    assert_eq!(dim(&s, d).text, "<> TYP");
}

#[test]
fn dimension_override_text_with_variables() {
    let mut s = Session::new();
    let d = h(&s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 2] })).unwrap());
    s.execute("dimoverride", &json!({ "handles": [d.hex()], "text": "<> OLD" })).unwrap();
    s.execute("dimoverride", &json!({ "handles": [d.hex()], "DIMTXT": 0.4 })).unwrap();
    let r = s.execute("dimoverride", &json!({ "handles": [d.hex()], "text": "<> TYP", "DIMDEC": 1 })).unwrap();
    assert_eq!(r["changed"], json!(1));
    let dm = dim(&s, d);
    assert_eq!(dm.text, "<> TYP");
    assert_eq!(dm.overrides.get("decimals"), Some(&json!(1)));
    assert!(dm.overrides.contains_key("textHeight"));
    // An unknown variable next to valid text is still an error and changes nothing.
    assert!(s.execute("dimoverride", &json!({ "handles": [d.hex()], "text": "X", "nope": 1 })).is_err());
    assert_eq!(dim(&s, d).text, "<> TYP");
}

#[test]
fn dimtedit_and_dimspace() {
    let mut s = Session::new();
    let a = h(&s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 1] })).unwrap());
    let b = h(&s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [6, 0], "at": [3, 1.3] })).unwrap());
    s.execute("dimtedit.left", &json!({ "handles": [a.hex()] })).unwrap();
    let g = cadcraft_render::dimension_in(s.doc().unwrap(), &dim(&s, a));
    assert!(g.text_pos.x < 5.0);
    s.execute("dimtedit.angle", &json!({ "handles": [a.hex()], "angle": 30 })).unwrap();
    let g = cadcraft_render::dimension_in(s.doc().unwrap(), &dim(&s, a));
    assert!((g.text_angle - 30f64.to_radians()).abs() < 1e-9);
    s.execute("dimtedit.home", &json!({ "handles": [a.hex()] })).unwrap();
    assert!(dim(&s, a).overrides.is_empty() && dim(&s, a).text_rotation == 0.0);
    s.execute("dimspace", &json!({ "base": a.hex(), "handles": [b.hex()], "spacing": 0.5 })).unwrap();
    assert!((dim(&s, b).defpt.y - 1.5).abs() < 1e-9, "{:?}", dim(&s, b).defpt);
}

#[test]
fn text_style_commands() {
    let mut s = Session::new();
    let r = s
        .execute(
            "style",
            &json!({ "name": "Notes", "font": "Arial", "height": 0.25, "widthFactor": 0.8, "oblique": 15, "backwards": true, "upsideDown": true }),
        )
        .unwrap();
    assert_eq!(r["current"], "Notes");
    let st = s.doc().unwrap().text_style("Notes").unwrap().clone();
    assert!(st.backwards && st.upside_down);
    assert!((st.oblique.to_degrees() - 15.0).abs() < 1e-9);
    s.execute("text", &json!({ "at": [0, 0], "height": 0.25, "text": "X" })).unwrap();
    let l = s.execute("style.list", &json!({})).unwrap();
    assert_eq!(l["styles"].as_array().unwrap().len(), 2);
    s.execute("style.rename", &json!({ "from": "Notes", "to": "Labels" })).unwrap();
    assert_eq!(s.doc().unwrap().header.str("TEXTSTYLE", ""), "Labels");
    assert!(s.execute("style.delete", &json!({ "name": "Labels" })).is_err(), "current style");
    s.execute("style.current", &json!({ "name": "Standard" })).unwrap();
    let used = s.doc().unwrap().model.iter().any(|e| matches!(&e.kind, EntityKind::Text(t) if t.style == "Labels"));
    if used {
        assert!(s.execute("style.delete", &json!({ "name": "Labels" })).is_err(), "in use");
    } else {
        s.execute("style.delete", &json!({ "name": "Labels" })).unwrap();
    }
    assert!(s.execute("style.delete", &json!({ "name": "Standard" })).is_err());
}

#[test]
fn table_create_edit_merge() {
    let mut s = Session::new();
    let t = h(&s
        .execute(
            "table",
            &json!({ "at": [0, 10], "rows": 2, "cols": 3, "title": "Parts", "header": ["No", "Name", "Qty"], "cells": [["1", "Bolt", "4"]] }),
        )
        .unwrap());
    let get = |s: &Session| match &s.doc().unwrap().entity(t).unwrap().kind {
        EntityKind::Table(x) => x.clone(),
        _ => panic!(),
    };
    let tb = get(&s);
    assert_eq!(tb.row_heights.len(), 4);
    assert_eq!(tb.col_widths.len(), 3);
    assert_eq!(tb.cells[0][0].merged, Some((1, 3)));
    assert_eq!(tb.cells[2][1].text, "Bolt");
    s.execute("table.set", &json!({ "handle": t.hex(), "row": 3, "col": 1, "text": "Nut" })).unwrap();
    assert_eq!(get(&s).cells[3][1].text, "Nut");
    // Writing into a covered title cell writes the title.
    s.execute("table.set", &json!({ "handle": t.hex(), "row": 0, "col": 2, "text": "Bill of materials" })).unwrap();
    assert_eq!(get(&s).cells[0][0].text, "Bill of materials");
    s.execute("table.insertcol", &json!({ "handle": t.hex(), "col": 1 })).unwrap();
    let tb = get(&s);
    assert_eq!(tb.col_widths.len(), 4);
    assert_eq!(tb.cells[0][0].merged, Some((1, 4)), "title merge grows");
    assert_eq!(tb.cells[2][2].text, "Bolt");
    s.execute("table.deletecol", &json!({ "handle": t.hex(), "col": 1 })).unwrap();
    s.execute("table.insertrow", &json!({ "handle": t.hex() })).unwrap();
    assert_eq!(get(&s).row_heights.len(), 5);
    s.execute("table.merge", &json!({ "handle": t.hex(), "row": 2, "col": 0, "rows": 2, "cols": 1 })).unwrap();
    let tb = get(&s);
    assert_eq!(tb.cells[2][0].merged, Some((2, 1)));
    assert!(s.execute("table.deleterow", &json!({ "handle": t.hex(), "row": 2 })).is_ok());
    assert_eq!(get(&s).cells[2][0].merged, None, "a one-row merge is no merge");
    s.execute("table.unmerge", &json!({ "handle": t.hex(), "row": 0, "col": 1 })).unwrap();
    assert_eq!(get(&s).cells[0][0].merged, None);
    assert!(s.execute("table.set", &json!({ "handle": t.hex(), "row": 99, "col": 0, "text": "x" })).is_err());
    assert!(s.execute("table", &json!({ "at": [0, 0], "rows": 1_000_000, "cols": 3 })).is_err());
    // It renders: grid lines plus text.
    let dl = cadcraft_render::build(s.doc().unwrap(), &cadcraft_doc::Space::Model, &cadcraft_render::Options::default());
    assert!(dl.prims.len() > 10);
}

#[test]
fn table_interactive() {
    let mut s = Session::new();
    s.cmdline("table").unwrap();
    s.cmdline("3").unwrap();
    s.cmdline("2").unwrap();
    s.cmdline("0,0").unwrap();
    assert!(s.running.is_none());
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::Table(t) => {
            assert_eq!(t.col_widths.len(), 3);
            assert_eq!(t.row_heights.len(), 4, "title + header + 2 data rows");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn hostile_annotation_params_never_panic() {
    let mut s = Session::new();
    for (id, p) in [
        ("dimstyle", json!({ "name": "X", "DIMDEC": -5, "DIMTAD": 1e300, "DIMCLRD": "nonsense", "DIMBLK": 7 })),
        ("dimoverride", json!({ "handles": ["zz"], "DIMDEC": 1 })),
        ("dimreassociate", json!({ "handles": [1, 2, 3] })),
        ("dimangular", json!({ "lines": ["1"], "at": [0, 0] })),
        ("dimlinear", json!({ "object": "ffff", "at": [0, 0] })),
        ("table.merge", json!({ "handle": "1", "row": 0, "col": 0, "rows": 1e9, "cols": -1 })),
        ("table", json!({ "at": [0, 0], "cols": 0 })),
        ("style", json!({ "name": "" })),
        ("style.rename", json!({ "from": "Standard", "to": "Y" })),
        ("dimspace", json!({ "base": "1", "handles": [] })),
        ("dimtedit", json!({ "handles": [], "mode": "angle" })),
    ] {
        let _ = s.execute(id, &p);
    }
    let d = h(&s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [1, 0], "at": [0, 1] })).unwrap());
    let _ = s.execute("dimoverride", &json!({ "handles": [d.hex()], "DIMTXT": -1, "DIMSCALE": 0, "DIMLUNIT": 99 }));
    let _ = cadcraft_render::build(s.doc().unwrap(), &cadcraft_doc::Space::Model, &cadcraft_render::Options::default());
    assert!(matches!(dim(&s, d).kind, DimKind::Linear { .. }));
}

#[test]
fn mtext_rejects_nonpositive_height() {
    let mut s = Session::new();
    for height in [0.0, -2.0] {
        assert!(s.execute("mtext", &json!({ "at": [0, 0], "text": "hi", "height": height })).is_err(), "height {height}");
    }
    assert!(s.doc().unwrap().model.is_empty());
    s.execute("mtext", &json!({ "at": [0, 0], "text": "hi", "height": 2 })).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 1);
}

#[test]
fn setvar_dim_variables_apply_to_new_dimensions() {
    // Issue #66: a DIM* variable set with SETVAR is a style override that new dimensions take on.
    let mut s = Session::new();
    let linear = |s: &mut Session, y: f64| h(&s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [100, 0], "at": [50, y] })).unwrap());
    let text_height = |s: &Session, hd: Handle| {
        let d = s.doc().unwrap();
        let dm = dim(s, hd);
        let st = d.dim_style(&dm.style).cloned().unwrap();
        cadcraft_render::dimension_geometry(&dm, &st, d.header.f64("DIMSCALE", 1.0)).text_height
    };
    let first = linear(&mut s, -20.0);
    let base = text_height(&s, first);
    s.execute("setvar", &json!({ "name": "DIMSCALE", "value": 24 })).unwrap();
    let scaled = linear(&mut s, -40.0);
    assert!((text_height(&s, scaled) / base - 24.0).abs() < 1e-9);
    assert_eq!(dim(&s, scaled).overrides.get("scale"), Some(&json!(24.0)));
    assert!(dim(&s, first).overrides.is_empty(), "existing dimensions keep their size");
    assert!((text_height(&s, first) - base).abs() < 1e-12);
    // Extents use the same scale as the geometry: room for 2.5 text heights beyond the points.
    let d = s.doc().unwrap();
    let margin = |hd: Handle, span: f64| (cadcraft_doc::entity_bounds(d, d.entity(hd).unwrap(), 0).height() - span) / 2.0;
    assert!((margin(first, 20.0) - 2.5 * base).abs() < 1e-9);
    assert!((margin(scaled, 40.0) - 2.5 * text_height(&s, scaled)).abs() < 1e-9);

    // Making a style current clears the overrides.
    s.execute("dimstyle.current", &json!({ "name": "Standard" })).unwrap();
    assert_eq!(crate::sysvars::get(&s, "DIMSCALE"), Some(json!(1.0)));
    let plain = linear(&mut s, -60.0);
    assert!(dim(&s, plain).overrides.is_empty());

    // Editing the current style updates the variables, so the issue's workaround keeps working.
    s.execute("dimstyle", &json!({ "name": "Standard", "DIMSCALE": 24 })).unwrap();
    assert_eq!(crate::sysvars::get(&s, "DIMSCALE"), Some(json!(24.0)));
    let restyled = linear(&mut s, -80.0);
    assert!(dim(&s, restyled).overrides.is_empty());
    assert!((text_height(&s, restyled) / base - 24.0).abs() < 1e-9);

    // A DIM* variable the header doesn't carry reads from the style and can be overridden too.
    let dec = crate::sysvars::get(&s, "DIMDEC").unwrap();
    assert_eq!(dec, json!(s.doc().unwrap().dim_style("Standard").unwrap().decimals));
    s.execute("setvar", &json!({ "name": "DIMDEC", "value": 1 })).unwrap();
    let one = linear(&mut s, -100.0);
    let one = dim(&s, one);
    assert_eq!(one.overrides.get("decimals"), Some(&json!(1)));
    assert_eq!(cadcraft_render::dimension_in(s.doc().unwrap(), &one).value, "100.0");

    // DIMSCALE 0 (AutoCAD: scale to the layout viewport) never shrinks a dimension to nothing.
    s.execute("setvar", &json!({ "name": "DIMSCALE", "value": 0 })).unwrap();
    let zero = linear(&mut s, -120.0);
    assert!((text_height(&s, zero) - base).abs() < 1e-12);
}
