//! Clipped, locked and turned-off layout viewports survive a DXF round trip (#453): VIEWPORT
//! status flags (group 90: 16384 locked, 65536 non-rectangular clipping, 131072 off), status 68
//! (0 = off) and the clip boundary entity (group 340), per the public DXF Reference.

use cadcraft_doc::*;
use cadcraft_geom::{PolyVertex, Vec2, Vec3};

/// A metric drawing with model lines and, on its first layout, an L-shaped closed polyline
/// clipping a viewport.
fn drawing(locked: bool, on: bool) -> (Drawing, Handle, Handle) {
    let mut d = Drawing::new_metric();
    for i in 0..=10 {
        let y = f64::from(i) * 10.0;
        d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(0.0, y, 0.0), b: Vec3::new(100.0, y, 0.0) })).unwrap();
    }
    let sheet = Space::Paper(d.layouts[0].name.clone());
    let l = [[10.0, 10.0], [110.0, 10.0], [110.0, 60.0], [60.0, 60.0], [60.0, 110.0], [10.0, 110.0]];
    let poly = LwPolyline {
        vertices: l.iter().map(|p| PolyVertex::new(Vec2::new(p[0], p[1]))).collect(),
        closed: true,
        const_width: 0.0,
        elevation: 0.0,
        plinegen: false,
    };
    let b = d.add(&sheet, Common::default(), EntityKind::LwPolyline(poly)).unwrap();
    let vp = Viewport {
        center: Vec3::new(60.0, 60.0, 0.0),
        width: 100.0,
        height: 100.0,
        view_center: Vec2::new(50.0, 50.0),
        view_height: 100.0,
        id: 2,
        locked,
        clip: Some(b),
        on,
        ..Viewport::default()
    };
    let v = d.add(&sheet, Common::default(), EntityKind::Viewport(vp)).unwrap();
    (d, v, b)
}

/// The DXF text as (code, value) pairs, and back.
fn groups(text: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    lines.chunks(2).filter_map(|c| Some((c.first()?.to_string(), c.get(1)?.to_string()))).collect()
}
fn text_of(gs: &[(String, String)]) -> String {
    gs.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect()
}

fn viewport(d: &Drawing, h: Handle) -> Viewport {
    match d.entity(h).map(|e| e.kind.clone()) {
        Some(EntityKind::Viewport(v)) => v,
        other => panic!("not a viewport: {other:?}"),
    }
}

#[test]
fn clip_boundary_lock_and_off_roundtrip() {
    for (locked, on) in [(false, true), (true, false), (true, true)] {
        let (d, v, b) = drawing(locked, on);
        let text = cadcraft_io::write_dxf(&d);
        let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
        let got = viewport(&back, v);
        assert_eq!(got.clip, Some(b));
        assert_eq!((got.locked, got.on), (locked, on));
        assert!(matches!(back.entity(b).map(|e| &e.kind), Some(EntityKind::LwPolyline(p)) if p.closed && p.vertices.len() == 6));
        // The flags as the DXF Reference has them.
        let flags = 32768 + 65536 + if locked { 16384 } else { 0 } + if on { 0 } else { 131072 };
        let gs = groups(&text);
        assert!(gs.contains(&("90".into(), flags.to_string())), "flags {flags}");
        assert!(gs.contains(&("340".into(), b.hex())));
    }
}

#[test]
fn clipped_viewport_plots_through_its_outline() {
    let (d, _, _) = drawing(false, true);
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    let sheet = Space::Paper(back.layouts[0].name.clone());
    // PNG, SVG and PDF all draw from this list.
    let l = cadcraft_render::build_plot(&back, &sheet, &cadcraft_render::Options::default());
    let pts: Vec<Vec2> = l.prims.iter().filter(|p| p.handle == cadcraft_render::VIEWPORT_CONTENT).flat_map(|p| l.points(p).to_vec()).collect();
    assert!(!pts.is_empty());
    assert!(pts.iter().all(|p| !(p.x > 60.0 + 1e-6 && p.y > 60.0 + 1e-6)), "content in the notch");
    for name in ["out.svg", "out.pdf", "out.png"] {
        assert!(!cadcraft_io::write(&back, name).unwrap().is_empty(), "{name}");
    }
}

#[test]
fn hostile_clip_handles_do_not_panic() {
    let (d, v, b) = drawing(false, true);
    let gs = groups(&cadcraft_io::write_dxf(&d));
    let at = gs.iter().position(|g| *g == ("340".to_string(), b.hex())).unwrap();
    for bad in ["0", "ZZZ", "", "FFFFFFFFFFFFFFFFFFFF", "-1", "7FFFFFFFFFFFFFFF", &v.hex()] {
        let mut g = gs.clone();
        g[at].1 = bad.to_string();
        let back = cadcraft_io::read_dxf(text_of(&g).as_bytes()).unwrap();
        let got = viewport(&back, v);
        let sheet = Space::Paper(back.layouts[0].name.clone());
        // A boundary that isn't there clips nothing: the rectangle shows model space.
        let l = cadcraft_render::build(&back, &sheet, &cadcraft_render::Options::default());
        assert!(l.prims.iter().any(|p| p.handle == cadcraft_render::VIEWPORT_CONTENT), "{bad}");
        // And it writes again without a dangling 340.
        if got.clip.is_none_or(|h| back.entity(h).is_none() || h == v) {
            let again = groups(&cadcraft_io::write_dxf(&back));
            let start = again.iter().position(|g| *g == ("5".to_string(), v.hex())).unwrap();
            let record = again[start..].iter().take_while(|(c, _)| c != "0");
            assert!(!record.into_iter().any(|(c, _)| c == "340"), "{bad}");
        }
    }
    // The clipping flag without a 340 group: a rectangular viewport.
    let mut g = gs.clone();
    g.remove(at);
    assert_eq!(viewport(&cadcraft_io::read_dxf(text_of(&g).as_bytes()).unwrap(), v).clip, None);
}

#[test]
fn viewport_and_vport_grid_flags_roundtrip() {
    assert!(!Viewport::default().grid, "new viewports default to grid off");
    let (mut d, v, _) = drawing(false, true);
    let sheet = Space::Paper(d.layouts[0].name.clone());
    let v_on = d
        .add(
            &sheet,
            Common::default(),
            EntityKind::Viewport(Viewport {
                center: Vec3::new(180.0, 60.0, 0.0),
                width: 40.0,
                height: 40.0,
                view_center: Vec2::ZERO,
                view_height: 40.0,
                id: 3,
                grid: true,
                ..Viewport::default()
            }),
        )
        .unwrap();
    d.vports.push(VportConfig {
        name: "GridTile".into(),
        tiles: vec![VportTile {
            min: Vec2::ZERO,
            max: Vec2::new(1.0, 1.0),
            center: Vec2::ZERO,
            height: 100.0,
            aspect: 1.0,
            twist: 0.0,
            grid: false,
        }],
    });
    let text = cadcraft_io::write_dxf(&d);
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    assert!(!viewport(&back, v).grid);
    assert!(viewport(&back, v_on).grid);
    let tile_cfg = back.vports.iter().find(|c| c.name == "GridTile").unwrap();
    assert!(!tile_cfg.tiles[0].grid);
}

#[test]
fn viewport_misc_fields_and_status68_roundtrip_dxf_and_dwg() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(100.0, 50.0, 20.0) })).unwrap();
    let sheet = Space::Paper(d.layouts[0].name.clone());
    let vp_in = Viewport {
        center: Vec3::new(90.0, 70.0, 2.5),
        width: 80.0,
        height: 50.0,
        view_center: Vec2::new(50.0, 25.0),
        view_height: 100.0,
        id: 2,
        locked: true,
        on: false,
        twist: 0.35,
        camera: cadcraft_geom::Camera3D::preset("se_isometric").unwrap(),
        target_z: 10.0,
        grid: true,
        ucs_per_viewport: false,
        shade_plot: 3,
        render_mode: 2,
        ..Viewport::default()
    };
    let h = d.add(&sheet, Common::default(), EntityKind::Viewport(vp_in.clone())).unwrap();

    // R2018 DXF roundtrip preserves 71, 170, 281, on=false, locked, grid, 3D camera.
    let dxf_bytes = cadcraft_io::write_with(
        &d,
        "vp_misc.dxf",
        &cadcraft_io::SaveOptions { version: Some(cadcraft_io::Version::R2018), binary: false },
    )
    .unwrap();
    let from_dxf = cadcraft_io::read(&dxf_bytes, "vp_misc.dxf").unwrap();
    let got_dxf = viewport(&from_dxf, h);
    assert!(!got_dxf.on);
    assert!(got_dxf.locked);
    assert!(got_dxf.grid);
    assert!(!got_dxf.ucs_per_viewport);
    assert_eq!(got_dxf.shade_plot, 3);
    assert_eq!(got_dxf.render_mode, 2);

    // DWG roundtrip preserves the same flags & misc fields.
    let dwg_bytes = cadcraft_io::write_with(
        &d,
        "vp_misc.dwg",
        &cadcraft_io::SaveOptions { version: Some(cadcraft_io::Version::R2018), binary: false },
    )
    .unwrap();
    let from_dwg = cadcraft_io::read(&dwg_bytes, "vp_misc.dwg").unwrap();
    let got_dwg = match from_dwg.layouts[0]
        .entities
        .iter()
        .find_map(|e| if let EntityKind::Viewport(v) = &e.kind { if v.id != 1 { Some(v) } else { None } } else { None })
    {
        Some(v) => v.clone(),
        None => panic!("missing viewport after DWG roundtrip"),
    };
    assert!(!got_dwg.on, "on=false must survive DWG roundtrip");
    assert!(got_dwg.locked);
    assert!(got_dwg.grid);
    assert!(!got_dwg.ucs_per_viewport, "ucs_per_viewport=false must survive DWG roundtrip");
    assert_eq!(got_dwg.shade_plot, 3, "shade_plot must survive DWG roundtrip");
    assert_eq!(got_dwg.render_mode, 2, "render_mode must survive DWG roundtrip");

    // Foreign DXF marking a viewport off via group 68 == 0 without bit 131072 in group 90,
    // and malformed non-positive size fallback.
    let dxf_str = String::from_utf8(dxf_bytes).unwrap();
    let mut gs = groups(&dxf_str);
    for pair in &mut gs {
        if pair.0 == "90" && pair.1.parse::<i64>().is_ok_and(|f| f & 131072 != 0) {
            let f = pair.1.parse::<i64>().unwrap() & !131072;
            pair.1 = f.to_string();
        }
        if pair.0 == "45" && pair.1 == "100" {
            pair.1 = "0".to_string();
        }
    }
    let from_68 = cadcraft_io::read_dxf(text_of(&gs).as_bytes()).unwrap();
    let got_68 = viewport(&from_68, h);
    assert!(!got_68.on, "group 68 == 0 must mark viewport off even without bit 0x20000");
    assert!(got_68.view_height > 0.0 && got_68.scale().is_finite(), "non-positive view_height falls back safely");
}
