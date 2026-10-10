use crate::{Arc, Circle, EPS, Line, Segment, Vec2};

/// Intersection of two infinite lines through (a1,a2) and (b1,b2), with parameters on each.
pub fn line_line_infinite(a1: Vec2, a2: Vec2, b1: Vec2, b2: Vec2) -> Option<(Vec2, f64, f64)> {
    let r = a2 - a1;
    let s = b2 - b1;
    let den = r.cross(s);
    if den.abs() < 1e-14 * (r.len() * s.len()).max(1e-300) {
        return None;
    }
    let t = (b1 - a1).cross(s) / den;
    let u = (b1 - a1).cross(r) / den;
    Some((a1 + r * t, t, u))
}

/// Intersection of two finite segments.
pub fn line_line(a: &Line, b: &Line) -> Option<Vec2> {
    let (p, t, u) = line_line_infinite(a.a, a.b, b.a, b.b)?;
    let tol = 1e-9;
    ((-tol..=1.0 + tol).contains(&t) && (-tol..=1.0 + tol).contains(&u)).then_some(p)
}

/// Intersections of the infinite line through `l` with a circle. Returns points with line params.
pub fn line_circle(l: &Line, c: &Circle) -> Vec<(Vec2, f64)> {
    let d = l.b - l.a;
    let f = l.a - c.center;
    let a = d.dot(d);
    if !(a >= EPS * EPS && a.is_finite() && c.radius.is_finite() && f.is_finite()) {
        return Vec::new();
    }
    // The quadratic discriminant subtracts two O(length^4) numbers. For a
    // long construction line nearly tangent to a small circle, that loses the
    // gap entirely and reports a false intersection. Measure from the closest
    // point on the infinite line instead.
    let t = -f.dot(d) / a;
    let nearest = f + d * t;
    let delta = c.radius * c.radius - nearest.len2();
    let tolerance = 1e-12 * c.radius * c.radius;
    if !delta.is_finite() || delta < -tolerance {
        return Vec::new();
    }
    let foot = c.center + nearest;
    if delta <= tolerance {
        return vec![(foot, t)];
    }
    let offset = (delta / a).sqrt();
    let (t1, t2) = (t - offset, t + offset);
    let step = d * offset;
    // Construct the intersections around the foot, not from a tiny change
    // to t on a very long line (which loses the small distance again).
    vec![(foot - step, t1), (foot + step, t2)]
}

/// Intersections of two full circles.
pub fn circle_circle(c1: &Circle, c2: &Circle) -> Vec<Vec2> {
    let d = c1.center.dist(c2.center);
    if d < EPS || d > c1.radius + c2.radius + 1e-9 || d < (c1.radius - c2.radius).abs() - 1e-9 {
        return Vec::new();
    }
    let a = (c1.radius * c1.radius - c2.radius * c2.radius + d * d) / (2.0 * d);
    let h2 = c1.radius * c1.radius - a * a;
    let dir = (c2.center - c1.center) / d;
    let p = c1.center + dir * a;
    if h2 <= 1e-12 {
        return vec![p];
    }
    let h = h2.sqrt();
    vec![p + dir.perp() * h, p - dir.perp() * h]
}

fn on_arc(a: &Arc, p: Vec2) -> bool {
    a.contains_angle(a.center.angle_to(p))
}

/// Intersections between two segments (finite).
pub fn intersect_segments(s1: &Segment, s2: &Segment) -> Vec<Vec2> {
    intersect_segments_ext(s1, s2, false)
}

/// Intersections; with `extend` true, lines are infinite and arcs are full circles
/// (used by TRIM/EXTEND edge mode and apparent intersections).
pub fn intersect_segments_ext(s1: &Segment, s2: &Segment, extend: bool) -> Vec<Vec2> {
    let tol = 1e-9;
    match (s1, s2) {
        (Segment::Line(a), Segment::Line(b)) => {
            if extend {
                line_line_infinite(a.a, a.b, b.a, b.b).map(|x| vec![x.0]).unwrap_or_default()
            } else {
                line_line(a, b).into_iter().collect()
            }
        }
        (Segment::Line(l), Segment::Arc { arc, .. }) | (Segment::Arc { arc, .. }, Segment::Line(l)) => {
            line_circle(l, &Circle::new(arc.center, arc.radius))
                .into_iter()
                .filter(|(p, t)| extend || ((-tol..=1.0 + tol).contains(t) && on_arc(arc, *p)))
                .map(|(p, _)| p)
                .collect()
        }
        (Segment::Arc { arc: a, .. }, Segment::Arc { arc: b, .. }) => {
            circle_circle(&Circle::new(a.center, a.radius), &Circle::new(b.center, b.radius))
                .into_iter()
                .filter(|p| extend || (on_arc(a, *p) && on_arc(b, *p)))
                .collect()
        }
    }
}

pub use intersect_segments_ext as intersect_ext;
