use cadcraft_doc::EntityKind;
use cadcraft_geom::Vec2;
use serde_json::json;

use crate::*;

fn kinds(s: &Session) -> Vec<&'static str> {
    s.doc().unwrap().model.iter().map(|e| e.kind.type_name()).collect()
}

#[test]
fn line_via_command_line() {
    let mut s = Session::new();
    s.cmdline("LINE").unwrap();
    s.cmdline("0,0").unwrap();
    s.cmdline("10,0").unwrap();
    s.cmdline("@0,5").unwrap();
    s.cmdline("c").unwrap();
    assert!(s.running.is_none());
    assert_eq!(kinds(&s), vec!["Line", "Line", "Line"]);
    // One undo step for the whole command.
    s.undo().unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 0);
    s.redo().unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 3);
}

#[test]
fn edits_while_a_command_runs_redraw() {
    // The canvas rebuilds when (revision, doc pointer) changes; each new segment must change it (issue #4).
    fn key(s: &Session) -> (u64, usize) {
        let st = s.state().unwrap();
        (st.revision, Arc::as_ptr(&st.doc) as usize)
    }
    let mut s = Session::new();
    for (cmd, pts) in [("line", ["0,0", "10,0", "10,5", "0,5"]), ("pline", ["20,0", "30,0", "30,5", "20,5"])] {
        s.cmdline(cmd).unwrap();
        s.cmdline(pts[0]).unwrap();
        for p in &pts[1..] {
            let before = key(&s);
            s.cmdline(p).unwrap();
            assert_ne!(key(&s), before, "{cmd}: segment to {p} not redrawn");
        }
        s.cmdline("").unwrap();
    }
}

#[test]
fn line_undo_option_and_aliases() {
    let mut s = Session::new();
    s.cmdline("l 0,0 5,5 10,0 u").unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 1);
    s.cmdline("").unwrap(); // Enter ends
    assert!(s.running.is_none());
}

#[test]
fn programmatic_undo_during_a_command_ends_it_first() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    // TEXT stays active, waiting for the next line.
    s.script("TEXT 0,0 5 0 HELLO\n").unwrap();
    assert!(s.running.is_some());
    assert_eq!(kinds(&s), vec!["Line", "Text"]);
    // Like Ctrl+Z during a command: end it (as Esc does), then undo it, not the line.
    s.execute("undo", &json!({})).unwrap();
    assert!(s.running.is_none());
    assert_eq!(kinds(&s), vec!["Line"]);
    s.execute("redo", &json!({})).unwrap();
    assert_eq!(kinds(&s), vec!["Line", "Text"]);
    // Redo while a command runs ends it the same way and leaves the drawing alone.
    s.script("TEXT 0,-10 5 0\n").unwrap();
    assert!(s.running.is_some());
    s.execute("redo", &json!({})).unwrap();
    assert!(s.running.is_none());
    assert_eq!(kinds(&s), vec!["Line", "Text"]);
}

#[test]
fn polar_and_direct_distance() {
    let mut s = Session::new();
    s.cmdline("line 1,1").unwrap();
    s.cursor = Vec2::new(100.0, 1.0);
    s.cmdline("4").unwrap(); // direct distance along +X
    s.cmdline("@3<90").unwrap();
    s.cmdline("").unwrap();
    let ends: Vec<_> = s.doc().unwrap().model.iter().map(|e| e.kind.grips()).collect();
    assert!(ends[0][2].near(Vec2::new(5.0, 1.0), 1e-9));
    assert!(ends[1][2].near(Vec2::new(5.0, 4.0), 1e-9));
}

#[test]
fn circle_variants() {
    let mut s = Session::new();
    s.cmdline("circle 0,0 5").unwrap();
    s.cmdline("c 10,0 d 4").unwrap();
    s.cmdline("circle 3p 0,0 2,2 4,0").unwrap();
    s.cmdline("circle 2p 0,0 0,6").unwrap();
    let d = s.doc().unwrap();
    let radii: Vec<f64> = d.model.iter().filter_map(|e| if let EntityKind::Circle(c) = &e.kind { Some(c.radius) } else { None }).collect();
    assert_eq!(radii.len(), 4);
    assert!((radii[0] - 5.0).abs() < 1e-9);
    assert!((radii[1] - 2.0).abs() < 1e-9);
    assert!((radii[2] - 2.0).abs() < 1e-9);
    assert!((radii[3] - 3.0).abs() < 1e-9);
}

#[test]
fn rectangle_polygon_and_pline() {
    let mut s = Session::new();
    s.cmdline("rectang 0,0 4,3").unwrap();
    s.cmdline("polygon 6 10,10 i 2").unwrap();
    s.cmdline("pline 0,0 5,0 a 5,5 l 0,5 c").unwrap();
    let d = s.doc().unwrap();
    let mut it = d.model.iter();
    match &it.next().unwrap().kind {
        EntityKind::LwPolyline(p) => assert_eq!(p.vertices.len(), 4),
        _ => panic!(),
    }
    match &it.next().unwrap().kind {
        EntityKind::LwPolyline(p) => assert_eq!(p.vertices.len(), 6),
        _ => panic!(),
    }
    match &it.next().unwrap().kind {
        EntityKind::LwPolyline(p) => {
            assert!(p.closed);
            assert!(p.vertices[1].bulge.abs() > 0.1, "arc segment has a bulge");
        }
        _ => panic!(),
    }
}

#[test]
fn json_execute_and_errors() {
    let mut s = Session::new();
    let r = s.execute("circle", &json!({"center": [1, 2], "radius": 3})).unwrap();
    assert!(r["handle"].is_string());
    assert!(s.execute("circle", &json!({"center": [1, 2]})).is_err());
    assert!(s.execute("circle", &json!("not an object")).is_err());
    assert!(s.execute("nope", &json!({})).is_err());
    // Bad params don't create undo steps.
    assert_eq!(s.state().unwrap().undo.len(), 1);
}

#[test]
fn move_with_pickfirst_and_window_selection() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [1, 0]]})).unwrap();
    s.execute("line", &json!({"points": [[5, 5], [6, 5]]})).unwrap();
    s.viewport_px = (1000.0, 1000.0);
    s.state_mut().unwrap().set_view(View { center: Vec2::new(3.0, 3.0), height: 10.0 });
    // Window selection from left to right picks only the first line.
    s.cmdline("move").unwrap();
    s.input(Input::Point(Vec2::new(-1.0, -1.0))).unwrap();
    s.input(Input::Point(Vec2::new(2.0, 1.0))).unwrap();
    s.input(Input::Enter).unwrap();
    s.cmdline("0,0").unwrap();
    s.cmdline("10,0").unwrap();
    assert!(s.running.is_none());
    let g: Vec<_> = s.doc().unwrap().model.iter().map(|e| e.kind.grips()[0]).collect();
    assert!(g[0].near(Vec2::new(10.0, 0.0), 1e-9));
    assert!(g[1].near(Vec2::new(5.0, 5.0), 1e-9));
}

#[test]
fn erase_all_and_selection_keywords() {
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    s.execute("circle", &json!({"center": [5, 0], "radius": 1})).unwrap();
    s.cmdline("erase").unwrap();
    s.cmdline("all").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 0);
}

#[test]
fn trim_line_between_edges() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    s.execute("line", &json!({"points": [[3, -2], [3, 2]]})).unwrap();
    s.execute("line", &json!({"points": [[7, -2], [7, 2]]})).unwrap();
    let h = s.doc().unwrap().model.iter().next().unwrap().handle;
    s.execute("trim", &json!({"handle": h.hex(), "pick": [5, 0]})).unwrap();
    let lines: Vec<_> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter(|e| matches!(&e.kind, EntityKind::Line(l) if l.a.y == 0.0 && l.b.y == 0.0))
        .map(|e| e.kind.grips())
        .collect();
    assert_eq!(lines.len(), 2);
}

#[test]
fn extend_line_to_boundary() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [5, 0]]})).unwrap();
    s.execute("line", &json!({"points": [[8, -2], [8, 2]]})).unwrap();
    let h = s.doc().unwrap().model.iter().next().unwrap().handle;
    s.execute("extend", &json!({"handle": h.hex(), "pick": [4.5, 0]})).unwrap();
    let g = s.doc().unwrap().entity(h).unwrap().kind.grips();
    assert!(g[2].near(Vec2::new(8.0, 0.0), 1e-9));
}

#[test]
fn properties_set_circle_size_and_dim_style() {
    // Issue #6: the Properties palette edits a circle's circumference/area and a dimension's style.
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    let c = s.doc().unwrap().model.last().unwrap().handle;
    let radius = |s: &Session| match &s.doc().unwrap().entity(c).unwrap().kind {
        EntityKind::Circle(c) => c.radius,
        _ => f64::NAN,
    };
    s.execute("properties.set", &json!({"handles": [c.hex()], "circumference": 4.0 * std::f64::consts::PI})).unwrap();
    assert!((radius(&s) - 2.0).abs() < 1e-12);
    s.execute("properties.set", &json!({"handles": [c.hex()], "area": 9.0 * std::f64::consts::PI})).unwrap();
    assert!((radius(&s) - 3.0).abs() < 1e-12);
    // Zero, negative or non-numeric sizes are ignored.
    for v in [json!(0), json!(-5), json!("NaN"), json!(null)] {
        s.execute("properties.set", &json!({"handles": [c.hex()], "circumference": v, "area": v})).unwrap();
        assert!((radius(&s) - 3.0).abs() < 1e-12, "{v}");
    }
    s.execute("properties.set", &json!({"handles": [c.hex()], "area": 1e308})).unwrap();
    assert!(radius(&s).is_finite());

    s.execute("dimstyle", &json!({"name": "Big", "current": false})).unwrap();
    s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [3, 8], "at": [6, 4]})).unwrap();
    let dm = s.doc().unwrap().model.last().unwrap().handle;
    let style = |s: &Session| match &s.doc().unwrap().entity(dm).unwrap().kind {
        EntityKind::Dimension(d) => d.style.clone(),
        _ => String::new(),
    };
    s.execute("properties.set", &json!({"handles": [dm.hex()], "dimStyle": "big"})).unwrap();
    assert_eq!(style(&s), "Big");
    assert!(s.execute("properties.set", &json!({"handles": [dm.hex()], "dimStyle": "Nope"})).is_err());
    assert_eq!(style(&s), "Big");
}

#[test]
fn properties_palette_current_property_commands() {
    // Issue #6: the commands the Properties palette runs with nothing selected.
    let mut s = Session::new();
    s.execute("linetype", &json!({"current": "ByBlock"})).unwrap();
    s.execute("lweight", &json!({"lineweight": 0.5})).unwrap();
    s.execute("setvar", &json!({"name": "THICKNESS", "value": 2.5})).unwrap();
    s.execute("dimstyle", &json!({"name": "Big", "current": false})).unwrap();
    s.execute("dimstyle.current", &json!({"name": "Big"})).unwrap();
    s.execute("mleaderstyle", &json!({"name": "Callout", "current": false})).unwrap();
    s.execute("mleaderstyle", &json!({"name": "Callout", "current": true})).unwrap();
    s.execute("tablestyle", &json!({"name": "Schedule", "current": false})).unwrap();
    s.execute("tablestyle", &json!({"name": "Schedule", "current": true})).unwrap();
    s.execute("style", &json!({"name": "Notes", "current": false})).unwrap();
    s.execute("style.current", &json!({"name": "Notes"})).unwrap();
    let h = &s.doc().unwrap().header;
    assert_eq!(h.str("CELTYPE", ""), "ByBlock");
    assert_eq!(h.i64("CELWEIGHT", 0), 50);
    assert_eq!(h.f64("THICKNESS", 0.0), 2.5);
    assert_eq!(h.str("DIMSTYLE", ""), "Big");
    assert_eq!(h.str("CMLEADERSTYLE", ""), "Callout");
    assert_eq!(h.str("CTABLESTYLE", ""), "Schedule");
    assert_eq!(h.str("TEXTSTYLE", ""), "Notes");
}

#[test]
fn pickadd_controls_idle_picks() {
    // Issue #6: the palette's PICKADD toggle must change how clicks select.
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let a = s.doc().unwrap().model.last().unwrap().handle;
    s.execute("line", &json!({"points": [[0, 5], [10, 5]]})).unwrap();
    let b = s.doc().unwrap().model.last().unwrap().handle;
    let (pa, pb) = (Vec2::new(5.0, 0.0), Vec2::new(5.0, 5.0));
    // PICKADD on: picks add, Shift removes.
    s.idle_click(pa, false).unwrap();
    s.idle_click(pb, false).unwrap();
    assert_eq!(s.selection(), vec![a, b]);
    s.idle_click(pa, true).unwrap();
    assert_eq!(s.selection(), vec![b]);
    // PICKADD off: each pick replaces, Shift adds, Shift on a selected object removes it.
    s.execute("setvar", &json!({"name": "PICKADD", "value": 0})).unwrap();
    s.idle_click(pa, false).unwrap();
    assert_eq!(s.selection(), vec![a]);
    s.idle_click(pb, true).unwrap();
    assert_eq!(s.selection(), vec![a, b]);
    s.idle_click(pa, true).unwrap();
    assert_eq!(s.selection(), vec![b]);
    s.idle_click(pa, false).unwrap();
    assert_eq!(s.selection(), vec![a]);
    // A window replaces the selection too.
    s.idle_click(Vec2::new(-1.0, 4.0), false).unwrap();
    s.idle_click(Vec2::new(11.0, 6.0), false).unwrap();
    assert_eq!(s.selection(), vec![b]);
}

#[test]
fn fillet_two_lines() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [0, 10]]})).unwrap();
    let hs: Vec<_> = s.doc().unwrap().model.handles();
    let r = s.execute("fillet", &json!({"h1": hs[0].hex(), "p1": [5, 0], "h2": hs[1].hex(), "p2": [0, 5], "radius": 2})).unwrap();
    assert!(r["arc"].is_string());
    let g = s.doc().unwrap().entity(hs[0]).unwrap().kind.grips();
    assert!(g[0].near(Vec2::new(10.0, 0.0), 1e-9) || g[2].near(Vec2::new(10.0, 0.0), 1e-9));
    assert!(g.iter().any(|p| p.near(Vec2::new(2.0, 0.0), 1e-9)));
}

#[test]
fn offset_circle_and_polyline() {
    let mut s = Session::new();
    let c = s.execute("circle", &json!({"center": [0, 0], "radius": 5})).unwrap();
    s.execute("offset", &json!({"handle": c["handle"], "distance": 1, "side": [0, 0]})).unwrap();
    let r = s.execute("rectang", &json!({"p1": [0, 0], "p2": [10, 10]})).unwrap();
    s.execute("offset", &json!({"handle": r["handle"], "distance": 1, "side": [5, 5]})).unwrap();
    let d = s.doc().unwrap();
    let last = d.model.last().unwrap();
    match &last.kind {
        EntityKind::LwPolyline(p) => {
            let b = cadcraft_geom::Bounds2::from_points(p.vertices.iter().map(|v| v.p));
            assert!(b.min.near(Vec2::new(1.0, 1.0), 1e-9), "{b:?}");
            assert!(b.max.near(Vec2::new(9.0, 9.0), 1e-9));
        }
        _ => panic!(),
    }
}

#[test]
fn copy_rotate_scale_mirror() {
    let mut s = Session::new();
    let l = s.execute("line", &json!({"points": [[0, 0], [1, 0]]})).unwrap();
    let h = l["handles"][0].clone();
    s.execute("copy", &json!({"handles": [h], "delta": [0, 2], "count": 3})).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 4);
    s.execute("rotate", &json!({"handles": [h], "base": [0, 0], "angle": 90})).unwrap();
    s.execute("scale", &json!({"handles": [h], "base": [0, 0], "factor": 3})).unwrap();
    let hh = cadcraft_doc::Handle::parse_hex(h.as_str().unwrap()).unwrap();
    let g = s.doc().unwrap().entity(hh).unwrap().kind.grips();
    assert!(g[2].near(Vec2::new(0.0, 3.0), 1e-9));
    s.execute("mirror", &json!({"handles": [h], "p1": [-1, 0], "p2": [-1, 1]})).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 5);
}

#[test]
fn scale_reference_typed_and_picked() {
    // Issue #17: SCALE ▸ Reference scales by new length / reference length.
    fn end(s: &Session) -> Vec2 {
        let e = s.doc().unwrap().model.iter().next().unwrap();
        let EntityKind::Line(l) = &e.kind else { panic!("expected a line") };
        l.b.xy()
    }
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [56, 0]]})).unwrap();
    // Typed lengths: 56 → 100.
    for t in ["scale", "all", "", "0,0", "r"] {
        s.cmdline(t).unwrap();
    }
    assert_eq!(s.current_prompt().unwrap().message, "Specify reference length");
    s.cmdline("56").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Specify new length");
    s.cmdline("100").unwrap();
    assert!(s.running.is_none());
    assert!(end(&s).near(Vec2::new(100.0, 0.0), 1e-9));
    // Reference from two points, a zero new length is rejected, new length picked from the base point.
    for t in ["scale", "all", "", "0,0", "r", "0,0", "100,0"] {
        s.cmdline(t).unwrap();
    }
    s.cmdline("0").unwrap();
    assert!(s.running.is_some());
    s.cmdline("0,25").unwrap();
    assert!(s.running.is_none());
    assert!(end(&s).near(Vec2::new(25.0, 0.0), 1e-9));
    // A zero reference length is rejected and asked again.
    for t in ["scale", "all", "", "0,0", "r", "0"] {
        s.cmdline(t).unwrap();
    }
    assert_eq!(s.current_prompt().unwrap().message, "Specify reference length");
    s.cmdline("25").unwrap();
    s.cmdline("50").unwrap();
    assert!(end(&s).near(Vec2::new(50.0, 0.0), 1e-9));
}

#[test]
fn layers_and_properties() {
    let mut s = Session::new();
    s.execute("layer.new", &json!({"name": "Walls", "color": "red", "current": true})).unwrap();
    let c = s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    let h = cadcraft_doc::Handle::parse_hex(c["handle"].as_str().unwrap()).unwrap();
    assert_eq!(s.doc().unwrap().entity(h).unwrap().common.layer, "Walls");
    assert!(s.execute("layer.set", &json!({"name": "Walls", "frozen": true})).is_err());
    s.execute("properties.set", &json!({"handles": [h.hex()], "color": 3, "radius": 4})).unwrap();
    let e = s.doc().unwrap().entity(h).unwrap();
    assert_eq!(e.common.color, cadcraft_color::Color::Index(3));
    assert!(matches!(&e.kind, EntityKind::Circle(c) if c.radius == 4.0));
    assert!(s.execute("layer.new", &json!({"name": "bad/name"})).is_err());
}

#[test]
fn properties_set_transparency() {
    use cadcraft_doc::Transparency;
    // Issue #132: `properties.set` used to ignore `transparency`.
    let mut s = Session::new();
    let r = s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let h = r["handles"][0].as_str().unwrap().to_string();
    let hh = cadcraft_doc::Handle::parse_hex(&h).unwrap();
    let tr = |s: &Session| s.doc().unwrap().entity(hh).unwrap().common.transparency;
    for (v, want) in [
        (json!(50), Transparency::Percent(50)),
        (json!("ByBlock"), Transparency::ByBlock),
        (json!("25%"), Transparency::Percent(25)),
        (json!(0), Transparency::Percent(0)),
        (json!("bylayer"), Transparency::ByLayer),
        (json!(90), Transparency::Percent(90)),
    ] {
        s.execute("properties.set", &json!({"handles": [h], "transparency": v})).unwrap();
        assert_eq!(tr(&s), want, "{v}");
    }
    // Out of range or not a transparency: an error, and the entity is unchanged.
    for v in [json!(91), json!(-1), json!("NaN"), json!("inf"), json!("half"), json!(null), json!([50])] {
        assert!(s.execute("properties.set", &json!({"handles": [h], "transparency": v})).is_err(), "{v}");
        assert_eq!(tr(&s), Transparency::Percent(90), "{v}");
    }
    // `properties` and `drawing.inspect` report it in the form `properties.set` takes.
    let p = s.execute("properties", &json!({"handles": [h]})).unwrap();
    assert_eq!(p["objects"][0]["transparency"], json!(90));
    s.execute("properties.set", &json!({"handles": [h], "transparency": "ByBlock"})).unwrap();
    let i = s.execute("drawing.inspect", &json!({"entities": true})).unwrap();
    assert_eq!(i["entities"][0]["transparency"], json!("ByBlock"));
}

#[test]
fn zoom_extents_fits_drawing() {
    let mut s = Session::new();
    s.viewport_px = (1000.0, 500.0);
    s.execute("line", &json!({"points": [[0, 0], [100, 10]]})).unwrap();
    s.execute("zoom.extents", &json!({})).unwrap();
    let v = s.state().unwrap().view();
    assert!((v.center.x - 50.0).abs() < 1e-9);
    assert!(v.height >= 50.0);
}

#[test]
fn sysvars_roundtrip() {
    let mut s = Session::new();
    s.execute("setvar", &json!({"name": "osmode", "value": 7})).unwrap();
    assert_eq!(s.settings.osmode, 7);
    s.execute("setvar", &json!({"name": "LTSCALE", "value": 2.5})).unwrap();
    assert_eq!(s.doc().unwrap().header.f64("LTSCALE", 0.0), 2.5);
    assert!(s.execute("setvar", &json!({"name": "ORTHOMODE", "value": "x"})).is_err());
    assert_eq!(sysvars::get(&s, "orthomode"), Some(json!(0)));
    // FONTALT is a process-wide font setting, not a drawing header variable.
    s.execute("setvar", &json!({"name": "FONTALT", "value": "no-such-font-130"})).unwrap();
    assert_eq!(sysvars::get(&s, "fontalt"), Some(json!("no-such-font-130")));
    assert!(s.doc().unwrap().header.get("FONTALT").is_none());
    assert!(s.execute("setvar", &json!({"name": "FONTALT", "value": 3})).is_err());
    s.execute("setvar", &json!({"name": "FONTALT", "value": "."})).unwrap();
    assert_eq!(sysvars::get(&s, "FONTALT"), Some(json!("")));
}

#[test]
fn setvar_at_command_line() {
    let mut s = Session::new();
    // One line, as typed in issue #30: Space separates the inputs.
    s.cmdline("SETVAR LUNITS 4").unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.doc().unwrap().header.i64("LUNITS", 0), 4);
    // Step by step, offering the current value as the default.
    s.cmdline("setvar").unwrap();
    s.cmdline("luprec").unwrap();
    assert_eq!(s.current_prompt().unwrap().default.as_deref(), Some("4"));
    s.cmdline("2").unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.doc().unwrap().header.i64("LUPREC", 0), 2);
    // Enter keeps the value.
    s.cmdline("set osmode").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    // A value of the wrong type re-prompts and changes nothing.
    s.cmdline("SETVAR LUNITS abc").unwrap();
    assert!(s.running.is_some());
    assert_eq!(s.doc().unwrap().header.i64("LUNITS", 0), 4);
    s.cancel();
    // Unknown and read-only names end the command without creating or changing anything.
    s.cmdline("SETVAR NOSUCHVAR 1").unwrap();
    assert!(sysvars::get(&s, "NOSUCHVAR").is_none());
    s.cmdline("SETVAR DWGNAME").unwrap();
    assert!(s.running.is_none());
    assert!(s.execute("setvar", &json!({"name": "dbmod", "value": 0})).is_err());
    // `?` lists the variables.
    s.cmdline("SETVAR ?").unwrap();
    assert!(s.running.is_none());
}

#[test]
fn setvar_keeps_header_types() {
    let mut s = Session::new();
    assert!(s.execute("setvar", &json!({"name": "LUNITS", "value": "x"})).is_err());
    assert!(s.execute("setvar", &json!({"name": "LUNITS", "value": 2.5})).is_err());
    s.execute("setvar", &json!({"name": "LUNITS", "value": 3.0})).unwrap();
    assert_eq!(s.doc().unwrap().header.get("LUNITS"), Some(&cadcraft_doc::HVal::Int(3)));
    assert!(s.execute("setvar", &json!({"name": "LTSCALE", "value": "big"})).is_err());
    assert_eq!(s.doc().unwrap().header.f64("LTSCALE", 0.0), 1.0);
}

#[test]
fn dynamic_input_pointer_settings() {
    let mut s = Session::new();
    assert_eq!(sysvars::get(&s, "DYNPIFORMAT"), Some(json!(0)));
    assert_eq!(sysvars::get(&s, "dynpicoords"), Some(json!(0)));
    s.execute("setvar", &json!({"name": "dynpiformat", "value": 1})).unwrap();
    s.execute("setvar", &json!({"name": "DYNPICOORDS", "value": 1})).unwrap();
    assert!(s.settings.dynpi_cartesian && s.settings.dynpi_absolute);
    assert!(s.execute("setvar", &json!({"name": "DYNPIFORMAT", "value": "x"})).is_err());
}

#[test]
fn script_runs_commands() {
    let mut s = Session::new();
    s.script("LINE 0,0 10,0 10,10\n\nCIRCLE 5,5 2\nTEXT 0,-2 0.5 0 Hello world\n").unwrap();
    assert_eq!(kinds(&s), vec!["Line", "Line", "Circle", "Text"]);
    let t = s.doc().unwrap().model.last().unwrap();
    assert!(matches!(&t.kind, EntityKind::Text(t) if t.value == "Hello world"));
}

#[test]
fn script_splits_line_at_keyword_prompt() {
    let mut s = Session::new();
    s.script("POLYGON 6 200,20 I 15").unwrap();
    assert_eq!(kinds(&s), vec!["Polyline"]);
}

#[test]
fn enter_repeats_last_command() {
    let mut s = Session::new();
    s.cmdline("circle 0,0 1").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("circle"));
    s.cmdline("5,5 1").unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 2);
}

#[test]
fn cancel_keeps_partial_line_as_one_undo() {
    let mut s = Session::new();
    s.cmdline("line 0,0 1,0 2,0").unwrap();
    s.cancel();
    assert_eq!(s.doc().unwrap().model.len(), 2);
    s.undo().unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 0);
}

#[test]
fn hostile_params_never_panic() {
    let mut s = Session::new();
    for c in command_specs() {
        for p in [
            json!(null),
            json!([]),
            json!("x"),
            json!({"points": [[1e308, -1e308], ["a"]]}),
            json!({"handles": ["zz", 99999]}),
            json!({"radius": -1, "center": [0, 0]}),
        ] {
            let _ = s.execute(c.id, &p);
        }
    }
}

#[test]
fn every_interactive_command_starts_and_cancels() {
    for c in command_specs().iter().filter(|c| c.interactive.is_some()) {
        let mut s = Session::new();
        s.start(c.id).unwrap();
        let _ = s.prompt_text();
        let _ = s.preview(Vec2::new(1.0, 1.0));
        s.input(Input::Point(Vec2::new(1.0, 2.0))).unwrap();
        let _ = s.preview(Vec2::new(3.0, 1.0));
        s.cancel();
        assert!(s.running.is_none(), "{} still running", c.id);
    }
}

#[test]
fn sample_drawing_builds() {
    let d = sample::bracket();
    assert!(d.model.len() > 30);
    let l = cadcraft_render::build(&d, &cadcraft_doc::Space::Model, &cadcraft_render::Options::default());
    assert!(l.prims.len() > 100);
}

#[test]
fn count_from_the_menu_shows_its_result() {
    let mut s = Session::new();
    s.start("count").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("No block references in model space."));
    s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    s.execute("selectall", &json!({})).unwrap();
    s.cmdline("block Valve 0,0").unwrap();
    s.cmdline("insert Valve 10,0 1 1 0").unwrap();
    s.start("count").unwrap();
    let n = s.log.len();
    assert_eq!(&s.log[n - 3..], ["Block references in model space:", "  Valve: 2", "  Total: 2"]);
    let r = s.execute("count", &json!({"block": "Valve"})).unwrap();
    assert_eq!((r["count"].as_u64(), r["message"].as_str()), (Some(2), Some("Block Valve: 2 in model space.")));
}

#[test]
fn osnap_toggle() {
    let mut s = Session::new();
    s.execute("osnap", &json!({"modes": ["end", "mid", "cen"]})).unwrap();
    assert_eq!(s.settings.osmode, 7);
    s.execute("osnap", &json!({"on": false})).unwrap();
    assert!(s.settings.osmode & snap::mode::OFF != 0);
}

#[test]
fn dimlinear_interactive_and_continue() {
    let mut s = Session::new();
    s.cmdline("dimlinear 0,0 10,0 5,2").unwrap();
    assert!(s.running.is_none());
    s.cmdline("dimcontinue 25,0").unwrap();
    s.cmdline("").unwrap();
    let dims: Vec<_> =
        s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Dimension(d) = &e.kind { Some(d.clone()) } else { None }).collect();
    assert_eq!(dims.len(), 2);
    assert!(dims[1].p13.xy().near(Vec2::new(10.0, 0.0), 1e-9));
    assert!(dims[1].p14.xy().near(Vec2::new(25.0, 0.0), 1e-9));
    assert!(s.log.iter().any(|l| l.contains("Dimension text = 10.0000")));
}

#[test]
fn dim_auto_vertical_and_radius() {
    let mut s = Session::new();
    s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [3, 8], "at": [6, 4]})).unwrap();
    let c = s.execute("circle", &json!({"center": [20, 0], "radius": 2})).unwrap();
    s.execute("dimradius", &json!({"handle": c["handle"], "at": [23, 1]})).unwrap();
    let d = s.doc().unwrap();
    let kinds: Vec<_> = d.model.iter().filter_map(|e| if let EntityKind::Dimension(d) = &e.kind { Some(d.kind) } else { None }).collect();
    assert!(matches!(kinds[0], cadcraft_doc::DimKind::Linear { rotation } if (rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-12));
    assert!(matches!(kinds[1], cadcraft_doc::DimKind::Radius));
}

#[test]
fn area_of_hatch_excludes_island() {
    let mut s = Session::new();
    s.execute("pline", &json!({"vertices": [[0, 0], [10, 0], [10, 10], [0, 10]], "closed": true})).unwrap();
    s.execute("pline", &json!({"vertices": [[4, 4], [6, 4], [6, 6], [4, 6]], "closed": true})).unwrap();
    s.execute("hatch", &json!({"points": [[1, 1]], "pattern": "SOLID"})).unwrap();
    let h = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::Hatch(_) = &e.kind { Some(e.handle) } else { None }).unwrap();
    let r = s.execute("area", &json!({"handle": h.hex()})).unwrap();
    assert!((r["area"].as_f64().unwrap() - 96.0).abs() < 1e-6);
    assert!((r["perimeter"].as_f64().unwrap() - 48.0).abs() < 1e-6);
    // A loop nested inside the island is filled again: 100 - 36 + 4.
    let mut s = Session::new();
    s.execute("pline", &json!({"vertices": [[0, 0], [10, 0], [10, 10], [0, 10]], "closed": true})).unwrap();
    s.execute("pline", &json!({"vertices": [[2, 2], [8, 2], [8, 8], [2, 8]], "closed": true})).unwrap();
    s.execute("pline", &json!({"vertices": [[4, 4], [6, 4], [6, 6], [4, 6]], "closed": true})).unwrap();
    s.execute("hatch", &json!({"points": [[1, 1]], "pattern": "SOLID"})).unwrap();
    let h = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::Hatch(_) = &e.kind { Some(e.handle) } else { None }).unwrap();
    let r = s.execute("area", &json!({"handle": h.hex()})).unwrap();
    assert!((r["area"].as_f64().unwrap() - 68.0).abs() < 1e-6);
}

#[test]
fn hatch_by_pick_point_with_island() {
    let mut s = Session::new();
    s.execute("rectang", &json!({"p1": [0, 0], "p2": [10, 10]})).unwrap();
    s.execute("circle", &json!({"center": [5, 5], "radius": 2})).unwrap();
    s.cmdline("hatch 1,1").unwrap();
    s.cmdline("").unwrap();
    let h = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some(h.clone()) } else { None }).unwrap();
    assert_eq!(h.loops.len(), 2, "outer boundary plus the circle island");
    // Hatches go to the back of the draw order.
    assert!(matches!(s.doc().unwrap().model.iter().next().unwrap().kind, EntityKind::Hatch(_)));
    // No boundary → error message, no hatch.
    let mut s2 = Session::new();
    s2.execute("line", &json!({"points": [[0, 0], [5, 0]]})).unwrap();
    assert!(s2.execute("hatch", &json!({"points": [[1, 1]]})).is_err());
}

#[test]
fn hatch_from_overlapping_lines() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[-1, 0], [11, 0]]})).unwrap();
    s.execute("line", &json!({"points": [[10, -1], [10, 6]]})).unwrap();
    s.execute("line", &json!({"points": [[11, 5], [-1, 5]]})).unwrap();
    s.execute("line", &json!({"points": [[0, 6], [0, -1]]})).unwrap();
    let r = s.execute("boundary", &json!({"points": [[3, 3]]})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 1);
    let area = s.execute("area", &json!({"handle": r["handles"][0]})).unwrap();
    assert!((area["area"].as_f64().unwrap() - 50.0).abs() < 1e-6);
}

/// Outer rectangle [0,0]-[20,20] with a [5,5]-[10,10] square hole drawn as four LINEs or as one
/// closed polyline (issue #106).
fn square_hole_fixture(lines: bool) -> Session {
    let mut s = Session::new();
    s.execute("rectang", &json!({"p1": [0, 0], "p2": [20, 20]})).unwrap();
    let sq = [[5, 5], [10, 5], [10, 10], [5, 10]];
    if lines {
        for i in 0..4 {
            s.execute("line", &json!({"points": [sq[i], sq[(i + 1) % 4]]})).unwrap();
        }
    } else {
        s.execute("pline", &json!({"vertices": sq, "closed": true})).unwrap();
    }
    s
}

fn only_hatch(s: &Session) -> std::sync::Arc<cadcraft_doc::Entity> {
    let hs: Vec<_> = s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Hatch(_))).cloned().collect();
    assert_eq!(hs.len(), 1);
    hs[0].clone()
}

/// The hatch stores the outer rectangle plus the square island.
fn assert_outer_and_square_island(h: &cadcraft_doc::Hatch) {
    assert_eq!(h.loops.len(), 2, "outer boundary plus the square island");
    assert!(h.loops[0].outer && !h.loops[1].outer);
    let island: Vec<Vec2> = h.loops[1].vertices.iter().map(|v| v.p).collect();
    assert!((cadcraft_geom::shoelace(&island).abs() - 25.0).abs() < 1e-9);
    let b = cadcraft_geom::Bounds2::from_points(island.iter().copied());
    assert!(b.min.near(Vec2::new(5.0, 5.0), 1e-9) && b.max.near(Vec2::new(10.0, 10.0), 1e-9));
}

fn inside_open_hole(q: Vec2) -> bool {
    q.x > 5.0 + 1e-6 && q.x < 10.0 - 1e-6 && q.y > 5.0 + 1e-6 && q.y < 10.0 - 1e-6
}

#[test]
fn hatch_pick_point_keeps_island_of_separate_lines() {
    for lines in [true, false] {
        let mut s = square_hole_fixture(lines);
        s.execute("hatch", &json!({"points": [[2, 2]]})).unwrap();
        let e = only_hatch(&s);
        let EntityKind::Hatch(h) = &e.kind else { panic!("not a hatch") };
        assert_outer_and_square_island(h);
        // The rendered pattern never enters the hole.
        let d = s.doc().unwrap();
        let list = cadcraft_render::build_entities(d, std::iter::once(e.as_ref()), &cadcraft_render::Options::default());
        assert!(list.segment_count() > 0);
        for p in &list.prims {
            for w in list.points(p).windows(2) {
                for k in 0..=16 {
                    let q = w[0] + (w[1] - w[0]) * (k as f64 / 16.0);
                    assert!(!inside_open_hole(q), "pattern line through the hole at {q:?} (lines: {lines})");
                }
            }
        }
    }
    // The interactive form finds the same island.
    let mut s = square_hole_fixture(true);
    s.cmdline("hatch 2,2").unwrap();
    s.cmdline("").unwrap();
    let e = only_hatch(&s);
    let EntityKind::Hatch(h) = &e.kind else { panic!("not a hatch") };
    assert_outer_and_square_island(h);
}

#[test]
fn gradient_pick_point_keeps_island_of_separate_lines() {
    for lines in [true, false] {
        let mut s = square_hole_fixture(lines);
        s.execute("gradient", &json!({"points": [[2, 2]]})).unwrap();
        let e = only_hatch(&s);
        let EntityKind::Hatch(h) = &e.kind else { panic!("not a hatch") };
        assert_outer_and_square_island(h);
        let loops: Vec<Vec<Vec2>> = h.loops.iter().map(|l| l.vertices.iter().map(|v| v.p).collect()).collect();
        let tris = cadcraft_render::triangulate_evenodd(&loops);
        let area: f64 = tris.chunks(3).map(|t| cadcraft_geom::shoelace(t).abs()).sum();
        assert!((area - 375.0).abs() < 1e-6, "filled area {area} (lines: {lines})");
        let sample = Vec2::new(7.25, 7.75);
        assert!(!tris.chunks(3).any(|t| cadcraft_geom::point_in_polygon(t, sample)), "hole filled (lines: {lines})");
    }
}

#[test]
fn boundary_pick_point_keeps_island_of_separate_lines() {
    for lines in [true, false] {
        let mut s = square_hole_fixture(lines);
        let r = s.execute("boundary", &json!({"points": [[2, 2]]})).unwrap();
        let hs = r["handles"].as_array().unwrap();
        assert_eq!(hs.len(), 2, "outer and island polylines (lines: {lines})");
        let mut areas: Vec<f64> = hs.iter().map(|h| s.execute("area", &json!({"handle": h})).unwrap()["area"].as_f64().unwrap()).collect();
        areas.sort_by(f64::total_cmp);
        assert!((areas[0] - 25.0).abs() < 1e-6 && (areas[1] - 400.0).abs() < 1e-6, "{areas:?}");
    }
    // Four separate outer lines and no hole: still exactly one boundary.
    let mut s = Session::new();
    let sq = [[0, 0], [20, 0], [20, 20], [0, 20]];
    for i in 0..4 {
        s.execute("line", &json!({"points": [sq[i], sq[(i + 1) % 4]]})).unwrap();
    }
    let r = s.execute("boundary", &json!({"points": [[2, 2]]})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 1);
    // Picking inside the line square bounds the square itself, with no island.
    let mut s = square_hole_fixture(true);
    let r = s.execute("boundary", &json!({"points": [[7, 7]]})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 1);
    let area = s.execute("area", &json!({"handle": r["handles"][0]})).unwrap();
    assert!((area["area"].as_f64().unwrap() - 25.0).abs() < 1e-6);
}

#[test]
fn block_insert_with_attributes() {
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    s.execute("attdef", &json!({"tag": "TAG", "prompt": "Tag?", "default": "A1", "at": [1.2, 0]})).unwrap();
    s.execute("selectall", &json!({})).unwrap();
    s.cmdline("block").unwrap();
    s.cmdline("Valve").unwrap();
    s.cmdline("0,0").unwrap(); // pickfirst selection used
    assert!(s.running.is_none());
    assert!(s.doc().unwrap().block("Valve").is_some());
    assert_eq!(s.doc().unwrap().model.len(), 1, "originals converted to one insert");
    s.cmdline("insert Valve 10,0 2 2 90 V-101").unwrap();
    let ins: Vec<_> =
        s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.clone()) } else { None }).collect();
    assert_eq!(ins.len(), 2);
    assert_eq!(ins[1].attribs[0].text.value, "V-101");
    assert!((ins[1].scale.x - 2.0).abs() < 1e-12);
    // Explode the scaled insert back into geometry.
    let h = s.doc().unwrap().model.last().unwrap().handle;
    s.execute("explode", &json!({"handles": [h.hex()]})).unwrap();
    assert!(s.doc().unwrap().model.iter().any(|e| matches!(&e.kind, EntityKind::Circle(c) if (c.radius - 2.0).abs() < 1e-9)));
    // PURGE keeps the used block.
    s.execute("purge", &json!({})).unwrap();
    assert!(s.doc().unwrap().block("Valve").is_some());
}

#[test]
fn mleader_and_qdim() {
    let mut s = Session::new();
    s.cmdline("mleader 0,0 3,2").unwrap();
    s.cmdline("NOTE A").unwrap();
    assert!(
        s.doc().unwrap().model.iter().any(|e| matches!(&e.kind, EntityKind::MLeader(m) if m.text.as_ref().is_some_and(|t| t.contents == "NOTE A")))
    );
    s.execute("line", &json!({"points": [[0, 0], [4, 0], [9, 0]]})).unwrap();
    let hs: Vec<String> = s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).map(|e| e.handle.hex()).collect();
    let r = s.execute("qdim", &json!({"handles": hs, "at": [0, -2]})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 2);
}

/// Click at `at` the way the canvas does: object snap for the current prompt, then the input.
fn snap_click(s: &mut Session, at: Vec2) {
    let prompt = s.current_prompt().unwrap();
    let hit = snap::osnap(s.doc().unwrap(), &cadcraft_doc::Space::Model, at, 0.5, s.settings.osmode, prompt.base, prompt.deferred);
    s.input(hit.map_or(Input::Point(at), |h| h.input())).unwrap();
}

fn line_ends(s: &Session) -> Vec<(Vec2, Vec2)> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Line(l) => Some((l.a.xy(), l.b.xy())),
            _ => None,
        })
        .collect()
}

/// Distance from `c` to the infinite line through `p` and `q`.
fn line_dist(c: Vec2, p: Vec2, q: Vec2) -> f64 {
    ((q - p).cross(c - p) / p.dist(q)).abs()
}

#[test]
fn line_first_point_deferred_tangent() {
    let mut s = Session::new();
    s.cmdline("circle 0,0 5").unwrap();
    s.execute("osnap", &json!({"modes": ["tan"]})).unwrap();
    s.cmdline("line").unwrap();
    assert!(s.current_prompt().unwrap().deferred);
    snap_click(&mut s, Vec2::new(0.1, 5.2)); // the top of the circle: deferred tangent
    let p = s.current_prompt().unwrap();
    assert_eq!(p.message, "Specify next point");
    assert!(p.base.is_none() && p.deferred);
    // The rubber band already runs tangent from the circle to the cursor.
    let pv = s.preview(Vec2::new(20.0, 5.0));
    assert_eq!(pv.len(), 1);
    snap_click(&mut s, Vec2::new(20.0, 5.0)); // a free point
    let ends = line_ends(&s);
    assert_eq!(ends.len(), 1);
    let (a, b) = ends[0];
    assert!(b.near(Vec2::new(20.0, 5.0), 1e-9));
    assert!((a.len() - 5.0).abs() < 1e-9 && (line_dist(Vec2::ZERO, a, b) - 5.0).abs() < 1e-9 && a.y > 0.0);
    // The line goes on from the second point as usual.
    let p = s.current_prompt().unwrap();
    assert_eq!(p.base, Some(b));
    assert!(!p.deferred);
    s.cmdline("@0,5").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(line_ends(&s).len(), 2);
    assert!(line_ends(&s)[1].0.near(b, 1e-12));
}

#[test]
fn line_belt_tangent_to_tangent() {
    let mut s = Session::new();
    s.cmdline("circle 0,0 10").unwrap();
    s.cmdline("circle 30,0 4").unwrap();
    s.execute("osnap", &json!({"modes": ["tan"]})).unwrap();
    // Top of the large circle to the top of the small one: the outer (belt) tangent.
    s.cmdline("line").unwrap();
    snap_click(&mut s, Vec2::new(-1.0, 10.1));
    // Hovering the small circle previews the line the click will draw.
    let hover = snap::osnap(s.doc().unwrap(), &cadcraft_doc::Space::Model, Vec2::new(31.0, 4.1), 0.5, s.settings.osmode, None, true).unwrap();
    s.cursor_deferred = hover.deferred;
    let pv = s.preview(hover.point);
    s.cursor_deferred = None;
    snap_click(&mut s, Vec2::new(31.0, 4.1));
    s.cmdline("").unwrap();
    assert_eq!(pv.len(), 1);
    assert_eq!(Some(&pv[0].kind), s.doc().unwrap().model.iter().last().map(|e| &e.kind));
    // Bottom of the large one to the top of the small one: the crossed tangent.
    s.cmdline("line").unwrap();
    snap_click(&mut s, Vec2::new(0.0, -10.2));
    snap_click(&mut s, Vec2::new(30.0, 4.2));
    s.cmdline("").unwrap();
    let ends = line_ends(&s);
    assert_eq!(ends.len(), 2);
    for (a, b) in &ends {
        assert!((a.dist(Vec2::ZERO) - 10.0).abs() < 1e-9 && (b.dist(Vec2::new(30.0, 0.0)) - 4.0).abs() < 1e-9);
        assert!((line_dist(Vec2::ZERO, *a, *b) - 10.0).abs() < 1e-9);
        assert!((line_dist(Vec2::new(30.0, 0.0), *a, *b) - 4.0).abs() < 1e-9);
    }
    assert!(ends[0].0.y > 0.0 && ends[0].1.y > 0.0);
    assert!(ends[1].0.y < 0.0 && ends[1].1.y > 0.0);
}

#[test]
fn line_deferred_tangent_without_solution_reprompts() {
    let mut s = Session::new();
    s.cmdline("circle 0,0 10").unwrap();
    s.cmdline("circle 0,0 4").unwrap();
    s.execute("osnap", &json!({"modes": ["tan"]})).unwrap();
    s.cmdline("line").unwrap();
    snap_click(&mut s, Vec2::new(0.0, 10.1));
    // A point inside the circle: no tangent; the command reports it and keeps prompting.
    snap_click(&mut s, Vec2::new(1.0, 1.0));
    assert!(s.running.is_some() && line_ends(&s).is_empty());
    assert!(s.current_prompt().unwrap().deferred);
    // Concentric circles: no common tangent either.
    snap_click(&mut s, Vec2::new(0.0, 4.1));
    assert!(s.running.is_some() && line_ends(&s).is_empty());
    // Undo drops the deferred first point.
    s.cmdline("u").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Specify first point");
    s.cmdline("20,0 30,0").unwrap();
    assert_eq!(line_ends(&s).len(), 1);
    s.cmdline("").unwrap();
    // Enter right after a deferred first point just ends the command.
    s.cmdline("line").unwrap();
    snap_click(&mut s, Vec2::new(0.0, 10.1));
    s.cmdline("").unwrap();
    assert!(s.running.is_none() && line_ends(&s).len() == 1);
}

#[test]
fn deferred_pick_elsewhere_is_a_plain_point() {
    let mut s = Session::new();
    let curve = snap::SnapCurve::Circle { center: Vec2::ZERO, radius: 5.0 };
    let d = snap::Deferred { mode: snap::mode::TAN, curve, at: Vec2::new(0.0, 5.0) };
    // CIRCLE's center prompt doesn't resolve deferred snaps: it gets the picked point.
    s.cmdline("circle").unwrap();
    assert!(!s.current_prompt().unwrap().deferred);
    s.input(Input::Deferred(d)).unwrap();
    s.cmdline("2").unwrap();
    let c = s.doc().unwrap().model.iter().find_map(|e| match &e.kind {
        EntityKind::Circle(c) => Some(c.center.xy()),
        _ => None,
    });
    assert_eq!(c, Some(Vec2::new(0.0, 5.0)));
}

/// #85: an `arc` request that builds a zero-radius arc is refused and the drawing is unchanged.
#[test]
fn arc_refuses_zero_radius() {
    for params in [json!({"center": [0, 0], "radius": 0, "start": 0, "end": 90}), json!({"start": [0, 0], "center": [0, 0], "end": [1, 0]})] {
        let mut s = Session::new();
        let err = s.execute("arc", &params).unwrap_err().to_string();
        assert!(err.contains("do not define an arc"), "{params}: {err}");
        let info = s.execute("drawing.inspect", &json!({})).unwrap();
        assert_eq!(info["entityCount"], 0, "{params}: {info}");
        assert_eq!(info["dirty"], false, "{params}: {info}");
        assert_eq!(info["undo"], json!([]), "{params}: {info}");
    }
    // The positive-radius control still adds the quarter circle.
    let mut s = Session::new();
    s.execute("arc", &json!({"center": [0, 0], "radius": 5, "start": 0, "end": 90})).unwrap();
    match &s.doc().unwrap().model.iter().next().unwrap().kind {
        EntityKind::Arc(a) => assert_eq!((a.radius, a.start, a.end), (5.0, 0.0, std::f64::consts::FRAC_PI_2)),
        other => panic!("{other:?}"),
    }
}

/// The single polyline in a fresh drawing after typing `script`, then pressing Enter.
fn typed_pline(script: &str) -> cadcraft_doc::LwPolyline {
    let mut s = Session::new();
    for line in script.split('\n') {
        s.cmdline(line).unwrap();
    }
    s.cmdline("").unwrap();
    let d = s.doc().unwrap();
    let mut it = d.model.iter();
    let pl = match &it.next().unwrap().kind {
        EntityKind::LwPolyline(p) => p.clone(),
        other => panic!("{other:?}"),
    };
    assert!(it.next().is_none(), "one polyline");
    pl
}

/// (const_width, [(start_width, end_width) per vertex]).
fn widths(p: &cadcraft_doc::LwPolyline) -> (f64, Vec<(f64, f64)>) {
    (p.const_width, p.vertices.iter().map(|v| (v.start_width, v.end_width)).collect())
}

#[test]
fn pline_halfwidth_is_centre_to_edge() {
    // #83: half-width 0.5 at both prompts is a band 1.0 wide; Width keeps the total width.
    assert_eq!(typed_pline("pline 0,0 h 0.5 0.5 10,0").const_width, 1.0);
    assert_eq!(typed_pline("pline 0,0 w 0.5 0.5 10,0").const_width, 0.5);
    assert_eq!(typed_pline("pline 0,0 w 1 1 10,0").const_width, 1.0);
    // The ending half-width defaults to the starting one.
    assert_eq!(typed_pline("pline 0,0 h 0.5\n\n10,0").const_width, 1.0);
    // A half-width taper.
    assert_eq!(widths(&typed_pline("pline 0,0 h 0.5 0.1 10,0")), (0.0, vec![(1.0, 0.2), (0.0, 0.0)]));
}

#[test]
fn pline_width_answers_taper_the_next_segment() {
    // #104: the starting and ending widths are the segment's own, not one constant width.
    assert_eq!(widths(&typed_pline("pline 0,0 w 1 0.2 10,0")), (0.0, vec![(1.0, 0.2), (0.0, 0.0)]));
    assert_eq!(widths(&typed_pline("pline 0,0 w 0.2 1 10,0")), (0.0, vec![(0.2, 1.0), (0.0, 0.0)]));
    // Equal answers, or Enter at the ending prompt, stay one constant width.
    assert_eq!(widths(&typed_pline("pline 0,0 w 1 1 10,0")), (1.0, vec![(0.0, 0.0), (0.0, 0.0)]));
    assert_eq!(widths(&typed_pline("pline 0,0 w 1\n\n10,0")), (1.0, vec![(0.0, 0.0), (0.0, 0.0)]));
    // After a taper, later segments are uniform at the ending width.
    let p = typed_pline("pline 0,0 w 1 0.2 10,0 20,0 30,0");
    assert_eq!(widths(&p), (0.0, vec![(1.0, 0.2), (0.2, 0.2), (0.2, 0.2), (0.0, 0.0)]));
    // A width set mid-polyline applies from that segment on; the closing segment takes it too.
    let p = typed_pline("pline 0,0 10,0 w 2 2 10,10 c");
    assert!(p.closed);
    assert_eq!(widths(&p), (0.0, vec![(0.0, 0.0), (2.0, 2.0), (2.0, 2.0)]));
    // Undo drops the taper with its segment.
    let p = typed_pline("pline 0,0 10,0 w 1 0.2 20,0 u");
    assert_eq!(widths(&p), (0.0, vec![(0.0, 0.0), (0.0, 0.0)]));
}

#[test]
fn press_and_drag_selection_window() {
    let mut s = Session::new();
    s.cmdline("line 0,0 10,0").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("circle 50,50 1").unwrap();
    // No command: a drag from (-1,-1) to (11,1) is a window around the line only.
    assert!(s.begin_window(Vec2::new(-1.0, -1.0)));
    s.idle_click(Vec2::new(11.0, 1.0), false).unwrap();
    assert_eq!(s.selection().len(), 1);
    assert!(s.pending_window.is_none());
    // Dragging right to left is a crossing window: it also catches the circle it touches.
    s.set_selection(Vec::new());
    assert!(s.begin_window(Vec2::new(60.0, 60.0)));
    s.idle_click(Vec2::new(50.0, 40.0), false).unwrap();
    assert_eq!(s.selection().len(), 1);
    // A command asking for objects: the window feeds its selection.
    s.set_selection(Vec::new());
    s.cmdline("erase").unwrap();
    assert!(s.begin_window(Vec2::new(-1.0, -1.0)));
    s.input(Input::Point(Vec2::new(11.0, 1.0))).unwrap();
    s.input(Input::Enter).unwrap();
    assert_eq!(kinds(&s), vec!["Circle"]);
    // A drawing command never opens one, and neither does a hostile point.
    s.cmdline("line").unwrap();
    assert!(!s.begin_window(Vec2::ZERO));
    s.cancel();
    assert!(!s.begin_window(Vec2::new(f64::NAN, 0.0)));
    assert!(s.pending_window.is_none());
}
