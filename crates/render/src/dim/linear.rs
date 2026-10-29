use super::text::linear_text;
use super::{DimConfig, DimGeometry, DimKind, LineRole, Vec2, ext_line, readable};

struct LinearDimState {
    p1: Vec2,
    p2: Vec2,
    f1: Vec2,
    f2: Vec2,
    u: Vec2,
    n: Vec2,
    measurement: f64,
    text_width: f64,
    text_height: f64,
    has_text: bool,
    tick_like: bool,
    arrow_width: f64,
    arrows_inside: bool,
    text_outside: bool,
    just: u8,
    centred: bool,
    moved: bool,
    outside_arrows: bool,
    angle: f64,
    half_extent: f64,
    along_center: Vec2,
    side_out: Vec2,
    text_axis: Vec2,
    offset: Vec2,
    text_mid: Vec2,
}

pub(super) fn draw_linear(config: &DimConfig<'_>, g: &mut DimGeometry, text_angle_out: &mut f64, text_mid_out: &mut Vec2) {
    let mut state = prepare_linear(config, g, *text_mid_out);
    position_linear_text(config, &mut state);
    draw_linear_lines(config, g, &state);
    *text_angle_out = state.angle;
    *text_mid_out = state.text_mid;
}

fn prepare_linear(config: &DimConfig<'_>, g: &mut DimGeometry, text_mid: Vec2) -> LinearDimState {
    let d = config.dimension;
    let st = config.style;
    let p1 = d.p13.xy();
    let p2 = d.p14.xy();
    let on = d.defpt.xy();
    let direction = match d.kind {
        DimKind::Linear { rotation } => Vec2::from_angle(rotation),
        _ => (p2 - p1).normalized(),
    };
    let direction = if direction == Vec2::ZERO || !direction.is_finite() { Vec2::X } else { direction };
    let oblique = d.marks.oblique.map(Vec2::from_angle).filter(|axis| axis.is_finite() && axis.cross(direction).abs() > 1e-6);
    let foot = |point: Vec2| match oblique {
        Some(axis) => point + axis * ((on - point).cross(direction) / axis.cross(direction)),
        None => on + direction * (point - on).dot(direction),
    };
    let (f1, f2) = (foot(p1), foot(p2));
    let raw_measurement = (p2 - p1).dot(direction).abs();
    g.mtext = linear_text(raw_measurement, d, st, "");
    g.measurement = raw_measurement;
    let measurement = f1.dist(f2);
    if !st.suppress_ext1 {
        g.ext_n(ext_line(p1, f1, st, config.scale), false);
    }
    if !st.suppress_ext2 {
        g.ext_n(ext_line(p2, f2, st, config.scale), true);
    }
    let dimension_axis = (f2 - f1).normalized();
    let u = if dimension_axis == Vec2::ZERO || !dimension_axis.is_finite() { direction } else { dimension_axis };
    let n = u.perp();
    let (text_width, text_height) = config.measure(&g.mtext);
    let has_text = text_width > 0.0;
    let tick_like = config.ticks || (config.first_arrow.is_tick() && config.second_arrow.is_tick());
    let arrow_width = if tick_like { 0.0 } else { config.arrow_size };
    let arrows_fit = tick_like || measurement > 2.0 * config.arrow_size + 1e-9;
    let along_angle = readable(u.angle());
    let extent = |angle: f64| {
        let axis = Vec2::from_angle(angle);
        (text_width * axis.dot(u).abs() + text_height * axis.perp().dot(u).abs()) / 2.0
    };
    let inside_angle = if st.text_inside_horizontal { 0.0 } else { along_angle };
    let tvp = if st.text_above == 0 && st.text_vertical_pos.is_finite() { st.text_vertical_pos } else { 0.0 };
    let centred = st.text_above == 0 && tvp.abs() < 0.7;
    let text_need = if centred { 2.0 * (extent(inside_angle) + config.gap) } else { 2.0 * extent(inside_angle) };
    let fits = !has_text || measurement >= text_need + 2.0 * arrow_width;
    let text_fits = !has_text || measurement >= text_need;
    let just = st.text_just.min(4);
    let (text_inside, arrows_inside) = if d.user_text_pos || !has_text || just >= 3 {
        (true, arrows_fit)
    } else if fits {
        (true, true)
    } else if st.text_inside {
        (true, false)
    } else {
        match st.fit {
            0 => (false, false),
            1 => (text_fits, false),
            2 => (false, arrows_fit),
            _ if arrows_fit => (false, true),
            _ => (text_fits, false),
        }
    };
    let text_outside = !text_inside;
    LinearDimState {
        p1,
        p2,
        f1,
        f2,
        u,
        n,
        measurement,
        text_width,
        text_height,
        has_text,
        tick_like,
        arrow_width,
        arrows_inside,
        text_outside,
        just,
        centred,
        moved: text_outside && st.text_move != 0,
        outside_arrows: !arrows_inside && !st.suppress_outside_arrows,
        angle: 0.0,
        half_extent: 0.0,
        along_center: f1.mid(f2),
        side_out: n,
        text_axis: Vec2::X,
        offset: Vec2::ZERO,
        text_mid,
    }
}

fn position_linear_text(config: &DimConfig<'_>, state: &mut LinearDimState) {
    let st = config.style;
    let along_angle = readable(state.u.angle());
    let extent = |angle: f64| {
        let axis = Vec2::from_angle(angle);
        (state.text_width * axis.dot(state.u).abs() + state.text_height * axis.perp().dot(state.u).abs()) / 2.0
    };
    let inside_angle = if st.text_inside_horizontal { 0.0 } else { along_angle };
    state.angle = if state.text_outside { if st.text_outside_horizontal { 0.0 } else { along_angle } } else { inside_angle };
    state.half_extent = extent(state.angle);
    state.along_center = if state.text_outside {
        state.f2 + state.u * (2.0 * config.arrow_size.max(config.tick_size) + config.gap + state.half_extent)
    } else {
        match state.just {
            1 => state.f1 + state.u * (state.arrow_width + config.gap + state.half_extent + config.arrow_size),
            2 => state.f2 - state.u * (state.arrow_width + config.gap + state.half_extent + config.arrow_size),
            _ => state.f1.mid(state.f2),
        }
    };
    state.side_out = if (config.dimension.defpt.xy() - state.p1).dot(state.n) >= 0.0 { state.n } else { -state.n };
    state.text_axis = Vec2::from_angle(state.angle);
    let off_along = |axis: Vec2| {
        let glyph_extent = state.text_width * state.text_axis.dot(axis).abs() / 2.0
            + state.text_height * state.text_axis.perp().dot(axis).abs() * if st.text_above == 1 { 0.6 } else { 0.5 };
        config.gap + glyph_extent
    };
    let above = if state.angle.abs() < 1e-12 && state.n.y.abs() > 1e-9 {
        if state.n.y > 0.0 { state.n } else { -state.n }
    } else if state.angle.abs() < 1e-12 {
        if state.n.x < 0.0 { state.n } else { -state.n }
    } else if state.text_axis.perp().dot(state.n) >= 0.0 {
        state.n
    } else {
        -state.n
    };
    state.offset = match st.text_above {
        0 => above * (if st.text_vertical_pos.is_finite() { st.text_vertical_pos } else { 0.0 } * config.text_height),
        2 => state.side_out * off_along(state.side_out),
        4 => -above * off_along(-above),
        _ => above * off_along(above),
    };
    if state.just >= 3 && !config.dimension.user_text_pos && state.has_text {
        let (point, foot) = if state.just == 3 { (state.p1, state.f1) } else { (state.p2, state.f2) };
        let extension_axis = (foot - point).normalized();
        let extension_axis = if extension_axis == Vec2::ZERO { state.side_out } else { extension_axis };
        state.angle = readable(extension_axis.angle());
        let outward = if state.just == 3 { -state.u } else { state.u };
        state.text_mid = foot
            + extension_axis * (st.ext_extend * config.scale + config.gap + state.text_width / 2.0)
            + outward * (config.gap + state.text_height / 2.0);
    } else if state.moved {
        let mid = state.f1.mid(state.f2);
        let lift = off_along(state.side_out) + 2.0 * config.arrow_size.max(config.tick_size);
        state.text_mid = if st.text_move == 1 {
            mid + state.side_out * lift + state.u * state.half_extent
        } else {
            mid + state.side_out * (lift + off_along(state.side_out))
        };
    } else if !config.dimension.user_text_pos {
        state.text_mid = state.along_center + state.offset;
    }
}

fn draw_linear_lines(config: &DimConfig<'_>, g: &mut DimGeometry, state: &LinearDimState) {
    let d = config.dimension;
    let st = config.style;
    let (u, n) = (state.u, state.n);
    let dle = if state.tick_like { st.dim_line_extend * config.scale } else { 0.0 };
    let lo = if st.suppress_dim1 { state.measurement / 2.0 } else { f64::NEG_INFINITY };
    let hi = if st.suppress_dim2 { state.measurement / 2.0 } else { f64::INFINITY };
    let seg = |geometry: &mut DimGeometry, a: Vec2, b: Vec2| {
        let (ta, tb) = ((a - state.f1).dot(u), (b - state.f1).dot(u));
        let (t0, t1) = (ta.min(tb).max(lo), ta.max(tb).min(hi));
        if t1 > t0 + 1e-12 {
            geometry.dim(vec![state.f1 + u * t0, state.f1 + u * t1]);
        }
    };
    let inner = |geometry: &mut DimGeometry, a: Vec2, b: Vec2| {
        if state.centred && state.has_text && !state.text_outside && state.just <= 2 && !d.user_text_pos {
            let half = state.half_extent + config.gap;
            let (t0, t1) = ((state.along_center - u * half - state.f1).dot(u), (state.along_center + u * half - state.f1).dot(u));
            if t0 > (a - state.f1).dot(u) + 1e-9 {
                seg(geometry, a, state.f1 + u * t0);
            }
            if t1 < (b - state.f1).dot(u) - 1e-9 {
                seg(geometry, state.f1 + u * t1, b);
            }
        } else {
            seg(geometry, a, b);
        }
    };
    let to_text = |geometry: &mut DimGeometry, from: Vec2| {
        let to = state.text_mid - state.offset - u * (state.half_extent + config.gap);
        if state.text_outside && (to - from).dot(u) > 0.0 {
            seg(geometry, from, from + u * (to - from).dot(u));
        }
    };
    if state.arrows_inside {
        let first_inset = if st.suppress_dim1 { 0.0 } else { config.end(g, config.first_arrow, state.f1, -u) };
        let second_inset = if st.suppress_dim2 { 0.0 } else { config.end(g, config.second_arrow, state.f2, u) };
        inner(g, state.f1 - u * dle + u * first_inset, state.f2 + u * dle - u * second_inset);
        to_text(g, state.f2);
    } else {
        let stub = config.arrow_size * 2.0;
        if state.outside_arrows {
            if !st.suppress_dim1 {
                config.end(g, config.first_arrow, state.f1, u);
            }
            if !st.suppress_dim2 {
                config.end(g, config.second_arrow, state.f2, -u);
            }
            seg(g, state.f1 - u * stub, state.f1);
            seg(g, state.f2, state.f2 + u * stub);
        }
        if st.force_line_inside {
            inner(g, state.f1, state.f2);
        }
        to_text(g, if state.outside_arrows { state.f2 + u * stub } else { state.f2 });
    }
    if (d.user_text_pos || state.moved) && st.text_move == 1 && state.has_text {
        let base = state.f1.mid(state.f2);
        let offset = (state.text_mid - base).dot(n).abs();
        if offset > state.text_height / 2.0 + config.gap + 1e-9 {
            let side = if (base - state.text_mid).dot(state.text_axis) >= 0.0 { state.text_axis } else { -state.text_axis };
            g.dim(vec![base, state.text_mid + side * (state.text_width / 2.0 + config.gap)]);
        }
    }
    if let Some(jog) = d.marks.jog_line {
        let at = state.f1 + u * (jog.xy() - state.f1).dot(u);
        let height = 1.5 * config.text_height;
        if at.is_finite() && height > 0.0 {
            let radius = height / 2.0;
            g.cut(at, radius, Some(LineRole::Dim));
            g.dim(vec![at - u * radius, at - u * (radius / 4.0) + n * (height / 2.0), at + u * (radius / 4.0) - n * (height / 2.0), at + u * radius]);
        }
    }
}
