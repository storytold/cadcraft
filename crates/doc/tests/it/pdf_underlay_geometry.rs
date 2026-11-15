//! A PDF underlay's place after a mirror or a height scaling, and its bounds with an inverted clip.

use cadcraft_doc::geom::{Mat3, Vec2, Vec3};
use cadcraft_doc::{Common, Drawing, EntityKind, PdfUnderlay, Space, entity_bounds};

fn page() -> PdfUnderlay {
    PdfUnderlay { insert: Vec3::new(2.0, 3.0, 0.0), size: Vec2::new(10.0, 5.0), scale: 2.0, ..PdfUnderlay::default() }
}

fn mirrored(u: &PdfUnderlay, m: &Mat3) -> PdfUnderlay {
    let mut k = EntityKind::PdfUnderlay(u.clone());
    k.transform(m);
    match k {
        EntityKind::PdfUnderlay(u) => u,
        _ => panic!("still an underlay"),
    }
}

/// `a` and `b` hold the same points, in any order.
fn same_points(a: &[Vec2], b: &[Vec2]) -> bool {
    a.len() == b.len() && a.iter().all(|p| b.iter().any(|q| (*p - *q).len() < 1e-9))
}

#[test]
fn a_mirrored_page_covers_the_mirrored_outline_and_stays_upright() {
    for (line, name) in [
        ((Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0)), "about a vertical line"),
        ((Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0)), "about a horizontal line"),
    ] {
        let m = Mat3::mirror(line.0, line.1);
        let u = page();
        let want: Vec<Vec2> = u.corners().iter().map(|p| m.apply(*p)).collect();
        let r = mirrored(&u, &m);
        assert!(same_points(&r.corners(), &want), "{name}: {:?} instead of {:?}", r.corners(), want);
        assert!(r.rotation.abs() < 1e-9, "{name}: the page stays upright, not turned by {}", r.rotation);
    }
}

#[test]
fn a_mirrored_page_keeps_its_clip_boundary_on_the_mirrored_one() {
    let mut u = page();
    u.clip = vec![Vec2::new(1.0, 1.0), Vec2::new(4.0, 1.0), Vec2::new(4.0, 3.0)];
    let m = Mat3::mirror(Vec2::new(-1.0, 7.0), Vec2::new(5.0, 2.0));
    let want: Vec<Vec2> = u.clip_outline().unwrap().iter().map(|p| m.apply(*p)).collect();
    let r = mirrored(&u, &m);
    assert!(same_points(&r.corners(), &u.corners().map(|p| m.apply(p))), "the page on the mirrored outline");
    assert!(same_points(&r.clip_outline().unwrap(), &want), "{:?} instead of {:?}", r.clip_outline(), want);
}

#[test]
fn an_inverted_clip_frame_is_inside_the_bounds_without_a_page_size() {
    // A page size is not stored in files: until it is read, the clip frame is all that is drawn.
    let mut u = PdfUnderlay { size: Vec2::ZERO, ..PdfUnderlay::default() };
    u.clip = vec![Vec2::new(1.0, 1.0), Vec2::new(4.0, 1.0), Vec2::new(4.0, 3.0)];
    u.clip_inverted = true;
    let mut d = Drawing::new_metric();
    let h = d.add(&Space::Model, Common::default(), EntityKind::PdfUnderlay(u.clone())).unwrap();
    let e = d.model.get(h).unwrap();
    let b = entity_bounds(&d, e, 0);
    for p in u.clip_outline().unwrap() {
        assert!(b.contains(p), "{p:?} outside {b:?}");
    }
}

/// SCALE's height scaling moves an underlay's insertion point up or down, as an image's.
#[test]
fn scaling_heights_moves_the_insertion_point() {
    let mut k = EntityKind::PdfUnderlay(PdfUnderlay { insert: Vec3::new(1.0, 2.0, 5.0), size: Vec2::new(10.0, 5.0), ..PdfUnderlay::default() });
    k.scale_heights(1.0, 2.0);
    let EntityKind::PdfUnderlay(u) = k else { panic!("still an underlay") };
    assert_eq!(u.insert, Vec3::new(1.0, 2.0, 9.0));
}
