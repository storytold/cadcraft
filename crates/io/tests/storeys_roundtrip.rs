//! Storeys survive saving as DXF and DWG and reopening: the storey list and the current storey
//! (the `CADCRAFT_STOREYS` XRECORD), each object's storey (its `CADCRAFT_STOREY` xdata, next to
//! other applications' xdata), never the session's ghost storey.
#![cfg(not(target_arch = "wasm32"))]

use cadcraft_doc::{Common, Drawing, EntityKind, Handle, Line, RawTag, Space, Storey, StoreyView, Storeys};
use cadcraft_geom::Vec3;

fn line(d: &mut Drawing, x: f64, storey: Option<u32>, other_app: bool) -> Handle {
    let mut c = Common::default();
    if other_app {
        c.xdata = vec![RawTag { code: 1001, value: "ACME".into() }, RawTag { code: 1000, value: "hello".into() }];
    }
    c.set_storey(storey);
    d.add(&Space::Model, c, EntityKind::Line(Line { a: Vec3::new(x, 0.0, 0.0), b: Vec3::new(x, 1.0, 0.0) })).unwrap()
}

/// Three storeys, OG1 current, EG the ghost; one object per storey, one with another
/// application's xdata too.
fn building() -> (Drawing, [Handle; 3]) {
    let mut d = Drawing::new_metric();
    d.storeys = Storeys {
        list: vec![Storey { id: 1, name: "UG".into() }, Storey { id: 2, name: "EG — Wohnen".into() }, Storey { id: 5, name: "OG1".into() }],
        current: 5,
        // EG is the start storey (number 0): it travels with the list.
        home: 2,
        ..Storeys::default()
    };
    d.storeys.sanitize();
    d.ghost_storey = Some(2);
    let a = line(&mut d, 0.0, Some(1), false);
    let b = line(&mut d, 1.0, Some(2), true);
    let c = line(&mut d, 2.0, Some(5), false);
    (d, [a, b, c])
}

/// Each line's x position and storey, in drawing order (handles may change through DWG).
fn storeys_by_position(d: &Drawing) -> Vec<(i64, Option<u32>)> {
    d.model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Line(l) => Some((l.a.x.round() as i64, e.common.storey())),
            _ => None,
        })
        .collect()
}

fn check(how: &str, r: &Drawing, d: &Drawing) {
    assert_eq!(r.storeys, d.storeys, "{how}: storey list and current storey");
    assert_eq!(r.storeys.home, 2, "{how}: the start storey");
    assert_eq!(storeys_by_position(r), vec![(0, Some(1)), (1, Some(2)), (2, Some(5))], "{how}: each object's storey");
    assert_eq!(r.ghost_storey, None, "{how}: the ghost is a view of the session, not saved");
    let acme = r.model.iter().find(|e| e.common.xdata.iter().any(|t| t.value == "ACME"));
    assert!(acme.is_some_and(|e| e.common.storey() == Some(2) && e.common.xdata.iter().any(|t| t.value == "hello")), "{how}: other xdata kept");
}

#[test]
fn storeys_round_trip_through_dxf() {
    let (d, _) = building();
    let r = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    check("DXF", &r, &d);
    // Twice: nothing doubles up.
    let r2 = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&r).as_bytes()).unwrap();
    check("DXF twice", &r2, &d);
    let text = cadcraft_io::write_dxf(&r2);
    assert_eq!(text.matches("CADCRAFT_STOREYS").count(), 1, "one storeys record");
    assert_eq!(text.lines().filter(|l| l.trim() == "CADCRAFT_STOREY").count(), 3 + 1, "xdata on three objects and the APPID record");
}

#[test]
fn storeys_round_trip_through_dwg() {
    let (d, _) = building();
    let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
    let r = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    check("DWG", &r, &d);
}

#[test]
fn a_drawing_without_storeys_writes_none() {
    let mut d = Drawing::new_metric();
    line(&mut d, 0.0, None, false);
    let text = cadcraft_io::write_dxf(&d);
    assert!(!text.contains("CADCRAFT_STOREY"));
    let r = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    assert!(!r.storeys.active());
}

#[test]
fn a_damaged_storeys_record_is_ignored_or_repaired() {
    let (d, _) = building();
    let text = cadcraft_io::write_dxf(&d);
    // Not JSON at all: no storeys (the objects keep their xdata, which then means nothing).
    let broken = text.replace("\"version\":1", "\"version\":9");
    let r = cadcraft_io::read_dxf(broken.as_bytes()).unwrap();
    assert!(!r.storeys.active());
    let d0 = r.model.iter().next().unwrap();
    assert_eq!(r.storey_view(d0), StoreyView::Current, "without storeys every object shows");
    // A current storey that doesn't exist: the bottom one.
    let odd = text.replace("\"current\":5", "\"current\":77");
    let r = cadcraft_io::read_dxf(odd.as_bytes()).unwrap();
    assert_eq!(r.storeys.current, 1);
}
