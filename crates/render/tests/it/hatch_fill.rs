//! Hatch drawing: island detection styles, background colour, two-colour gradients and
//! pattern definitions kept from a file (issues #289, #290).

use cadcraft_color::{Color, Rgb, aci_rgb};
use cadcraft_doc::library::PatternLine;
use cadcraft_doc::{Common, Drawing, EntityKind, Gradient, Hatch, HatchLoop, Layer, Space};
use cadcraft_geom::{PolyVertex, Vec2};
use cadcraft_render::{DisplayList, Kind, Options, build};

fn square(lo: f64, hi: f64) -> HatchLoop {
    let v = [(lo, lo), (hi, lo), (hi, hi), (lo, hi)].into_iter().map(|(x, y)| PolyVertex::new(Vec2::new(x, y))).collect();
    HatchLoop { vertices: v, outer: lo == 0.0, sources: Vec::new() }
}

fn hatch(pattern: &str, loops: Vec<HatchLoop>) -> Hatch {
    Hatch {
        pattern: pattern.into(),
        solid: pattern == "SOLID",
        loops,
        scale: 1.0,
        angle: 0.0,
        associative: false,
        style: 0,
        elevation: 0.0,
        gradient: None,
        origin: Vec2::ZERO,
        background: None,
        pattern_lines: Vec::new(),
    }
}

/// Square 0..10 with the island 2..8 and the island-in-an-island 4..6.
fn nested(pattern: &str, style: u8) -> Hatch {
    Hatch { style, ..hatch(pattern, vec![square(0.0, 10.0), square(2.0, 8.0), square(4.0, 6.0)]) }
}

fn draw(h: Hatch, layer: &str) -> DisplayList {
    let mut d = Drawing::new_imperial();
    d.layers.push(Layer { color: Color::Index(1), ..Layer::new("RED") });
    let common = Common { layer: layer.into(), ..Common::default() };
    d.add(&Space::Model, common, EntityKind::Hatch(h)).expect("add");
    build(&d, &Space::Model, &Options::default())
}

fn tri_area(l: &DisplayList, color: Option<Rgb>) -> f64 {
    l.prims
        .iter()
        .filter(|p| p.kind == Kind::Tris && color.is_none_or(|c| p.color == c))
        .flat_map(|p| l.points(p).chunks(3))
        .map(|t| ((t[1] - t[0]).cross(t[2] - t[0]) / 2.0).abs())
        .sum()
}

/// Mid points of the pattern line runs.
fn line_mids(l: &DisplayList) -> Vec<Vec2> {
    l.prims.iter().filter(|p| p.kind == Kind::Polyline).filter_map(|p| Some(l.points(p).first()?.mid(*l.points(p).last()?))).collect()
}

#[test]
fn island_detection_styles() {
    // Normal: odd parity (100 - 36 + 4); outer: the innermost island stays empty; ignore: all.
    for (style, area) in [(0, 68.0), (1, 64.0), (2, 100.0)] {
        let a = tri_area(&draw(nested("SOLID", style), "0"), None);
        assert!((a - area).abs() < 1e-6, "style {style}: {a}");
    }
    let inside = |p: &Vec2, lo: f64, hi: f64| p.x > lo && p.x < hi && p.y > lo && p.y < hi;
    let island = |p: &Vec2| inside(p, 2.0, 8.0) && !inside(p, 4.0, 6.0);
    let normal = line_mids(&draw(nested("ANSI31", 0), "0"));
    assert!(!normal.iter().any(island) && normal.iter().any(|p| inside(p, 4.0, 6.0)));
    let outer = line_mids(&draw(nested("ANSI31", 1), "0"));
    assert!(!outer.is_empty() && !outer.iter().any(|p| inside(p, 2.0, 8.0)), "outer hatches only the outer band");
    let ignore = line_mids(&draw(nested("ANSI31", 2), "0"));
    assert!(ignore.iter().any(island) && ignore.iter().any(|p| inside(p, 4.0, 6.0)));
}

#[test]
fn pattern_background_is_filled_under_the_lines() {
    let plain = draw(nested("ANSI31", 0), "0");
    assert!(plain.prims.iter().all(|p| p.kind != Kind::Tris));
    let l = draw(Hatch { background: Some(Color::Index(3)), ..nested("ANSI31", 0) }, "0");
    assert!((tri_area(&l, Some(aci_rgb(3))) - 68.0).abs() < 1e-6);
    // The fill comes first so the pattern lines draw over it.
    assert_eq!(l.prims.first().map(|p| p.kind), Some(Kind::Tris));
    assert!(l.prims.iter().any(|p| p.kind == Kind::Polyline));
}

fn gradient(name: &str, angle: f64, color1: Color) -> Hatch {
    let g = Gradient { name: name.into(), color1, color2: Color::Index(3), angle, centered: true };
    Hatch { gradient: Some(g), ..hatch("SOLID", vec![square(0.0, 10.0)]) }
}

/// Mean position of the triangles drawn in `c` (the base fill has the middle colour).
fn centre_of(l: &DisplayList, c: Rgb) -> Option<Vec2> {
    let pts: Vec<Vec2> = l.prims.iter().filter(|p| p.kind == Kind::Tris && p.color == c).flat_map(|p| l.points(p).iter().copied()).collect();
    (!pts.is_empty()).then(|| pts.iter().fold(Vec2::ZERO, |a, p| a + *p) * (1.0 / pts.len() as f64))
}

#[test]
fn linear_gradient_blends_from_the_layer_colour_to_colour_2() {
    let (red, green) = (aci_rgb(1), aci_rgb(3));
    let l = draw(gradient("LINEAR", 0.0, Color::ByLayer), "RED");
    assert!(!l.prims.iter().any(|p| p.color == Rgb(255, 255, 255)), "ByLayer colour 1 is the layer's red, not white");
    let (a, b) = (centre_of(&l, red).expect("red end"), centre_of(&l, green).expect("green end"));
    assert!(a.x < 1.0 && b.x > 9.0, "{a:?} {b:?}");
    assert!(l.prims.iter().filter(|p| p.kind == Kind::Tris).count() > 20, "blended in steps");
    // Base fill plus the steps cover the square twice.
    assert!((tri_area(&l, None) - 200.0).abs() < 1e-6);
    // 90°: the blend runs bottom to top.
    let l = draw(gradient("LINEAR", std::f64::consts::FRAC_PI_2, Color::ByLayer), "RED");
    let (a, b) = (centre_of(&l, red).expect("red end"), centre_of(&l, green).expect("green end"));
    assert!(a.y < 1.0 && b.y > 9.0, "{a:?} {b:?}");
}

/// Mean position of the gradient step closest in colour to `c` (the first fill is the base).
fn nearest_step(l: &DisplayList, c: Rgb) -> Vec2 {
    let d = |x: Rgb| (i32::from(x.0) - i32::from(c.0)).abs() + (i32::from(x.1) - i32::from(c.1)).abs() + (i32::from(x.2) - i32::from(c.2)).abs();
    let best = l.prims.iter().skip(1).filter(|p| p.kind == Kind::Tris).min_by_key(|p| d(p.color)).expect("steps");
    centre_of(l, best.color).expect("step")
}

#[test]
fn radial_and_inverted_gradients() {
    let (blue, green) = (aci_rgb(5), aci_rgb(3));
    let c = Vec2::new(5.0, 5.0);
    let l = draw(gradient("SPHERICAL", 0.0, Color::Index(5)), "0");
    assert!(nearest_step(&l, green).dist(c) < 0.5);
    let corners: Vec<Vec2> = l.prims.iter().filter(|p| p.kind == Kind::Tris && p.color == blue).flat_map(|p| l.points(p).iter().copied()).collect();
    assert!(!corners.is_empty() && corners.iter().all(|p| p.dist(c) > 4.5), "colour 1 only in the corners: {corners:?}");
    let l = draw(gradient("INVSPHERICAL", 0.0, Color::Index(5)), "0");
    assert!(nearest_step(&l, blue).dist(c) < 0.5);
    let l = draw(gradient("CYLINDER", 0.0, Color::Index(5)), "0");
    assert!((nearest_step(&l, green).x - 5.0).abs() < 0.5);
    let l = draw(gradient("HEMISPHERICAL", 0.0, Color::Index(5)), "0");
    assert!((nearest_step(&l, green) - Vec2::new(5.0, 0.0)).len() < 0.5);
    // Shifted: the highlight moves off the centre.
    let mut h = gradient("SPHERICAL", 0.0, Color::Index(5));
    if let Some(g) = h.gradient.as_mut() {
        g.centered = false;
    }
    assert!(nearest_step(&draw(h, "0"), green).dist(c) > 1.0);
}

#[test]
fn hostile_gradients_stay_bounded() {
    let mut h = gradient("NOSUCHGRADIENT", f64::NAN, Color::ByBlock);
    h.loops.push(square(-1e300, 1e300));
    let l = draw(h, "0");
    assert!(l.prims.len() <= 65);
    let l = draw(Hatch { style: 200, ..gradient("CURVED", 1e308, Color::Index(5)) }, "0");
    assert!(l.prims.len() <= 65 && l.tris.len() <= 3 * 200_000 + 6);
}

#[test]
fn pattern_lines_from_a_file_are_drawn_for_unknown_names() {
    // A grid: horizontal and vertical lines one unit apart.
    let fam = |angle| PatternLine { angle, origin: (0.0, 0.0), delta: (0.0, 1.0), dashes: vec![] };
    let h = Hatch { pattern_lines: vec![fam(0.0), fam(90.0)], ..hatch("MYGRID", vec![square(0.5, 9.5)]) };
    let l = draw(h, "0");
    let runs: Vec<&[Vec2]> = l.prims.iter().filter(|p| p.kind == Kind::Polyline).map(|p| l.points(p)).collect();
    assert_eq!(runs.len(), 18);
    assert!(runs.iter().all(|r| { r.len() == 2 && ((r[0].x - r[1].x).abs() < 1e-9 || (r[0].y - r[1].y).abs() < 1e-9) }));
    // Without a definition an unknown name still falls back to ANSI31 (diagonal lines).
    let l = draw(hatch("MYGRID", vec![square(0.5, 9.5)]), "0");
    let p = l.prims.iter().find(|p| p.kind == Kind::Polyline).map(|p| l.points(p)).expect("lines");
    assert!(((p[1] - p[0]).angle().to_degrees().rem_euclid(180.0) - 45.0).abs() < 1e-6);
}

#[test]
fn pattern_swatches_stay_in_their_square_and_show_a_few_repeats() {
    for p in cadcraft_doc::library::standard_patterns() {
        let runs = cadcraft_render::pattern_swatch(&p.lines, 1.0);
        if p.lines.is_empty() {
            assert!(runs.is_empty(), "{}", p.name);
            continue;
        }
        assert!(runs.len() >= 3, "{} has {} runs", p.name, runs.len());
        assert!(runs.len() <= 4000, "{} has {} runs", p.name, runs.len());
        for v in runs.iter().flatten() {
            assert!((-1e-9..=1.0 + 1e-9).contains(&v.x) && (-1e-9..=1.0 + 1e-9).contains(&v.y), "{} at {v:?}", p.name);
        }
    }
    assert!(cadcraft_render::pattern_swatch(&[], 1.0).is_empty());
}

#[test]
fn brick_joints_are_drawn_once_half_a_brick_apart_course_to_course() {
    let l = draw(hatch("BRICK", vec![square(0.0, 2.0)]), "0");
    // Vertical joints: (x, course) for every vertical segment, rounded.
    let mut joints: Vec<(i64, i64)> = l
        .prims
        .iter()
        .filter(|p| p.kind == Kind::Polyline)
        .filter_map(|p| match l.points(p) {
            [a, b] if (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() > 1e-9 => {
                Some(((a.x * 1000.0).round() as i64, ((a.y + b.y) / 2.0 / 0.25).floor() as i64))
            }
            _ => None,
        })
        .collect();
    let n = joints.len();
    joints.sort_unstable();
    joints.dedup();
    assert_eq!(joints.len(), n, "a joint drawn twice");
    // Within a course joints are a brick (0.5) apart; the next course is offset half a brick.
    let xs = |course: i64| joints.iter().filter(|j| j.1 == course).map(|j| j.0).collect::<Vec<_>>();
    for course in 0..4 {
        let x = xs(course);
        assert!(x.len() >= 3, "course {course}: {x:?}");
        assert!(x.windows(2).all(|w| w[1] - w[0] == 500), "course {course}: {x:?}");
        let next = xs(course + 1);
        assert!(next.iter().all(|nx| (nx - x[0]).rem_euclid(500) == 250), "courses {course}/{}: {x:?} {next:?}", course + 1);
    }
}
