//! Multilines in DXF (DXF Reference: MLINE entity, MLINESTYLE object). An MLINE holds its
//! vertices, each with the direction of the segment that starts there, the miter direction and,
//! per style element, the element's distance from the vertex along the miter followed by where
//! the element runs and breaks on that segment. It points (group 340) to its MLINESTYLE object,
//! which the `ACAD_MLINESTYLE` dictionary names and which holds the elements' offsets, colours
//! and linetypes, the caps and the fill.

use std::collections::HashMap;

use cadcraft_color::Color;
use cadcraft_doc::{
    Drawing, Entity, EntityKind, EntityStore, MAX_MLINE_CUTS, MAX_MLINE_ELEMENTS, MAX_MLINE_VERTICES, MLine, MLineCaps, MLineCut, MLineElement,
    MLineJust, MLineStyle, MLineVertex, STANDARD_MLINE_STYLE,
};
use cadcraft_dxf::{Section, Tag};
use cadcraft_geom::{Vec2, Vec3};

/// Most groups 41 kept per element and vertex (start, then break pairs).
const MAX_PARAMS: usize = 2 * MAX_MLINE_CUTS + 2;
/// Most multiline styles read from a file.
const MAX_STYLES: usize = 10_000;

/// MLINESTYLE flags (group 70).
const FILL: i64 = 1;
const JOINTS: i64 = 2;
const START_LINE: i64 = 16;
const START_INNER: i64 = 32;
const START_OUTER: i64 = 64;
const END_LINE: i64 = 256;
const END_INNER: i64 = 512;
const END_OUTER: i64 = 1024;

fn key(s: &str) -> String {
    s.trim().to_ascii_uppercase()
}

fn finite(v: f64) -> f64 {
    if v.is_finite() { v } else { 0.0 }
}

#[derive(Default)]
struct RawVertex {
    p: Vec3,
    dir: Vec3,
    miter: Vec3,
    /// Per element: its groups 41.
    params: Vec<Vec<f64>>,
}

/// The MLINE entity's own groups (after the `AcDbMline` marker when present). The style's
/// elements are recovered from the vertex data; [`resolve`] replaces them with the style's.
pub(crate) fn read_entity(tags: &[Tag]) -> MLine {
    let start = tags.iter().position(|t| t.code == 100 && t.str() == "AcDbMline").unwrap_or(0);
    let tags = tags.get(start..).unwrap_or(&[]);
    let mut style = String::new();
    let mut scale = 1.0;
    let mut just = MLineJust::Top;
    let mut flags = 0;
    let mut verts: Vec<RawVertex> = Vec::new();
    let mut in_params = false;
    for t in tags {
        let v = t.f64();
        match t.code {
            2 => style = t.str(),
            40 => scale = finite(v),
            70 => just = MLineJust::from_dxf(t.i64()),
            71 => flags = t.i64(),
            11 => {
                in_params = false;
                if verts.len() >= MAX_MLINE_VERTICES {
                    break;
                }
                verts.push(RawVertex { p: Vec3::new(finite(v), 0.0, 0.0), ..RawVertex::default() });
            }
            21 | 31 | 12 | 22 | 32 | 13 | 23 | 33 => {
                let Some(r) = verts.last_mut() else { continue };
                // 1x/2x/3x: x/y/z of the vertex (x1), segment direction (x2) or miter (x3).
                let target = match t.code % 10 {
                    1 => &mut r.p,
                    2 => &mut r.dir,
                    _ => &mut r.miter,
                };
                match t.code / 10 {
                    1 => target.x = finite(v),
                    2 => target.y = finite(v),
                    _ => target.z = finite(v),
                }
            }
            74 => {
                if let Some(r) = verts.last_mut()
                    && r.params.len() < MAX_MLINE_ELEMENTS
                {
                    r.params.push(Vec::new());
                    in_params = true;
                } else {
                    in_params = false;
                }
            }
            41 if in_params => {
                if let Some(list) = verts.last_mut().and_then(|r| r.params.last_mut())
                    && list.len() < MAX_PARAMS
                {
                    list.push(finite(v));
                }
            }
            _ => {}
        }
    }
    let nelem = verts.first().map(|r| r.params.len()).unwrap_or(0).clamp(1, MAX_MLINE_ELEMENTS);
    // Element offsets from the first vertex: its distance along the miter, measured across the
    // first segment.
    let first = verts.first();
    let pts: Vec<Vec2> = verts.iter().map(|r| r.p.xy()).collect();
    let dir0 = first.map(|r| r.dir.xy().normalized()).filter(|d| *d != Vec2::ZERO).unwrap_or_else(|| match (pts.first(), pts.get(1)) {
        (Some(a), Some(b)) => (*b - *a).normalized(),
        _ => Vec2::X,
    });
    let miter0 = first.map(|r| r.miter.xy().normalized()).filter(|d| *d != Vec2::ZERO).unwrap_or(dir0.perp());
    let across = miter0.dot(dir0.perp());
    let s = if scale.abs() > 1e-12 { scale } else { 1.0 };
    let elements: Vec<MLineElement> = (0..nelem)
        .map(|e| {
            let d = first.and_then(|r| r.params.get(e)).and_then(|l| l.first()).copied().unwrap_or(0.0) * across;
            MLineElement { offset: finite(d / s), ..MLineElement::default() }
        })
        .collect();
    let def = MLineStyle { name: if style.is_empty() { STANDARD_MLINE_STYLE.into() } else { style.clone() }, elements, ..MLineStyle::default() };
    let vertices = verts
        .iter()
        .map(|r| {
            let mut cuts = Vec::new();
            for (e, list) in r.params.iter().enumerate() {
                cuts.extend(gaps(list).into_iter().map(|(start, end)| MLineCut { element: e, start, end }));
            }
            cuts.truncate(MAX_MLINE_CUTS);
            MLineVertex { p: r.p, cuts }
        })
        .collect();
    MLine {
        style: def.name.clone(),
        def,
        scale,
        justification: just,
        closed: flags & 2 != 0,
        no_start_caps: flags & 4 != 0,
        no_end_caps: flags & 8 != 0,
        vertices,
    }
}

/// End of a break that runs to the end of its segment.
const TO_END: f64 = 1e12;

/// Breaks from an element's groups 41: the first is its distance along the miter, the second
/// where the element starts (from the miter point), then alternately where it stops and starts
/// again (from its start).
fn gaps(list: &[f64]) -> Vec<(f64, f64)> {
    let Some(start) = list.get(1).copied() else { return Vec::new() };
    let mut out = Vec::new();
    if start > 1e-12 {
        out.push((0.0, start));
    }
    let rest = list.get(2..).unwrap_or(&[]);
    for pair in rest.chunks(2) {
        match pair {
            [a, b] => out.push((start + a, start + b)),
            [a] => out.push((start + a, TO_END)),
            _ => {}
        }
    }
    out.retain(|(a, b)| b > a);
    out
}

/// An element's groups 41 for one segment: `along` (its distance along the miter), then where
/// it starts and the breaks (`on`: the drawn pieces along the element, `len` its length).
fn params(along: f64, on: &[(f64, f64)], len: f64) -> Vec<f64> {
    let mut v = vec![along];
    let Some(&(s0, _)) = on.first() else {
        v.push(len);
        return v;
    };
    v.push(s0);
    for (i, (s, e)) in on.iter().enumerate() {
        if i > 0 {
            v.push(s - s0);
        }
        if (e - len).abs() > 1e-9 || i + 1 < on.len() {
            v.push(e - s0);
        }
    }
    v
}

/// The MLINE groups after the common entity groups.
pub(crate) fn entity_tags(m: &MLine, style_handle: &str) -> Vec<Tag> {
    let mut t = Vec::new();
    let p = |t: &mut Vec<Tag>, c: i32, v: Vec3| {
        t.push(Tag::f(c, v.x));
        t.push(Tag::f(c + 10, v.y));
        t.push(Tag::f(c + 20, v.z));
    };
    t.push(Tag::s(2, m.style.chars().take(32).collect::<String>()));
    t.push(Tag::s(340, style_handle));
    t.push(Tag::f(40, finite(m.scale)));
    t.push(Tag::i(70, m.justification.to_dxf()));
    let flags =
        i64::from(!m.vertices.is_empty()) | if m.closed { 2 } else { 0 } | if m.no_start_caps { 4 } else { 0 } | if m.no_end_caps { 8 } else { 0 };
    t.push(Tag::i(71, flags));
    t.push(Tag::i(72, m.vertices.len() as i64));
    t.push(Tag::i(73, m.def.elements.len() as i64));
    p(&mut t, 10, m.vertices.first().map(|v| v.p).unwrap_or_default());
    p(&mut t, 210, Vec3::Z);
    let pts = m.points();
    let n = pts.len();
    let dirs = m.directions();
    let miters = m.miters();
    let offs = m.offsets();
    let nseg = m.segment_count();
    for (k, v) in m.vertices.iter().enumerate() {
        p(&mut t, 11, v.p);
        let dir = dirs.get(k.min(nseg.saturating_sub(1))).copied().unwrap_or(Vec2::X);
        p(&mut t, 12, dir.to3(0.0));
        let mv = miters.get(k).copied().unwrap_or(Vec2::Y);
        let stretch = mv.len();
        p(&mut t, 13, mv.normalized().to3(0.0));
        let has_seg = k < nseg;
        for (e, d) in offs.iter().enumerate() {
            let along = d * stretch;
            let (a, b) = (pts.get(k).copied().unwrap_or_default() + mv * *d, {
                let j = (k + 1) % n.max(1);
                pts.get(j).copied().unwrap_or_default() + miters.get(j).copied().unwrap_or_default() * *d
            });
            let len = if has_seg { a.dist(b) } else { 0.0 };
            let on = on_pieces(len, v.cuts.iter().filter(|c| c.element == e).map(|c| (c.start, c.end)));
            let list = if has_seg { params(along, &on, len) } else { vec![along, 0.0] };
            t.push(Tag::i(74, list.len() as i64));
            t.extend(list.into_iter().map(|x| Tag::f(41, x)));
            t.push(Tag::i(75, 0));
        }
    }
    t
}

/// The drawn pieces of `[0, len]` with `gaps` taken out.
fn on_pieces(len: f64, gaps: impl Iterator<Item = (f64, f64)>) -> Vec<(f64, f64)> {
    let mut gaps: Vec<(f64, f64)> =
        gaps.filter(|(a, b)| a.is_finite() && b.is_finite()).map(|(a, b)| (a.min(b).max(0.0), a.max(b).min(len))).filter(|(a, b)| b > a).collect();
    gaps.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut from = 0.0;
    for (a, b) in gaps {
        if a > from + 1e-12 {
            out.push((from, a));
        }
        from = f64::max(from, b);
    }
    if len > from + 1e-12 {
        out.push((from, len));
    }
    out
}

/// Call `f` with each record (`0 TYPE` and the groups after it) of a section, without copying.
fn each_record<'a>(tags: &'a [Tag], mut f: impl FnMut(&str, &'a [Tag])) {
    let mut open: Option<(String, usize)> = None;
    for (i, t) in tags.iter().enumerate() {
        if t.code == 0 {
            if let Some((k, s)) = open.take() {
                f(&k, tags.get(s..i).unwrap_or(&[]));
            }
            open = Some((t.str(), i + 1));
        }
    }
    if let Some((k, s)) = open {
        f(&k, tags.get(s..).unwrap_or(&[]));
    }
}

/// A MLINESTYLE object.
fn read_style(tags: &[Tag]) -> Option<MLineStyle> {
    let start = tags.iter().position(|t| t.code == 100 && t.str() == "AcDbMlineStyle").unwrap_or(0);
    let body = tags.get(start..).unwrap_or(&[]);
    let mut st = MLineStyle { elements: Vec::new(), ..MLineStyle::default() };
    let mut flags = 0;
    let mut fill = Color::ByLayer;
    for t in body {
        match t.code {
            2 => st.name = t.str(),
            3 => st.description = t.str(),
            70 => flags = t.i64(),
            51 => st.start_caps.angle = t.f64().to_radians(),
            52 => st.end_caps.angle = t.f64().to_radians(),
            49 if st.elements.len() < MAX_MLINE_ELEMENTS => st.elements.push(MLineElement { offset: finite(t.f64()), ..MLineElement::default() }),
            62 => {
                let c = Color::from_aci(i16::try_from(t.i64()).unwrap_or(256));
                match st.elements.last_mut() {
                    Some(e) => e.color = c,
                    None => fill = c,
                }
            }
            6 => {
                if let Some(e) = st.elements.last_mut() {
                    e.linetype = t.str();
                }
            }
            _ => {}
        }
    }
    if st.name.trim().is_empty() {
        return None;
    }
    for e in &mut st.elements {
        if e.linetype.eq_ignore_ascii_case("bylayer") || e.linetype.is_empty() {
            e.linetype = "ByLayer".into();
        } else if e.linetype.eq_ignore_ascii_case("byblock") {
            e.linetype = "ByBlock".into();
        }
    }
    st.fill = (flags & FILL != 0).then_some(fill);
    st.joints = flags & JOINTS != 0;
    st.start_caps.line = flags & START_LINE != 0;
    st.start_caps.inner_arcs = flags & START_INNER != 0;
    st.start_caps.outer_arc = flags & START_OUTER != 0;
    st.end_caps.line = flags & END_LINE != 0;
    st.end_caps.inner_arcs = flags & END_INNER != 0;
    st.end_caps.outer_arc = flags & END_OUTER != 0;
    Some(st.sanitized())
}

fn has_mlines(store: &EntityStore) -> bool {
    store.iter().any(|e| matches!(e.kind, EntityKind::MLine(_)))
}

/// Read the multiline styles, then give every multiline its style's elements, caps and fill.
pub(crate) fn resolve(secs: &[Section], d: &mut Drawing) {
    let mut styles: Vec<MLineStyle> = Vec::new();
    for s in secs.iter().filter(|s| s.name == "OBJECTS") {
        each_record(&s.tags, |kind, tags| {
            if kind == "MLINESTYLE"
                && styles.len() < MAX_STYLES
                && let Some(st) = read_style(tags)
            {
                styles.push(st);
            }
        });
    }
    for st in styles {
        match d.mline_styles.iter_mut().find(|x| x.name.eq_ignore_ascii_case(&st.name)) {
            Some(x) => *x = st,
            None => d.mline_styles.push(st),
        }
    }
    let by_name: HashMap<String, MLineStyle> = d.mline_styles.iter().map(|s| (key(&s.name), s.clone())).collect();
    fill(&mut d.model, &by_name);
    for l in &mut d.layouts {
        fill(&mut l.entities, &by_name);
    }
    for b in d.blocks.values_mut() {
        if has_mlines(&b.entities) {
            fill(&mut std::sync::Arc::make_mut(b).entities, &by_name);
        }
    }
}

fn fill(store: &mut EntityStore, by_name: &HashMap<String, MLineStyle>) {
    let hs: Vec<cadcraft_doc::Handle> = store.iter().filter(|e| matches!(e.kind, EntityKind::MLine(_))).map(|e| e.handle).collect();
    for h in hs {
        store.modify(h, |e| {
            if let EntityKind::MLine(m) = &mut e.kind
                && let Some(st) = by_name.get(&key(&m.style))
                && st.elements.len() == m.def.elements.len()
            {
                m.def = st.clone();
            }
        });
    }
}

/// The multiline styles to write, with handles allocated before anything is written.
#[derive(Default)]
pub(crate) struct Plan {
    /// The `ACAD_MLINESTYLE` dictionary.
    pub dict: String,
    /// Each style and its MLINESTYLE handle.
    styles: Vec<(MLineStyle, String)>,
}

impl Plan {
    /// The MLINESTYLE handle of style `name` (STANDARD's when it is missing).
    pub fn handle(&self, name: &str) -> String {
        let find = |n: &str| self.styles.iter().find(|(s, _)| s.name.eq_ignore_ascii_case(n.trim())).map(|(_, h)| h.clone());
        find(name).or_else(|| find(STANDARD_MLINE_STYLE)).unwrap_or_else(|| "0".into())
    }
}

/// The drawing's multiline styles, STANDARD and the style of every multiline in `every`.
pub(crate) fn plan<'a>(d: &Drawing, every: impl Iterator<Item = &'a Entity>, mut new_handle: impl FnMut() -> String) -> Plan {
    let mut list: Vec<MLineStyle> = d.mline_styles.clone();
    if !list.iter().any(|s| s.name.eq_ignore_ascii_case(STANDARD_MLINE_STYLE)) {
        list.insert(0, MLineStyle::default());
    }
    for e in every {
        if let EntityKind::MLine(m) = &e.kind
            && !list.iter().any(|s| s.name.eq_ignore_ascii_case(m.style.trim()))
        {
            list.push(MLineStyle { name: m.style.trim().to_string(), ..m.def.clone() });
        }
    }
    let dict = new_handle();
    Plan { dict, styles: list.into_iter().filter(|s| !s.name.trim().is_empty()).map(|s| (s, new_handle())).collect() }
}

/// The `ACAD_MLINESTYLE` dictionary (owned by the root dictionary `root`) and its styles.
pub(crate) fn objects(plan: &Plan, root: &str) -> Vec<Tag> {
    let mut t = vec![Tag::s(0, "DICTIONARY"), Tag::s(5, plan.dict.clone()), Tag::s(330, root), Tag::s(100, "AcDbDictionary"), Tag::i(281, 1)];
    for (s, h) in &plan.styles {
        t.push(Tag::s(3, s.name.trim()));
        t.push(Tag::s(350, h.clone()));
    }
    for (s, h) in &plan.styles {
        t.extend([Tag::s(0, "MLINESTYLE"), Tag::s(5, h.clone()), Tag::s(102, "{ACAD_REACTORS"), Tag::s(330, plan.dict.clone()), Tag::s(102, "}")]);
        t.extend([Tag::s(330, plan.dict.clone()), Tag::s(100, "AcDbMlineStyle"), Tag::s(2, s.name.trim())]);
        let caps = |c: &MLineCaps, line: i64, inner: i64, outer: i64| {
            (if c.line { line } else { 0 }) | (if c.inner_arcs { inner } else { 0 }) | (if c.outer_arc { outer } else { 0 })
        };
        let flags = (if s.fill.is_some() { FILL } else { 0 })
            | (if s.joints { JOINTS } else { 0 })
            | caps(&s.start_caps, START_LINE, START_INNER, START_OUTER)
            | caps(&s.end_caps, END_LINE, END_INNER, END_OUTER);
        t.push(Tag::i(70, flags));
        t.push(Tag::s(3, s.description.clone()));
        t.push(Tag::i(62, i64::from(s.fill.unwrap_or(Color::ByLayer).to_aci())));
        t.push(Tag::f(51, finite(s.start_caps.angle.to_degrees())));
        t.push(Tag::f(52, finite(s.end_caps.angle.to_degrees())));
        t.push(Tag::i(71, s.elements.len() as i64));
        for e in &s.elements {
            t.push(Tag::f(49, finite(e.offset)));
            t.push(Tag::i(62, i64::from(e.color.to_aci())));
            let lt = if e.linetype.trim().is_empty() { "BYLAYER".to_string() } else { e.linetype.to_ascii_uppercase() };
            t.push(Tag::s(6, if lt == "BYLAYER" || lt == "BYBLOCK" { lt } else { e.linetype.clone() }));
        }
    }
    t
}
