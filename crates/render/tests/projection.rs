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

fn face(z: f64, color: u8) -> (Common, EntityKind) {
    let c = |x: f64, y: f64| Vec3::new(x, y, z);
    let common = Common { color: cadcraft_color::Color::Index(color), ..Common::default() };
    (common, EntityKind::Face3d(cadcraft_doc::Face3d { corners: [c(0.0, 0.0), c(4.0, 0.0), c(4.0, 4.0), c(0.0, 4.0)], hidden_edges: 0 }))
}

#[test]
fn shaded_faces_are_filled_lit_and_drawn_back_to_front() {
    use cadcraft_render::Kind;
    let mut d = Drawing::new_metric();
    // The upper face first in the drawing: it is nearer the viewer from above, so drawn last.
    for (z, color) in [(5.0, 1), (0.0, 5)] {
        let (common, kind) = face(z, color);
        d.add(&Space::Model, common, kind).unwrap();
    }
    d.add(&Space::Model, Common::default(), line(Vec3::new(-1.0, -1.0, 0.0), Vec3::new(5.0, -1.0, 0.0))).unwrap();
    let iso = Mat4::view(Vec3::new(1.0, -1.0, 1.0), Vec3::ZERO, 0.0);
    let opts = Options { projection: Some(iso), shaded: true, ..Options::default() };
    let l = build(&d, &Space::Model, &opts);
    let fills: Vec<_> = l.prims.iter().filter(|p| p.kind == Kind::Tris).collect();
    assert_eq!(fills.len(), 2);
    // Faces first, farthest first: the blue (5) ground face, then the red (1) upper face.
    assert_eq!(l.prims[0].kind, Kind::Tris);
    assert!(fills[0].color.2 > fills[0].color.0 && fills[1].color.0 > fills[1].color.2, "{:?}", fills.iter().map(|p| p.color).collect::<Vec<_>>());
    // Lit: darker than the plain colour, seen at an angle.
    assert!(fills[1].color.0 < 255);
    // Shaded: no face edges, only the line. With edges: the faces' outlines too.
    assert_eq!(l.prims.iter().filter(|p| p.kind == Kind::Polyline).count(), 1);
    let edges = build(&d, &Space::Model, &Options { shaded_edges: true, ..opts.clone() });
    assert_eq!(edges.prims.iter().filter(|p| p.kind == Kind::Polyline).count(), 9);
    assert!(edges.prims.iter().position(|p| p.kind == Kind::Polyline) > edges.prims.iter().rposition(|p| p.kind == Kind::Tris));
    // Without shading: no fills.
    let wire = build(&d, &Space::Model, &Options { projection: Some(iso), ..Options::default() });
    assert!(wire.prims.iter().all(|p| p.kind != Kind::Tris));
}

#[test]
fn a_layout_viewport_shows_model_space_from_its_view_direction() {
    let mut d = Drawing::new_metric();
    // A 10 high column.
    d.add(&Space::Model, Common::default(), line(Vec3::ZERO, Vec3::new(0.0, 0.0, 10.0))).unwrap();
    let paper = Space::Paper(d.layouts[0].name.clone());
    // Seen from the front at 1:1, centred on the column's middle: it stands 10 high on paper.
    let vp = cadcraft_doc::Viewport {
        center: Vec3::new(50.0, 50.0, 0.0),
        width: 40.0,
        height: 40.0,
        view_center: cadcraft_geom::Vec2::new(0.0, 5.0),
        view_height: 40.0,
        id: 2,
        view3d: Some(cadcraft_doc::ViewDir { dir: Vec3::new(0.0, -1.0, 0.0), target: Vec3::ZERO }),
        ..cadcraft_doc::Viewport::default()
    };
    d.add(&paper, Common::default(), EntityKind::Viewport(vp)).unwrap();
    let l = build(&d, &paper, &Options::default());
    // Model space drawn in a viewport isn't pickable on the layout: no handle.
    let column: Vec<_> = l.prims.iter().filter(|p| p.handle == Handle(0)).map(|p| l.points(p)).collect();
    assert_eq!(column.len(), 1);
    let (a, b) = (column[0][0], column[0][1]);
    assert!((a.x - 50.0).abs() < 1e-6 && (b.x - 50.0).abs() < 1e-6 && (a.y - 45.0).abs() < 1e-6 && (b.y - 55.0).abs() < 1e-6, "{a:?} {b:?}");
}

#[test]
fn plotted_shaded_faces_keep_their_layers_and_transparency() {
    use cadcraft_render::{Kind, build_plot};
    let mut d = Drawing::new_metric();
    for name in ["Faces", "Lines"] {
        d.layers.push(cadcraft_doc::Layer::new(name));
    }
    let (mut common, kind) = face(0.0, 1);
    common.layer = "Faces".into();
    common.transparency = cadcraft_doc::Transparency::Percent(40);
    d.add(&Space::Model, common, kind).unwrap();
    for x in [0.0, 2.0] {
        let lines = Common { layer: "Lines".into(), ..Common::default() };
        d.add(&Space::Model, lines, line(Vec3::new(x, -1.0, 0.0), Vec3::new(x + 1.0, -1.0, 0.0))).unwrap();
    }
    let index = |n: &str| d.layers.iter().position(|l| l.name == n).unwrap() as u32;
    let iso = Mat4::view(Vec3::new(1.0, -1.0, 1.0), Vec3::ZERO, 0.0);
    let opts = Options { projection: Some(iso), shaded: true, transparency: true, ..Options::default() };
    // A plot's layers stay parallel to its primitives, the faces (drawn first) on their own.
    let plot = build_plot(&d, &Space::Model, &opts);
    assert_eq!(plot.prim_layers.len(), plot.prims.len());
    assert_eq!(plot.prims.first().map(|p| p.kind), Some(Kind::Tris));
    for (p, layer) in plot.prims.iter().zip(&plot.prim_layers) {
        let want = if p.kind == Kind::Tris { index("Faces") } else { index("Lines") };
        assert_eq!(*layer, want, "{p:?}");
    }
    // A face is as transparent as itself, not as what contains it.
    let screen = build(&d, &Space::Model, &opts);
    assert!(screen.prim_layers.is_empty());
    let fill = screen.prims.iter().find(|p| p.kind == Kind::Tris).unwrap();
    assert_eq!(fill.transparency, 40);
    let opaque = build(&d, &Space::Model, &Options { transparency: false, ..opts.clone() });
    assert_eq!(opaque.prims.iter().find(|p| p.kind == Kind::Tris).map(|p| p.transparency), Some(0));
}
