//! Plot style line ends, joins and fill patterns (issue #564): wide lines become outlines with
//! the style's ends and joins, solid fills become the style's pattern.

use std::sync::Arc;

use cadcraft_color::Color;
use cadcraft_doc::{
    Common, Drawing, EndStyle, EntityKind, FillStyle, JoinStyle, Line, LwPolyline, PlotStyle, PlotStyleTable, Solid, Space, Viewport,
};
use cadcraft_geom::{PolyVertex, Vec2, Vec3};
use cadcraft_render::{DisplayList, Kind, Options, VIEWPORT_CONTENT, build, build_plot, pen};

/// A colour-dependent table whose colour 1 prints 1.00 mm wide with `style`'s end/join/fill.
fn table(end: EndStyle, join: JoinStyle, fill: FillStyle) -> Options {
    let t = PlotStyleTable::color_dependent("pens.ctb", "", |i| {
        (i == 1).then(|| PlotStyle { lineweight: Some(100), end, join, fill, ..PlotStyle::default() })
    });
    Options { plot_style_table: Some(Arc::new(t)), lineweights: true, ..Default::default() }
}

fn red() -> Common {
    Common { color: Color::Index(1), ..Default::default() }
}

fn tri_area(l: &DisplayList) -> f64 {
    l.prims
        .iter()
        .filter(|p| p.kind == Kind::Tris)
        .flat_map(|p| l.points(p).as_chunks::<3>().0.iter().map(|[a, b, c]| (*b - *a).cross(*c - *a) / 2.0))
        .sum()
}

/// A 10-unit line plotted at 1 mm per unit.
fn line_plot(end: EndStyle, join: JoinStyle) -> DisplayList {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, red(), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(10.0, 0.0, 0.0) })).unwrap();
    let mut l = build_plot(&d, &Space::Model, &table(end, join, FillStyle::Object));
    pen::apply(&mut l, 1.0);
    l
}

#[test]
fn line_ends_shape_wide_lines() {
    let object = line_plot(EndStyle::Object, JoinStyle::Object);
    assert_eq!(object.prims[0].kind, Kind::Polyline, "use-object ends keep the stroke");
    assert_eq!(object.prims[0].lw, 1.0);
    let butt = line_plot(EndStyle::Butt, JoinStyle::Object);
    assert_eq!(butt.prims.len(), 1);
    assert_eq!(butt.prims[0].kind, Kind::Tris);
    assert!((tri_area(&butt) - 10.0).abs() < 1e-9, "butt: {}", tri_area(&butt));
    let square = line_plot(EndStyle::Square, JoinStyle::Object);
    assert!((tri_area(&square) - 11.0).abs() < 1e-9, "square: {}", tri_area(&square));
    let diamond = line_plot(EndStyle::Diamond, JoinStyle::Object);
    assert!((tri_area(&diamond) - 10.5).abs() < 1e-9, "diamond: {}", tri_area(&diamond));
    let round = line_plot(EndStyle::Round, JoinStyle::Object);
    assert!(tri_area(&round) > 10.7 && tri_area(&round) < 10.0 + std::f64::consts::PI / 4.0);
    // Applying again changes nothing; pens are spent.
    let mut again = butt.clone();
    pen::apply(&mut again, 1.0);
    assert_eq!(again.prims, butt.prims);
    assert!(butt.pens.is_empty());
}

#[test]
fn joins_shape_the_corner() {
    // An L: (0,0) → (10,0) → (10,10), 1 mm wide, butt ends.
    let corner = |join| {
        let mut d = Drawing::new_metric();
        let v = |x, y| PolyVertex::new(Vec2::new(x, y));
        let pl =
            LwPolyline { vertices: vec![v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0)], closed: false, const_width: 0.0, elevation: 0.0, plinegen: false };
        d.add(&Space::Model, red(), EntityKind::LwPolyline(pl)).unwrap();
        let mut l = build_plot(&d, &Space::Model, &table(EndStyle::Butt, join, FillStyle::Object));
        pen::apply(&mut l, 1.0);
        // The union of the outline: rasterise coarsely by sampling the outer corner region.
        let inside = |p: Vec2| {
            l.prims.iter().any(|pr| {
                l.points(pr).as_chunks::<3>().0.iter().any(|[a, b, c]| {
                    let s = |u: Vec2, w: Vec2| (w - u).cross(p - u) >= -1e-12;
                    s(*a, *b) && s(*b, *c) && s(*c, *a)
                })
            })
        };
        (inside(Vec2::new(10.45, -0.45)), inside(Vec2::new(10.3, -0.3)), inside(Vec2::new(10.2, -0.2)))
    };
    assert_eq!(corner(JoinStyle::Miter), (true, true, true), "miter fills the outer corner");
    assert_eq!(corner(JoinStyle::Bevel), (false, false, true), "bevel cuts it");
    assert_eq!(corner(JoinStyle::Round), (false, true, true), "round rounds it");
    assert_eq!(corner(JoinStyle::Diamond), (false, true, true), "diamond points into it");
}

#[test]
fn fill_patterns_replace_solid_fills() {
    let mut d = Drawing::new_metric();
    let c = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(10.0, 10.0, 0.0)];
    d.add(&Space::Model, red(), EntityKind::Solid(Solid { corners: c })).unwrap();
    let area = |fill| {
        let mut l = build_plot(&d, &Space::Model, &table(EndStyle::Object, JoinStyle::Object, fill));
        assert_eq!(l.prims.len(), 1);
        pen::apply(&mut l, 1.0);
        assert_eq!(l.prims.len(), 1, "one primitive per object");
        tri_area(&l)
    };
    assert!((area(FillStyle::Solid) - 100.0).abs() < 1e-9);
    assert!((area(FillStyle::Object) - 100.0).abs() < 1e-9);
    for (fill, expect) in [
        (FillStyle::Checkerboard, 50.0),
        (FillStyle::HorizontalBars, 100.0 / 3.0),
        (FillStyle::VerticalBars, 100.0 / 3.0),
        (FillStyle::SquareDots, 100.0 * 0.16),
    ] {
        let a = area(fill);
        assert!((a - expect).abs() < 1.5, "{fill:?}: {a} (expected about {expect})");
    }
    for fill in [FillStyle::Crosshatch, FillStyle::Diamonds, FillStyle::SlantLeft, FillStyle::SlantRight] {
        let a = area(fill);
        assert!(a > 15.0 && a < 60.0, "{fill:?}: {a}");
    }
}

#[test]
fn layouts_draw_pens_while_building() {
    let space = Space::Paper("Layout1".into());
    let mut d = Drawing::new_metric();
    d.add(&space, red(), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(10.0, 0.0, 0.0) })).unwrap();
    let opts = table(EndStyle::Square, JoinStyle::Object, FillStyle::Object);
    let l = build(&d, &space, &opts);
    assert_eq!(l.prims[0].kind, Kind::Tris);
    assert!(l.pens.is_empty(), "pens are used up");
    assert!((tri_area(&l) - 11.0).abs() < 1e-9);
    // Thin lines stay strokes.
    let thin = PlotStyleTable::color_dependent("thin.ctb", "", |i| {
        (i == 1).then(|| PlotStyle { lineweight: Some(18), end: EndStyle::Square, ..PlotStyle::default() })
    });
    let l = build(&d, &space, &Options { plot_style_table: Some(Arc::new(thin)), lineweights: true, ..Default::default() });
    assert_eq!(l.prims[0].kind, Kind::Polyline);
}

#[test]
fn hostile_scales_and_huge_fills_do_not_hang() {
    let mut d = Drawing::new_metric();
    let c = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(1e12, 0.0, 0.0), Vec3::new(0.0, 1e12, 0.0), Vec3::new(1e12, 1e12, 0.0)];
    d.add(&Space::Model, red(), EntityKind::Solid(Solid { corners: c })).unwrap();
    d.add(&Space::Model, red(), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(1e300, 0.0, 0.0) })).unwrap();
    let l = build_plot(&d, &Space::Model, &table(EndStyle::Diamond, JoinStyle::Diamond, FillStyle::Checkerboard));
    for mm in [1.0, 1e-300, 1e300, 0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut x = l.clone();
        pen::apply(&mut x, mm);
        assert_eq!(x.prims.len(), l.prims.len());
    }
}

#[test]
fn viewport_contents_get_their_pens() {
    // A 2:1 viewport onto a red model-space line: 1 mm wide on paper, square ends.
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, red(), EntityKind::Line(Line { a: Vec3::new(-10.0, 0.0, 0.0), b: Vec3::new(10.0, 0.0, 0.0) })).unwrap();
    let vp = Viewport {
        center: Vec3::new(50.0, 50.0, 0.0),
        width: 80.0,
        height: 60.0,
        view_center: Vec2::ZERO,
        view_height: 30.0,
        id: 2,
        locked: false,
        frozen_layers: Vec::new(),
        layer_colors: Vec::new(),
        clip: None,
        on: true,
    };
    let space = Space::Paper("Layout1".into());
    d.add(&space, Common::default(), EntityKind::Viewport(vp)).unwrap();
    let l = build_plot(&d, &space, &table(EndStyle::Square, JoinStyle::Object, FillStyle::Object));
    let content: Vec<_> = l.prims.iter().filter(|p| p.handle == VIEWPORT_CONTENT).collect();
    assert_eq!(content.len(), 1);
    assert_eq!(content[0].kind, Kind::Tris);
    // 40 mm long on paper plus two 0.5 mm square ends, 1 mm wide.
    assert!((tri_area(&l) - 41.0).abs() < 1e-6, "{}", tri_area(&l));
}
