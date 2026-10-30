//! A twisted view's centre is stored in display coordinates, turned by the twist: VIEWPORT and
//! VPORT group 12 and VIEW group 10. The reader turns it back to the drawing point at the middle
//! of the view and the writer turns it again, so a twisted view far from the origin (a site plan
//! on a national grid) shows its own part of the drawing.

use cadcraft_doc::*;
use cadcraft_geom::{Vec2, Vec3};

/// The far site: a national-grid point.
const SITE: Vec2 = Vec2 { x: 2_600_000.0, y: 1_200_000.0 };

fn twist() -> f64 {
    (-30.0_f64).to_radians()
}

/// The display-coordinate centre another program stores for a view on `SITE` with `twist()`.
fn dcs() -> Vec2 {
    SITE.rotate(twist())
}

fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).len() < 1e-6
}

/// Every `(code, value)` pair of a DXF text.
fn pairs(dxf: &str) -> Vec<(i32, String)> {
    let lines: Vec<&str> = dxf.lines().collect();
    lines.chunks(2).filter_map(|p| Some((p.first()?.trim().parse().ok()?, p.get(1)?.trim().to_string()))).collect()
}

/// Groups `x`/`x + 10` of each record of type `kind` that has group `has` (its twist).
fn centres(dxf: &str, kind: &str, x: i32, has: i32) -> Vec<Vec2> {
    let mut out = Vec::new();
    let mut rec: Option<(Option<f64>, Option<f64>, bool)> = None;
    for (c, v) in pairs(dxf).into_iter().chain(std::iter::once((0, String::new()))) {
        if c == 0 {
            if let Some((Some(px), Some(py), true)) = rec.take() {
                out.push(Vec2::new(px, py));
            }
            rec = (v == kind).then_some((None, None, false));
        } else if let Some(r) = rec.as_mut() {
            if c == x {
                r.0 = v.parse().ok();
            } else if c == x + 10 {
                r.1 = v.parse().ok();
            } else if c == has && v.parse::<f64>().is_ok_and(|a| a != 0.0) {
                r.2 = true;
            }
        }
    }
    out
}

fn viewport(view_center: Vec2) -> EntityKind {
    EntityKind::Viewport(Viewport {
        center: Vec3::new(230.0, 80.0, 0.0),
        width: 420.0,
        height: 120.0,
        view_center,
        view_height: 60.0,
        id: 2,
        twist: twist().rem_euclid(std::f64::consts::TAU),
        ..Default::default()
    })
}

fn sheet_viewport(d: &Drawing) -> Viewport {
    d.layouts
        .iter()
        .flat_map(|l| l.entities.iter())
        .find_map(|e| match &e.kind {
            EntityKind::Viewport(v) if v.id == 2 => Some(v.clone()),
            _ => None,
        })
        .expect("the sheet viewport")
}

#[test]
fn a_twisted_layout_viewport_far_from_the_origin_looks_at_its_site() {
    let mut d = Drawing::new_metric();
    let sheet = Space::Paper(d.layouts[0].name.clone());
    d.add(&sheet, Common::default(), viewport(SITE)).unwrap();
    let dxf = cadcraft_io::write_dxf(&d);
    // Written as other programs write it: the centre in display coordinates.
    let written = centres(&dxf, "VIEWPORT", 12, 51);
    assert_eq!(written.len(), 1, "{written:?}");
    assert!(written.first().is_some_and(|c| close(*c, dcs())), "{written:?} vs {:?}", dcs());
    // Read back, the site is at the middle of the viewport.
    let v = sheet_viewport(&cadcraft_io::read_dxf(dxf.as_bytes()).unwrap());
    assert!(close(v.view_center, SITE), "{:?}", v.view_center);
    assert!(close(v.model_to_paper(SITE), Vec2::new(230.0, 80.0)), "{:?}", v.model_to_paper(SITE));
}

#[test]
fn twisted_model_views_and_named_views_store_display_centres() {
    let mut d = Drawing::new_metric();
    let tile = |center: Vec2, twist: f64| VportTile { min: Vec2::ZERO, max: Vec2::new(1.0, 1.0), center, height: 50.0, aspect: 1.5, twist };
    // A saved configuration of two tiles, one twisted.
    d.vports = vec![VportConfig { name: "Site".into(), tiles: vec![tile(SITE, twist()), tile(Vec2::new(5.0, 5.0), 0.0)] }];
    d.views.push(NamedView { name: "Site".into(), center: SITE, height: 50.0, width: 75.0, twist: twist(), ..Default::default() });
    let dxf = cadcraft_io::write_dxf(&d);
    let vports = centres(&dxf, "VPORT", 12, 51);
    assert_eq!(vports.len(), 1, "{vports:?}");
    assert!(vports.iter().all(|c| close(*c, dcs())), "{vports:?} vs {:?}", dcs());
    let views = centres(&dxf, "VIEW", 10, 50);
    assert_eq!(views.len(), 1, "{views:?}");
    assert!(views.iter().all(|c| close(*c, dcs())), "{views:?} vs {:?}", dcs());
    let back = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    let tiles: Vec<Vec2> = back.vports.iter().filter(|c| c.name == "Site").flat_map(|c| c.tiles.iter().map(|t| t.center)).collect();
    assert_eq!(tiles.len(), 2, "{:?}", back.vports);
    assert!(close(tiles[0], SITE) && close(tiles[1], Vec2::new(5.0, 5.0)), "{tiles:?}");
    assert!(back.views.iter().all(|v| close(v.center, SITE)), "{:?}", back.views);
}

/// `dxf` with every record of type `kind` that has a nonzero group `has` (its twist) moved to
/// view target `target` (groups `t`/`t + 10`): its centre (groups `x`/`x + 10`) is set to the
/// display-coordinate centre of `SITE` seen from that target, as another program stores it.
fn with_target(dxf: &str, kind: &str, x: i32, t: i32, has: i32, target: Vec2) -> String {
    let c = (SITE - target).rotate(twist());
    let mut records: Vec<Vec<(i32, String)>> = Vec::new();
    for (code, v) in pairs(dxf) {
        match records.last_mut() {
            Some(r) if code != 0 => r.push((code, v)),
            _ => records.push(vec![(code, v)]),
        }
    }
    let mut out = String::new();
    for mut r in records {
        let twisted = r.first().is_some_and(|(_, v)| v == kind) && r.iter().any(|(code, v)| *code == has && v.parse::<f64>().is_ok_and(|a| a != 0.0));
        if twisted {
            r.retain(|(code, _)| ![x, x + 10, t, t + 10, t + 20].contains(code));
            r.extend([
                (x, c.x.to_string()),
                (x + 10, c.y.to_string()),
                (t, target.x.to_string()),
                (t + 10, target.y.to_string()),
                (t + 20, "0".into()),
            ]);
        }
        for (code, v) in r {
            out.push_str(&format!("{code}\n{v}\n"));
        }
    }
    out
}

#[test]
fn display_centres_are_measured_from_the_view_target() {
    let target = Vec2::new(2_599_000.0, 1_199_500.0);
    // A layout viewport.
    let mut d = Drawing::new_metric();
    let sheet = Space::Paper(d.layouts[0].name.clone());
    d.add(&sheet, Common::default(), viewport(SITE)).unwrap();
    let dxf = with_target(&cadcraft_io::write_dxf(&d), "VIEWPORT", 12, 17, 51, target);
    let v = sheet_viewport(&cadcraft_io::read_dxf(dxf.as_bytes()).unwrap());
    assert!(close(v.view_center, SITE), "{:?}", v.view_center);
    // A model view tile and a named view.
    let mut d = Drawing::new_metric();
    let tile = VportTile { min: Vec2::ZERO, max: Vec2::new(1.0, 1.0), center: SITE, height: 50.0, aspect: 1.5, twist: twist() };
    d.vports = vec![VportConfig { name: "Site".into(), tiles: vec![tile, VportTile { center: Vec2::new(5.0, 5.0), twist: 0.0, ..tile }] }];
    d.views.push(NamedView { name: "Site".into(), center: SITE, height: 50.0, width: 75.0, twist: twist(), ..Default::default() });
    let dxf = with_target(&cadcraft_io::write_dxf(&d), "VPORT", 12, 17, 51, target);
    let dxf = with_target(&dxf, "VIEW", 10, 12, 50, target);
    let back = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    let tiles: Vec<Vec2> = back.vports.iter().filter(|c| c.name == "Site").flat_map(|c| c.tiles.iter().map(|t| t.center)).collect();
    assert!(tiles.first().is_some_and(|c| close(*c, SITE)), "{tiles:?}");
    assert!(back.views.iter().all(|v| close(v.center, SITE)), "{:?}", back.views);
}
