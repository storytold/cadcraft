//! Dimension style variables beyond the basics: fit (DIMATFIT, DIMTIX, DIMSOXD, DIMTOFL),
//! suppressed dimension line halves (DIMSD1/2), fixed-length extension lines (DIMFXL/DIMFXLON),
//! DIMTVP, DIMTMOVE, DIMTOLJ, DIMAZIN, DIMARCSYM, DIMLDRBLK and the dimension and extension
//! line linetypes and lineweights. They are SETVAR overrides, DIMOVERRIDE values and style fields.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{DimStyle, Dimension, EntityKind, Linetype};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::render::{DimGeometry, LineRole};
use serde_json::{Value, json};

fn set(s: &mut Session, name: &str, value: Value) {
    s.execute("setvar", &json!({ "name": name, "value": value })).unwrap();
}

fn last_dim(s: &Session) -> (Dimension, DimGeometry) {
    let d = s.doc().unwrap();
    let dm = d
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Dimension(dm) => Some(dm.clone()),
            _ => None,
        })
        .last()
        .unwrap();
    let g = cadcraft_engine::render::dimension_in(d, &dm);
    (dm, g)
}

fn dim_lines(g: &DimGeometry) -> Vec<Vec<Vec2>> {
    g.lines_of(LineRole::Dim).cloned().collect()
}

/// Whether a dimension line on y = `y` covers `x`.
fn covers(g: &DimGeometry, y: f64, x: f64) -> bool {
    dim_lines(g)
        .iter()
        .any(|l| l.windows(2).any(|w| (w[0].y - y).abs() < 1e-9 && (w[1].y - y).abs() < 1e-9 && w[0].x.min(w[1].x) < x && x < w[0].x.max(w[1].x)))
}

fn linear(s: &mut Session, x2: f64) {
    s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [x2, 0], "at": [x2 / 2.0, 3] })).unwrap();
}

#[test]
fn new_variables_are_style_overrides_and_read_back() {
    let mut s = Session::new();
    for (name, v) in [
        ("DIMTOFL", json!(1)),
        ("DIMATFIT", json!(1)),
        ("DIMTIX", json!(1)),
        ("DIMSOXD", json!(1)),
        ("DIMTMOVE", json!(2)),
        ("DIMTVP", json!(1.5)),
        ("DIMTOLJ", json!(0)),
        ("DIMAZIN", json!(2)),
        ("DIMARCSYM", json!(1)),
        ("DIMLWD", json!(50)),
        ("DIMLWE", json!("ByLayer")),
        ("DIMFXLON", json!(1)),
        ("DIMFXL", json!(0.5)),
        ("DIMJOGANG", json!(60)),
        ("DIMSD1", json!(1)),
    ] {
        set(&mut s, name, v);
    }
    let o = s.doc().unwrap().dim_overrides();
    assert_eq!(o.get("forceLineInside"), Some(&json!(true)));
    assert_eq!(o.get("fit"), Some(&json!(1)));
    assert_eq!(o.get("textMove"), Some(&json!(2)));
    assert_eq!(o.get("tolJust"), Some(&json!(0)));
    assert_eq!(o.get("dimLineweight"), Some(&json!(50)));
    assert_eq!(o.get("extLineweight"), Some(&json!(-1)));
    assert_eq!(o.get("jogAngle"), Some(&json!(60.0)));
    assert_eq!(o.get("suppressDim1"), Some(&json!(true)));
    // A new dimension takes the overrides.
    linear(&mut s, 10.0);
    let (dm, _) = last_dim(&s);
    let st = DimStyle::default().with_overrides(&dm.overrides);
    assert!(st.force_line_inside && st.text_inside && st.suppress_outside_arrows && st.fixed_ext && st.suppress_dim1);
    assert_eq!((st.fit, st.text_move, st.arc_symbol, st.angular_zero_suppression), (1, 2, 1, 2));
    // Out-of-range values are refused and leave the style alone.
    let mut st = DimStyle::default();
    let bad = st.apply_fields(&serde_json::Map::from_iter([
        ("DIMATFIT".to_string(), json!(7)),
        ("DIMTMOVE".to_string(), json!(3)),
        ("DIMLWD".to_string(), json!(500)),
        ("DIMJOGANG".to_string(), json!(120)),
        ("DIMFXL".to_string(), json!(-1)),
    ]));
    assert_eq!(bad.len(), 5, "{bad:?}");
    assert_eq!(st, DimStyle::default());
    // DIMOVERRIDE takes them per dimension.
    let h = s.doc().unwrap().model.last().unwrap().handle.hex();
    s.execute("dimoverride", &json!({ "handles": [h], "DIMTOFL": 0, "DIMLTYPE": "DASHED" })).unwrap();
    let (dm, _) = last_dim(&s);
    let st = DimStyle::default().with_overrides(&dm.overrides);
    assert!(!st.force_line_inside && st.text_inside);
    assert_eq!(st.dim_linetype, "DASHED");
}

#[test]
fn text_fill_variables_are_style_overrides() {
    let mut s = Session::new();
    set(&mut s, "DIMTFILL", json!(2));
    set(&mut s, "DIMTFILLCLR", json!(3));
    let o = s.doc().unwrap().dim_overrides();
    assert_eq!(o.get("textFill"), Some(&json!(2)));
    linear(&mut s, 10.0);
    let (dm, g) = last_dim(&s);
    let st = DimStyle::default().with_overrides(&dm.overrides);
    assert_eq!((st.text_fill, st.text_fill_color), (2, cadcraft_engine::doc::color::Color::Index(3)));
    assert_eq!(g.text_fill.len(), 4, "the new dimension has a text background");
    // DIMTFILL takes 0, 1 or 2.
    let mut st = DimStyle::default();
    assert_eq!(st.apply_fields(&serde_json::Map::from_iter([("DIMTFILL".to_string(), json!(3))])).len(), 1);
    // EXPLODE keeps the background as a SOLID in DIMTFILLCLR, before the text.
    let h = s.doc().unwrap().model.last().unwrap().handle.hex();
    s.execute("explode", &json!({ "handles": [h] })).unwrap();
    let d = s.doc().unwrap();
    let solid = d.model.iter().position(|e| matches!(e.kind, EntityKind::Solid(_)) && e.common.color == cadcraft_engine::doc::color::Color::Index(3));
    let mtext = d.model.iter().position(|e| matches!(e.kind, EntityKind::MText(_)));
    assert!(solid.is_some() && solid < mtext, "{solid:?}, {mtext:?}");
    // The arrowheads (the other SOLIDs) come before the background, as the dimension draws them.
    let arrows: Vec<usize> =
        d.model.iter().enumerate().filter(|(i, e)| matches!(e.kind, EntityKind::Solid(_)) && Some(*i) != solid).map(|(i, _)| i).collect();
    assert!(!arrows.is_empty() && arrows.iter().all(|i| Some(*i) < solid), "{arrows:?} before {solid:?}");
}

#[test]
fn fit_moves_text_or_arrows_outside() {
    // 0.5 between the extension lines: the text "0.5000" doesn't fit, the arrows (2 × 0.18) do.
    let mut s = Session::new();
    linear(&mut s, 0.5);
    let (_, g) = last_dim(&s);
    assert!(g.text_pos.x > 0.5, "best fit: text outside, {:?}", g.text_pos);
    assert!(covers(&g, 3.0, 0.25), "arrows inside: the line runs between them");
    // DIMATFIT 0: both outside; no line between the extension lines without DIMTOFL.
    set(&mut s, "DIMATFIT", json!(0));
    linear(&mut s, 0.5);
    let (_, g) = last_dim(&s);
    assert!(g.text_pos.x > 0.5);
    assert!(!covers(&g, 3.0, 0.25));
    assert!(covers(&g, 3.0, -0.2) && covers(&g, 3.0, 0.7), "stubs outside both extension lines");
    // DIMTOFL draws it anyway.
    set(&mut s, "DIMTOFL", json!(1));
    linear(&mut s, 0.5);
    let (_, g) = last_dim(&s);
    assert!(covers(&g, 3.0, 0.25));
    // DIMTIX keeps the text between the extension lines; DIMSOXD drops the outside arrows.
    set(&mut s, "DIMTOFL", json!(0));
    set(&mut s, "DIMTIX", json!(1));
    linear(&mut s, 0.5);
    let (_, g) = last_dim(&s);
    assert!((g.text_pos.x - 0.25).abs() < 1e-9, "{:?}", g.text_pos);
    assert_eq!(g.fills.len(), 2, "arrows outside");
    set(&mut s, "DIMSOXD", json!(1));
    linear(&mut s, 0.5);
    let (_, g) = last_dim(&s);
    assert!(g.fills.is_empty(), "outside arrows suppressed");
    // DIMATFIT 1 (arrows first): text that fits alone stays inside, the arrows go out.
    let mut s = Session::new();
    set(&mut s, "DIMATFIT", json!(1));
    set(&mut s, "DIMDEC", json!(0));
    linear(&mut s, 0.5);
    let (_, g) = last_dim(&s);
    assert!((g.text_pos.x - 0.25).abs() < 1e-9, "{:?}", g.text_pos);
    assert!(g.fills.iter().flatten().any(|p| p.x < 0.0) && g.fills.iter().flatten().any(|p| p.x > 0.5));
}

#[test]
fn suppressed_halves_fixed_extension_lines_and_vertical_position() {
    let mut s = Session::new();
    set(&mut s, "DIMTAD", json!(1));
    set(&mut s, "DIMSD1", json!(1));
    linear(&mut s, 10.0);
    let (_, g) = last_dim(&s);
    assert!(!covers(&g, 3.0, 2.0) && covers(&g, 3.0, 8.0));
    assert_eq!(g.fills.len(), 1, "only the second arrowhead");
    // Fixed-length extension lines: 0.5 from the dimension line toward the origin.
    let mut s = Session::new();
    set(&mut s, "DIMFXLON", json!(1));
    set(&mut s, "DIMFXL", json!(0.5));
    linear(&mut s, 10.0);
    let (_, g) = last_dim(&s);
    for l in g.lines_of(LineRole::Ext) {
        let lo = l.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
        assert!((lo - 2.5).abs() < 1e-9, "{l:?}");
    }
    // DIMTVP lifts centred text by text heights.
    let mut s = Session::new();
    set(&mut s, "DIMTVP", json!(1.0));
    linear(&mut s, 10.0);
    let (_, g) = last_dim(&s);
    assert!((g.text_pos.y - (3.0 + 0.18)).abs() < 1e-9, "{:?}", g.text_pos);
    assert!(covers(&g, 3.0, 5.0), "text off the line: the line isn't broken");
}

#[test]
fn text_move_settings() {
    // DIMTMOVE 1: a leader joins text moved off the line; the line stays put.
    let mut s = Session::new();
    set(&mut s, "DIMTMOVE", json!(1));
    linear(&mut s, 10.0);
    let h = s.doc().unwrap().model.last().unwrap().handle.hex();
    s.execute("dimtedit", &json!({ "handles": [h], "at": [5, 8] })).unwrap();
    let (dm, g) = last_dim(&s);
    assert!((dm.defpt.y - 3.0).abs() < 1e-9);
    assert!(dim_lines(&g).iter().any(|l| l.first().is_some_and(|p| p.dist(Vec2::new(5.0, 3.0)) < 1e-9) && l.last().is_some_and(|p| p.y > 7.0)));
    // DIMTMOVE 0: the line follows (DIMTAD 1 keeps it below the text).
    let mut s = Session::new();
    set(&mut s, "DIMTAD", json!(1));
    linear(&mut s, 10.0);
    let h = s.doc().unwrap().model.last().unwrap().handle.hex();
    s.execute("dimtedit", &json!({ "handles": [h], "at": [5, 8] })).unwrap();
    let (dm, _) = last_dim(&s);
    assert!((dm.defpt.y - (8.0 - 0.09 - 0.09)).abs() < 1e-9, "{:?}", dm.defpt);
    // Moving the text grip does the same.
    let grips = s.doc().unwrap().model.last().unwrap().kind.grips();
    let i = grips.iter().position(|p| p.dist(dm.text_mid.xy()) < 1e-9).unwrap();
    s.execute("grip.stretch", &json!({ "handle": h, "index": i, "to": [5, 12] })).unwrap();
    let (dm, _) = last_dim(&s);
    assert!((dm.defpt.y - (12.0 - 0.18)).abs() < 1e-9, "{:?}", dm.defpt);
}

#[test]
fn setvar_refuses_the_values_the_style_refuses() {
    let mut s = Session::new();
    set(&mut s, "DIMTMOVE", json!(1));
    let get = |s: &mut Session, name: &str| s.execute("getvar", &json!({ "name": name })).unwrap()["value"].clone();
    for (name, v) in [
        ("DIMTMOVE", json!(3)),
        ("DIMATFIT", json!(7)),
        ("DIMLWD", json!(500)),
        ("DIMJOGANG", json!(120)),
        ("DIMFXL", json!(-1)),
        ("DIMTXT", json!(0)),
        ("DIMDEC", json!(300)),
        ("DIMCLRT", json!("no such color")),
    ] {
        let before = get(&mut s, name);
        assert!(s.execute("setvar", &json!({ "name": name, "value": v })).is_err(), "{name} {v}");
        assert_eq!(get(&mut s, name), before, "{name} keeps its value");
        // DIMSTYLE and DIMOVERRIDE refuse the same value.
        let mut st = DimStyle::default();
        assert_eq!(st.apply_fields(&serde_json::Map::from_iter([(name.to_string(), v)])).len(), 1, "{name}");
    }
    assert_eq!(s.doc().unwrap().dim_overrides().get("textMove"), Some(&json!(1)));
    // Values in range still go in, typed as text too.
    set(&mut s, "DIMTMOVE", json!("2"));
    set(&mut s, "DIMCLRT", json!(3));
    let o = s.doc().unwrap().dim_overrides();
    assert_eq!(o.get("textMove"), Some(&json!(2)));
    assert_eq!(o.get("textColor"), serde_json::to_value(cadcraft_engine::doc::color::Color::Index(3)).ok().as_ref(), "{o:?}");
}

#[test]
fn tolerance_justification_angular_zeros_and_arc_symbol() {
    let mut s = Session::new();
    set(&mut s, "DIMTOL", json!(1));
    set(&mut s, "DIMTP", json!(0.1));
    set(&mut s, "DIMTM", json!(0.2));
    linear(&mut s, 10.0);
    let (_, middle) = last_dim(&s);
    assert!(!middle.mtext.contains("\\A"));
    set(&mut s, "DIMTOLJ", json!(0));
    linear(&mut s, 10.0);
    let (_, bottom) = last_dim(&s);
    assert!(bottom.mtext.contains("\\A0;"), "{}", bottom.mtext);
    // Bottom alignment: the lower tolerance sits on the baseline, higher than when centred.
    let low = |g: &DimGeometry| g.text.iter().flatten().map(|p| p.y).fold(f64::INFINITY, f64::min);
    assert!(low(&bottom) > low(&middle) + 1e-6);

    // DIMAZIN 2 drops trailing zeros of angles.
    let mut s = Session::new();
    set(&mut s, "DIMADEC", json!(2));
    s.execute("dimangular", &json!({ "vertex": [0, 0], "p1": [10, 0], "p2": [0, 10], "at": [3, 3] })).unwrap();
    assert_eq!(last_dim(&s).1.value, "90.00°");
    set(&mut s, "DIMAZIN", json!(2));
    s.execute("dimangular", &json!({ "vertex": [0, 0], "p1": [10, 0], "p2": [0, 10], "at": [3, 3] })).unwrap();
    assert_eq!(last_dim(&s).1.value, "90°");

    // DIMARCSYM: before the text, over it, or none.
    let mut s = Session::new();
    s.execute("arc", &json!({ "center": [0, 0], "radius": 5, "start": 0, "end": 90 })).unwrap();
    let a = s.doc().unwrap().model.last().unwrap().handle.hex();
    s.execute("dimarc", &json!({ "handle": a, "at": [5, 5] })).unwrap();
    let (_, g0) = last_dim(&s);
    assert!(g0.value.starts_with('⌒'), "{}", g0.value);
    set(&mut s, "DIMARCSYM", json!(2));
    s.execute("dimarc", &json!({ "handle": a, "at": [5, 5] })).unwrap();
    let (_, g2) = last_dim(&s);
    assert!(!g2.value.contains('⌒'));
    set(&mut s, "DIMARCSYM", json!(1));
    s.execute("dimarc", &json!({ "handle": a, "at": [5, 5] })).unwrap();
    let (_, g1) = last_dim(&s);
    assert!(!g1.value.contains('⌒'));
    assert_eq!(dim_lines(&g1).len(), dim_lines(&g2).len() + 1, "the symbol drawn over the text");
}

#[test]
fn linetypes_and_lineweights_of_dimension_and_extension_lines() {
    let mut s = Session::new();
    s.doc_mut().unwrap().linetypes.push(Linetype::simple("DASHED", "", &[0.5, -0.25]));
    linear(&mut s, 10.0);
    let opts = cadcraft_engine::render::Options { lineweights: true, ..Default::default() };
    let count = |s: &Session| cadcraft_engine::render::build(s.doc().unwrap(), &cadcraft_engine::doc::Space::Model, &opts).prims.len();
    let plain = count(&s);
    let h = s.doc().unwrap().model.last().unwrap().handle.hex();
    s.execute("dimoverride", &json!({ "handles": [h], "DIMLTEX2": "DASHED" })).unwrap();
    let dashed_ext2 = count(&s);
    assert!(dashed_ext2 > plain, "extension line 2 dashed: {plain} → {dashed_ext2}");
    s.execute("dimoverride", &json!({ "handles": [h], "DIMLTYPE": "DASHED" })).unwrap();
    assert!(count(&s) > dashed_ext2 + 4, "dimension line dashed too");
    // Lineweights: 0.50 mm dimension lines, 0.13 mm extension lines.
    s.execute("dimoverride", &json!({ "handles": [h], "clear": true })).unwrap();
    s.execute("dimoverride", &json!({ "handles": [h], "DIMLWD": 50, "DIMLWE": 13 })).unwrap();
    let dl = cadcraft_engine::render::build(s.doc().unwrap(), &cadcraft_engine::doc::Space::Model, &opts);
    let lws: Vec<f32> = dl.prims.iter().filter(|p| p.kind == cadcraft_engine::render::Kind::Polyline).map(|p| p.lw).collect();
    assert!(lws.contains(&0.5) && lws.contains(&0.13), "{lws:?}");
}

fn ignored(r: &Value) -> Vec<&str> {
    r["ignored"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default()
}

#[test]
fn dimstyle_and_dimoverride_refuse_negative_whole_numbers() {
    let mut s = Session::new();
    s.execute("dimstyle", &json!({ "name": "Standard", "DIMTMOVE": 2, "DIMLWD": 50 })).unwrap();
    let r = s
        .execute(
            "dimstyle",
            &json!({
                "name": "Standard",
                "DIMDEC": -1,
                "DIMTMOVE": -1,
                "DIMLUNIT": "-1",
                "DIMTDEC": -0.6,
                "DIMADEC": -2,
                "DIMALTD": "300",
                "DIMLWE": "300",
            }),
        )
        .unwrap();
    let ign = ignored(&r);
    for k in ["DIMDEC", "DIMTMOVE", "DIMLUNIT", "DIMTDEC", "DIMADEC", "DIMALTD", "DIMLWE"] {
        assert!(ign.contains(&k), "{k}: {r}");
    }
    let st = s.doc().unwrap().dim_style("Standard").unwrap().clone();
    let def = DimStyle::default();
    assert_eq!((st.decimals, st.text_move, st.linear_unit, st.tol_decimals), (def.decimals, 2, def.linear_unit, def.tol_decimals));
    assert_eq!((st.angular_decimals, st.alt_decimals, st.ext_lineweight), (def.angular_decimals, def.alt_decimals, def.ext_lineweight));
    // DIMADEC -1 is a documented value (stored as 0); ByLayer lineweights stay -1.
    let r = s.execute("dimstyle", &json!({ "name": "Standard", "DIMADEC": -1, "DIMLWD": "ByLayer" })).unwrap();
    assert!(r.get("ignored").is_none(), "{r}");
    assert_eq!(s.doc().unwrap().dim_style("Standard").unwrap().dim_lineweight, -1);

    // DIMOVERRIDE keeps the valid value and leaves the negative one out.
    linear(&mut s, 10.0);
    let h = s.doc().unwrap().model.last().unwrap().handle.hex();
    s.execute("dimoverride", &json!({ "handles": [h], "DIMDEC": -1, "DIMTMOVE": 1 })).unwrap();
    let (dm, _) = last_dim(&s);
    assert!(!dm.overrides.contains_key("decimals"), "{:?}", dm.overrides);
    assert_eq!(dm.overrides.get("textMove"), Some(&json!(1)));
    assert!(s.execute("dimoverride", &json!({ "handles": [h], "DIMZIN": -3 })).is_err());
}

#[test]
fn setvar_negative_whole_numbers_do_not_reach_new_dimensions() {
    let mut s = Session::new();
    for (name, v) in [("DIMDEC", json!(-1)), ("DIMLUNIT", json!("-1")), ("DIMTDEC", json!(-0.6))] {
        // The header may hold the value; new dimensions keep the style's own.
        let _ = s.execute("setvar", &json!({ "name": name, "value": v }));
    }
    let o = s.doc().unwrap().dim_overrides();
    for f in ["decimals", "linearUnit", "tolDecimals"] {
        assert!(!o.contains_key(f), "{f}: {o:?}");
    }
    linear(&mut s, 10.0);
    let (dm, g) = last_dim(&s);
    assert_eq!(g.value, "10.0000");
    assert!(dm.overrides.is_empty(), "{:?}", dm.overrides);
}

#[test]
fn leaders_use_the_leader_arrow() {
    let mut s = Session::new();
    for st in &mut s.doc_mut().unwrap().dim_styles {
        st.arrow_block = "_DOT".into();
    }
    s.execute("leader", &json!({ "points": [[0, 0], [5, 5]], "text": "A" })).unwrap();
    let tris = |s: &Session| {
        let dl = cadcraft_engine::render::build(s.doc().unwrap(), &cadcraft_engine::doc::Space::Model, &Default::default());
        dl.prims.iter().filter(|p| p.kind == cadcraft_engine::render::Kind::Tris).map(|p| p.len).sum::<u32>()
    };
    // DIMLDRBLK "" is a closed filled arrow (one triangle) whatever DIMBLK is.
    assert_eq!(tris(&s), 3);
    for st in &mut s.doc_mut().unwrap().dim_styles {
        st.leader_arrow_block = "_DOT".into();
    }
    assert_eq!(tris(&s), 72, "a dot: 24 triangles");
}
