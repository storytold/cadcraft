//! Text embedded in complex linetypes is drawn along the line (issue #312).

use cadcraft_doc::{Common, DashElement, Drawing, EntityKind, Line, Linetype, Space, library};
use cadcraft_geom::{Bounds2, Vec2, Vec3};
use cadcraft_render::{Kind, Options, build};

fn drawing(lt: Linetype, a: Vec2, b: Vec2) -> Drawing {
    let mut d = Drawing::new_imperial();
    let name = lt.name.clone();
    d.linetypes.push(lt);
    let common = Common { linetype: name, ..Default::default() };
    d.add(&Space::Model, common, EntityKind::Line(Line { a: Vec3::new(a.x, a.y, 0.0), b: Vec3::new(b.x, b.y, 0.0) })).unwrap();
    d
}

/// Bounds of the drawn polylines that leave the line through `a`-`b` (the text strokes).
fn off_line(d: &Drawing, opts: &Options, a: Vec2, b: Vec2) -> Vec<Bounds2> {
    let l = build(d, &Space::Model, opts);
    let dir = (b - a).normalized();
    let dist = |p: Vec2| (p - a).x * dir.y - (p - a).y * dir.x;
    l.prims
        .iter()
        .filter(|p| p.kind == Kind::Polyline)
        .map(|p| l.points(p))
        .filter(|pts| pts.iter().any(|q| dist(*q).abs() > 1e-6))
        .map(|pts| Bounds2::from_points(pts.iter().copied()))
        .collect()
}

#[test]
fn fenceline_draws_a_circle_in_every_gap() {
    let fence = library::standard_linetypes().into_iter().find(|l| l.name == "FENCELINE1").unwrap();
    let (a, b) = (Vec2::ZERO, Vec2::new(9.5, 0.0));
    let d = drawing(fence, a, b);
    let marks = off_line(&d, &Options::default(), a, b);
    // Pattern 0.25 on, 0.1 off, "o", 0.1 off, 0.5 on = 0.95: ten repeats along 9.5.
    assert_eq!(marks.len(), 10, "{marks:?}");
    for (i, m) in marks.iter().enumerate() {
        let gap = i as f64 * 0.95 + 0.25;
        assert!(m.min.x > gap && m.max.x < gap + 0.2, "mark {i} at {m:?} outside its gap");
        // Centred on the line.
        assert!((m.min.y + m.max.y).abs() < 0.01 && m.max.y > 0.02, "{m:?}");
    }
    // No text at all when text is off (coarse previews).
    assert!(off_line(&d, &Options { text: false, ..Options::default() }, a, b).is_empty());
}

fn gas(absolute: bool) -> Linetype {
    Linetype {
        name: "GAS".into(),
        description: "Gas line ---- GAS ---- GAS ----".into(),
        pattern: vec![
            DashElement::dash(0.5),
            DashElement {
                text: Some("GAS".into()),
                style: Some("Standard".into()),
                scale: 0.1,
                absolute,
                offset: Vec2::new(-0.1, -0.05),
                ..DashElement::dash(-0.2)
            },
            DashElement::dash(-0.25),
        ],
    }
}

#[test]
fn text_follows_the_line_or_keeps_an_absolute_rotation() {
    // A vertical line: relative text runs up the line, absolute text stays horizontal.
    let (a, b) = (Vec2::ZERO, Vec2::new(0.0, 1.0));
    for absolute in [false, true] {
        let marks = off_line(&drawing(gas(absolute), a, b), &Options::default(), a, b);
        assert!(!marks.is_empty(), "absolute {absolute}: no text");
        let all = Bounds2::from_points(marks.iter().flat_map(|m| [m.min, m.max]));
        // Anchor 0.7 up the line, shifted 0.1 back along it and 0.05 to its right: the text
        // starts at (0.05, 0.6).
        assert!((all.min.y - 0.6).abs() < 0.01, "absolute {absolute}: {all:?}");
        if absolute {
            assert!(all.width() > all.height(), "{all:?}");
            assert!((all.min.x - 0.05).abs() < 0.01, "{all:?}");
        } else {
            assert!(all.height() > all.width(), "{all:?}");
            assert!((all.max.x - 0.05).abs() < 0.01, "{all:?}");
        }
    }
}

#[test]
fn text_height_follows_the_linetype_scale() {
    let (a, b) = (Vec2::ZERO, Vec2::new(10.0, 0.0));
    let height = |ltscale: f64| {
        let mut d = drawing(gas(false), a, b);
        d.header.set_f64("LTSCALE", ltscale);
        let marks = off_line(&d, &Options::default(), a, b);
        marks.iter().map(|m| m.max.y).fold(f64::MIN, f64::max) - marks.iter().map(|m| m.min.y).fold(f64::MAX, f64::min)
    };
    let (h1, h2) = (height(1.0), height(2.0));
    assert!(h1 > 0.05 && h1 < 0.2, "{h1}");
    assert!((h2 / h1 - 2.0).abs() < 1e-6, "{h1} {h2}");
}
