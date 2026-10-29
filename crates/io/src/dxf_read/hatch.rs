use super::common::T;
use super::{Color, EntityKind, Gradient, Handle, Hatch, HatchLoop, PolyVertex, Rgb, Spline, Tag, Vec2};

pub(super) fn hatch(tags: &[Tag], t: &T) -> Option<EntityKind> {
    let (origin, background) = hatch_xdata(tags);
    let pattern = t.s(2).unwrap_or_else(|| "SOLID".into());
    let solid = t.i(70).unwrap_or(0) == 1;
    let n_loops = t.i(91).unwrap_or(0).clamp(0, 100_000) as usize;
    let mut loops = Vec::new();
    // Walk the boundary data after the 91 tag.
    let start = tags.iter().position(|x| x.code == 91).map(|i| i + 1)?;
    let mut i = start;
    let get = |i: usize| tags.get(i);
    for _ in 0..n_loops {
        // 92: loop type flags.
        while get(i).is_some_and(|x| x.code != 92) {
            i += 1;
        }
        let flags = get(i)?.i64();
        i += 1;
        let outer = flags & 1 != 0;
        let mut verts = if flags & 2 != 0 { polyline_boundary(tags, &mut i)? } else { edge_boundary(tags, &mut i)? };
        if verts.len() > 1 && verts.first().map(|f| f.p).zip(verts.last().map(|l| l.p)).is_some_and(|(a, b)| a.near(b, 1e-9)) {
            verts.pop();
        }
        let sources = boundary_sources(tags, &mut i);
        loops.push(HatchLoop { vertices: verts, outer, sources });
    }
    let solid = solid || pattern.eq_ignore_ascii_case("SOLID");
    let (scale, angle) = (t.fd(41, 1.0), t.fd(52, 0.0).to_radians());
    // Keep the file's definition lines even for a name the library knows: the library's versions
    // of some patterns (AR-CONC) only approximate AutoCAD's, and drawing them at the file's scale
    // could be hundreds of times denser than the hatch the author saw.
    let pattern_lines = if solid { Vec::new() } else { pattern_definition(tags, scale, angle, origin) };
    Some(EntityKind::Hatch(Hatch {
        pattern: pattern.to_ascii_uppercase(),
        solid,
        loops,
        scale,
        angle,
        associative: t.i(71).unwrap_or(0) != 0,
        style: t.i(75).unwrap_or(0) as u8,
        // The first 30 is the elevation point's z (boundary points are 2D).
        elevation: t.f(30).filter(|z| z.is_finite()).unwrap_or(0.0),
        gradient: gradient(tags),
        origin,
        background,
        pattern_lines,
    }))
}

fn seek_code(tags: &[Tag], index: &mut usize, code: i32) {
    while tags.get(*index).is_some_and(|tag| tag.code != code) {
        *index += 1;
    }
}

fn polyline_boundary(tags: &[Tag], index: &mut usize) -> Option<Vec<PolyVertex>> {
    let get = |i: usize| tags.get(i);
    let has_bulge = get(*index).filter(|tag| tag.code == 72).map(Tag::i64).unwrap_or(0) != 0;
    seek_code(tags, index, 93);
    let count = get(*index)?.i64().clamp(0, 1_000_000) as usize;
    *index += 1;
    let mut vertices = Vec::new();
    for _ in 0..count {
        let x = get(*index).filter(|tag| tag.code == 10)?.f64();
        let y = get(*index + 1).filter(|tag| tag.code == 20)?.f64();
        *index += 2;
        let mut bulge = 0.0;
        if has_bulge && get(*index).is_some_and(|tag| tag.code == 42) {
            bulge = get(*index)?.f64();
            *index += 1;
        }
        vertices.push(PolyVertex::with_bulge(Vec2::new(x, y), bulge));
    }
    Some(vertices)
}

fn edge_boundary(tags: &[Tag], index: &mut usize) -> Option<Vec<PolyVertex>> {
    let get = |i: usize| tags.get(i);
    seek_code(tags, index, 93);
    let count = get(*index)?.i64().clamp(0, 1_000_000) as usize;
    *index += 1;
    let mut vertices = Vec::new();
    for _ in 0..count {
        seek_code(tags, index, 72);
        let kind = get(*index)?.i64();
        *index += 1;
        match kind {
            1 => line_edge(tags, index, &mut vertices),
            2 => circular_edge(tags, index, &mut vertices),
            3 => elliptic_edge(tags, index, &mut vertices),
            4 => spline_edge(tags, index, &mut vertices),
            _ => {}
        }
    }
    Some(vertices)
}

fn edge_value(tags: &[Tag], index: usize) -> f64 {
    tags.get(index).map(Tag::f64).unwrap_or(0.0)
}

fn line_edge(tags: &[Tag], index: &mut usize, vertices: &mut Vec<PolyVertex>) {
    let start = Vec2::new(edge_value(tags, *index), edge_value(tags, *index + 1));
    let end = Vec2::new(edge_value(tags, *index + 2), edge_value(tags, *index + 3));
    *index += 4;
    if vertices.last().is_none_or(|last| !last.p.near(start, 1e-9)) {
        vertices.push(PolyVertex::new(start));
    }
    vertices.push(PolyVertex::new(end));
}

fn circular_edge(tags: &[Tag], index: &mut usize, vertices: &mut Vec<PolyVertex>) {
    let center = Vec2::new(edge_value(tags, *index), edge_value(tags, *index + 1));
    let radius = edge_value(tags, *index + 2);
    let start_angle = edge_value(tags, *index + 3).to_radians();
    let end_angle = edge_value(tags, *index + 4).to_radians();
    let ccw = tags.get(*index + 5).map(Tag::i64).unwrap_or(1) != 0;
    *index += 6;
    let (start, end) = if ccw { (start_angle, end_angle) } else { (-start_angle, -end_angle) };
    let start_point = Vec2::polar(center, radius, start);
    let end_point = Vec2::polar(center, radius, end);
    let sweep = if ccw { cadcraft_geom::ccw_sweep(start, end) } else { -cadcraft_geom::ccw_sweep(end, start) };
    if vertices.last().is_none_or(|last| !last.p.near(start_point, 1e-9)) {
        vertices.push(PolyVertex::new(start_point));
    }
    // A full circle needs two bulges because one bulge cannot span coincident endpoints.
    let full = (sweep.abs() - std::f64::consts::TAU).abs() < 1e-9;
    let bulge = cadcraft_geom::arc_to_bulge(if full { sweep / 2.0 } else { sweep });
    if let Some(last) = vertices.last_mut() {
        last.bulge = bulge;
    }
    if full {
        vertices.push(PolyVertex::with_bulge(Vec2::polar(center, radius, start + sweep / 2.0), bulge));
    }
    vertices.push(PolyVertex::new(end_point));
}

fn elliptic_edge(tags: &[Tag], index: &mut usize, vertices: &mut Vec<PolyVertex>) {
    let center = Vec2::new(edge_value(tags, *index), edge_value(tags, *index + 1));
    let major = Vec2::new(edge_value(tags, *index + 2), edge_value(tags, *index + 3));
    let ratio = edge_value(tags, *index + 4);
    let start_angle = edge_value(tags, *index + 5).to_radians();
    let end_angle = edge_value(tags, *index + 6).to_radians();
    let ccw = tags.get(*index + 7).filter(|tag| tag.code == 73).map(Tag::i64).unwrap_or(1) != 0;
    *index += 8;
    let (start, end) = if ccw { (start_angle, end_angle) } else { (-end_angle, -start_angle) };
    let ellipse = cadcraft_geom::Ellipse { center, major, ratio, start, end };
    let mut points = Vec::new();
    ellipse.tessellate(major.len() * 1e-3, &mut points);
    if !ccw {
        points.reverse();
    }
    vertices.extend(points.into_iter().map(PolyVertex::new));
}

fn spline_edge(tags: &[Tag], index: &mut usize, vertices: &mut Vec<PolyVertex>) {
    let get = |i: usize| tags.get(i);
    let mut degree = 3;
    let (mut knot_count, mut control_count) = (0, 0);
    while let Some(tag) = get(*index).filter(|tag| matches!(tag.code, 94 | 73 | 74 | 95 | 96)) {
        let count = tag.i64().clamp(0, 100_000) as usize;
        match tag.code {
            94 => degree = count.clamp(1, 10),
            95 => knot_count = count,
            96 => control_count = count,
            _ => {}
        }
        *index += 1;
    }
    let mut knots = Vec::new();
    while knots.len() < knot_count && get(*index).is_some_and(|tag| tag.code == 40) {
        knots.push(edge_value(tags, *index));
        *index += 1;
    }
    let (mut control, mut weights) = (Vec::new(), Vec::new());
    while control.len() < control_count && get(*index).is_some_and(|tag| tag.code == 10) {
        control.push(Vec2::new(edge_value(tags, *index), edge_value(tags, *index + 1)));
        *index += 2;
        if get(*index).is_some_and(|tag| tag.code == 42) {
            weights.push(edge_value(tags, *index));
            *index += 1;
        }
    }
    let mut fit = Vec::new();
    if get(*index).is_some_and(|tag| tag.code == 97) {
        let count = get(*index).map(Tag::i64).unwrap_or(0).clamp(0, 100_000) as usize;
        *index += 1;
        while fit.len() < count && get(*index).is_some_and(|tag| tag.code == 11) {
            fit.push(Vec2::new(edge_value(tags, *index), edge_value(tags, *index + 1)));
            *index += 2;
        }
    }
    if weights.len() != control.len() {
        weights.clear();
    }
    let spline = Spline { degree, knots, control, weights, ..Spline::default() };
    let spline = if spline.is_valid() {
        spline
    } else if spline.control.is_empty() && fit.len() > 1 {
        Spline::from_fit_points(&fit)
    } else {
        Spline::from_control(spline.control, degree)
    };
    let points = spline.tessellate(1e-3);
    let skip = usize::from(vertices.last().zip(points.first()).is_some_and(|(last, point)| last.p.near(*point, 1e-9)));
    vertices.extend(points.into_iter().skip(skip).map(PolyVertex::new));
}

fn boundary_sources(tags: &[Tag], index: &mut usize) -> Vec<Handle> {
    let get = |i: usize| tags.get(i);
    let mut sources = Vec::new();
    for _ in 0..2 {
        while get(*index).is_some_and(|tag| !matches!(tag.code, 97 | 92 | 75)) {
            *index += 1;
        }
        if get(*index).is_none_or(|tag| tag.code != 97) {
            break;
        }
        let count = get(*index).map(Tag::i64).unwrap_or(0).clamp(0, MAX_HATCH_SOURCES as i64) as usize;
        *index += 1;
        if get(*index).is_some_and(|tag| !matches!(tag.code, 330 | 92 | 75)) {
            continue;
        }
        while sources.len() < count
            && let Some(tag) = get(*index).filter(|tag| tag.code == 330)
        {
            sources.extend(Handle::parse_hex(&tag.str()).filter(|handle| handle.0 != 0));
            *index += 1;
        }
        break;
    }
    sources
}

/// Upper bound on the source boundary objects of one HATCH loop.
const MAX_HATCH_SOURCES: usize = 4096;

/// Upper bounds on a HATCH's pattern definition lines and on the dashes of one line.
const MAX_PATTERN_LINES: usize = 1024;

const MAX_PATTERN_DASHES: usize = 256;

/// The pattern definition lines of a HATCH (groups 78, then per line 53 angle, 43/44 base
/// point, 45/46 offset, 79 dash count, 49 dash lengths). A file holds them as drawn: scaled,
/// rotated with the hatch and the offset as a world vector. They are kept the library's way
/// (unit scale, no hatch rotation, base point relative to the hatch origin, offset as shift
/// along the line and spacing), which the writer and the renderer turn back. Lines with
/// non-finite values are skipped.
fn pattern_definition(tags: &[Tag], scale: f64, angle: f64, origin: Vec2) -> Vec<cadcraft_doc::library::PatternLine> {
    let Some(at) = tags.iter().position(|x| x.code == 78) else { return Vec::new() };
    let scale = if scale.is_finite() && scale > 1e-9 { scale } else { 1.0 };
    let angle = if angle.is_finite() { angle } else { 0.0 };
    // Raw lines: angle (degrees), base, offset, dashes.
    let mut raw: Vec<(f64, Vec2, Vec2, Vec<f64>)> = Vec::new();
    for x in tags.iter().skip(at + 1) {
        let cur = raw.last_mut();
        match (x.code, cur) {
            (53, _) => {
                if raw.len() >= MAX_PATTERN_LINES {
                    break;
                }
                raw.push((x.f64(), Vec2::ZERO, Vec2::ZERO, Vec::new()));
            }
            (43, Some(l)) => l.1.x = x.f64(),
            (44, Some(l)) => l.1.y = x.f64(),
            (45, Some(l)) => l.2.x = x.f64(),
            (46, Some(l)) => l.2.y = x.f64(),
            (79, Some(_)) => {}
            (49, Some(l)) => {
                if l.3.len() < MAX_PATTERN_DASHES {
                    l.3.push(x.f64());
                }
            }
            _ => break,
        }
    }
    raw.into_iter()
        .filter(|(a, b, o, d)| a.is_finite() && b.is_finite() && o.is_finite() && d.iter().all(|v| v.is_finite()))
        .map(|(a, base, offset, dashes)| {
            let local = (base - origin).rotate(-angle) * (1.0 / scale);
            let delta = offset.rotate(-a.to_radians()) * (1.0 / scale);
            cadcraft_doc::library::PatternLine {
                angle: a - angle.to_degrees(),
                origin: (local.x, local.y),
                delta: (delta.x, delta.y),
                dashes: dashes.into_iter().map(|d| d / scale).collect(),
            }
        })
        .collect()
}

/// Hatch origin and background colour from `CADCRAFT` xdata: `1000 HATCH`, `1011` origin,
/// `1000` background colour name (empty for none). Defaults when absent.
fn hatch_xdata(tags: &[Tag]) -> (Vec2, Option<Color>) {
    let x = crate::dxf_ext::xdata(tags, crate::dxf_ext::APP);
    let Some(at) = x.windows(2).position(|w| matches!(w, [m, o] if m.code == 1000 && m.str() == "HATCH" && o.code == 1011)) else {
        return (Vec2::ZERO, None);
    };
    let val = |k: usize, code: i32| x.get(at + k).filter(|t| t.code == code);
    let coord = |k: usize, code: i32| val(k, code).map(Tag::f64).filter(|v| v.is_finite()).unwrap_or(0.0);
    let origin = Vec2::new(coord(1, 1011), coord(2, 1021));
    (origin, val(4, 1000).and_then(|t| Color::parse(&t.str())))
}

/// Gradient values missing from a file fall back to the GRADIENT command's defaults.
fn default_gradient() -> Gradient {
    Gradient { name: "LINEAR".into(), color1: Color::Index(5), color2: Color::Index(7), angle: 0.0, centered: true }
}

/// A hatch's gradient fill: the native groups when present (another program may have edited
/// them), otherwise CADCraft's `GRADIENT` xdata (kept by R2000 files and DWG conversions).
fn gradient(tags: &[Tag]) -> Option<Gradient> {
    if tags.iter().any(|x| x.code == 450) { native_gradient(tags) } else { xdata_gradient(tags) }
}

/// `CADCRAFT` xdata: `1000 GRADIENT`, then name, 1040 angle, 1070 centered, colour 1 and 2 names.
fn xdata_gradient(tags: &[Tag]) -> Option<Gradient> {
    let x = crate::dxf_ext::xdata(tags, crate::dxf_ext::APP);
    let at = x.iter().position(|t| t.code == 1000 && t.str() == "GRADIENT")?;
    let o = x.get(at + 1..).unwrap_or(&[]);
    let d = default_gradient();
    let strs: Vec<String> = o.iter().filter(|t| t.code == 1000).take(3).map(Tag::str).collect();
    let color = |i: usize, d: Color| strs.get(i).and_then(|s| Color::parse(s)).unwrap_or(d);
    Some(Gradient {
        name: strs.first().cloned().unwrap_or(d.name),
        color1: color(1, d.color1),
        color2: color(2, d.color2),
        angle: o.iter().find(|t| t.code == 1040).map(Tag::f64).filter(|a| a.is_finite()).unwrap_or(d.angle),
        centered: o.iter().find(|t| t.code == 1070).map_or(d.centered, |t| t.i64() != 0),
    })
}

/// DXF Reference HATCH groups 450–470, when 450 says gradient. Each 463 starts a colour record
/// whose 63 (ACI) and 421 (true colour) groups follow.
fn native_gradient(tags: &[Tag]) -> Option<Gradient> {
    let start = tags.iter().position(|x| x.code == 450)?;
    if tags.get(start)?.i64() != 1 {
        return None;
    }
    let mut g = default_gradient();
    // Per colour record: (ACI, true colour); a true colour wins over its ACI fallback.
    let mut colors: [(Option<Color>, Option<Rgb>); 2] = [(None, None); 2];
    let mut slot: Option<usize> = None;
    for x in tags.iter().skip(start + 1).take_while(|x| x.code != 1001) {
        let rec = slot.and_then(|s| colors.get_mut(s));
        match x.code {
            460 => g.angle = Some(x.f64()).filter(|a| a.is_finite()).unwrap_or(0.0),
            461 => g.centered = x.f64().abs() < 1e-9,
            463 => slot = Some(slot.map_or(0, |s| s.saturating_add(1))),
            63 => {
                if let Some(r) = rec {
                    r.0 = Some(Color::from_aci(i16::try_from(x.i64()).unwrap_or(256)));
                }
            }
            421 => {
                if let Some(r) = rec {
                    r.1 = Some(Rgb::from_u32(u32::try_from(x.i64() & 0xff_ffff).unwrap_or(0)));
                }
            }
            470 => g.name = x.str(),
            _ => {}
        }
    }
    let [c1, c2] = colors;
    let pick = |(aci, rgb): (Option<Color>, Option<Rgb>), d: Color| rgb.map(Color::True).or(aci).unwrap_or(d);
    g.color1 = pick(c1, g.color1);
    g.color2 = pick(c2, g.color2);
    Some(g)
}
