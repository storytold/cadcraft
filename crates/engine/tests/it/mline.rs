//! Multilines (#510): the MLINE entity with its style's elements, MLSTYLE, MLEDIT, and the
//! edits, grips, snaps, selection and rendering every object gets.

use cadcraft_doc::{EntityKind, Handle, MLine, Space};
use cadcraft_engine::Session;
use cadcraft_engine::snap::{self, mode};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

fn h(v: &Value) -> Handle {
    Handle::parse_hex(v["handle"].as_str().unwrap()).unwrap()
}

fn mline(s: &Session, hd: Handle) -> MLine {
    match &s.doc().unwrap().entity(hd).unwrap().kind {
        EntityKind::MLine(m) => m.clone(),
        k => panic!("not a multiline: {k:?}"),
    }
}

/// Element pieces of a multiline, rounded, sorted: (x1, y1, x2, y2).
fn pieces(m: &MLine) -> Vec<[f64; 4]> {
    let r = |v: f64| (v * 1e6).round() / 1e6 + 0.0;
    let mut v: Vec<[f64; 4]> = m.geometry().lines.iter().map(|(_, a, b)| [r(a.x), r(a.y), r(b.x), r(b.y)]).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

/// A horizontal multiline (0,0)–(10,0) and a vertical one (5,-5)–(5,5), both centred (Zero)
/// with scale 2: elements at y = ±1 and x = 4 / 6.
fn crossing(s: &mut Session) -> (Handle, Handle) {
    let a = h(&s.execute("mline", &json!({"points": [[0, 0], [10, 0]], "justification": "zero", "scale": 2})).unwrap());
    let b = h(&s.execute("mline", &json!({"points": [[5, -5], [5, 5]], "justification": "zero", "scale": 2})).unwrap());
    (a, b)
}

#[test]
fn mline_is_one_object_with_the_standard_style() {
    let mut s = Session::new();
    let a = h(&s.execute("mline", &json!({"points": [[0, 0], [10, 0], [10, 5]]})).unwrap());
    assert_eq!(s.doc().unwrap().model.len(), 1);
    let m = mline(&s, a);
    assert_eq!(m.style, "STANDARD");
    // Top justification: the first element runs through the points, the other one unit to the right.
    assert_eq!(pieces(&m), vec![[0.0, -1.0, 11.0, -1.0], [0.0, 0.0, 10.0, 0.0], [10.0, 0.0, 10.0, 5.0], [11.0, -1.0, 11.0, 5.0]]);
    assert_eq!(s.doc().unwrap().model.iter().next().unwrap().kind.type_name(), "MLine");
    assert!(s.execute("mline", &json!({"points": [[0, 0]]})).is_err());
    assert!(s.execute("mline", &json!({"points": [[0, 0], [1, 0]], "style": "NOPE"})).is_err());
}

#[test]
fn mlstyle_defines_elements_caps_and_fill() {
    let mut s = Session::new();
    let r = s
        .execute(
            "mlstyle",
            &json!({"name": "Wall", "elements": [{"offset": 0.5, "color": "red"}, {"offset": 0}, {"offset": -0.5, "linetype": "Hidden"}],
                    "fill": "yellow", "joints": true, "startCaps": {"line": true}, "endCaps": {"outerArc": true}, "current": false}),
        )
        .unwrap();
    assert_eq!(r["current"], "STANDARD");
    assert_eq!(r["styles"].as_array().unwrap().len(), 2);
    let a = h(&s.execute("mline", &json!({"points": [[0, 0], [10, 0], [10, 10]], "style": "wall", "justification": "zero"})).unwrap());
    let m = mline(&s, a);
    assert_eq!(m.style, "Wall");
    assert_eq!(m.def.elements.len(), 3);
    let g = m.geometry();
    assert_eq!(g.lines.len(), 6);
    // Start cap line, end half circle, one joint at the corner.
    assert_eq!(g.caps.len(), 3);
    assert_eq!(g.fill.len(), 1);

    // A style in use keeps its elements; its description may change.
    assert!(s.execute("mlstyle", &json!({"name": "Wall", "elements": [{"offset": 1}, {"offset": -1}]})).is_err());
    s.execute("mlstyle", &json!({"name": "Wall", "description": "outer walls", "current": false})).unwrap();
    assert!(s.execute("mlstyle", &json!({"delete": "Wall"})).is_err(), "in use");
    assert!(s.execute("mlstyle", &json!({"delete": "STANDARD"})).is_err());
    assert!(s.execute("mlstyle", &json!({"name": "Bad", "elements": []})).is_err());
    assert!(s.execute("mlstyle", &json!({"name": "Bad", "startCaps": {"angle": 5}})).is_err());

    // New styles become current; MLINE then draws with them.
    s.execute("mlstyle", &json!({"name": "Thin", "copyFrom": "Wall", "fill": false})).unwrap();
    assert_eq!(s.doc().unwrap().header.str("CMLSTYLE", ""), "Thin");
    let b = h(&s.execute("mline", &json!({"points": [[0, 20], [5, 20]]})).unwrap());
    assert_eq!(mline(&s, b).def.elements.len(), 3);
    assert!(mline(&s, b).def.fill.is_none());
    s.execute("erase", &json!({"handles": [b.hex()]})).unwrap();
    s.execute("mlstyle", &json!({"name": "Thin", "rename": "Slim"})).unwrap();
    assert_eq!(s.doc().unwrap().header.str("CMLSTYLE", ""), "Slim");
    s.execute("mlstyle", &json!({"name": "STANDARD"})).unwrap();
    s.execute("mlstyle", &json!({"delete": "Slim"})).unwrap();
    assert!(s.doc().unwrap().mline_style("Slim").is_none());
}

#[test]
fn mline_command_line_style_option() {
    let mut s = Session::new();
    s.execute("mlstyle", &json!({"name": "Triple", "elements": [{"offset": 1}, {"offset": 0}, {"offset": -1}], "current": false})).unwrap();
    s.cmdline("mline st ?").unwrap();
    assert!(s.log.iter().any(|l| l.contains("Triple")), "{:?}", s.log);
    for t in ["triple", "j z 0,0 10,0", ""] {
        s.cmdline(t).unwrap();
    }
    assert!(s.running.is_none());
    let m = match &s.doc().unwrap().model.iter().last().unwrap().kind {
        EntityKind::MLine(m) => m.clone(),
        k => panic!("{k:?}"),
    };
    assert_eq!((m.style.as_str(), m.def.elements.len()), ("Triple", 3));
    assert_eq!(s.doc().unwrap().header.str("CMLSTYLE", ""), "Triple");
    assert!(s.cmdline("mline st nosuch").is_err() || s.log.iter().any(|l| l.contains("Cannot find")));
}

#[test]
fn mledit_crosses() {
    // Closed cross: the first multiline is broken where the second crosses it.
    let mut s = Session::new();
    let (a, b) = crossing(&mut s);
    s.execute("mledit", &json!({"tool": "cc", "first": a.hex(), "p1": [2, 0], "second": b.hex(), "p2": [5, 3]})).unwrap();
    assert_eq!(pieces(&mline(&s, a)), vec![[0.0, -1.0, 4.0, -1.0], [0.0, 1.0, 4.0, 1.0], [6.0, -1.0, 10.0, -1.0], [6.0, 1.0, 10.0, 1.0]]);
    assert_eq!(pieces(&mline(&s, b)).len(), 2);

    // Open cross: the second one's outer elements open too.
    let mut s = Session::new();
    let (a, b) = crossing(&mut s);
    s.cmdline("mledit oc 2,1 4,3").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(pieces(&mline(&s, a)).len(), 4);
    assert_eq!(pieces(&mline(&s, b)), vec![[4.0, -5.0, 4.0, -1.0], [4.0, 1.0, 4.0, 5.0], [6.0, -5.0, 6.0, -1.0], [6.0, 1.0, 6.0, 5.0]]);

    // Merged cross: the outer elements of both break, inner ones run through.
    let mut s = Session::new();
    s.execute("mlstyle", &json!({"name": "C", "elements": [{"offset": 0.5}, {"offset": 0}, {"offset": -0.5}]})).unwrap();
    let (a, b) = crossing(&mut s);
    s.execute("mledit", &json!({"tool": "mc", "first": a.hex(), "p1": [2, 0], "second": b.hex(), "p2": [5, 3]})).unwrap();
    assert_eq!(pieces(&mline(&s, a)).len(), 5);
    assert_eq!(pieces(&mline(&s, b)).len(), 5);
    assert!(pieces(&mline(&s, a)).contains(&[0.0, 0.0, 10.0, 0.0]), "centre line continues");
}

#[test]
fn mledit_tees_and_corner() {
    // Closed tee: the first multiline ends at the second one's near side.
    let mut s = Session::new();
    let (a, b) = crossing(&mut s);
    s.execute("mledit", &json!({"tool": "ct", "first": a.hex(), "p1": [1, 0], "second": b.hex(), "p2": [5, 3]})).unwrap();
    let m = mline(&s, a);
    assert_eq!(m.points(), vec![Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0)]);
    assert!(m.no_end_caps);
    assert_eq!(pieces(&mline(&s, b)).len(), 2);

    // Open tee: the second one opens where the first meets it.
    let mut s = Session::new();
    let (a, b) = crossing(&mut s);
    s.execute("mledit", &json!({"tool": "ot", "first": a.hex(), "p1": [9, 0], "second": b.hex(), "p2": [5, 3]})).unwrap();
    assert_eq!(mline(&s, a).points(), vec![Vec2::new(6.0, 0.0), Vec2::new(10.0, 0.0)]);
    assert_eq!(pieces(&mline(&s, b)), vec![[4.0, -5.0, 4.0, 5.0], [6.0, -5.0, 6.0, -1.0], [6.0, 1.0, 6.0, 5.0]]);

    // Merged tee: the outer elements stop at the second's near side, which opens, and the inner
    // ones run on to the second's inner element, which runs through.
    let mut s = Session::new();
    s.execute("mlstyle", &json!({"name": "C", "elements": [{"offset": 0.5}, {"offset": 0}, {"offset": -0.5}]})).unwrap();
    let (a, b) = crossing(&mut s);
    s.execute("mledit", &json!({"tool": "mt", "first": a.hex(), "p1": [9, 0], "second": b.hex(), "p2": [5, 3]})).unwrap();
    assert_eq!(mline(&s, a).points(), vec![Vec2::new(5.0, 0.0), Vec2::new(10.0, 0.0)]);
    assert_eq!(pieces(&mline(&s, a)), vec![[5.0, 0.0, 10.0, 0.0], [6.0, -1.0, 10.0, -1.0], [6.0, 1.0, 10.0, 1.0]]);
    assert_eq!(pieces(&mline(&s, b)), vec![[4.0, -5.0, 4.0, 5.0], [5.0, -5.0, 5.0, 5.0], [6.0, -5.0, 6.0, -1.0], [6.0, 1.0, 6.0, 5.0]]);
    // Picked on the other side, from the command line.
    let mut s = Session::new();
    s.execute("mlstyle", &json!({"name": "C", "elements": [{"offset": 0.5}, {"offset": 0}, {"offset": -0.5}]})).unwrap();
    let (a, b) = crossing(&mut s);
    s.cmdline("mledit mt 1,0 5,3").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(pieces(&mline(&s, a)), vec![[0.0, -1.0, 4.0, -1.0], [0.0, 0.0, 5.0, 0.0], [0.0, 1.0, 4.0, 1.0]]);
    assert_eq!(pieces(&mline(&s, b)), vec![[4.0, -5.0, 4.0, -1.0], [4.0, 1.0, 4.0, 5.0], [5.0, -5.0, 5.0, 5.0], [6.0, -5.0, 6.0, 5.0]]);
    // Two elements have nothing inner to merge: the same as the open tee.
    let mut s = Session::new();
    let (a, b) = crossing(&mut s);
    s.execute("mledit", &json!({"tool": "mt", "first": a.hex(), "p1": [9, 0], "second": b.hex(), "p2": [5, 3]})).unwrap();
    assert_eq!(mline(&s, a).points(), vec![Vec2::new(6.0, 0.0), Vec2::new(10.0, 0.0)]);
    assert_eq!(pieces(&mline(&s, b)), vec![[4.0, -5.0, 4.0, 5.0], [6.0, -5.0, 6.0, -1.0], [6.0, 1.0, 6.0, 5.0]]);

    // Corner joint: two multilines become one round the corner.
    let mut s = Session::new();
    let a = h(&s.execute("mline", &json!({"points": [[0, 0], [10, 0]]})).unwrap());
    let b = h(&s.execute("mline", &json!({"points": [[12, 2], [12, 10]]})).unwrap());
    s.execute("mledit", &json!({"tool": "cj", "first": a.hex(), "p1": [2, 0], "second": b.hex(), "p2": [12, 8]})).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 1);
    assert_eq!(mline(&s, a).points(), vec![Vec2::new(0.0, 0.0), Vec2::new(12.0, 0.0), Vec2::new(12.0, 10.0)]);
}

#[test]
fn mledit_vertices_cuts_and_welds() {
    let mut s = Session::new();
    let a = h(&s.execute("mline", &json!({"points": [[0, 0], [10, 0]], "justification": "zero", "scale": 2})).unwrap());
    s.execute("mledit", &json!({"tool": "av", "handle": a.hex(), "point": [5, 0.3]})).unwrap();
    assert_eq!(mline(&s, a).vertices.len(), 3);
    s.execute("mledit", &json!({"tool": "dv", "handle": a.hex(), "point": [5.2, 0]})).unwrap();
    assert_eq!(mline(&s, a).points(), vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)]);
    assert!(s.execute("mledit", &json!({"tool": "dv", "handle": a.hex(), "point": [0, 0]})).is_err(), "two vertices stay");

    // Cut single: only the element picked.
    s.execute("mledit", &json!({"tool": "cs", "handle": a.hex(), "p1": [3, 1.1], "p2": [5, 1]})).unwrap();
    assert_eq!(pieces(&mline(&s, a)), vec![[0.0, -1.0, 10.0, -1.0], [0.0, 1.0, 3.0, 1.0], [5.0, 1.0, 10.0, 1.0]]);
    // Cut all, then weld all over both breaks.
    s.cmdline("mledit ca 7,1 8,1").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(pieces(&mline(&s, a)).len(), 5);
    s.execute("mledit", &json!({"tool": "wa", "handle": a.hex(), "p1": [2, 0], "p2": [9, 0]})).unwrap();
    assert_eq!(pieces(&mline(&s, a)), vec![[0.0, -1.0, 10.0, -1.0], [0.0, 1.0, 10.0, 1.0]]);
    // Undo brings the breaks back.
    s.execute("u", &json!({})).unwrap();
    assert_eq!(pieces(&mline(&s, a)).len(), 5);
}

#[test]
fn transforms_grips_explode() {
    let mut s = Session::new();
    let a = h(&s.execute("mline", &json!({"points": [[0, 0], [10, 0]]})).unwrap());
    s.execute("mledit", &json!({"tool": "ca", "handle": a.hex(), "p1": [4, 0], "p2": [6, 0]})).unwrap();
    // Mirrored: the second element moves to the other side.
    s.execute("mirror", &json!({"handles": [a.hex()], "p1": [0, 0], "p2": [1, 0], "erase": true})).unwrap();
    let a = s.doc().unwrap().model.iter().last().unwrap().handle;
    assert_eq!(pieces(&mline(&s, a)), vec![[0.0, 0.0, 4.0, 0.0], [0.0, 1.0, 4.0, 1.0], [6.0, 0.0, 10.0, 0.0], [6.0, 1.0, 10.0, 1.0]]);
    // Scaled: offsets and breaks scale.
    s.execute("scale", &json!({"handles": [a.hex()], "base": [0, 0], "factor": 2})).unwrap();
    assert_eq!(pieces(&mline(&s, a)), vec![[0.0, 0.0, 8.0, 0.0], [0.0, 2.0, 8.0, 2.0], [12.0, 0.0, 20.0, 0.0], [12.0, 2.0, 20.0, 2.0]]);
    s.execute("move", &json!({"handles": [a.hex()], "delta": [1, 1]})).unwrap();
    s.execute("rotate", &json!({"handles": [a.hex()], "base": [1, 1], "angle": 90})).unwrap();
    let near = |p: &[Vec2], q: &[Vec2]| p.len() == q.len() && p.iter().zip(q).all(|(a, b)| a.near(*b, 1e-9));
    assert!(near(&mline(&s, a).points(), &[Vec2::new(1.0, 1.0), Vec2::new(1.0, 21.0)]));

    // Grips are the vertices.
    let k = s.doc().unwrap().entity(a).unwrap().kind.clone();
    assert!(near(&k.grips(), &mline(&s, a).points()));
    let moved = cadcraft_engine::grips::stretch_grip(&k, 1, Vec2::new(5.0, 21.0)).unwrap();
    let EntityKind::MLine(m) = moved else { panic!() };
    assert_eq!(m.points()[1], Vec2::new(5.0, 21.0));

    // Explode: lines in the elements' colours.
    let mut s = Session::new();
    s.execute(
        "mlstyle",
        &json!({"name": "R", "elements": [{"offset": 0.5, "color": "red"}, {"offset": -0.5}], "endCaps": {"line": true, "outerArc": true}}),
    )
    .unwrap();
    let a = h(&s.execute("mline", &json!({"points": [[0, 0], [10, 0]]})).unwrap());
    s.execute("explode", &json!({"handles": [a.hex()]})).unwrap();
    let d = s.doc().unwrap();
    let lines = d.model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count();
    let arcs = d.model.iter().filter(|e| matches!(e.kind, EntityKind::Arc(_))).count();
    assert_eq!((lines, arcs), (3, 1));
    assert!(d.model.iter().any(|e| e.common.color == cadcraft_color::Color::Index(1)));
    assert!(!d.model.iter().any(|e| matches!(e.kind, EntityKind::MLine(_))));
}

#[test]
fn snaps_selection_extents_and_render() {
    let mut s = Session::new();
    s.execute("mlstyle", &json!({"name": "R", "elements": [{"offset": 0.5, "color": "red"}, {"offset": -0.5}], "fill": "blue"})).unwrap();
    let a = h(&s.execute("mline", &json!({"points": [[0, 0], [10, 0]], "justification": "zero"})).unwrap());
    let d = s.doc().unwrap();
    // Endpoint of the lower element, and the intersection-free midpoint of the upper one.
    let hit = snap::osnap(d, &Space::Model, Vec2::new(10.05, -0.45), 0.3, mode::END, None, false).unwrap();
    assert_eq!(hit.point, Vec2::new(10.0, -0.5));
    let hit = snap::osnap(d, &Space::Model, Vec2::new(5.05, 0.45), 0.3, mode::MID, None, false).unwrap();
    assert_eq!(hit.point, Vec2::new(5.0, 0.5));
    // Picking an element picks the multiline.
    assert_eq!(cadcraft_engine::select::pick(d, &Space::Model, Vec2::new(3.0, 0.52), 0.1), Some(a));
    let ext = d.extents(&Space::Model);
    assert!(ext.min.near(Vec2::new(0.0, -0.5), 1e-9) && ext.max.near(Vec2::new(10.0, 0.5), 1e-9), "{ext:?}");
    // Drawn: the fill, and each element in its own colour.
    let list = cadcraft_render::build(d, &Space::Model, &cadcraft_render::Options::default());
    let red = cadcraft_color::aci_rgb(1);
    let blue = cadcraft_color::aci_rgb(5);
    assert!(list.prims.iter().any(|p| p.color == red && matches!(p.kind, cadcraft_render::Kind::Polyline)));
    assert!(list.prims.iter().any(|p| p.color == blue && matches!(p.kind, cadcraft_render::Kind::Tris)));
}

#[test]
fn hostile_mline_params_never_panic() {
    let mut s = Session::new();
    let a = h(&s.execute("mline", &json!({"points": [[0, 0], [10, 0]]})).unwrap());
    let b = h(&s.execute("mline", &json!({"points": [[0, 0], [10, 0]]})).unwrap());
    let bad = [
        ("mline", json!({"points": [[0, 0], [1e308, 1e308], [-1e308, 0]], "scale": 1e308})),
        ("mline", json!({"points": [[0, 0], [0, 0], [0, 0]]})),
        ("mline", json!({"points": "x", "closed": true})),
        ("mlstyle", json!({"name": "X", "elements": [{"offset": 1e308}, {"offset": "a"}]})),
        ("mlstyle", json!({"name": "X", "elements": (0..100).map(|i| json!({"offset": i})).collect::<Vec<_>>()})),
        ("mlstyle", json!({"name": "", "fill": 99999})),
        ("mlstyle", json!({"name": "a<b"})),
        ("mledit", json!({"tool": "cc", "first": a.hex(), "p1": [0, 0], "second": b.hex(), "p2": [0, 0]})),
        ("mledit", json!({"tool": "cj", "first": a.hex(), "p1": [0, 0], "second": b.hex(), "p2": [1e308, 0]})),
        ("mledit", json!({"tool": "ct", "first": a.hex(), "p1": [0, 0], "second": a.hex(), "p2": [0, 0]})),
        ("mledit", json!({"tool": "cs", "handle": a.hex(), "p1": [1e308, 0], "p2": [-1e308, 0]})),
        ("mledit", json!({"tool": "av", "handle": "zz", "point": [0, 0]})),
        ("mledit", json!({"tool": "nope"})),
    ];
    for (id, p) in bad {
        let _ = s.execute(id, &p);
    }
    for t in ["mledit", "cc", "0,0", "0,0", "", "mline", "s", "-1e308", "0,0", "1,1", ""] {
        let _ = s.cmdline(t);
    }
    // Everything left draws.
    let d = s.doc().unwrap();
    let _ = cadcraft_render::build(d, &Space::Model, &cadcraft_render::Options::default());
}
