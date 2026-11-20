//! Approximate entity bounds (used for extents, zoom and selection pre-filtering).

use std::collections::HashMap;

use cadcraft_geom::{Arc, Bounds2, EPS, Polyline, Vec2, bulge_to_arc};

use crate::{Drawing, Entity, EntityKind, LwPolyline, Prim};

/// Nested block references deeper than this are ignored (cyclic or hostile files).
pub const MAX_BLOCK_DEPTH: usize = 16;

/// Positions in `tags` of the groups outside application-defined `102 {` .. `102 }` groups, whose
/// codes are the application's own and not the entity's.
fn outside_app(tags: &[crate::RawTag]) -> impl Iterator<Item = usize> + '_ {
    let mut in_app = false;
    tags.iter().enumerate().filter_map(move |(i, t)| {
        if t.code == 102 {
            in_app = t.value.starts_with('{');
            None
        } else {
            (!in_app).then_some(i)
        }
    })
}

/// The positions of the frame's corner groups 10, 20, 11, 21 (in that order) and their values:
/// the first of each outside application groups, found in one pass that stops at the last.
fn ole_corner_groups(tags: &[crate::RawTag]) -> Option<[(usize, f64); 4]> {
    let mut found: [Option<(usize, f64)>; 4] = [None; 4];
    for i in outside_app(tags) {
        let Some(t) = tags.get(i) else { continue };
        let slot = match t.code {
            10 => 0,
            20 => 1,
            11 => 2,
            21 => 3,
            _ => continue,
        };
        if let Some(f) = found.get_mut(slot).filter(|f| f.is_none()) {
            *f = Some((i, t.value.trim().parse::<f64>().ok().filter(|v| v.is_finite())?));
            if found.iter().all(Option::is_some) {
                break;
            }
        }
    }
    let [Some(a), Some(b), Some(c), Some(d)] = found else { return None };
    Some([a, b, c, d])
}

/// The closed outline of an OLE2FRAME from its upper-left (groups 10, 20) and lower-right
/// (groups 11, 21) corners (DXF Reference, OLE2FRAME); `None` when either is missing or not finite.
pub fn ole_frame(tags: &[crate::RawTag]) -> Option<[Vec2; 5]> {
    let [(_, x1), (_, y1), (_, x2), (_, y2)] = ole_corner_groups(tags)?;
    let (a, b) = (Vec2::new(x1, y1), Vec2::new(x2, y2));
    Some([a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y), a])
}

/// The bytes of the OLE data before its corners: a 2-byte marker.
const OLE_HEADER: usize = 2;
/// The corners in the OLE data: four of x, y, z as little-endian doubles (upper left, upper right,
/// lower right, lower left).
const OLE_CORNERS: usize = 12;

/// Replace the four corners at the start of the OLE data (the 310 groups' hex, in order) by `new`
/// (upper left, upper right, lower right, lower left), keeping z and every group's length and
/// letter case. Only the groups that cover the header are decoded and rewritten; the rest of the
/// payload is not touched. Data that is not valid hex, too short, or whose corners are not the old
/// `frame`'s is left alone.
fn ole_data_transform(tags: &mut [crate::RawTag], new: [Vec2; 4], frame: [f64; 4]) {
    const END: usize = OLE_HEADER + OLE_CORNERS * 8;
    let mut bytes: Vec<u8> = Vec::with_capacity(END);
    // The groups read, and how many leading bytes of each belong to the header.
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for i in outside_app(tags).filter(|&i| tags.get(i).is_some_and(|t| t.code == 310)) {
        if bytes.len() >= END {
            break;
        }
        let Some(t) = tags.get(i) else { return };
        let s = t.value.trim();
        if s.len() % 2 != 0 || !s.is_ascii() {
            return;
        }
        let take = (s.len() / 2).min(END - bytes.len());
        for k in 0..take {
            // `from_str_radix` also takes a sign, so each digit is checked first.
            let Some(h) = s.get(2 * k..2 * k + 2).filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit())) else { return };
            let Ok(b) = u8::from_str_radix(h, 16) else { return };
            bytes.push(b);
        }
        spans.push((i, take));
    }
    if bytes.len() < END {
        return;
    }
    // Only data whose corners are this frame's corners (UL, UR, LR, LL) has the layout above; any
    // other header is not ours to rewrite.
    let [x1, y1, x2, y2] = frame;
    let want = [(x1, y1), (x2, y1), (x2, y2), (x1, y2)];
    // Two corners match within a fixed tolerance for ordinary coordinates (CORNER_EPS at most), or
    // within a few ulps of the coordinate where that is larger, which is from about 1.1e9 up.
    let near = |a: f64, b: f64| {
        let m = a.abs().max(b.abs());
        (a - b).abs() <= (1e-9 * m.max(1.0)).min(CORNER_EPS).max(4.0 * f64::EPSILON * m)
    };
    for (c, (wx, wy)) in want.iter().enumerate() {
        let at = OLE_HEADER + c * 24;
        let rd = |o: usize| bytes.get(o..o + 8).and_then(|s| <[u8; 8]>::try_from(s).ok()).map(f64::from_le_bytes);
        let (Some(x), Some(y)) = (rd(at), rd(at + 8)) else { return };
        if !(near(x, *wx) && near(y, *wy)) {
            return;
        }
    }
    for (c, p) in new.iter().enumerate() {
        let at = OLE_HEADER + c * 24;
        if let Some(s) = bytes.get_mut(at..at + 8) {
            s.copy_from_slice(&p.x.to_le_bytes());
        }
        if let Some(s) = bytes.get_mut(at + 8..at + 16) {
            s.copy_from_slice(&p.y.to_le_bytes());
        }
    }
    let mut pos = 0;
    for (i, n) in spans {
        let (Some(t), Some(chunk)) = (tags.get_mut(i), bytes.get(pos..pos + n)) else { return };
        // Leave any padding around the digits, and the digits after the header, as they were.
        let start = t.value.len() - t.value.trim_start().len();
        // Only the rewritten digits and a few after them, for the case of the nearest letter: a
        // hostile 310 line can be as long as the file (the value is checked to be ASCII above).
        let end = start.saturating_add(2 * n + 64).min(t.value.len());
        let hex = recase(chunk, t.value.get(start..end).unwrap_or_default());
        t.value.replace_range(start..start + 2 * n, &hex);
        pos += n;
    }
}

/// The hex of `chunk`, each digit in the case of the digit at the same place in `old`. A digit that
/// was a number and becomes a letter takes the case of the nearest letter in `old`, upper when there
/// is none, so bytes that did not change keep their exact text.
fn recase(chunk: &[u8], old: &str) -> String {
    let old = old.as_bytes();
    // For each place, whether the letter nearest to it (the one at it, else the closest before or
    // after; the one before on a tie) is lower case: one pass for the next letter at or after each
    // place, one for the previous, then the nearer of the two.
    let mut next: Vec<Option<(usize, bool)>> = vec![None; old.len()];
    let mut seen: Option<(usize, bool)> = None;
    for (i, c) in old.iter().enumerate().rev() {
        if c.is_ascii_alphabetic() {
            seen = Some((i, c.is_ascii_lowercase()));
        }
        if let Some(slot) = next.get_mut(i) {
            *slot = seen;
        }
    }
    let mut lower_at = vec![false; old.len()];
    let mut prev: Option<(usize, bool)> = None;
    for (i, c) in old.iter().enumerate() {
        if c.is_ascii_alphabetic() {
            prev = Some((i, c.is_ascii_lowercase()));
        }
        let after = next.get(i).copied().flatten();
        let lower = match (prev, after) {
            (Some((p, lp)), Some((q, lq))) => {
                if i - p <= q - i {
                    lp
                } else {
                    lq
                }
            }
            (Some((_, l)), None) | (None, Some((_, l))) => l,
            (None, None) => false,
        };
        if let Some(slot) = lower_at.get_mut(i) {
            *slot = lower;
        }
    }
    let mut out = String::with_capacity(chunk.len() * 2);
    for (k, b) in chunk.iter().enumerate() {
        for (j, d) in [b >> 4, b & 15].into_iter().enumerate() {
            let digit = char::from_digit(d.into(), 16).unwrap_or('0');
            out.push(if lower_at.get(2 * k + j).copied().unwrap_or(false) { digit } else { digit.to_ascii_uppercase() });
        }
    }
    out
}

/// The fixed part of the tolerance when matching the corners in the OLE data to the frame's. The
/// header holds the same f64 the frame groups print, so they agree to a few ulps. Up to about 1.1e9
/// a fixed tolerance of at most this decides, so that distinct geometry does not match; above that,
/// four ulps of the coordinate (`4 * EPSILON * |x|`) exceed it and decide instead.
const CORNER_EPS: f64 = 1e-6;

/// Transform an OLE2FRAME by `m`: its two corners are mapped through `m` (a DWG save writes the
/// frame's position from these groups), and so are the corners at the start of its OLE data (310
/// groups), which a reader of the saved file may trust instead. The frame stays axis aligned with
/// its upper-left corner first. A matrix that keeps the axes or swaps them (a move, scale, mirror or
/// quarter turn) gives the box of the two mapped corners. Any other turn would collapse that box
/// (a square turned 45° maps its two corners level), so the frame's centre is mapped and its size
/// kept, scaled by `m`'s, its sides swapped when the turn is nearer a quarter turn. Commands refuse
/// a rotation or mirror (see [`ole_frame_accepts`]), but internal callers such as EXPLODE, REFEDIT
/// and paste still apply them, and must leave a consistent frame. A frame with no usable corners is
/// left alone.
pub fn ole_frame_transform(tags: &mut [crate::RawTag], m: &cadcraft_geom::Mat3) {
    let Some([(ix1, x1), (iy1, y1), (ix2, x2), (iy2, y2)]) = ole_corner_groups(tags) else { return };
    let frame = [x1, y1, x2, y2];
    let t = 1e-12 * (m.a.abs() + m.b.abs() + m.c.abs() + m.d.abs());
    let axes_kept = (m.b.abs() <= t && m.c.abs() <= t) || (m.a.abs() <= t && m.d.abs() <= t);
    let (a, b) = if axes_kept {
        (m.apply(Vec2::new(x1, y1)), m.apply(Vec2::new(x2, y2)))
    } else {
        let center = m.apply(Vec2::new((x1 + x2) / 2.0, (y1 + y2) / 2.0));
        // A plain turn keeps the size exactly; cos² + sin² can miss 1 by an ulp or two.
        let s = (m.a * m.d - m.b * m.c).abs().sqrt();
        let scale = if (s - 1.0).abs() < 1e-12 { 1.0 } else { s };
        let (mut w, mut h) = ((x2 - x1).abs() * scale, (y2 - y1).abs() * scale);
        // Where the frame's x axis goes: nearer to y than to x is nearer a quarter turn.
        if m.b.abs() > m.a.abs() {
            std::mem::swap(&mut w, &mut h);
        }
        (center - Vec2::new(w, h) / 2.0, center + Vec2::new(w, h) / 2.0)
    };
    if !(a.x.is_finite() && a.y.is_finite() && b.x.is_finite() && b.y.is_finite()) {
        return;
    }
    let (lo, hi) = (Vec2::new(a.x.min(b.x), a.y.min(b.y)), Vec2::new(a.x.max(b.x), a.y.max(b.y)));
    let (ul, lr) = (Vec2::new(lo.x, hi.y), Vec2::new(hi.x, lo.y));
    for (i, v) in [(ix1, ul.x), (iy1, ul.y), (ix2, lr.x), (iy2, lr.y)] {
        if let Some(t) = tags.get_mut(i) {
            t.value = v.to_string();
        }
    }
    ole_data_transform(tags, [ul, Vec2::new(lr.x, ul.y), lr, Vec2::new(ul.x, lr.y)], frame);
}

/// Whether `m` can move an OLE2FRAME: a translation (move, copy, stretch of the whole frame) or a
/// uniform scale with a positive factor, which keep the frame axis aligned and its upper-left corner
/// upper-left.
pub fn ole_frame_accepts(m: &cadcraft_geom::Mat3) -> bool {
    const T: f64 = 1e-12;
    // The tolerance grows with the scale: a matrix built from two angles (ALIGN) leaves rounding noise
    // in b and c of about `scale * 1e-16`.
    let t = T * m.a.abs();
    m.b.abs() < t && m.c.abs() < t && m.a > 0.0 && (m.a - m.d).abs() <= t
}

/// Bounds of a single-line TEXT / ATTDEF / ATTRIB of `len` characters, following its justification
/// the same way the renderer places it (the stroke font is about 0.88 of the height per character).
fn text_box(t: &crate::Text, len: usize) -> Bounds2 {
    use crate::{HAlign, VAlign};
    let w = t.height * 0.9 * t.width_factor.abs().max(0.01) * len.max(1) as f64;
    // Justified text (TL, ML and BL too) hangs from its alignment point (DXF group 11).
    let origin = t.justify_point();
    let dx = match t.halign {
        HAlign::Left | HAlign::Aligned | HAlign::Fit => 0.0,
        HAlign::Center | HAlign::Middle => -w / 2.0,
        HAlign::Right => -w,
    };
    let dy = match (t.halign, t.valign) {
        (HAlign::Middle, _) => -t.height / 2.0,
        (_, VAlign::Baseline) => 0.0,
        (_, VAlign::Bottom) => t.height / 3.0,
        (_, VAlign::Middle) => -t.height / 2.0,
        (_, VAlign::Top) => -t.height,
    };
    let local = [Vec2::new(dx, dy), Vec2::new(dx + w, dy), Vec2::new(dx + w, dy + t.height), Vec2::new(dx, dy + t.height)];
    Bounds2::from_points(local.map(|p| origin + p.rotate(t.rotation)))
}

/// Whether an entity inside a block definition shows: its own invisible flag and its layer count,
/// as when drawing. Dimensional constraints of dynamic blocks (layer `*ADSK_CONSTRAINTS`) and the
/// entities a visibility state hides are invisible this way, and they sit far from the block's
/// geometry in real drawings. Layer "0" takes the insert's layer, which the caller has checked.
/// `off_counts`: a layer turned off counts too, as in the drawing's extents ([`extent_bounds`]).
fn in_block_visible(d: &Drawing, e: &Entity, off_counts: bool) -> bool {
    // Attribute definitions are not drawn in an insert (its attribs are, see below).
    !matches!(e.kind, EntityKind::AttDef(_)) && e.common.visible && layer_counts(d, &e.common.layer, off_counts)
}

/// Whether what lies on `layer` inside an insert counts: layer "0" takes the insert's layer, which
/// the caller has checked; a frozen layer never counts, one turned off only with `off_counts`.
fn layer_counts(d: &Drawing, layer: &str, off_counts: bool) -> bool {
    layer == "0" || d.layer(layer).is_none_or(|l| !l.frozen && (l.on || off_counts))
}

/// The layer an insert's attribute draws on, as the renderer takes it: its own, else its
/// definition's (layer "0" being the insert's).
fn attrib_layer<'a>(d: &'a Drawing, ins: &crate::Insert, a: &'a crate::Attrib) -> &'a str {
    a.props.layer.as_deref().unwrap_or_else(|| {
        d.block(&ins.block)
            .and_then(|blk| blk.entities.iter().find(|be| matches!(&be.kind, EntityKind::AttDef(ad) if ad.tag.eq_ignore_ascii_case(&a.tag))))
            .map_or("0", |be| be.common.layer.as_str())
    })
}

/// A text that draws no glyph (empty, or only blanks and non-breaking spaces) takes no room.
fn blank(s: &str) -> bool {
    s.replace("\\~", " ").chars().all(char::is_whitespace)
}

/// Approximate box of an MTEXT, from its attachment point, width and line count.
fn mtext_box(t: &crate::MText) -> Bounds2 {
    let shown = crate::fields::display_mtext(&t.contents);
    let lines = shown.split("\\P").count().max(1);
    let longest = shown.split("\\P").map(|l| l.chars().count()).max().unwrap_or(1);
    let estimate = t.height * 0.8 * longest as f64;
    // The wrap width only narrows the box: a garbage width must not stretch the drawing extents (#2465).
    let mut w = if t.width > 0.0 { t.width.min(estimate) } else { estimate };
    let mut h = t.height * 1.66 * lines as f64;
    if let Some(c) = t.columns.as_ref().filter(|_| t.width > 0.0) {
        // Columns side by side, the text flowing down each in turn.
        w = c.total_width(t.width);
        h = if c.max_height() > 0.0 { c.max_height() } else { h };
    }
    let (dx, dy) = match t.attach {
        2 | 5 | 8 => (-w / 2.0, 0.0),
        3 | 6 | 9 => (-w, 0.0),
        _ => (0.0, 0.0),
    };
    let top = match t.attach {
        4..=6 => h / 2.0,
        7..=9 => h,
        _ => 0.0,
    };
    let o = t.insert.xy();
    let pts = [Vec2::new(dx, top + dy), Vec2::new(dx + w, top + dy), Vec2::new(dx + w, top - h + dy), Vec2::new(dx, top - h + dy)]
        .map(|p| o + p.rotate(t.rotation));
    Bounds2::from_points(pts)
}

/// The box of what `e` draws (block contents on layers turned off left out).
pub fn entity_bounds(d: &Drawing, e: &Entity, depth: usize) -> Bounds2 {
    bounds_of(d, e, depth, false, &mut HashMap::new())
}

/// The box `e` takes in the drawing's extents: as [`entity_bounds`], but block contents on a layer
/// turned off count, as AutoCAD 2024's ZOOM Extents counts them (a frozen layer's do not).
pub fn extent_bounds(d: &Drawing, e: &Entity) -> Bounds2 {
    bounds_of(d, e, 0, true, &mut HashMap::new())
}

/// The bounds of block contents measured so far, kept across the entities of one unchanged
/// drawing. Measuring many entities with [`entity_bounds_cached`] or [`extent_bounds_cached`]
/// and one cache measures each block definition once per depth for all of them; a fresh cache per
/// entity would measure a block again for every reference to it, R references to a block of B
/// entities costing R x B (CWE-407). Use a cache only while the drawing it measured is unchanged.
#[derive(Default)]
pub struct BlockBoundsCache<'a> {
    /// By block name and depth, block contents on layers turned off left out ([`entity_bounds`]).
    drawn: HashMap<(&'a str, usize), Bounds2>,
    /// The same with those contents counted ([`extent_bounds`]).
    extents: HashMap<(&'a str, usize), Bounds2>,
}

/// [`entity_bounds`] reusing the block bounds `cache` holds from earlier calls on drawing `d`.
pub fn entity_bounds_cached<'a>(d: &'a Drawing, e: &Entity, depth: usize, cache: &mut BlockBoundsCache<'a>) -> Bounds2 {
    bounds_of(d, e, depth, false, &mut cache.drawn)
}

/// [`extent_bounds`] reusing the block bounds `cache` holds from earlier calls on drawing `d`.
pub fn extent_bounds_cached<'a>(d: &'a Drawing, e: &Entity, cache: &mut BlockBoundsCache<'a>) -> Bounds2 {
    bounds_of(d, e, 0, true, &mut cache.extents)
}

fn bounds_of<'a>(d: &'a Drawing, e: &Entity, depth: usize, off_counts: bool, blocks: &mut HashMap<(&'a str, usize), Bounds2>) -> Bounds2 {
    // A top-level annotative object measures as it draws at the current annotation scale.
    if depth == 0 {
        match d.anno_draw(e) {
            crate::AnnoDraw::Plain => {}
            crate::AnnoDraw::Hidden => return Bounds2::EMPTY,
            crate::AnnoDraw::Scaled(f) => return bounds_in(d, &crate::anno_scaled(e, f), 0, blocks, off_counts),
        }
    }
    bounds_in(d, e, depth, blocks, off_counts)
}

/// [`entity_bounds`] with the bounds of block contents (in block coordinates) already computed,
/// by block name and depth: every further reference to a block at that depth reuses them instead of
/// expanding the block again, so nested or self-referencing blocks cost at most one pass over each
/// definition per depth instead of one per reference (which grows exponentially with the nesting
/// depth). The depth is part of the key because [`MAX_BLOCK_DEPTH`] cuts a deep visit short: its
/// bounds must not stand in for a shallower reference to the same block (#706).
fn bounds_in<'a>(d: &'a Drawing, e: &Entity, depth: usize, blocks: &mut HashMap<(&'a str, usize), Bounds2>, off_counts: bool) -> Bounds2 {
    match &e.kind {
        EntityKind::Table(t) if t.is_broken() => t.bounds(),
        EntityKind::Text(t) if blank(&crate::fields::display(&t.value)) => Bounds2::EMPTY,
        EntityKind::MText(t) if blank(&crate::fields::display_mtext(&t.contents)) => Bounds2::EMPTY,
        // Text that matches the layout's orientation is drawn upright (#2498).
        EntityKind::Text(t) if e.common.matches_layout_orientation() => {
            text_box(&crate::Text { rotation: 0.0, ..t.clone() }, crate::fields::display(&t.value).chars().count())
        }
        EntityKind::MText(t) if e.common.matches_layout_orientation() => mtext_box(&crate::MText { rotation: 0.0, ..t.clone() }),
        EntityKind::Text(t) => text_box(t, crate::fields::display(&t.value).chars().count()),
        EntityKind::AttDef(a) => text_box(&a.text, a.tag.chars().count()),
        EntityKind::MText(t) => mtext_box(t),
        EntityKind::Insert(ins) => {
            if depth >= MAX_BLOCK_DEPTH {
                return Bounds2::EMPTY;
            }
            // A reference to a missing block (an unloaded xref, a purged definition) or to an empty
            // one draws nothing, so it contributes nothing: ZOOM Extents must not fly off to its
            // insertion point. Attributes below still count.
            let mut b = Bounds2::EMPTY;
            if let Some(blk) = d.block(&ins.block) {
                let m = ins.transform(blk.base.xy());
                let inner = match blocks.get(&(blk.name.as_str(), depth)) {
                    Some(b) => *b,
                    None => {
                        let mut inner = Bounds2::EMPTY;
                        for be in blk.entities.iter().filter(|be| in_block_visible(d, be, off_counts)) {
                            inner = inner.union(&bounds_in(d, be, depth + 1, blocks, off_counts));
                        }
                        blocks.insert((blk.name.as_str(), depth), inner);
                        inner
                    }
                };
                b = if inner.is_empty() { Bounds2::EMPTY } else { Bounds2::from_points(inner.corners().map(|c| m.apply(c))) };
                // XCLIP: the reference app takes the boundary's box, also where it reaches past the
                // block or misses it (an inverted clip can show anything, so it keeps the block's).
                if let Some(c) = ins.clip.as_ref().filter(|c| c.active() && !c.inverted)
                    && !b.is_empty()
                {
                    b = Bounds2::from_points(c.polygon().into_iter().map(|p| m.apply(p)));
                }
                if ins.cols > 1 || ins.rows > 1 {
                    // The grid's four corner copies (as many as are drawn): turned, the far corner
                    // alone misses the side ones.
                    let (cols, rows) =
                        (f64::from(ins.cols.clamp(1, 10_000).saturating_sub(1)), f64::from(ins.rows.clamp(1, 10_000).saturating_sub(1)));
                    let one = b;
                    for (c, r) in [(cols, 0.0), (0.0, rows), (cols, rows)] {
                        let off = Vec2::new(ins.col_spacing * c, ins.row_spacing * r).rotate(ins.rotation);
                        b = b.union(&Bounds2::from_points(one.corners().map(|p| p + off)));
                    }
                }
            }
            for a in &ins.attribs {
                if !a.invisible && layer_counts(d, attrib_layer(d, ins, a), off_counts) {
                    b = b.union(&match a.multiline_text() {
                        Some(m) => mtext_box(&m),
                        None => text_box(&a.text, crate::fields::display(&a.text.value).chars().count()),
                    });
                }
            }
            b
        }
        EntityKind::Dimension(dm) => {
            let pts: Vec<Vec2> = match dm.kind {
                crate::DimKind::Linear { .. } | crate::DimKind::Aligned | crate::DimKind::ArcLength => vec![dm.defpt.xy(), dm.p13.xy(), dm.p14.xy()],
                crate::DimKind::Radius | crate::DimKind::Diameter => vec![dm.defpt.xy(), dm.p15.xy()],
                crate::DimKind::Angular3P => vec![dm.defpt.xy(), dm.p13.xy(), dm.p14.xy(), dm.p15.xy()],
                crate::DimKind::Angular => vec![dm.defpt.xy(), dm.p13.xy(), dm.p14.xy(), dm.p15.xy(), dm.p16.xy()],
                crate::DimKind::Ordinate { .. } => vec![dm.p13.xy(), dm.p14.xy()],
            };
            let mut b = Bounds2::from_points(pts);
            if dm.user_text_pos {
                b.add(dm.text_mid.xy());
            }
            // Room for text and arrows.
            // The same scale the dimension's geometry uses.
            let th = d
                .dim_style(&dm.style)
                .map(|s| {
                    let s = s.with_overrides(&dm.overrides);
                    s.text_height * s.effective_scale(d.header.f64("DIMSCALE", 1.0))
                })
                .unwrap_or(0.18 * d.header.f64("DIMSCALE", 1.0).max(1e-9));
            b = b.expand(th * 2.5);
            if let Some(blk) = dm.block.as_ref().and_then(|n| d.block(n))
                && depth < MAX_BLOCK_DEPTH
            {
                // Reuse the shared block-bounds cache (as the Insert branch does) so a dimension
                // block with branching or self-referencing children costs one pass per definition
                // per depth, not one per reference (which grows exponentially) (CWE-400).
                let inner = match blocks.get(&(blk.name.as_str(), depth)) {
                    Some(cached) => *cached,
                    None => {
                        let mut inner = Bounds2::EMPTY;
                        for be in blk.entities.iter().filter(|be| in_block_visible(d, be, off_counts)) {
                            inner = inner.union(&bounds_in(d, be, depth + 1, blocks, off_counts));
                        }
                        blocks.insert((blk.name.as_str(), depth), inner);
                        inner
                    }
                };
                // The block draws translated by the insertion point (group 12), so its bounds
                // move with it (#2634); the cache above keeps the block's own bounds.
                let off = dm.insertion_point.xy();
                let inner = if off == Vec2::ZERO { inner } else { Bounds2 { min: inner.min + off, max: inner.max + off } };
                b = b.union(&inner);
            }
            b
        }
        EntityKind::MLeader(m) => {
            let mut b = Bounds2::from_points(m.leaders.iter().flatten().map(|v| v.xy()).chain(std::iter::once(m.landing.xy())));
            if let Some(t) = &m.text {
                b.add(t.insert.xy());
                if !blank(&t.contents) {
                    b = b.union(&mtext_box(t));
                }
            }
            if let Some(ins) = &m.block {
                let be = Entity { handle: e.handle, common: e.common.clone(), kind: EntityKind::Insert(ins.clone()) };
                b = b.union(&bounds_in(d, &be, depth, blocks, off_counts));
            }
            b
        }
        EntityKind::Table(t) => {
            let w: f64 = t.col_widths.iter().sum();
            let h: f64 = t.row_heights.iter().sum();
            Bounds2::new(t.insert.xy(), t.insert.xy() + Vec2::new(w, -h))
        }
        // Text widths estimated like `text_box`; the renderer measures the font.
        EntityKind::Tolerance(t) => t.layout(d, &|s, h| h * 0.9 * s.chars().count() as f64).bounds,
        EntityKind::Image(i) => {
            let o = i.insert.xy();
            let u = i.u.xy() * i.size.x;
            let v = i.v.xy() * i.size.y;
            Bounds2::from_points([o, o + u, o + v, o + u + v])
        }
        EntityKind::LwPolyline(p) => lwpolyline_bounds(p),
        // An OLE object's content is not drawn, but its frame is, so the frame counts (#2422).
        EntityKind::Unknown(u) if u.is_ole_frame() => ole_frame(&u.tags).map_or(Bounds2::EMPTY, Bounds2::from_points),
        kind => {
            let mut b = Bounds2::EMPTY;
            for p in kind.prims() {
                let pb = match p {
                    Prim::Seg(s) => s.bounds(),
                    Prim::Circle(c) => c.bounds(),
                    Prim::Ellipse(e) => e.bounds(),
                    Prim::Spline(s) => s.bounds(),
                    Prim::Point(p) => Bounds2::from_points([p]),
                    // Infinite lines don't contribute to extents, rays included (as in ZOOM Extents;
                    // real drawings carry stray rays millions of units away).
                    Prim::Infinite { .. } => Bounds2::EMPTY,
                    Prim::Fill(pts) => Bounds2::from_points(pts),
                };
                b = b.union(&pb);
            }
            b
        }
    }
}

/// The polyline's drawn extent: its centre line, widened by the filled band of every segment,
/// with the constant width or each segment's own start/end widths tapering along it, as drawn.
fn lwpolyline_bounds(p: &LwPolyline) -> Bounds2 {
    let mut b = Polyline { vertices: p.vertices.clone(), closed: p.closed }.bounds();
    let n = p.vertices.len();
    let count = if p.closed { n } else { n.saturating_sub(1) };
    // The constant width wins, as when drawing; non-finite or negative widths count as zero.
    let half = |w: f64| if w.is_finite() && w > 0.0 { w / 2.0 } else { 0.0 };
    for i in 0..count {
        let (Some(v), Some(w)) = (p.vertices.get(i), p.vertices.get((i + 1) % n)) else { continue };
        let (h0, h1) = if p.const_width > 0.0 { (half(p.const_width), half(p.const_width)) } else { (half(v.start_width), half(v.end_width)) };
        if h0 == 0.0 && h1 == 0.0 {
            // A thin segment: the centre line already counts.
            continue;
        }
        match bulge_to_arc(v.p, w.p, v.bulge) {
            // A straight segment's band is a trapezoid: its four corners are exact.
            None if !v.p.near(w.p, EPS) => {
                let nrm = (w.p - v.p).normalized().perp();
                for q in [v.p + nrm * h0, v.p - nrm * h0, w.p + nrm * h1, w.p - nrm * h1] {
                    b.add(q);
                }
            }
            None => {}
            // An arc segment: both band edges, sampled finely along the arc with the width
            // interpolated along it, then widened by the sampling tolerance so the curve between
            // samples is inside.
            Some((arc, ccw)) => {
                let outer = arc.radius + h0.max(h1);
                let tol = outer * 1e-4;
                let mut pts = Vec::new();
                Arc { radius: outer, ..arc }.tessellate(tol, &mut pts);
                let last = pts.len().saturating_sub(1).max(1) as f64;
                let mut edges = Bounds2::EMPTY;
                for (k, q) in pts.iter().enumerate() {
                    // The fraction along the segment, which runs against the arc when not ccw.
                    let t = k as f64 / last;
                    let t = if ccw { t } else { 1.0 - t };
                    let h = h0 + (h1 - h0) * t;
                    let dir = (*q - arc.center).normalized();
                    edges.add(arc.center + dir * (arc.radius + h));
                    edges.add(arc.center + dir * (arc.radius - h));
                }
                if !edges.is_empty() {
                    b = b.union(&edges.expand(tol));
                }
            }
        }
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HAlign, Text, VAlign};
    use cadcraft_geom::Vec3;

    fn text(halign: HAlign, valign: VAlign) -> Text {
        Text {
            insert: Vec3::new(0.0, 0.0, 0.0),
            align_pt: Some(Vec3::new(0.0, 0.0, 0.0)),
            height: 1.0,
            value: "HELLO".into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign,
            valign,
        }
    }

    fn bounds(kind: EntityKind) -> Bounds2 {
        entity_bounds(&Drawing::default(), &Entity::new(crate::Handle(1), kind), 0)
    }

    fn mtext(contents: &str, width: f64) -> crate::MText {
        crate::MText {
            insert: Vec3::new(10.0, 10.0, 0.0),
            height: 2.5,
            width,
            attach: 1,
            rotation: 0.0,
            style: "Standard".into(),
            contents: contents.into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        }
    }

    /// A garbage wrap width only narrows an MTEXT's box, never widens it: a huge group 41 must not
    /// stretch the drawing extents (#2465).
    #[test]
    fn a_huge_mtext_wrap_width_does_not_inflate_the_extents() {
        let b = bounds(EntityKind::MText(mtext("AB", 300_000_009_519_104.0)));
        // "AB" at height 2.5 is 2 chars * 0.8 * 2.5 = 4 wide.
        assert!(b.max.x <= 10.0 + 4.0 + 1e-9, "box ends at x = {}", b.max.x);
        assert!(b.min.x >= 10.0 - 1e-9);
    }

    /// A wrap width narrower than the text still bounds the box to that width.
    #[test]
    fn a_narrow_mtext_wrap_width_still_sets_the_box_width() {
        let b = bounds(EntityKind::MText(mtext("A LONG LINE OF WRAPPING TEXT", 5.0)));
        assert!((b.max.x - b.min.x - 5.0).abs() < 1e-9, "width {}", b.max.x - b.min.x);
    }
    #[test]
    fn right_justified_text_is_left_of_alignment_point() {
        let b = bounds(EntityKind::Text(text(HAlign::Right, VAlign::Baseline)));
        // Rendered strokes span x -4.4..0, y 0..1.
        assert!(b.min.x <= -4.4 && b.max.x >= 0.0 && b.max.x < 1.0);
        assert!(b.min.y <= 0.0 && b.max.y >= 1.0);
    }

    #[test]
    fn centered_and_top_left_text_follow_placement() {
        let c = bounds(EntityKind::Text(text(HAlign::Center, VAlign::Baseline)));
        assert!(c.min.x <= -2.2 && c.max.x >= 2.2);
        let tl = bounds(EntityKind::Text(text(HAlign::Left, VAlign::Top)));
        assert!(tl.min.x <= 0.0 && tl.max.x >= 4.4);
        assert!(tl.min.y <= -1.0 && tl.max.y >= 0.0 && tl.max.y < 1.0);
    }

    #[test]
    fn right_justified_attdef_is_left_of_alignment_point() {
        let text = text(HAlign::Right, VAlign::Baseline);
        let a = crate::Attrib {
            tag: "HELLO".into(),
            text,
            invisible: false,
            constant: false,
            prompt: String::new(),
            props: Default::default(),
            multiline: None,
            modes: Default::default(),
        };
        let b = bounds(EntityKind::AttDef(a));
        assert!(b.min.x <= -4.4 && b.max.x < 1.0);
    }

    #[test]
    fn multileader_text_is_inside_its_bounds() {
        let mtext = crate::MText {
            insert: Vec3::new(25.45, 0.0, 0.0),
            height: 5.0,
            width: 0.0,
            attach: 4,
            rotation: 0.0,
            style: "Standard".into(),
            contents: "N".into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        };
        let ml = crate::MLeader {
            leaders: vec![vec![Vec3::new(0.0, 0.0, 0.0)]],
            landing: Vec3::new(25.0, 0.0, 0.0),
            dogleg: 0.45,
            text: Some(mtext.clone()),
            style: "Standard".into(),
            arrow_size: 1.0,
            spline: false,
            block: None,
        };
        let b = bounds(EntityKind::MLeader(ml.clone()));
        // The glyph spans x 25.45..28.78, y -2.5..2.5.
        assert!(b.min.x <= 0.0 && b.max.x >= 28.79 && b.min.y <= -2.5 && b.max.y >= 2.5);
        let mut empty = ml;
        empty.text = Some(crate::MText { contents: " ".into(), ..mtext });
        assert!(bounds(EntityKind::MLeader(empty)).max.x < 26.0);
    }

    /// An OLE2FRAME counts in the extents through its corner tags (#2422); a missing or
    /// non-finite corner counts as nothing.
    #[test]
    fn ole_frame_counts_in_extents() {
        let tag = |code: i32, value: &str| crate::RawTag { code, value: value.into() };
        let frame = |tags: Vec<crate::RawTag>| EntityKind::Unknown(crate::Unknown { dxf_type: "OLE2FRAME".into(), tags });
        let b = bounds(frame(vec![tag(10, "30"), tag(20, "-19"), tag(11, "35"), tag(21, "-22")]));
        assert_eq!((b.min, b.max), (Vec2::new(30.0, -22.0), Vec2::new(35.0, -19.0)));
        assert!(bounds(frame(vec![tag(10, "1"), tag(20, "2")])).is_empty(), "one corner is not a frame");
        assert!(bounds(frame(vec![tag(10, "NaN"), tag(20, "0"), tag(11, "1"), tag(21, "inf")])).is_empty());
        assert!(bounds(EntityKind::Unknown(crate::Unknown { dxf_type: "OTHER".into(), tags: vec![tag(10, "5"), tag(20, "5")] })).is_empty());
    }

    /// Groups 10/20 inside an application-defined `102 {...}` group belong to the application, not
    /// to the frame: the real corners come after it (review on #2587).
    #[test]
    fn ole_frame_skips_application_groups() {
        let tag = |code: i32, value: &str| crate::RawTag { code, value: value.into() };
        let tags = vec![
            tag(102, "{ACAD_REACTORS"),
            tag(10, "900"),
            tag(20, "900"),
            tag(11, "901"),
            tag(21, "901"),
            tag(102, "}"),
            tag(10, "30"),
            tag(20, "-19"),
            tag(11, "35"),
            tag(21, "-22"),
        ];
        let q = ole_frame(&tags).expect("frame");
        assert_eq!((q[0], q[2]), (Vec2::new(30.0, -19.0), Vec2::new(35.0, -22.0)));
        assert!(ole_frame(&tags[..6]).is_none(), "corners only inside the group are not a frame");
    }

    #[test]
    fn a_shared_block_cache_measures_as_a_fresh_one_for_each_entity() {
        let line = |a: (f64, f64), b: (f64, f64), layer: &str| {
            let mut e = Entity::new(crate::Handle(1), EntityKind::Line(crate::Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) }));
            e.common.layer = layer.into();
            e
        };
        let ins = |block: &str, at: f64| {
            Entity::new(
                crate::Handle(2),
                EntityKind::Insert(crate::Insert {
                    block: block.into(),
                    insert: Vec3::new(at, at, 0.0),
                    scale: Vec3::new(1.0, 1.0, 1.0),
                    rotation: 0.0,
                    attribs: Vec::new(),
                    cols: 1,
                    rows: 1,
                    col_spacing: 0.0,
                    row_spacing: 0.0,
                    clip: None,
                }),
            )
        };
        let mut d = Drawing::new_imperial();
        d.ensure_layer("OFF");
        if let Some(l) = d.layer_mut("OFF") {
            l.on = false;
        }
        // A block with a part on a layer turned off (left out when drawn, counted in extents),
        // and a block that nests it.
        let mut b = crate::Block::new("B");
        b.entities.push(line((0.0, 0.0), (1.0, 1.0), "0"));
        b.entities.push(line((0.0, 0.0), (5.0, 5.0), "OFF"));
        d.blocks.insert("B".into(), std::sync::Arc::new(b));
        let mut n = crate::Block::new("N");
        n.entities.push(ins("B", 10.0));
        d.blocks.insert("N".into(), std::sync::Arc::new(n));
        let ents: Vec<Entity> = (0..50).map(|i| ins(if i % 2 == 0 { "B" } else { "N" }, f64::from(i))).collect();
        // One cache for both kinds of measure, interleaved: neither may answer for the other.
        let mut cache = BlockBoundsCache::default();
        for e in &ents {
            let (drawn, extents) = (entity_bounds_cached(&d, e, 0, &mut cache), extent_bounds_cached(&d, e, &mut cache));
            let (want_drawn, want_extents) = (entity_bounds(&d, e, 0), extent_bounds(&d, e));
            assert_eq!((drawn.min, drawn.max), (want_drawn.min, want_drawn.max));
            assert_eq!((extents.min, extents.max), (want_extents.min, want_extents.max));
            assert!(extents.width() > drawn.width());
        }
    }

    /// A turned array of references reaches its side copies, not only the first and the last;
    /// a hostile count measures only the copies that are drawn.
    #[test]
    fn a_rotated_array_counts_every_corner_copy() {
        let mut d = Drawing::new_metric();
        let mut b = crate::Block::new("B");
        b.entities.push(Entity::new(crate::Handle(1), EntityKind::Line(crate::Line { a: Vec3::ZERO, b: Vec3::new(1.0, 0.0, 0.0) })));
        d.blocks.insert("B".into(), std::sync::Arc::new(b));
        let array = |cols: u32, rows: u32, rotation: f64| {
            let ins = crate::Insert {
                block: "B".into(),
                scale: Vec3::new(1.0, 1.0, 1.0),
                rotation,
                cols,
                rows,
                col_spacing: 10.0,
                row_spacing: 10.0,
                ..crate::Insert::default()
            };
            Entity::new(crate::Handle(2), EntityKind::Insert(ins))
        };
        // Turned 45°: the copies sit at (0, 0), (7.07, 7.07), (-7.07, 7.07) and (0, 14.14).
        let b = entity_bounds(&d, &array(2, 2, std::f64::consts::FRAC_PI_4), 0);
        let side = 10.0 * std::f64::consts::FRAC_1_SQRT_2;
        assert!((b.min.x + side).abs() < 1e-9 && b.max.x >= side, "{b:?}");
        assert!((b.max.y - 2.0 * side - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9, "{b:?}");
        // The renderer draws at most 10,000 columns.
        let b = entity_bounds(&d, &array(u32::MAX, 1, 0.0), 0);
        assert!((b.max.x - (10.0 * 9_999.0 + 1.0)).abs() < 1e-6, "{b:?}");
    }

    /// An OLE2FRAME at large coordinates, with its OLE data header holding the frame's corners
    /// shifted by `dx` in x.
    fn big_frame(dx: f64) -> Vec<crate::RawTag> {
        let (x1, y1, x2, y2) = (1e9, 1e9, 1e9 + 10.0, 1e9 + 5.0);
        let mut blob = vec![1u8, 0];
        for (x, y) in [(x1, y1), (x2, y1), (x2, y2), (x1, y2)] {
            for v in [x + dx, y, 7.0] {
                blob.extend_from_slice(&v.to_le_bytes());
            }
        }
        let tag = |code: i32, value: String| crate::RawTag { code, value };
        vec![
            tag(10, x1.to_string()),
            tag(20, y1.to_string()),
            tag(11, x2.to_string()),
            tag(21, y2.to_string()),
            tag(310, blob.iter().map(|b| format!("{b:02X}")).collect()),
        ]
    }

    /// The x of the first corner in the OLE data of a [`big_frame`].
    fn data_x(tags: &[crate::RawTag]) -> f64 {
        let hex = tags.last().map(|t| t.value.as_str()).unwrap_or_default();
        let bytes: Vec<u8> = (0..hex.len() / 2).filter_map(|i| u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()).collect();
        bytes.get(2..10).and_then(|s| <[u8; 8]>::try_from(s).ok()).map(f64::from_le_bytes).unwrap_or(f64::NAN)
    }

    /// The match between the frame and the corners in its OLE data does not loosen with the
    /// coordinate: half a unit off at 1e9 is not the frame, so the bytes stay as they are (review
    /// on #2587).
    #[test]
    fn a_half_unit_offset_at_large_coordinates_is_not_the_frame() {
        let mut tags = big_frame(0.5);
        let before = tags.last().cloned();
        ole_frame_transform(&mut tags, &cadcraft_geom::Mat3::translate(Vec2::new(1.0, 1.0)));
        assert_eq!(tags.first().map(|t| t.value.as_str()), Some("1000000001"), "the frame tags move");
        assert_eq!(tags.last().map(|t| &t.value), before.as_ref().map(|t| &t.value), "the 310 bytes stay identical");
    }

    #[test]
    fn exact_corners_at_large_coordinates_still_move() {
        let mut tags = big_frame(0.0);
        ole_frame_transform(&mut tags, &cadcraft_geom::Mat3::translate(Vec2::new(1.0, 1.0)));
        assert_eq!(data_x(&tags), 1e9 + 1.0);
    }

    /// A translation, and a uniform positive scale about any point, keep a frame axis aligned and
    /// upper-left; a rotation, a mirror, a non-uniform or non-positive scale do not.
    #[test]
    fn ole_frame_accepts_translations_and_uniform_positive_scales_only() {
        use cadcraft_geom::Mat3;
        let p = Vec2::new(3.0, -4.0);
        assert!(ole_frame_accepts(&Mat3::translate(p)));
        assert!(ole_frame_accepts(&Mat3::scale_about(p, 2.5)));
        assert!(ole_frame_accepts(&Mat3::scale_about(p, 1e-3)));
        assert!(!ole_frame_accepts(&Mat3::scale(2.0, 3.0)));
        assert!(!ole_frame_accepts(&Mat3::scale_about(p, -1.0)));
        assert!(!ole_frame_accepts(&Mat3::scale_about(p, 0.0)));
        assert!(!ole_frame_accepts(&Mat3::rotate_about(p, 0.5)));
        assert!(!ole_frame_accepts(&Mat3::mirror(Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0))));
    }
    /// ALIGN with a scale builds its matrix from two angles that agree only to rounding, so a large
    /// scale leaves a rotation part of about `scale * 1e-16`. That is a scale, not a turn: the
    /// tolerance for it grows with the scale, while a real turn is still refused (review on #2587).
    #[test]
    fn ole_frame_accepts_a_large_scale_with_rounding_noise_in_its_rotation() {
        use cadcraft_geom::Mat3;
        let p = Vec2::new(3.0, -4.0);
        let noisy = Mat3::scale_about(p, 1e6).then_before(Mat3::rotate_about(p, 2e-16));
        assert!(noisy.b.abs() > 1e-12, "the setup must exceed an absolute tolerance: {}", noisy.b);
        assert!(ole_frame_accepts(&noisy));
        let turned = Mat3::scale_about(p, 1e6).then_before(Mat3::rotate_about(p, 1e-6));
        assert!(!ole_frame_accepts(&turned));
    }

    /// The corners (x, y) in the OLE data header of a frame built by [`big_frame`]-style tags.
    fn header_corners(tags: &[crate::RawTag]) -> Vec<(f64, f64)> {
        let hex: String = tags.iter().filter(|t| t.code == 310).map(|t| t.value.as_str()).collect();
        let bytes: Vec<u8> = (0..hex.len() / 2).filter_map(|i| u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()).collect();
        let f = |o: usize| bytes.get(o..o + 8).and_then(|s| <[u8; 8]>::try_from(s).ok()).map(f64::from_le_bytes).unwrap_or(f64::NAN);
        (0..4).map(|c| (f(2 + c * 24), f(10 + c * 24))).collect()
    }

    fn small_frame() -> Vec<crate::RawTag> {
        let mut blob = vec![1u8, 0];
        for (x, y) in [(30.0f64, -19.0f64), (35.0, -19.0), (35.0, -22.0), (30.0, -22.0)] {
            for v in [x, y, 7.0] {
                blob.extend_from_slice(&v.to_le_bytes());
            }
        }
        let tag = |code: i32, value: &str| crate::RawTag { code, value: value.into() };
        vec![
            tag(10, "30"),
            tag(20, "-19"),
            tag(11, "35"),
            tag(21, "-22"),
            crate::RawTag { code: 310, value: blob.iter().map(|b| format!("{b:02X}")).collect() },
        ]
    }

    /// Internal callers (EXPLODE, REFEDIT, paste, array placement) transform a frame by any matrix.
    /// It stays axis aligned: groups 10/20 are the upper-left of its box, 11/21 the lower-right,
    /// and the OLE data holds the same box (review on #2587). A mirror keeps the axes, so the box
    /// is that of the mapped corners; a 30° turn keeps the frame's own 5 x 3 about its turned
    /// centre, where the corners' box would flatten it.
    #[test]
    fn a_rotated_frame_stays_axis_aligned_and_consistent() {
        use cadcraft_geom::Mat3;
        let rotate = Mat3::rotate_about(Vec2::new(1.0, 2.0), 30f64.to_radians());
        let mirror = Mat3::mirror(Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0));
        for (m, turned) in [(rotate, true), (mirror, false)] {
            let mut tags = small_frame();
            let mapped = if turned {
                let c = m.apply(Vec2::new(32.5, -20.5));
                [c - Vec2::new(2.5, 1.5), c + Vec2::new(2.5, 1.5)]
            } else {
                [m.apply(Vec2::new(30.0, -19.0)), m.apply(Vec2::new(35.0, -22.0))]
            };
            let (minx, maxx) = (mapped[0].x.min(mapped[1].x), mapped[0].x.max(mapped[1].x));
            let (miny, maxy) = (mapped[0].y.min(mapped[1].y), mapped[0].y.max(mapped[1].y));
            ole_frame_transform(&mut tags, &m);
            let g = |code: i32| tags.iter().find(|t| t.code == code).and_then(|t| t.value.parse::<f64>().ok()).unwrap_or(f64::NAN);
            assert_eq!((g(10), g(20), g(11), g(21)), (minx, maxy, maxx, miny));
            assert_eq!(header_corners(&tags), vec![(minx, maxy), (maxx, maxy), (maxx, miny), (minx, miny)]);
        }
    }
    /// `from_str_radix` accepts a leading sign, so "+A" must not count as a hex byte (review on
    /// #2587): data with one is not valid and MOVE leaves its bytes alone.
    #[test]
    fn a_plus_sign_in_ole_data_is_not_hex() {
        let mut tags = small_frame();
        let hex = tags.last().map(|t| t.value.clone()).unwrap_or_default();
        // The marker's first byte is 01: "+1" would decode to the same byte.
        let signed = format!("+1{}", hex.get(2..).unwrap_or_default());
        if let Some(t) = tags.last_mut() {
            t.value = signed.clone();
        }
        ole_frame_transform(&mut tags, &cadcraft_geom::Mat3::translate(Vec2::new(10.0, 5.0)));
        assert_eq!(tags.first().map(|t| t.value.as_str()), Some("40"), "the frame tags still move");
        assert_eq!(tags.last().map(|t| t.value.as_str()), Some(signed.as_str()));
    }

    /// A payload of 1,000 groups of 127 bytes: only the group(s) holding the 98-byte header are
    /// rewritten. Later groups stay as they were, even ones that are not valid hex or have padding
    /// (the old code re-encoded the whole payload and gave up on any bad group).
    #[test]
    fn a_large_ole_payload_only_rewrites_its_leading_groups() {
        let mut tags = small_frame();
        let first = tags.pop().map(|t| t.value).unwrap_or_default();
        let mut head = first.clone();
        // Fill the first group up to 127 bytes.
        head.push_str(&"ab".repeat(127 - first.len() / 2));
        tags.push(crate::RawTag { code: 310, value: head });
        for g in 1..1000 {
            let value = match g {
                2 => format!("  {}  ", "Ab".repeat(127)),
                500 => "ZZ".repeat(127),
                _ => "ab".repeat(127),
            };
            tags.push(crate::RawTag { code: 310, value });
        }
        let rest: Vec<String> = tags.iter().skip(5).map(|t| t.value.clone()).collect();
        ole_frame_transform(&mut tags, &cadcraft_geom::Mat3::translate(Vec2::new(10.0, 5.0)));
        assert_eq!(header_corners(&tags[..5]), vec![(40.0, -14.0), (45.0, -14.0), (45.0, -17.0), (40.0, -17.0)]);
        let after: Vec<String> = tags.iter().skip(5).map(|t| t.value.clone()).collect();
        assert_eq!(after, rest, "every group after the first is string-identical");
        assert_eq!(tags.get(4).map(|t| t.value.len()), Some(254), "the first group keeps its length");
    }

    /// Above about 8.6e9 one ulp of an f64 is more than the fixed ceiling on the corner match, so a
    /// text corner one ulp from the header double must still count as the same corner (review on
    /// #2587).
    #[test]
    fn ole_frame_moves_at_2e10_with_a_one_ulp_text_corner() {
        let mut tags = big_frame(0.0);
        // Rebuild the frame at 2e10: the header keeps the exact corners, the text is one ulp off.
        let (x1, y1, x2, y2) = (2e10f64, 2e10f64, 2e10 + 10.0, 2e10 + 5.0);
        let mut blob = vec![1u8, 0];
        for (x, y) in [(x1, y1), (x2, y1), (x2, y2), (x1, y2)] {
            for v in [x, y, 7.0] {
                blob.extend_from_slice(&v.to_le_bytes());
            }
        }
        let text_x1 = f64::from_bits(x1.to_bits() + 1);
        tags[0].value = text_x1.to_string();
        tags[1].value = y1.to_string();
        tags[2].value = x2.to_string();
        tags[3].value = y2.to_string();
        if let Some(t) = tags.last_mut() {
            t.value = blob.iter().map(|b| format!("{b:02X}")).collect();
        }
        ole_frame_transform(&mut tags, &cadcraft_geom::Mat3::translate(Vec2::new(1e4, 1e4)));
        assert_eq!(header_corners(&tags)[0].0, text_x1 + 1e4, "the header moved with the frame");
    }

    /// Below scale 1 the rotation bound is still relative to the scale: a small real turn at a tiny
    /// scale is refused, and so is a non-uniform scale whose two factors differ by more than a
    /// relative 1e-12 (review on #2587).
    #[test]
    fn ole_frame_refuses_a_small_turn_at_a_tiny_scale() {
        use cadcraft_geom::Mat3;
        let p = Vec2::new(3.0, -4.0);
        let turned = Mat3::scale_about(p, 1e-9).then_before(Mat3::rotate_about(p, 0.05f64.to_radians()));
        assert!(!ole_frame_accepts(&turned));
        assert!(ole_frame_accepts(&Mat3::scale_about(p, 1e-9)));
        assert!(!ole_frame_accepts(&Mat3::scale(1e-6, 1e-6 + 5e-13)));
    }

    /// Hex digits keep the case they had: bytes the transform does not change come back as the
    /// identical text, also in a group that mixes upper and lower case (review on #2587).
    #[test]
    fn moving_an_ole_frame_keeps_each_hex_digits_case() {
        let mut tags = small_frame();
        let old = tags.last().map(|t| t.value.clone()).unwrap_or_default();
        let mixed: String = old.chars().enumerate().map(|(i, c)| if i < 24 { c.to_ascii_lowercase() } else { c }).collect();
        assert_ne!(mixed, old);
        if let Some(t) = tags.last_mut() {
            t.value = mixed.clone();
        }
        ole_frame_transform(&mut tags, &cadcraft_geom::Mat3::translate(Vec2::new(10.0, 5.0)));
        let new = tags.last().map(|t| t.value.clone()).unwrap_or_default();
        assert_eq!(new.len(), mixed.len());
        let byte = |s: &str, i: usize| s.get(2 * i..2 * i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
        let mut same = 0;
        for i in 0..mixed.len() / 2 {
            if byte(&mixed, i) == byte(&new, i) {
                same += 1;
                assert_eq!(mixed.get(2 * i..2 * i + 2), new.get(2 * i..2 * i + 2), "byte {i}");
            }
        }
        assert!(same > 40, "most of the header is unchanged: {same}");
        assert_eq!(header_corners(&tags)[0], (40.0, -14.0));
    }

    /// A frame from `(x1, y1)` (upper left) to `(x2, y2)` (lower right), its OLE header holding the
    /// same corners.
    fn frame_at(x1: f64, y1: f64, x2: f64, y2: f64) -> Vec<crate::RawTag> {
        let mut blob = vec![1u8, 0];
        for (x, y) in [(x1, y1), (x2, y1), (x2, y2), (x1, y2)] {
            for v in [x, y, 0.0] {
                blob.extend_from_slice(&v.to_le_bytes());
            }
        }
        let tag = |code: i32, value: String| crate::RawTag { code, value };
        vec![
            tag(10, x1.to_string()),
            tag(20, y1.to_string()),
            tag(11, x2.to_string()),
            tag(21, y2.to_string()),
            tag(310, blob.iter().map(|b| format!("{b:02X}")).collect()),
        ]
    }

    fn size(tags: &[crate::RawTag]) -> (f64, f64, Vec2) {
        let o = ole_frame(tags).unwrap();
        let b = Bounds2::from_points(o);
        (b.width(), b.height(), b.center())
    }

    /// A frame turned by an angle that isn't a quarter turn keeps its size about its turned
    /// centre instead of collapsing to the box of its two corners (a 10 x 10 square turned 45°
    /// became 14.14 x 0), and the OLE header follows (#2587 review).
    #[test]
    fn a_frame_turned_off_the_axes_keeps_its_size() {
        use cadcraft_geom::Mat3;
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        let mut tags = frame_at(0.0, 10.0, 10.0, 0.0);
        ole_frame_transform(&mut tags, &Mat3::rotate(std::f64::consts::FRAC_PI_4));
        let (w, h, c) = size(&tags);
        assert!(close(w, 10.0) && close(h, 10.0), "{w} x {h}");
        assert!(close(c.x, 0.0) && close(c.y, 50f64.sqrt()), "{c:?}");
        let (ul, lr) = (header_corners(&tags)[0], header_corners(&tags)[2]);
        assert!(close(ul.0, -5.0) && close(lr.1, 50f64.sqrt() - 5.0), "the header follows: {ul:?} {lr:?}");
        // Nearer a quarter turn, the sides swap: a 10 wide, 4 tall frame turned 100° is 4 x 10.
        let mut tags = frame_at(0.0, 4.0, 10.0, 0.0);
        ole_frame_transform(&mut tags, &Mat3::rotate(100f64.to_radians()));
        let (w, h, _) = size(&tags);
        assert!(close(w, 4.0) && close(h, 10.0), "{w} x {h}");
        // A turn and its inverse bring the frame back.
        let mut tags = frame_at(2.0, 9.0, 7.0, 3.0);
        let m = Mat3::rotate(0.3);
        ole_frame_transform(&mut tags, &m);
        ole_frame_transform(&mut tags, &m.inverse().unwrap());
        let (w, h, c) = size(&tags);
        assert!(close(w, 5.0) && close(h, 6.0) && close(c.x, 4.5) && close(c.y, 6.0), "{w} x {h} at {c:?}");
    }
}
