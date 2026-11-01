//! Parallel 3D views of model space (`Options::projection`): plan view draws as before, 3D lines
//! and flat objects at an elevation land where the view puts them, and block references carry
//! their insertion point's z.

use std::sync::Arc;

use cadcraft_doc::{Block, Circle, Common, Drawing, Entity, EntityKind, Handle, Insert, Line, Space};
use cadcraft_geom::{Mat4, Vec3};
use cadcraft_render::{Options, build};

fn front() -> Mat4 {
    Mat4::view(Vec3::new(0.0, -1.0, 0.0), Vec3::ZERO, 0.0)
}

fn bounds(d: &Drawing, projection: Option<Mat4>) -> (f64, f64, f64, f64) {
    let l = build(d, &Space::Model, &Options { projection, ..Options::default() });
    (l.bounds.min.x, l.bounds.min.y, l.bounds.max.x, l.bounds.max.y)
}

fn near(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    [a.0 - b.0, a.1 - b.1, a.2 - b.2, a.3 - b.3].iter().all(|d| d.abs() < 1e-6)
}

fn line(a: Vec3, b: Vec3) -> EntityKind {
    EntityKind::Line(Line { a, b })
}

#[test]
fn plan_view_draws_as_without_a_projection() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), line(Vec3::new(1.0, 2.0, 5.0), Vec3::new(8.0, 3.0, 9.0))).unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::Circle(Circle { center: Vec3::new(4.0, 4.0, 3.0), radius: 2.0 })).unwrap();
    let plan = build(&d, &Space::Model, &Options::default());
    let viewed = build(&d, &Space::Model, &Options { projection: Some(Mat4::view(Vec3::Z, Vec3::ZERO, 0.0)), ..Options::default() });
    assert_eq!(plan.verts, viewed.verts);
}

#[test]
fn front_view_shows_heights() {
    let mut d = Drawing::new_metric();
    // A vertical line: a point in plan, a line 10 high from the front.
    d.add(&Space::Model, Common::default(), line(Vec3::new(3.0, 7.0, 0.0), Vec3::new(3.0, 7.0, 10.0))).unwrap();
    assert!(near(bounds(&d, Some(front())), (3.0, 0.0, 3.0, 10.0)));

    // A circle at elevation 5, seen edge-on: a horizontal segment at height 5.
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Circle(Circle { center: Vec3::new(0.0, 0.0, 5.0), radius: 2.0 })).unwrap();
    assert!(near(bounds(&d, Some(front())), (-2.0, 5.0, 2.0, 5.0)));
}

#[test]
fn block_references_carry_their_elevation() {
    let mut d = Drawing::new_metric();
    let mut blk = Block::new("B");
    blk.entities.push(Entity::new(Handle(1), line(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0))));
    d.blocks.insert("B".into(), Arc::new(blk));
    let ins = Insert {
        block: "B".into(),
        insert: Vec3::new(10.0, 0.0, 4.0),
        scale: Vec3::new(2.0, 2.0, 2.0),
        rotation: 0.0,
        attribs: vec![],
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap();
    assert!(near(bounds(&d, Some(front())), (10.0, 4.0, 12.0, 4.0)));
}

#[test]
fn isometric_view_of_a_cube() {
    let mut d = Drawing::new_metric();
    let c = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
    for (a, b) in [
        (c(0.0, 0.0, 0.0), c(1.0, 0.0, 0.0)),
        (c(1.0, 0.0, 0.0), c(1.0, 1.0, 0.0)),
        (c(1.0, 1.0, 0.0), c(0.0, 1.0, 0.0)),
        (c(0.0, 1.0, 0.0), c(0.0, 0.0, 0.0)),
        (c(0.0, 0.0, 0.0), c(0.0, 0.0, 1.0)),
        (c(1.0, 1.0, 0.0), c(1.0, 1.0, 1.0)),
    ] {
        d.add(&Space::Model, Common::default(), line(a, b)).unwrap();
    }
    let iso = Mat4::view(Vec3::new(1.0, -1.0, 1.0), Vec3::ZERO, 0.0);
    let (x0, y0, x1, y1) = bounds(&d, Some(iso));
    // Wider than the cube's 1-unit sides (a diagonal across), and its height shows.
    assert!((x1 - x0 - 2f64.sqrt()).abs() < 1e-6, "width {}", x1 - x0);
    assert!(y1 - y0 > 1.0, "height {}", y1 - y0);
}

#[test]
fn helices_and_infinite_lines_are_projected_in_3d() {
    use cadcraft_doc::{Helix, RayLine};
    // A helix 10 high is a column from the front, not a flat coil at z = 0.
    let mut d = Drawing::new_metric();
    let helix = Helix {
        center: Vec3::ZERO,
        base_radius: 2.0,
        top_radius: 2.0,
        height: 10.0,
        turns: 3.0,
        start_angle: 0.0,
        ccw: true,
        constrain: Default::default(),
    };
    d.add(&Space::Model, Common::default(), EntityKind::Helix(helix)).unwrap();
    let (_, y0, _, y1) = bounds(&d, Some(front()));
    assert!(y0.abs() < 1e-6 && (y1 - 10.0).abs() < 1e-6, "{y0}..{y1}");
    // A vertical construction line is vertical from the front (its direction keeps its z).
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::XLine(RayLine { base: Vec3::new(3.0, 0.0, 0.0), dir: Vec3::Z })).unwrap();
    let l = build(&d, &Space::Model, &Options { projection: Some(front()), ..Options::default() });
    let dir = l.prims.iter().find(|p| matches!(p.kind, cadcraft_render::Kind::Infinite { .. })).map(|p| l.points(p)[1]).unwrap();
    assert!(dir.x.abs() < 1e-9 && (dir.y.abs() - 1.0).abs() < 1e-9, "{dir:?}");
}

#[test]
fn a_2d_solid_is_a_flat_object_at_its_elevation() {
    use cadcraft_doc::Solid;
    // A SOLID lies in a plane like a circle: from the front it is an edge at its elevation.
    let c = |x: f64, y: f64| Vec3::new(x, y, 4.0);
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Solid(Solid { corners: [c(0.0, 0.0), c(2.0, 0.0), c(0.0, 2.0), c(2.0, 2.0)] })).unwrap();
    let (_, y0, _, y1) = bounds(&d, Some(front()));
    assert!((y0 - 4.0).abs() < 1e-6 && (y1 - 4.0).abs() < 1e-6, "{y0}..{y1}");
}

#[test]
fn points_seen_edge_on_keep_their_size() {
    use cadcraft_doc::Point;
    // A relative point size is measured in the view, not by the foreshortened plane: from the
    // front a point on the XY plane stays a small mark, the list's bounds stay finite and small.
    let mut d = Drawing::new_metric();
    d.header.set_f64("PDMODE", 3.0);
    d.header.set_f64("PDSIZE", -5.0);
    d.add(&Space::Model, Common::default(), EntityKind::Point(Point { p: Vec3::new(1.0, 1.0, 0.0), angle: 0.0 })).unwrap();
    let l = build(&d, &Space::Model, &Options { projection: Some(front()), view_height: 100.0, ..Options::default() });
    assert!(l.bounds.width() < 20.0 && l.bounds.height() < 20.0, "{:?}", l.bounds);
}
