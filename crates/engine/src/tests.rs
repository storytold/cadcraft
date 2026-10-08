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
fn line_undo_option_and_aliases() {
    let mut s = Session::new();
    s.cmdline("l 0,0 5,5 10,0 u").unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 1);
    s.cmdline("").unwrap(); // Enter ends
    assert!(s.running.is_none());
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
    s.cmdline("insert Valve 10,0 2 90 V-101").unwrap();
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
