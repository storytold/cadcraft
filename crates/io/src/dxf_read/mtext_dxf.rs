//! MTEXT specifics of DXF (DXF Reference, MTEXT): control characters in the contents written
//! as a caret and a letter (`^I` a tab, `^J` a line feed, `^ ` a caret), the column groups
//! 75 (type), 76 (count), 78 (flow reversed), 79 (auto height), 48 (width), 49 (gutter) and
//! 50 (column heights, after the column type; before it, 50 is the rotation), and the
//! background mask: 90 (fill setting: 1 a color, 2 the drawing's background), 63 / 421 (the
//! color) and 45 (fill box scale, the border offset factor).

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{MASK_SCALE, MText, MTextColumnType, MTextColumns, MTextMask};
use cadcraft_dxf::Tag;

/// Upper bound on column heights read from a file.
const MAX_COLUMNS: usize = 1_000;

/// Decode the contents' `^I` (tab), `^J` (line feed) and `^ ` (caret). Other carets stay as
/// they are (older files write stacked tolerances `\S+0.1^-0.1;` with a bare caret).
pub(crate) fn decode_contents(s: &str) -> String {
    if !s.contains('^') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '^' {
            out.push(c);
            continue;
        }
        match it.peek().copied() {
            Some(' ') => out.push('^'),
            Some('I') => out.push('\t'),
            Some('J') => out.push('\n'),
            _ => {
                out.push('^');
                continue;
            }
        }
        it.next();
    }
    out
}

/// Encode the contents for a DXF string: a tab as `^I`, a line feed as `^J`, a caret as `^ `;
/// other control characters (which draw nothing) are dropped.
pub(crate) fn encode_contents(s: &str) -> String {
    if !s.chars().any(|c| c == '^' || c.is_control()) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '^' => out.push_str("^ "),
            '\t' => out.push_str("^I"),
            '\n' => out.push_str("^J"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// The MTEXT's own tags: up to an embedded object (group 101), whose groups reuse MTEXT's codes.
fn own(tags: &[Tag]) -> &[Tag] {
    let end = tags.iter().position(|t| t.code == 101).unwrap_or(tags.len());
    tags.get(..end).unwrap_or(tags)
}

/// The tags before the column groups (where group 50 is the rotation angle).
pub(crate) fn before_columns(tags: &[Tag]) -> &[Tag] {
    let tags = own(tags);
    let end = tags.iter().position(|t| t.code == 75).unwrap_or(tags.len());
    tags.get(..end).unwrap_or(tags)
}

fn finite(v: f64) -> f64 {
    if v.is_finite() { v } else { 0.0 }
}

/// The columns of an MTEXT, and the column width (group 48) when there are columns.
pub(crate) fn read_columns(tags: &[Tag]) -> Option<(MTextColumns, f64)> {
    let tags = own(tags);
    let start = tags.iter().position(|t| t.code == 75)?;
    let kind = match tags.get(start).map(Tag::i64) {
        Some(1) => MTextColumnType::Static,
        Some(2) => MTextColumnType::Dynamic,
        _ => return None,
    };
    let cols = tags.get(start..).unwrap_or_default();
    let first = |code: i32| cols.iter().find(|t| t.code == code);
    let heights: Vec<f64> = cols.iter().filter(|t| t.code == 50).take(MAX_COLUMNS).map(|t| finite(t.f64()).max(0.0)).collect();
    let count = first(76).map(Tag::i64).unwrap_or(heights.len() as i64).clamp(1, MAX_COLUMNS as i64);
    let auto_height = first(79).map(Tag::i64).unwrap_or(0) != 0;
    let defined = tags.iter().find(|t| t.code == 46).map(|t| finite(t.f64()).max(0.0)).unwrap_or(0.0);
    let manual = kind == MTextColumnType::Dynamic && !auto_height;
    let height = if manual { heights.iter().copied().fold(0.0, f64::max) } else { heights.first().copied().filter(|h| *h > 0.0).unwrap_or(defined) };
    let c = MTextColumns {
        kind,
        count: u16::try_from(count).unwrap_or(1),
        gutter: first(49).map(|t| finite(t.f64()).max(0.0)).unwrap_or(0.0),
        height,
        auto_height,
        heights: if manual { heights } else { Vec::new() },
        flow_reversed: first(78).map(Tag::i64).unwrap_or(0) != 0,
    };
    Some((c, first(48).map(|t| finite(t.f64()).max(0.0)).unwrap_or(0.0)))
}

/// The border offset factors AutoCAD's Background Mask takes.
const MASK_SCALES: std::ops::RangeInclusive<f64> = 1.0..=5.0;

/// An MTEXT's background mask, from its own groups (`None` when group 90 turns it off).
pub(crate) fn read_mask(tags: &[Tag]) -> Option<MTextMask> {
    let tags = own(tags);
    let first = |code: i32| tags.iter().find(|t| t.code == code);
    let setting = first(90).map(Tag::i64).unwrap_or(0);
    if setting & 3 == 0 {
        return None;
    }
    let mut color = first(63).map(|t| Color::from_aci(i16::try_from(t.i64()).unwrap_or(256))).unwrap_or(Color::ByLayer);
    if let Some(tc) = first(421) {
        color = Color::True(Rgb::from_u32(u32::try_from(tc.i64() & 0xff_ffff).unwrap_or(0)));
    }
    let scale = first(45).map(|t| finite(t.f64())).filter(|s| MASK_SCALES.contains(s)).unwrap_or(MASK_SCALE);
    Some(MTextMask { use_background: setting & 2 != 0, color, scale })
}

/// The background mask groups of an MTEXT with one.
pub(crate) fn mask_tags(t: &MText) -> Vec<Tag> {
    let Some(m) = &t.background else { return Vec::new() };
    let mut v = vec![Tag::i(90, if m.use_background { 2 } else { 1 })];
    match m.color {
        Color::True(rgb) => {
            v.push(Tag::i(63, i64::from(cadcraft_color::nearest_aci(rgb))));
            v.push(Tag::i(421, i64::from(rgb.to_u32())));
        }
        c => v.push(Tag::i(63, i64::from(c.to_aci()))),
    }
    v.push(Tag::f(45, if MASK_SCALES.contains(&m.scale) { m.scale } else { MASK_SCALE }));
    v
}

/// The column groups of an MTEXT with columns.
pub(crate) fn column_tags(t: &MText) -> Vec<Tag> {
    let Some(c) = &t.columns else { return Vec::new() };
    let manual = c.kind == MTextColumnType::Dynamic && !c.auto_height;
    let heights: Vec<f64> = if manual { c.heights.clone() } else { vec![c.height; usize::from(c.count.max(1))] };
    let count = if manual { heights.len().max(1) } else { usize::from(c.count.max(1)) };
    let mut v = vec![
        Tag::i(75, if c.kind == MTextColumnType::Dynamic { 2 } else { 1 }),
        Tag::i(76, count as i64),
        Tag::i(78, i64::from(c.flow_reversed)),
        Tag::i(79, i64::from(c.auto_height)),
        Tag::f(48, t.width),
        Tag::f(49, c.gutter),
    ];
    v.extend(heights.into_iter().map(|h| Tag::f(50, h)));
    v
}
