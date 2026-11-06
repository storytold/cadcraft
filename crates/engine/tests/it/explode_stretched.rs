//! EXPLODE of a block reference whose X and Y scales differ: its circles, arcs, ellipses and
//! polyline arcs come out as the ellipses and elliptical arcs the reference draws, not as circles
//! scaled by the mean of the two scales.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Entity, EntityKind};
use serde_json::json;

const AT: [f64; 2] = [100.0, 50.0];
const ROTATION: f64 = 30.0;

/// Block S (base 0,0): a circle (r 5 at 0,0), an arc (r 4 at 20,0, 0°..90°), an elliptical arc
/// (centre 40,0, major 6 along X, ratio 0.5, 0°..180°), a slot polyline (two lines, two
/// semicircles of r 2 at 60,0 and 70,0) and a line. One reference to it at `AT`, rotated by
/// `ROTATION`, scaled by `scale`, exploded.
fn exploded(scale: [f64; 2]) -> Vec<Entity> {
    let mut s = Session::new();
    let mut hs = Vec::new();
    for (id, p) in [
        ("circle", json!({"center": [0, 0], "radius": 5})),
        ("arc", json!({"center": [20, 0], "radius": 4, "start": 0, "end": 90})),
        ("ellipse", json!({"center": [40, 0], "major": [6, 0], "ratio": 0.5, "start": 0, "end": 180})),
        ("pline", json!({"vertices": [[60, -2], {"p": [70, -2], "bulge": 1}, [70, 2], {"p": [60, 2], "bulge": 1}], "closed": true})),
        ("line", json!({"points": [[0, 10], [10, 10]]})),
    ] {
        s.execute(id, &p).unwrap();
        hs.push(s.doc().unwrap().model.iter().last().unwrap().handle.hex());
    }
    s.execute("block", &json!({"name": "S", "base": [0, 0], "keep": "delete", "handles": hs})).unwrap();
    let r = s.execute("insert", &json!({"name": "S", "at": AT, "scale": scale, "rotation": ROTATION})).unwrap();
    s.execute("explode", &json!({"handles": [r["handle"]]})).unwrap();
    s.doc().unwrap().model.iter().map(|e| (**e).clone()).collect()
}

/// A drawing point back in block coordinates.
fn to_block(p: [f64; 2], scale: [f64; 2]) -> [f64; 2] {
    let (s, c) = (-ROTATION.to_radians()).sin_cos();
    let (dx, dy) = (p[0] - AT[0], p[1] - AT[1]);
    [(dx * c - dy * s) / scale[0], (dx * s + dy * c) / scale[1]]
}

/// Points along an ELLIPSE from its start to its end parameter.
fn samples(e: &cadcraft_engine::doc::Ellipse) -> Vec<[f64; 2]> {
    let mut sweep = e.end - e.start;
    if sweep <= 0.0 {
        sweep += std::f64::consts::TAU;
    }
    let (mx, my) = (e.major.x, e.major.y);
    let (nx, ny) = (-my * e.ratio, mx * e.ratio);
    (0..=16)
        .map(|i| {
            let t = e.start + sweep * f64::from(i) / 16.0;
            [e.center.x + mx * t.cos() + nx * t.sin(), e.center.y + my * t.cos() + ny * t.sin()]
        })
        .collect()
}

fn ellipses(v: &[Entity]) -> Vec<cadcraft_engine::doc::Ellipse> {
    v.iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Ellipse(el) => Some(el.clone()),
            _ => None,
        })
        .collect()
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9
}

/// Every point of `el`, taken back into the block, lies on the circle of radius `r` about `c`
/// in block coordinates, and its ends are `ends` (the whole circle when `None`).
fn on_circle(el: &cadcraft_engine::doc::Ellipse, scale: [f64; 2], c: [f64; 2], r: f64, ends: Option<([f64; 2], [f64; 2])>) {
    let pts: Vec<[f64; 2]> = samples(el).into_iter().map(|p| to_block(p, scale)).collect();
    for q in &pts {
        let d = (q[0] - c[0]).hypot(q[1] - c[1]);
        assert!((d - r).abs() < 1e-9, "{q:?} is {d} from {c:?}, not {r}: {el:?}");
    }
    if let Some((a, b)) = ends {
        let (first, last) = (pts[0], pts[pts.len() - 1]);
        assert!(near(first, a) && near(last, b) || near(first, b) && near(last, a), "ends {first:?} {last:?}, expected {a:?} {b:?}");
    }
}

#[test]
fn explode_of_a_stretched_reference_draws_circles_and_arcs_as_ellipses() {
    for scale in [[2.0, 0.5], [-2.0, 0.5], [1.0, 3.0]] {
        let v = exploded(scale);
        assert!(!v.iter().any(|e| matches!(e.kind, EntityKind::Circle(_) | EntityKind::Arc(_) | EntityKind::LwPolyline(_))), "{scale:?}: {v:?}");
        let els = ellipses(&v);
        // Circle, arc, elliptical arc and the slot's two semicircles.
        assert_eq!(els.len(), 5, "{scale:?}: {els:?}");
        let at = |c: [f64; 2]| els.iter().filter(move |e| near(to_block([e.center.x, e.center.y], scale), c));
        let circle = at([0.0, 0.0]).next().unwrap();
        on_circle(circle, scale, [0.0, 0.0], 5.0, None);
        // Its axes are the scaled radii: 10 and 2.5 for 2 x 0.5.
        let major = circle.major.x.hypot(circle.major.y);
        let (big, small) = (5.0 * scale[0].abs().max(scale[1].abs()), 5.0 * scale[0].abs().min(scale[1].abs()));
        assert!((major - big).abs() < 1e-9 && (major * circle.ratio - small).abs() < 1e-9, "{circle:?}");
        on_circle(at([20.0, 0.0]).next().unwrap(), scale, [20.0, 0.0], 4.0, Some(([24.0, 0.0], [20.0, 4.0])));
        on_circle(at([60.0, 0.0]).next().unwrap(), scale, [60.0, 0.0], 2.0, Some(([60.0, 2.0], [60.0, -2.0])));
        on_circle(at([70.0, 0.0]).next().unwrap(), scale, [70.0, 0.0], 2.0, Some(([70.0, -2.0], [70.0, 2.0])));
        // The elliptical arc: its points, back in the block, are on the block's ellipse
        // ((x-40)/6)^2 + (y/3)^2 = 1, from (46,0) over the top to (34,0).
        let ell = at([40.0, 0.0]).next().unwrap();
        let pts: Vec<[f64; 2]> = samples(ell).into_iter().map(|p| to_block(p, scale)).collect();
        for q in &pts {
            let f = ((q[0] - 40.0) / 6.0).powi(2) + (q[1] / 3.0).powi(2);
            assert!((f - 1.0).abs() < 1e-9 && q[1] > -1e-9, "{q:?}: {ell:?}");
        }
        let (first, last) = (pts[0], pts[pts.len() - 1]);
        assert!(near(first, [46.0, 0.0]) && near(last, [34.0, 0.0]) || near(first, [34.0, 0.0]) && near(last, [46.0, 0.0]), "{pts:?}");
        // The slot's straight sides and the line are lines, where the reference draws them.
        let lines: Vec<[[f64; 2]; 2]> = v
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::Line(l) => Some([to_block([l.a.x, l.a.y], scale), to_block([l.b.x, l.b.y], scale)]),
                _ => None,
            })
            .collect();
        assert_eq!(lines.len(), 3);
        for [a, b] in [[[60.0, -2.0], [70.0, -2.0]], [[70.0, 2.0], [60.0, 2.0]], [[0.0, 10.0], [10.0, 10.0]]] {
            assert!(lines.iter().any(|l| near(l[0], a) && near(l[1], b)), "{a:?}-{b:?} not in {lines:?}");
        }
    }
}

#[test]
fn explode_of_an_evenly_scaled_or_mirrored_reference_keeps_circles_arcs_and_polylines() {
    for scale in [[2.0, 2.0], [-1.0, 1.0]] {
        let v = exploded(scale);
        let count = |f: fn(&EntityKind) -> bool| v.iter().filter(|e| f(&e.kind)).count();
        assert_eq!(count(|k| matches!(k, EntityKind::Circle(_))), 1, "{scale:?}");
        assert_eq!(count(|k| matches!(k, EntityKind::Arc(_))), 1, "{scale:?}");
        assert_eq!(count(|k| matches!(k, EntityKind::LwPolyline(_))), 1, "{scale:?}");
        assert_eq!(count(|k| matches!(k, EntityKind::Ellipse(_))), 1, "{scale:?}");
        let r = v.iter().find_map(|e| if let EntityKind::Circle(c) = &e.kind { Some(c.radius) } else { None }).unwrap();
        assert!((r - 5.0 * scale[0].abs()).abs() < 1e-9);
    }
}
