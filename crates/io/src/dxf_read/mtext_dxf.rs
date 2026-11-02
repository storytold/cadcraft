//! MTEXT specifics of DXF (DXF Reference, MTEXT): control characters in the contents written
//! as a caret and a letter (`^I` a tab, `^J` a line feed, `^ ` a caret), and the column groups
//! 75 (type), 76 (count), 78 (flow reversed), 79 (auto height), 48 (width), 49 (gutter) and
//! 50 (column heights, after the column type; before it, 50 is the rotation). TEXT, ATTDEF,
//! ATTRIB and DIMENSION strings use the same encoding ([`encode_line`]): AutoCAD reads a bare
//! `^2` in `10^2` as a control character.

use cadcraft_doc::{MText, MTextColumnType, MTextColumns, MTextMask};
use cadcraft_dxf::Tag;

/// Upper bound on column heights read from a file.
const MAX_COLUMNS: usize = 1_000;

/// Native background flags are a bit field: 1 enables the fill, 2 uses the drawing colour.
/// R2000 has no native MTEXT fill; keep its mask in bounded CADCraft xdata instead.
pub(crate) fn read_background(tags: &[Tag]) -> Option<MTextMask> {
    let t = super::common::T(tags);
    if let Some(flags) = t.i(90)
        && (flags < 0 || flags & 3 == 0)
    {
        return None;
    }
    if t.i(90).unwrap_or(0) & 3 != 0 {
        let color = t
            .i(421)
            .and_then(|c| u32::try_from(c).ok())
            .filter(|c| *c <= 0xFF_FFFF)
            .map(|c| cadcraft_doc::color::Color::True(cadcraft_doc::color::Rgb::from_u32(c)))
            .unwrap_or_else(|| {
                cadcraft_doc::color::Color::from_aci(t.i(63).and_then(|v| i16::try_from(v).ok()).filter(|v| (0..=256).contains(v)).unwrap_or(256))
            });
        return Some(MTextMask { use_background: t.i(90).unwrap_or(0) & 2 != 0, color, scale: t.fd(45, 1.5) }.normalized());
    }
    let mut own_app = false;
    for (i, tag) in tags.iter().enumerate() {
        let value = match &tag.value {
            cadcraft_dxf::Value::Str(s) => s.as_str(),
            _ => "",
        };
        if tag.code == 1001 {
            own_app = value.eq_ignore_ascii_case(crate::dxf_ext::APP);
        }
        if own_app
            && tag.code == 1000
            && value == "MTEXTMASK"
            && let Some(next) = i.checked_add(1).and_then(|i| tags.get(i)).filter(|v| v.code == 1000)
            && let cadcraft_dxf::Value::Str(value) = &next.value
            && value.len() <= 512
            && let Ok(mask) = serde_json::from_str::<MTextMask>(value)
        {
            return Some(mask.normalized());
        }
    }
    None
}

pub(crate) fn background_tags(mask: Option<MTextMask>, native: bool) -> Vec<Tag> {
    let Some(mask) = mask.map(MTextMask::normalized) else { return Vec::new() };
    if !native {
        return match serde_json::to_string(&mask) {
            Ok(value) => vec![Tag::s(1001, crate::dxf_ext::APP), Tag::s(1000, "MTEXTMASK"), Tag::s(1000, value)],
            Err(_) => Vec::new(),
        };
    }
    let mut tags = vec![Tag::i(90, if mask.use_background { 3 } else { 1 }), Tag::f(45, mask.scale), Tag::i(441, 0)];
    match mask.color {
        cadcraft_doc::color::Color::True(rgb) => tags.push(Tag::i(421, i64::from(rgb.to_u32()))),
        color => tags.push(Tag::i(63, i64::from(color.to_aci()))),
    }
    tags
}

/// Decode the contents' `^I` (tab), `^J` (line feed) and `^ ` (caret). Other carets stay as
/// they are (older files write stacked tolerances `\S+0.1^-0.1;` with a bare caret).
pub(crate) fn decode_contents(s: &str) -> String {
    decode(s, true)
}

/// A single-line string as read: [`decode_contents`] without line feeds (`^J` stays as it is).
pub(crate) fn decode_line(s: &str) -> String {
    decode(s, false)
}

fn decode(s: &str, line_feeds: bool) -> String {
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
            Some('J') if line_feeds => out.push('\n'),
            _ => {
                out.push('^');
                continue;
            }
        }
        it.next();
    }
    out
}

/// A single-line string (TEXT, ATTDEF/ATTRIB, DIMENSION) for a DXF file: a caret as `^ ` and a
/// tab as `^I`, read back by [`decode_line`]. It holds no line break (the writer makes it a
/// space).
pub(crate) fn encode_line(s: &str) -> String {
    s.replace('^', "^ ").replace('\t', "^I")
}

/// Apply `f` ([`encode_line`] or [`decode_line`]) to the groups of a `kind` record that hold
/// an encoded string: the text (1), and an ATTDEF's prompt (3). An embedded MTEXT (from group 101)
/// has its own and is left alone. AutoLISP's entget and entmake/entmod use it.
pub(crate) fn map_text_groups(kind: &str, tags: &mut [Tag], f: fn(&str) -> String) {
    let kind = kind.to_ascii_uppercase();
    if !matches!(kind.as_str(), "TEXT" | "ATTRIB" | "ATTDEF" | "DIMENSION") {
        return;
    }
    for t in tags.iter_mut().take_while(|t| t.code != 101) {
        if t.code == 1 || (t.code == 3 && kind == "ATTDEF") {
            *t = Tag::s(t.code, f(&t.str()));
        }
    }
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
pub(crate) fn own(tags: &[Tag]) -> &[Tag] {
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
    // An application-defined group in the record may hold any code, 50 included.
    let own_tags: Vec<&Tag> = super::common::outside_app(own(tags)).map(|(_, t)| t).collect();
    let Some(start) = own_tags.iter().position(|t| t.code == 75) else { return read_embedded_columns(tags) };
    let tags = own_tags;
    let kind = match tags.get(start).map(|t| t.i64()) {
        Some(1) => MTextColumnType::Static,
        Some(2) => MTextColumnType::Dynamic,
        _ => return None,
    };
    let cols = tags.get(start..).unwrap_or_default();
    let first = |code: i32| cols.iter().find(|t| t.code == code).copied();
    // The first 50 is the number of heights that follow, unless the file predates the count
    // (CADCraft up to f536bf4 wrote the n heights of group 76 alone).
    let fifties = cols.iter().filter(|t| t.code == 50);
    let counted = first(76).is_some_and(|t| fifties.clone().count() as i64 - 1 == t.i64());
    let heights: Vec<f64> = fifties.skip(usize::from(counted)).take(MAX_COLUMNS).map(|t| finite(t.f64()).max(0.0)).collect();
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

/// The columns of an R2018 MTEXT, from its embedded object (group 101), whose groups reuse
/// MTEXT's codes: 71 type, 72 count, 44 column width, 45 gutter, 73 auto height, 74 flow
/// reversed, 46 heights. The column width is also the MTEXT's width.
fn read_embedded_columns(tags: &[Tag]) -> Option<(MTextColumns, f64)> {
    let obj = tags.get(tags.iter().position(|t| t.code == 101)? + 1..)?;
    let first = |code: i32| obj.iter().find(|t| t.code == code);
    let kind = match first(71).map(Tag::i64) {
        Some(1) => MTextColumnType::Static,
        Some(2) => MTextColumnType::Dynamic,
        _ => return None,
    };
    let heights: Vec<f64> = obj.iter().filter(|t| t.code == 46).take(MAX_COLUMNS).map(|t| finite(t.f64()).max(0.0)).collect();
    let auto_height = first(73).map(Tag::i64).unwrap_or(0) != 0;
    let manual = kind == MTextColumnType::Dynamic && !auto_height;
    // The column height: the entity's own 46 (defined height), else the object's 41 or first 46.
    let own_46 = own(tags).iter().find(|t| t.code == 46).map(|t| finite(t.f64()).max(0.0));
    let defined = [own_46, first(41).map(|t| finite(t.f64())), heights.first().copied()].into_iter().flatten().find(|h| *h > 0.0).unwrap_or(0.0);
    // A dynamic auto-height object stores 0 columns in 72: the count follows from the total width
    // (42 = n * 44 + (n - 1) * 45).
    let [width, gutter, total] = [44, 45, 42].map(|g| first(g).map_or(0.0, |t| finite(t.f64())));
    let count = match first(72).map(Tag::i64) {
        Some(n) if n > 0 => n,
        Some(_) if width + gutter > 0.0 && total > 0.0 => ((total + gutter) / (width + gutter)).round() as i64,
        _ => 1,
    };
    let c = MTextColumns {
        kind,
        count: u16::try_from(count.clamp(1, MAX_COLUMNS as i64)).unwrap_or(1),
        gutter: first(45).map(|t| finite(t.f64()).max(0.0)).unwrap_or(0.0),
        height: if manual { heights.iter().copied().fold(0.0, f64::max) } else { defined },
        auto_height,
        heights: if manual { heights } else { Vec::new() },
        flow_reversed: first(74).map(Tag::i64).unwrap_or(0) != 0,
    };
    Some((c, first(44).map(|t| finite(t.f64()).max(0.0)).unwrap_or(0.0)))
}

/// The column groups of an MTEXT with columns: the legacy groups 75/76/78/79/48/49/50 before
/// R2018, an embedded object (group 101) from R2018 on, which reads those as the entity's own.
pub(crate) fn column_tags(t: &MText, embedded: bool) -> Vec<Tag> {
    let Some(c) = &t.columns else { return Vec::new() };
    let manual = c.kind == MTextColumnType::Dynamic && !c.auto_height;
    let heights: Vec<f64> = if manual { c.heights.clone() } else { vec![c.height; usize::from(c.count.max(1))] };
    let count = if manual { heights.len().max(1) } else { usize::from(c.count.max(1)) };
    if embedded {
        let (sin, cos) = t.rotation.sin_cos();
        let total_width = t.width * count as f64 + c.gutter * (count as f64 - 1.0);
        // The column height (static, and dynamic with auto height) is the entity's group 46.
        let height = if manual { heights.iter().copied().fold(0.0, f64::max) } else { c.height };
        let mut v = vec![
            Tag::f(46, height),
            Tag::s(101, "Embedded Object"),
            Tag::i(70, 1),
            Tag::f(10, cos),
            Tag::f(20, sin),
            Tag::f(30, 0.0),
            Tag::f(11, t.insert.x),
            Tag::f(21, t.insert.y),
            Tag::f(31, t.insert.z),
            Tag::f(40, t.width),
            Tag::f(41, height),
            Tag::f(42, total_width),
            Tag::f(43, height),
            Tag::i(71, if c.kind == MTextColumnType::Dynamic { 2 } else { 1 }),
            Tag::i(72, count as i64),
            Tag::f(44, t.width),
            Tag::f(45, c.gutter),
            Tag::i(73, i64::from(c.auto_height)),
            Tag::i(74, i64::from(c.flow_reversed)),
        ];
        if manual {
            v.extend(heights.into_iter().map(|h| Tag::f(46, h)));
        }
        return v;
    }
    let mut v = vec![
        Tag::i(75, if c.kind == MTextColumnType::Dynamic { 2 } else { 1 }),
        Tag::i(76, count as i64),
        Tag::i(78, i64::from(c.flow_reversed)),
        Tag::i(79, i64::from(c.auto_height)),
        Tag::f(48, t.width),
        Tag::f(49, c.gutter),
    ];
    // No count for an empty list: a lone 50 = 0 would read back as one height of 0.
    if !heights.is_empty() {
        v.push(Tag::i(50, heights.len() as i64));
    }
    v.extend(heights.into_iter().map(|h| Tag::f(50, h)));
    v
}
