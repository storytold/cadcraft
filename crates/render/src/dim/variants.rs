use super::text::{apply_override, inspected, linear_text};
use super::{Arrowhead, DimConfig, DimGeometry, DimKind, Vec2, ext_line, readable};
use crate::units::format_angle_with_separator;
use cadcraft_geom::{Arc, ccw_sweep};

pub(super) fn draw_jogged_radius(config: &DimConfig<'_>, g: &mut DimGeometry, text_angle_out: &mut f64, text_mid_out: &mut Vec2) {
    let d = config.dimension;
    let st = config.style;
    let asz = config.arrow_size;
    let gap = config.gap;
    let tsz = config.tick_size;
    let blk2 = config.second_arrow;
    let measure = |text: &str| config.measure(text);
    let end = |geometry: &mut DimGeometry, kind: Arrowhead, tip: Vec2, direction: Vec2| config.end(geometry, kind, tip, direction);
    let mut text_angle = 0.0;
    let mut text_mid = *text_mid_out;
    'jogged: {
        // DIMJOGGED: from the arrowhead on the arc toward the centre, a jog at DIMJOGANG,
        // then on to the overriding centre (parallel to the first part).
        let center = d.defpt.xy();
        let tip = d.p15.xy();
        let r = center.dist(tip);
        let j = d.marks.jogged.clone().unwrap_or_default();
        let (o, jp) = (j.center.xy(), j.jog.xy());
        g.mtext = linear_text(r, d, st, "R");
        // A NaN or infinite radius leaves no line to draw (and NaN is no bound for `clamp`).
        if !r.is_finite() {
            break 'jogged;
        }
        let (tw, _) = measure(&g.mtext);
        let u = (tip - center).normalized();
        let u = if u == Vec2::ZERO || !u.is_finite() { Vec2::X } else { u };
        let n = u.perp();
        let h = (o - center).dot(n);
        let ang = if st.jog_angle.is_finite() { st.jog_angle.clamp(5.0, 90.0) } else { 45.0 }.to_radians();
        let run = h.abs() / ang.tan().max(1e-6);
        let tj = (jp - center).dot(u).clamp(0.0, r);
        let (j1, j2) = (center + u * (tj + run / 2.0), center + u * (tj - run / 2.0) + n * h);
        g.dim(vec![tip, j1, j2, o]);
        end(g, blk2, tip, u);
        let tpos = if d.user_text_pos { text_mid } else { tip + u * (asz.max(tsz) * 3.0) };
        if tpos.dist(center) > r {
            g.dim(vec![tip, tpos]);
        }
        text_angle = if st.text_outside_horizontal { 0.0 } else { readable(u.angle()) };
        if !d.user_text_pos {
            let x = Vec2::from_angle(text_angle);
            let x = if x.dot(u) >= 0.0 { x } else { -x };
            text_mid = tpos + x * (tw / 2.0 + gap);
        }
    }
    *text_angle_out = text_angle;
    *text_mid_out = text_mid;
}

pub(super) fn draw_radius_or_diameter(config: &DimConfig<'_>, g: &mut DimGeometry, text_angle_out: &mut f64, text_mid_out: &mut Vec2) {
    let d = config.dimension;
    let st = config.style;
    let k = config.scale;
    let asz = config.arrow_size;
    let gap = config.gap;
    let tsz = config.tick_size;
    let blk1 = config.first_arrow;
    let blk2 = config.second_arrow;
    let measure = |text: &str| config.measure(text);
    let end = |geometry: &mut DimGeometry, kind: Arrowhead, tip: Vec2, direction: Vec2| config.end(geometry, kind, tip, direction);
    let text_angle: f64;
    let mut text_mid = *text_mid_out;

    let a = d.defpt.xy();
    let b = d.p15.xy();
    let radius = matches!(d.kind, DimKind::Radius);
    let (center, r) = if radius { (a, a.dist(b)) } else { (a.mid(b), a.dist(b) / 2.0) };
    let meas = if radius { r } else { 2.0 * r };
    g.mtext = linear_text(meas, d, st, if radius { "R" } else { "%%c" });
    g.measurement = meas;
    let (tw, tht) = measure(&g.mtext);
    let u = (b - center).normalized();
    let u = if u == Vec2::ZERO || !u.is_finite() { Vec2::X } else { u };
    let tip = center + u * r;
    let tpos = if d.user_text_pos { text_mid } else { tip + u * (asz.max(tsz) * 3.0) };
    // Text placed inside the circle (a dimension line location picked inside): the text
    // sits on the dimension line, which runs from the text out to the arrowhead (for a
    // diameter it spans the circle, broken around the text). DIMTIH turns it horizontal.
    let inside = d.user_text_pos && text_mid.dist(center) < r;
    let horizontal = if inside { st.text_inside_horizontal } else { st.text_outside_horizontal };
    if inside {
        let tr = Vec2::from_angle(if horizontal { 0.0 } else { readable(u.angle()) });
        let half = if tw > 0.0 { (tw * tr.dot(u).abs() + tht * tr.perp().dot(u).abs()) / 2.0 + gap } else { 0.0 };
        let start = if radius { center } else { center - u * r };
        let (t0, t1) = ((text_mid - start).dot(u) - half, (text_mid - start).dot(u) + half);
        if !radius {
            if t0 > 0.0 {
                g.dim(vec![start, start + u * t0]);
            }
            end(g, blk1, start, -u);
        }
        if t1 < tip.dist(start) {
            g.dim(vec![start + u * t1.max(0.0), tip]);
        }
    } else {
        // Text outside: a diameter line spans the circle and a radius line runs in to the
        // centre only with DIMTOFL; otherwise just the leader to the text.
        if st.force_line_inside {
            let start = if radius { center } else { center - u * r };
            g.dim(vec![start, tip]);
            if !radius {
                end(g, blk1, start, -u);
            }
        }
        // Text the user placed: the leader stops short of it by the offset the default
        // placement leaves between leader end and text.
        let lead = if !d.user_text_pos {
            tpos
        } else if horizontal {
            text_mid - Vec2::new(tw / 2.0 + gap, 0.0) * if u.x >= 0.0 { 1.0 } else { -1.0 }
        } else {
            text_mid - u * (tw / 2.0 + gap)
        };
        g.dim(vec![tip, lead]);
    }
    end(g, blk2, tip, u);
    if horizontal {
        text_angle = 0.0;
        let sx = if u.x >= 0.0 { 1.0 } else { -1.0 };
        if !d.user_text_pos {
            text_mid = tpos + Vec2::new(tw / 2.0 + gap, 0.0) * sx;
        }
    } else {
        text_angle = readable(u.angle());
        if !d.user_text_pos {
            text_mid = tpos + u * (tw / 2.0 + gap);
        }
    }
    // The DIMCEN mark is drawn only when no dimension line runs inside the circle (text
    // outside without DIMTOFL), never with a line through the centre.
    let line_outside = !st.force_line_inside && tpos.dist(center) >= r;
    if line_outside && st.center_mark != 0.0 && st.center_mark.is_finite() {
        let c = st.center_mark.abs() * k;
        g.ext(vec![center - Vec2::X * c, center + Vec2::X * c]);
        g.ext(vec![center - Vec2::Y * c, center + Vec2::Y * c]);
        if st.center_mark < 0.0 && r > 2.0 * c {
            // Centre lines out past the circle.
            for dv in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
                g.ext(vec![center + dv * (2.0 * c), center + dv * (r + c)]);
            }
        }
    }
    *text_angle_out = text_angle;
    *text_mid_out = text_mid;
}

pub(super) fn draw_angular(config: &DimConfig<'_>, g: &mut DimGeometry, text_angle_out: &mut f64, text_mid_out: &mut Vec2) {
    let d = config.dimension;
    let st = config.style;
    let k = config.scale;
    let gap = config.gap;
    let blk1 = config.first_arrow;
    let blk2 = config.second_arrow;
    let measure = |text: &str| config.measure(text);
    let end = |geometry: &mut DimGeometry, kind: Arrowhead, tip: Vec2, direction: Vec2| config.end(geometry, kind, tip, direction);
    let mut text_mid = *text_mid_out;

    // Angular3P: p15 vertex, p13/p14 on the legs, defpt on the arc.
    // Angular (2 lines): lines p13-p14 and defpt-p15; p16 on the arc.
    let (vertex, a1, a2, arc_pt) = if matches!(d.kind, DimKind::Angular3P) {
        (d.p15.xy(), d.p13.xy(), d.p14.xy(), d.defpt.xy())
    } else {
        let v = cadcraft_geom::line_line_infinite(d.p13.xy(), d.p14.xy(), d.defpt.xy(), d.p15.xy()).map(|x| x.0).unwrap_or(d.p14.xy());
        (v, d.p14.xy(), d.p15.xy(), d.p16.xy())
    };
    let r = vertex.dist(arc_pt).max(1e-9);
    let s = vertex.angle_to(a1);
    let e = vertex.angle_to(a2);
    let arc = if cadcraft_geom::angle_in_sweep(vertex.angle_to(arc_pt), s, e) { Arc::new(vertex, r, s, e) } else { Arc::new(vertex, r, e, s) };
    let sweep = ccw_sweep(arc.start, arc.end);
    g.measurement = sweep;
    // DIMAZIN 1 / 2 suppress leading / trailing zeros (DIMZIN's bits 4 / 8).
    let azin = (st.angular_zero_suppression & 3) * 4;
    let txt = format_angle_with_separator(sweep, st.angular_unit, st.angular_decimals, azin, &st.decimal_separator);
    // DIMPOST is a linear-dimension prefix/suffix; angles have their own format only.
    g.mtext = inspected(apply_override(&txt, d), d);
    let mut pts = Vec::new();
    arc.tessellate(r * 1e-3, &mut pts);
    g.dim(pts);
    let sp = arc.start_point();
    let ep = arc.end_point();
    // The arc's ends with the outward tangents there, paired with leg 1 and leg 2: when
    // the arc location is outside the leg 1 → leg 2 sweep the arc runs from leg 2 to
    // leg 1, and each leg keeps its own extension line, DIMSE1/2 and DIMBLK1/2.
    let (start, finish) = ((sp, -(sp - vertex).perp().normalized()), (ep, (ep - vertex).perp().normalized()));
    let ((p1, dir1), (p2, dir2)) = if cadcraft_geom::angle_in_sweep(vertex.angle_to(arc_pt), s, e) { (start, finish) } else { (finish, start) };
    if !st.suppress_ext1 {
        g.ext_n(ext_line(vertex + (p1 - vertex).normalized() * vertex.dist(a1).min(r), p1, st, k), false);
    }
    if !st.suppress_ext2 {
        g.ext_n(ext_line(vertex + (p2 - vertex).normalized() * vertex.dist(a2).min(r), p2, st, k), true);
    }
    end(g, blk1, p1, dir1);
    end(g, blk2, p2, dir2);
    let (tw, tht) = measure(&g.mtext);
    let mid = arc.mid_point();
    let radial = (mid - vertex).normalized();
    let text_angle = if st.text_inside_horizontal { 0.0 } else { readable(radial.perp().angle()) };
    if !d.user_text_pos {
        let tr = Vec2::from_angle(text_angle);
        let off = gap + (tw * tr.dot(radial).abs() + tht * tr.perp().dot(radial).abs()) / 2.0;
        text_mid = mid + radial * off;
    }
    *text_angle_out = text_angle;
    *text_mid_out = text_mid;
}

pub(super) fn draw_ordinate(config: &DimConfig<'_>, g: &mut DimGeometry, text_angle_out: &mut f64, text_mid_out: &mut Vec2, x_type: bool) {
    let d = config.dimension;
    let st = config.style;
    let gap = config.gap;
    let measure = |text: &str| config.measure(text);
    let mut text_angle = 0.0;
    let mut text_mid = *text_mid_out;

    let feature = d.p13.xy();
    let leader_end = d.p14.xy();
    let origin = d.defpt.xy();
    let meas = if x_type { (feature.x - origin.x).abs() } else { (feature.y - origin.y).abs() };
    g.mtext = linear_text(meas, d, st, "");
    g.measurement = meas;
    let (tw, _) = measure(&g.mtext);
    g.dim(vec![feature, leader_end]);
    if x_type {
        text_angle = std::f64::consts::FRAC_PI_2;
    }
    if !d.user_text_pos {
        let dirv = (leader_end - feature).normalized();
        let dirv = if dirv == Vec2::ZERO { if x_type { Vec2::Y } else { Vec2::X } } else { dirv };
        text_mid = leader_end + dirv * (tw / 2.0 + gap);
    }
    *text_angle_out = text_angle;
    *text_mid_out = text_mid;
}

pub(super) fn draw_arc_length(config: &DimConfig<'_>, g: &mut DimGeometry, text_angle_out: &mut f64, text_mid_out: &mut Vec2) {
    let d = config.dimension;
    let st = config.style;
    let k = config.scale;
    let th = config.text_height;
    let gap = config.gap;
    let blk1 = config.first_arrow;
    let blk2 = config.second_arrow;
    let measure = |text: &str| config.measure(text);
    let end = |geometry: &mut DimGeometry, kind: Arrowhead, tip: Vec2, direction: Vec2| config.end(geometry, kind, tip, direction);
    let mut text_mid = *text_mid_out;

    let c = d.p15.xy();
    let r = c.dist(d.p13.xy());
    let arc = Arc::new(c, c.dist(d.defpt.xy()).max(1e-9), c.angle_to(d.p13.xy()), c.angle_to(d.p14.xy()));
    let meas = r * arc.sweep();
    // DIMARCSYM: the arc symbol before the text (0), over it (1) or none (2).
    g.mtext = linear_text(meas, d, st, if st.arc_symbol == 0 { "⌒" } else { "" });
    g.measurement = meas;
    let mut pts = Vec::new();
    arc.tessellate(arc.radius * 1e-3, &mut pts);
    g.dim(pts);
    for (i, (p, q)) in [(d.p13.xy(), arc.start_point()), (d.p14.xy(), arc.end_point())].into_iter().enumerate() {
        g.ext_n(ext_line(p, q, st, k), i == 1);
    }
    end(g, blk1, arc.start_point(), -(arc.start_point() - c).perp().normalized());
    end(g, blk2, arc.end_point(), (arc.end_point() - c).perp().normalized());
    let m = arc.mid_point();
    let radial = (m - c).normalized();
    let text_angle = if st.text_inside_horizontal { 0.0 } else { readable(radial.perp().angle()) };
    if !d.user_text_pos {
        text_mid = m + radial * (gap + th / 2.0);
    }
    if st.arc_symbol == 1 {
        // A small arc over the text.
        let (tw, tht) = measure(&g.mtext);
        let (x, y) = (Vec2::from_angle(text_angle), Vec2::from_angle(text_angle).perp());
        let half = (tw / 2.0).max(th / 2.0);
        let base = text_mid + y * (tht / 2.0 + gap / 2.0);
        let sym: Vec<Vec2> = (0..=12)
            .map(|i| {
                let t = f64::from(i) / 6.0 - 1.0;
                base + x * (t * half) + y * ((1.0 - t * t).max(0.0).sqrt() * th * 0.35)
            })
            .collect();
        g.text_mark(sym);
    }
    *text_angle_out = text_angle;
    *text_mid_out = text_mid;
}
