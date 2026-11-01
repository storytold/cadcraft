//! Unit tests for USAGE's scan limit (#1439): a scan cut at its limit must say so. A drawing with
//! more than 5,000,000 objects is impractical to build in a test, so the limit is a parameter and
//! these tests cut a tiny drawing instead.

use std::sync::Arc;

use cadcraft_doc::{Block, Common, Drawing, Entity, EntityKind, Handle, Line};
use cadcraft_geom::Vec3;

use super::{Incomplete, Scan, entity_total};

/// A LINE on `layer`, one unit long.
fn line(handle: u64, layer: &str) -> Entity {
    let mut e = Entity::new(Handle(handle), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 0.0, 0.0) }));
    e.common = Common { layer: layer.into(), ..Common::default() };
    e
}

/// A drawing with `top` lines on layer Used in model space and `held` lines on layer Held in the
/// block HELD, so the block's objects come last in the scan, exactly as they do in a real drawing.
fn drawing(top: usize, held: usize) -> Drawing {
    let mut d = Drawing::new_metric();
    for i in 0..top {
        d.model.push(line(0x100 + i as u64, "Used"));
    }
    let mut b = Block::new("HELD");
    for i in 0..held {
        b.entities.push(line(0x200 + i as u64, "Held"));
    }
    d.blocks.insert("HELD".into(), Arc::new(b));
    d
}

/// Guards the detector's boundary: a drawing holding exactly the limit is read whole and is not
/// reported as cut short, so `incomplete` never cries wolf on the last readable object.
#[test]
fn a_drawing_at_the_limit_is_complete() {
    let d = drawing(2, 1);
    assert_eq!(entity_total(&d), 3);
    let sc = Scan::new(&d, 3);
    assert_eq!(sc.incomplete, None);
    assert_eq!(sc.layers.get("used").copied(), Some((2, 0)));
    assert_eq!(sc.layers.get("held").copied(), Some((0, 1)));
}

/// Guards that a scan stopped one object early reports what it omitted and why: HELD's line is
/// past the limit, so its layer looks unused — the miscount the `incomplete` signal exists to
/// prevent a caller from trusting.
#[test]
fn a_cut_scan_reports_the_objects_it_omitted() {
    let d = drawing(2, 1);
    let sc = Scan::new(&d, 2);
    let inc = sc.incomplete.expect("a cut scan must report it");
    assert_eq!(inc, Incomplete { scanned: 2, total: 3 });
    assert_eq!(sc.layers.get("held").copied(), None);
    let reason = inc.reason();
    assert!(reason.contains("first 2 of 3 objects"), "{reason}");
    assert!(reason.contains("partial"), "{reason}");
}
