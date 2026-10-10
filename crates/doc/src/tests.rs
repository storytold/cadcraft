use crate::*;
use cadcraft_geom::{Bounds2, Mat3, PolyVertex, Vec2, Vec3};

fn line(a: (f64, f64), b: (f64, f64)) -> EntityKind {
    EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) })
}

#[test]
fn new_drawing_has_standard_tables() {
    let d = Drawing::new_imperial();
    assert!(d.layer("0").is_some());
    assert!(d.linetype("continuous").is_some());
    assert!(d.text_style("Standard").is_some());
    assert_eq!(d.header.f64("TEXTSIZE", 0.0), 0.2);
    let m = Drawing::new_metric();
    assert!(m.dim_style("ISO-25").is_some());
}

#[test]
fn add_and_extents() {
    let mut d = Drawing::new_imperial();
    d.add(&Space::Model, Common::default(), line((0.0, 0.0), (10.0, 5.0))).unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::Circle(Circle { center: Vec3::new(20.0, 0.0, 0.0), radius: 2.0 })).unwrap();
    let b = d.extents(&Space::Model);
    assert_eq!(b.min, Vec2::new(0.0, -2.0));
    assert_eq!(b.max, Vec2::new(22.0, 5.0));
}

#[test]
fn hidden_layers_excluded_from_extents() {
    let mut d = Drawing::new_imperial();
    let c = Common { layer: "Hidden".into(), ..Default::default() };
    d.add(&Space::Model, c, line((100.0, 100.0), (200.0, 200.0))).unwrap();
    d.add(&Space::Model, Common::default(), line((0.0, 0.0), (1.0, 1.0))).unwrap();
    d.layer_mut("Hidden").unwrap().on = false;
    assert_eq!(d.extents(&Space::Model).max, Vec2::new(1.0, 1.0));
}

#[test]
fn transform_arc_mirror_keeps_shape() {
    let mut k = EntityKind::Arc(Arc { center: Vec3::ZERO, radius: 1.0, start: 0.0, end: std::f64::consts::FRAC_PI_2 });
    k.transform(&Mat3::mirror(Vec2::ZERO, Vec2::Y));
    if let EntityKind::Arc(a) = k {
        let g = cadcraft_geom::Arc::new(a.center.xy(), a.radius, a.start, a.end);
        assert!(g.mid_point().near(Vec2::from_angle(3.0 * std::f64::consts::FRAC_PI_4), 1e-9));
    } else {
        panic!()
    }
}

#[test]
fn insert_bounds_with_cyclic_block_terminate() {
    let mut d = Drawing::new_imperial();
    let mut b = Block::new("A");
    b.entities.push(Entity::new(
        Handle(1),
        EntityKind::Insert(Insert {
            block: "A".into(),
            insert: Vec3::ZERO,
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: vec![],
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        }),
    ));
    b.entities.push(Entity::new(Handle(2), line((0.0, 0.0), (1.0, 1.0))));
    d.blocks.insert("A".into(), std::sync::Arc::new(b));
    let h = d
        .add(
            &Space::Model,
            Common::default(),
            EntityKind::Insert(Insert {
                block: "A".into(),
                insert: Vec3::new(5.0, 5.0, 0.0),
                scale: Vec3::new(2.0, 2.0, 1.0),
                rotation: 0.0,
                attribs: vec![],
                cols: 1,
                rows: 1,
                col_spacing: 0.0,
                row_spacing: 0.0,
            }),
        )
        .unwrap();
    let e = d.entity(h).unwrap().clone();
    let bb = entity_bounds(&d, &e, 0);
    assert_eq!(bb.max, Vec2::new(7.0, 7.0));
}

#[test]
fn entity_serde_roundtrip() {
    let e = Entity::new(Handle(7), line((1.0, 2.0), (3.0, 4.0)));
    let s = serde_json::to_string(&e).unwrap();
    let back: Entity = serde_json::from_str(&s).unwrap();
    assert_eq!(e, back);
}

#[test]
fn paper_space_entities() {
    let mut d = Drawing::new_imperial();
    let h = d.add(&Space::Paper("Layout1".into()), Common::default(), line((0.0, 0.0), (1.0, 0.0))).unwrap();
    assert_eq!(d.space_of(h), Some(Space::Paper("Layout1".into())));
    assert!(d.remove_entity(h).is_some());
    assert_eq!(d.entity_count(), 0);
}

fn wide_pline(verts: &[(f64, f64, f64, f64, f64)], closed: bool, const_width: f64) -> Bounds2 {
    // (x, y, start width, end width, bulge) per vertex.
    let mut d = Drawing::new_metric();
    let vertices = verts.iter().map(|&(x, y, s, e, b)| PolyVertex { p: Vec2::new(x, y), bulge: b, start_width: s, end_width: e }).collect();
    let kind = EntityKind::LwPolyline(LwPolyline { vertices, closed, const_width, elevation: 0.0, plinegen: false });
    d.add(&Space::Model, Common::default(), kind).unwrap();
    d.extents(&Space::Model)
}

fn near_bounds(b: Bounds2, min: (f64, f64), max: (f64, f64)) -> bool {
    let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
    close(b.min.x, min.0) && close(b.min.y, min.1) && close(b.max.x, max.0) && close(b.max.y, max.1)
}

/// #90: a polyline's extents enclose its drawn band, whether the width is constant or per segment.
#[test]
fn wide_polyline_extents_follow_the_band() {
    // The issue's cases: constant width, the same band from per-segment widths, a taper,
    // two segments of different widths, and a width on the open end vertex (which draws nothing).
    let b = wide_pline(&[(0.0, 0.0, 0.0, 0.0, 0.0), (10.0, 0.0, 0.0, 0.0, 0.0)], false, 2.0);
    assert!(near_bounds(b, (0.0, -1.0), (10.0, 1.0)), "{b:?}");
    let b = wide_pline(&[(0.0, 0.0, 2.0, 2.0, 0.0), (10.0, 0.0, 2.0, 2.0, 0.0)], false, 0.0);
    assert!(near_bounds(b, (0.0, -1.0), (10.0, 1.0)), "{b:?}");
    let b = wide_pline(&[(0.0, 0.0, 0.0, 4.0, 0.0), (10.0, 0.0, 4.0, 0.0, 0.0)], false, 0.0);
    assert!(near_bounds(b, (0.0, -2.0), (10.0, 2.0)), "{b:?}");
    let b = wide_pline(&[(0.0, 0.0, 2.0, 2.0, 0.0), (10.0, 0.0, 6.0, 6.0, 0.0), (20.0, 0.0, 0.0, 0.0, 0.0)], false, 0.0);
    assert!(near_bounds(b, (0.0, -3.0), (20.0, 3.0)), "{b:?}");
    let b = wide_pline(&[(0.0, 0.0, 0.0, 0.0, 0.0), (10.0, 0.0, 6.0, 6.0, 0.0)], false, 0.0);
    assert!(near_bounds(b, (0.0, 0.0), (10.0, 0.0)), "{b:?}");
    // A diagonal band: its exact corners, not the centre line padded by the half-width.
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let b = wide_pline(&[(0.0, 0.0, 0.0, 0.0, 0.0), (10.0, 10.0, 0.0, 0.0, 0.0)], false, 2.0);
    assert!(near_bounds(b, (-h, -h), (10.0 + h, 10.0 + h)), "{b:?}");
    // A closed polyline's closing segment counts (here the only wide one).
    let sq = [(0.0, 0.0, 0.0, 0.0, 0.0), (10.0, 0.0, 0.0, 0.0, 0.0), (10.0, 10.0, 0.0, 0.0, 0.0), (0.0, 10.0, 2.0, 2.0, 0.0)];
    assert!(near_bounds(wide_pline(&sq, true, 0.0), (-1.0, 0.0), (10.0, 10.0)));
    assert!(near_bounds(wide_pline(&sq, false, 0.0), (0.0, 0.0), (10.0, 10.0)), "open: the end vertex starts no segment");
    // Non-finite or negative widths count as zero.
    let b = wide_pline(&[(0.0, 0.0, f64::NAN, f64::INFINITY, 0.0), (10.0, 0.0, 0.0, 0.0, 0.0)], false, 0.0);
    assert!(near_bounds(b, (0.0, 0.0), (10.0, 0.0)), "{b:?}");
    let b = wide_pline(&[(0.0, 0.0, -4.0, -4.0, 0.0), (10.0, 0.0, 0.0, 0.0, 0.0)], false, 0.0);
    assert!(near_bounds(b, (0.0, 0.0), (10.0, 0.0)), "{b:?}");
}

/// A wide arc segment reaches its outer edge (radius plus half the width), never less, and only
/// by the sampling tolerance more.
#[test]
fn wide_arc_segment_extents_reach_the_outer_edge() {
    // A half circle below the axis (bulge 1, counter-clockwise from (0,0) to (10,0)), radius 5.
    let exact_min = (-1.0, -6.0);
    let exact_max = (11.0, 0.0);
    for b in [
        wide_pline(&[(0.0, 0.0, 0.0, 0.0, 1.0), (10.0, 0.0, 0.0, 0.0, 0.0)], false, 2.0),
        wide_pline(&[(0.0, 0.0, 2.0, 2.0, 1.0), (10.0, 0.0, 0.0, 0.0, 0.0)], false, 0.0),
    ] {
        assert!(b.min.x <= exact_min.0 && b.min.y <= exact_min.1 && b.max.x >= exact_max.0 && b.max.y >= exact_max.1, "never smaller: {b:?}");
        assert!(
            b.min.x > exact_min.0 - 1e-3 && b.min.y > exact_min.1 - 1e-3 && b.max.x < exact_max.0 + 1e-3 && b.max.y < exact_max.1 + 1e-3,
            "tight: {b:?}"
        );
    }
    // A taper along the arc (0 to 4 wide): compared with a brute-force sampling of both band
    // edges, since the extremes of a tapering curve lie between the obvious points.
    let b = wide_pline(&[(0.0, 0.0, 0.0, 4.0, 1.0), (10.0, 0.0, 0.0, 0.0, 0.0)], false, 0.0);
    let mut exact = Bounds2::EMPTY;
    for k in 0..=100_000 {
        let t = f64::from(k) / 100_000.0;
        // Counter-clockwise from angle pi (the start, (0,0)) to 2 pi (the end, (10,0)).
        let a = std::f64::consts::PI * (1.0 + t);
        let dir = Vec2::new(a.cos(), a.sin());
        let h = 2.0 * t;
        exact.add(Vec2::new(5.0, 0.0) + dir * (5.0 + h));
        exact.add(Vec2::new(5.0, 0.0) + dir * (5.0 - h));
    }
    for (got, want, outward) in [(b.min.x, exact.min.x, -1.0), (b.min.y, exact.min.y, -1.0), (b.max.x, exact.max.x, 1.0), (b.max.y, exact.max.y, 1.0)]
    {
        let over = (got - want) * outward;
        assert!(over > -1e-6 && over < 1e-3, "got {b:?}, exact {exact:?}");
    }
}
