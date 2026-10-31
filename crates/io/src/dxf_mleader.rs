//! Multileaders in DXF (DXF Reference: MULTILEADER entity). The entity's own groups (style,
//! leader type, landing and dogleg switches, content type, block attribute values) come after
//! the `AcDbMLeader` marker; the geometry lives in nested sections that reuse the same group
//! codes: `300 CONTEXT_DATA{` … `301 }` holds the content (MText or block) and the plane, each
//! `302 LEADER{` … `303 }` inside it a landing point with its dogleg, and each
//! `304 LEADER_LINE{` … `305 }` inside that the vertices of one leader line from the arrowhead.
//!
//! The reader walks those sections with a level counter over the record's tags, so an
//! unterminated or unbalanced section ends with the record; counts of leaders, lines,
//! vertices and attribute values are capped.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cadcraft_doc::{Attrib, Drawing, Entity, EntityKind, Handle, Insert, MLeader, MLeaderStyle, MText};
use cadcraft_dxf::Tag;
use cadcraft_geom::Vec3;

/// Most leader roots (landings) read per multileader.
const MAX_ROOTS: usize = 256;
/// Most leader lines read per multileader, over all its roots.
const MAX_LINES: usize = 4096;
/// Most leader line vertices read per multileader.
const MAX_VERTICES: usize = 100_000;
/// Most block attribute values read per multileader.
const MAX_ATTRIBS: usize = 4096;

/// ByBlock as a 32-bit colour value (0xC1000000).
const BYBLOCK: i64 = -1_056_964_608;
/// No background colour (0xC8000000).
const NO_COLOR: i64 = -939_524_096;

/// Data of a multileader that needs the file's objects: its MLEADERSTYLE handle and the values
/// of its block attributes by ATTDEF handle (all upper case).
#[derive(Debug, Default)]
pub(crate) struct Pending {
    style: Option<String>,
    attribs: Vec<(String, String)>,
}

/// A point read group by group (x, then y and z).
#[derive(Clone, Copy, Default)]
struct P {
    x: Option<f64>,
    y: Option<f64>,
    z: Option<f64>,
}

impl P {
    /// Take group `code` when it is coordinate `base`, `base + 10` or `base + 20`.
    fn take(&mut self, base: i32, code: i32, v: f64) -> bool {
        let slot = match code - base {
            0 => &mut self.x,
            10 => &mut self.y,
            20 => &mut self.z,
            _ => return false,
        };
        *slot = Some(v);
        true
    }
    fn get(&self) -> Option<Vec3> {
        self.x.map(|x| Vec3::new(x, self.y.unwrap_or(0.0), self.z.unwrap_or(0.0)))
    }
}

#[derive(Default)]
struct Root {
    landing: P,
    dogleg: Option<f64>,
    lines: Vec<Vec<Vec3>>,
}

#[derive(Default)]
struct Parsed {
    // Entity level.
    style: Option<String>,
    leader_type: Option<i64>,
    dogleg_on: Option<bool>,
    dogleg: Option<f64>,
    arrow: Option<f64>,
    /// Arrowhead BLOCK_RECORD (342 on the entity, else 341 of a leader line).
    arrow_block: Option<String>,
    line_arrow_block: Option<String>,
    content: Option<i64>,
    text_style: Option<String>,
    block: Option<String>,
    block_scale: P,
    block_rotation: Option<f64>,
    attribs: Vec<(String, String)>,
    // CONTEXT_DATA.
    base: P,
    text_height: Option<f64>,
    arrow_ctx: Option<f64>,
    has_text: Option<bool>,
    text: Option<String>,
    text_style_ctx: Option<String>,
    text_location: P,
    text_direction: P,
    text_rotation: Option<f64>,
    text_width: Option<f64>,
    line_spacing: Option<f64>,
    line_spacing_style: Option<i64>,
    /// Text side (171: 1 left, 2 centre, 3 right).
    side: Option<i64>,
    /// How the text's left / right side attaches to the landing (174 / 175, or 173 / 95 on
    /// the entity): which line of the text the landing points at.
    left_attach: Option<i64>,
    right_attach: Option<i64>,
    has_block: Option<bool>,
    block_ctx: Option<String>,
    block_location: P,
    block_scale_ctx: P,
    block_rotation_ctx: Option<f64>,
    roots: Vec<Root>,
}

#[derive(Clone, Copy, PartialEq)]
enum Level {
    Entity,
    Context,
    Root,
    Line,
}

fn handle(t: &Tag) -> Option<String> {
    let h = t.str().trim().to_ascii_uppercase();
    (!h.is_empty() && h != "0").then_some(h)
}

fn finite(v: f64) -> Option<f64> {
    v.is_finite().then_some(v)
}

fn parse(tags: &[Tag]) -> Parsed {
    // The entity's own groups follow the AcDbMLeader marker (or, without one, the last marker).
    let start =
        tags.iter().position(|t| t.code == 100 && t.str() == "AcDbMLeader").or_else(|| tags.iter().rposition(|t| t.code == 100)).map_or(0, |i| i + 1);
    let mut p = Parsed::default();
    let mut lvl = Level::Entity;
    // Whether the current root / line is kept (under the caps) and the last vertex was.
    let (mut root_kept, mut line_kept, mut vertex_kept) = (false, false, false);
    let (mut lines, mut vertices) = (0usize, 0usize);
    let mut attrib: Option<String> = None;
    for t in tags.get(start..).unwrap_or(&[]) {
        let (c, f) = (t.code, t.f64());
        match lvl {
            Level::Entity => match c {
                300 if t.str().trim_start().starts_with("CONTEXT_DATA") => lvl = Level::Context,
                340 => p.style = handle(t),
                170 => p.leader_type = Some(t.i64()),
                291 => p.dogleg_on = Some(t.i64() != 0),
                41 => p.dogleg = finite(f),
                42 => p.arrow = finite(f),
                342 => p.arrow_block = handle(t),
                172 => p.content = Some(t.i64()),
                343 => p.text_style = handle(t),
                344 => p.block = handle(t),
                10 | 20 | 30 => {
                    p.block_scale.take(10, c, f);
                }
                43 => p.block_rotation = finite(f),
                173 => p.left_attach = p.left_attach.or(Some(t.i64())),
                95 => p.right_attach = p.right_attach.or(Some(t.i64())),
                // Block attribute values: ATTDEF handle (330), index, width, then the text (302).
                330 => attrib = handle(t),
                302 => {
                    if let Some(h) = attrib.take()
                        && p.attribs.len() < MAX_ATTRIBS
                    {
                        p.attribs.push((h, t.str()));
                    }
                }
                _ => {}
            },
            Level::Context => match c {
                301 => lvl = Level::Entity,
                302 if t.str().trim_start().starts_with("LEADER") => {
                    root_kept = p.roots.len() < MAX_ROOTS;
                    if root_kept {
                        p.roots.push(Root::default());
                    }
                    lvl = Level::Root;
                }
                10 | 20 | 30 => {
                    p.base.take(10, c, f);
                }
                41 => p.text_height = finite(f),
                140 => p.arrow_ctx = finite(f),
                290 => p.has_text = Some(t.i64() != 0),
                304 => p.text = Some(t.str()),
                340 => p.text_style_ctx = handle(t),
                12 | 22 | 32 => {
                    p.text_location.take(12, c, f);
                }
                13 | 23 | 33 => {
                    p.text_direction.take(13, c, f);
                }
                42 => p.text_rotation = finite(f),
                43 => p.text_width = finite(f),
                45 => p.line_spacing = finite(f),
                170 => p.line_spacing_style = Some(t.i64()),
                171 => p.side = Some(t.i64()),
                174 => p.left_attach = Some(t.i64()),
                175 => p.right_attach = Some(t.i64()),
                296 => p.has_block = Some(t.i64() != 0),
                341 => p.block_ctx = handle(t),
                15 | 25 | 35 => {
                    p.block_location.take(15, c, f);
                }
                16 | 26 | 36 => {
                    p.block_scale_ctx.take(16, c, f);
                }
                46 => p.block_rotation_ctx = finite(f),
                _ => {}
            },
            Level::Root => match c {
                303 => lvl = Level::Context,
                301 => lvl = Level::Entity,
                304 if t.str().trim_start().starts_with("LEADER_LINE") => {
                    line_kept = root_kept && lines < MAX_LINES;
                    if line_kept && let Some(r) = p.roots.last_mut() {
                        r.lines.push(Vec::new());
                        lines += 1;
                    }
                    vertex_kept = false;
                    lvl = Level::Line;
                }
                10 | 20 | 30 if root_kept => {
                    if let Some(r) = p.roots.last_mut() {
                        r.landing.take(10, c, f);
                    }
                }
                40 if root_kept => {
                    if let Some(r) = p.roots.last_mut() {
                        r.dogleg = finite(f);
                    }
                }
                _ => {}
            },
            Level::Line => match c {
                305 => lvl = Level::Root,
                303 => lvl = Level::Context,
                301 => lvl = Level::Entity,
                10 => {
                    vertex_kept = line_kept && vertices < MAX_VERTICES;
                    if vertex_kept && let Some(l) = p.roots.last_mut().and_then(|r| r.lines.last_mut()) {
                        l.push(Vec3::new(f, 0.0, 0.0));
                        vertices += 1;
                    }
                }
                20 | 30 if vertex_kept => {
                    if let Some(v) = p.roots.last_mut().and_then(|r| r.lines.last_mut()).and_then(|l| l.last_mut()) {
                        if c == 20 {
                            v.y = f;
                        } else {
                            v.z = f;
                        }
                    }
                }
                341 => p.line_arrow_block = p.line_arrow_block.take().or_else(|| handle(t)),
                _ => {}
            },
        }
    }
    p
}

/// A MULTILEADER record as a multileader. Names that come from handles (style, text style,
/// block) are filled in by [`resolve`] and [`apply`].
pub(crate) fn read(tags: &[Tag]) -> MLeader {
    let p = parse(tags);
    let def = MLeaderStyle::default();
    let pos = |v: Option<f64>| v.filter(|x| *x > 0.0);
    // Leader lines: those of the first root run to its landing; a further root's lines get
    // their own landing as a last vertex (one landing per multileader here).
    let mut leaders: Vec<Vec<Vec3>> = Vec::new();
    for (i, r) in p.roots.iter().enumerate() {
        for l in r.lines.iter().filter(|l| !l.is_empty()) {
            let mut l = l.clone();
            if i > 0
                && let Some(land) = r.landing.get()
            {
                l.push(land);
            }
            leaders.push(l);
        }
    }
    // Leader type None: the leader lines and the landing's length are kept (written back as
    // they were, to come back with Straight or Spline) but not drawn.
    let none = p.leader_type == Some(0);
    let text_location = p.text_location.get();
    let block_location = p.block_location.get();
    let landing = p.roots.iter().find_map(|r| r.landing.get()).or(p.base.get()).or(text_location).or(block_location).unwrap_or(Vec3::ZERO);
    let dogleg = if leaders.is_empty() || p.dogleg_on == Some(false) {
        0.0
    } else {
        p.roots.iter().find_map(|r| r.dogleg).or(p.dogleg).filter(|d| *d >= 0.0).unwrap_or(0.0)
    };
    let has_block = p.has_block.unwrap_or(p.content == Some(1));
    let has_text = !has_block && p.has_text.unwrap_or(p.content == Some(2));
    let text = has_text.then(|| {
        let rotation = match p.text_direction.get() {
            Some(d) if d.x != 0.0 || d.y != 0.0 => d.y.atan2(d.x),
            _ => p.text_rotation.unwrap_or(0.0),
        };
        MText {
            frame: false,
            formula: None,
            insert: text_location.unwrap_or(landing),
            height: pos(p.text_height).unwrap_or(def.text_height),
            width: p.text_width.unwrap_or(0.0).max(0.0),
            attach: attach_point(&p, text_location.unwrap_or(landing).x < landing.x),
            rotation,
            style: def.text_style.clone(),
            contents: p.text.clone().unwrap_or_default(),
            line_spacing: pos(p.line_spacing).unwrap_or(1.0),
            line_spacing_exact: p.line_spacing_style == Some(2),
            columns: None,
        }
    });
    let block = has_block.then(|| {
        let scale = p.block_scale_ctx.get().or(p.block_scale.get()).unwrap_or(Vec3::new(1.0, 1.0, 1.0));
        let nz = |v: f64| if v != 0.0 { v } else { 1.0 };
        Insert {
            block: String::new(),
            insert: block_location.unwrap_or(landing),
            scale: Vec3::new(nz(scale.x), nz(scale.y), nz(scale.z)),
            rotation: p.block_rotation_ctx.or(p.block_rotation).unwrap_or(0.0),
            attribs: Vec::new(),
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        }
    });
    let no_leader = none && !leaders.is_empty();
    MLeader {
        leaders,
        landing,
        dogleg,
        text,
        style: def.name,
        arrow_size: pos(p.arrow_ctx).or(pos(p.arrow)).unwrap_or(def.arrow_size),
        arrow: String::new(),
        spline: p.leader_type == Some(2),
        no_leader,
        block,
    }
}

/// The MText attachment point (1..=9) of the text: its column from the text side (171), its
/// row from how the side facing the landing attaches (top line, middle or bottom line).
fn attach_point(p: &Parsed, text_left_of_landing: bool) -> u8 {
    let col = p.side.unwrap_or(1).clamp(1, 3);
    let side = if text_left_of_landing { p.right_attach.or(p.left_attach) } else { p.left_attach.or(p.right_attach) };
    let row = match side.unwrap_or(0) {
        2 | 9 => 1,
        3..=5 => 2,
        _ => 0,
    };
    u8::try_from(row * 3 + col).unwrap_or(1)
}

/// Text style and block names from the file's STYLE and BLOCK_RECORD handles (upper case →
/// name); block content whose block is unknown is dropped. Returns what needs the objects.
pub(crate) fn resolve(tags: &[Tag], m: &mut MLeader, styles: &HashMap<String, String>, brs: &HashMap<String, String>) -> Pending {
    let p = parse(tags);
    if let Some(t) = &mut m.text
        && let Some(n) = p.text_style_ctx.iter().chain(&p.text_style).find_map(|h| styles.get(h)).filter(|n| !n.is_empty())
    {
        t.style = n.clone();
    }
    // The arrowhead: a BLOCK_RECORD (`_DotSmall`, or a user block); none means the style's.
    if let Some(n) = p.arrow_block.iter().chain(&p.line_arrow_block).find_map(|h| brs.get(h)).filter(|n| !n.trim().is_empty()) {
        m.arrow = n.clone();
    }
    if m.block.is_some() {
        match p.block_ctx.iter().chain(&p.block).find_map(|h| brs.get(h)).filter(|n| !n.is_empty()) {
            Some(n) => {
                if let Some(b) = &mut m.block {
                    b.block = n.clone();
                }
            }
            None => m.block = None,
        }
    }
    Pending { style: p.style, attribs: p.attribs }
}

/// Multileader style names (by MLEADERSTYLE handle, from the ACAD_MLEADERSTYLE dictionary) and
/// block attribute values (by the ATTDEF of the content block), for multileaders in model
/// space, the layouts and block definitions.
pub(crate) fn apply(d: &mut Drawing, fixes: &[(Handle, Pending)], names: &HashMap<String, String>) {
    let fixes: Vec<&(Handle, Pending)> = fixes.iter().filter(|(_, p)| p.style.is_some() || !p.attribs.is_empty()).collect();
    if fixes.is_empty() {
        return;
    }
    // Block definitions holding the multileaders that are in no space.
    let wanted: HashSet<Handle> = fixes.iter().map(|(h, _)| *h).filter(|h| d.entity(*h).is_none()).collect();
    let mut owner: HashMap<Handle, String> = HashMap::new();
    if !wanted.is_empty() {
        for (name, b) in &d.blocks {
            for e in b.entities.iter().filter(|e| wanted.contains(&e.handle)) {
                owner.entry(e.handle).or_insert_with(|| name.clone());
            }
        }
    }
    for (h, pend) in fixes {
        let block_of = owner.get(h);
        let current = match block_of {
            Some(b) => d.block(b).and_then(|b| b.entities.get(*h)),
            None => d.entity(*h),
        };
        let Some(EntityKind::MLeader(m)) = current.map(|e| &e.kind) else { continue };
        let mut m = m.clone();
        if let Some(s) = pend.style.as_ref().and_then(|s| names.get(s)).filter(|n| !n.is_empty()) {
            m.style = s.clone();
        }
        if !pend.attribs.is_empty()
            && let Some(ins) = &m.block
        {
            let a = attribs(d, ins, &pend.attribs);
            if let Some(ins) = &mut m.block {
                ins.attribs = a;
            }
        }
        let set = |e: &mut Entity| e.kind = EntityKind::MLeader(m);
        match block_of {
            Some(b) => {
                if let Some(b) = d.blocks.get_mut(b) {
                    Arc::make_mut(b).entities.modify(*h, set);
                }
            }
            None => {
                let _ = d.modify_entity(*h, set);
            }
        }
    }
}

/// The attributes of a multileader's block content: each value placed like its ATTDEF.
fn attribs(d: &Drawing, ins: &Insert, values: &[(String, String)]) -> Vec<Attrib> {
    let Some(blk) = d.block(&ins.block) else { return Vec::new() };
    let m = ins.transform(blk.base.xy());
    let mut out = Vec::new();
    for (h, value) in values {
        let def = Handle::parse_hex(h).and_then(|h| blk.entities.get(h));
        let Some(EntityKind::AttDef(ad)) = def.map(|e| &e.kind) else { continue };
        let mut k = EntityKind::Text(ad.text.clone());
        k.transform(&m);
        let EntityKind::Text(mut text) = k else { continue };
        text.value = value.clone();
        out.push(Attrib {
            tag: ad.tag.clone(),
            text,
            invisible: ad.invisible,
            constant: false,
            prompt: String::new(),
            props: Default::default(),
            multiline: None,
            modes: Default::default(),
        });
    }
    out
}

/// What a MULTILEADER record points at in the file being written.
pub(crate) struct Refs {
    /// MLEADERSTYLE handle.
    pub style: String,
    /// STYLE record handle of the text.
    pub text_style: Option<String>,
    /// BLOCK_RECORD handle of the content block.
    pub block: Option<String>,
    /// BLOCK_RECORD handle of its own arrowhead.
    pub arrow: Option<String>,
}

/// The groups of a MULTILEADER record after its `AcDbMLeader` marker.
pub(crate) fn entity_tags(d: &Drawing, m: &MLeader, r: &Refs) -> Vec<Tag> {
    let mut t: Vec<Tag> = Vec::new();
    let s = |t: &mut Vec<Tag>, c: i32, v: &str| t.push(Tag::s(c, v));
    let f = |t: &mut Vec<Tag>, c: i32, v: f64| t.push(Tag::f(c, v));
    let i = |t: &mut Vec<Tag>, c: i32, v: i64| t.push(Tag::i(c, v));
    let p = |t: &mut Vec<Tag>, c: i32, v: Vec3| {
        t.push(Tag::f(c, v.x));
        t.push(Tag::f(c + 10, v.y));
        t.push(Tag::f(c + 20, v.z));
    };
    let style = d.mleader_styles.iter().find(|x| x.name.eq_ignore_ascii_case(&m.style)).cloned().unwrap_or_default();
    // Block content wins over text when an entity has both (DXF has one content type).
    let block = m.block.as_ref().zip(r.block.as_ref());
    let text = if block.is_some() { None } else { m.text.as_ref() };
    let content_pt = block.map(|(b, _)| b.insert).or(text.map(|x| x.insert));
    let dir = if content_pt.is_some_and(|c| c.x < m.landing.x) { -1.0 } else { 1.0 };
    let attach = text.map_or(1, |x| x.attach.clamp(1, 9));
    // The text's row as its attachment to the landing (top of top line, middle of text,
    // bottom of bottom line) and its column as the text side and alignment.
    let vert = match attach {
        1..=3 => 0,
        4..=6 => 2,
        _ => 4,
    };
    let align = i64::from((attach - 1) % 3);
    i(&mut t, 270, 2);
    s(&mut t, 300, "CONTEXT_DATA{");
    f(&mut t, 40, 1.0);
    p(&mut t, 10, m.landing + Vec3::new(m.dogleg * dir, 0.0, 0.0));
    f(&mut t, 41, text.map_or(style.text_height, |x| x.height));
    f(&mut t, 140, m.arrow_size);
    f(&mut t, 145, style.landing_gap);
    i(&mut t, 174, vert);
    i(&mut t, 175, vert);
    i(&mut t, 176, align);
    i(&mut t, 177, 1);
    i(&mut t, 290, i64::from(text.is_some()));
    if let Some(x) = text {
        s(&mut t, 304, &x.contents);
        p(&mut t, 11, Vec3::Z);
        if let Some(h) = &r.text_style {
            s(&mut t, 340, h);
        }
        p(&mut t, 12, x.insert);
        p(&mut t, 13, Vec3::new(x.rotation.cos(), x.rotation.sin(), 0.0));
        f(&mut t, 42, x.rotation);
        f(&mut t, 43, x.width);
        f(&mut t, 44, 0.0);
        f(&mut t, 45, x.line_spacing);
        i(&mut t, 170, if x.line_spacing_exact { 2 } else { 1 });
        i(&mut t, 90, BYBLOCK);
        i(&mut t, 171, align + 1);
        i(&mut t, 172, 1);
        i(&mut t, 91, NO_COLOR);
        f(&mut t, 141, 1.5);
        i(&mut t, 92, 0);
        i(&mut t, 291, 0);
        i(&mut t, 292, 0);
        i(&mut t, 173, 0);
        i(&mut t, 293, 0);
        f(&mut t, 142, 0.0);
        f(&mut t, 143, 0.0);
        i(&mut t, 294, 0);
        i(&mut t, 295, 0);
    }
    i(&mut t, 296, i64::from(block.is_some()));
    if let Some((b, h)) = block {
        s(&mut t, 341, h);
        p(&mut t, 14, Vec3::Z);
        p(&mut t, 15, b.insert);
        p(&mut t, 16, b.scale);
        f(&mut t, 46, b.rotation);
        i(&mut t, 93, BYBLOCK);
        // Block → world, row by row.
        let base = d.block(&b.block).map_or(Vec3::ZERO, |x| x.base);
        let mx = b.transform(base.xy());
        let z = b.insert.z - b.scale.z * base.z;
        for v in [mx.a, mx.c, 0.0, mx.e, mx.b, mx.d, 0.0, mx.f, 0.0, 0.0, b.scale.z, z, 0.0, 0.0, 0.0, 1.0] {
            f(&mut t, 47, v);
        }
    }
    p(&mut t, 110, Vec3::new(0.0, 0.0, m.landing.z));
    p(&mut t, 111, Vec3::new(1.0, 0.0, 0.0));
    p(&mut t, 112, Vec3::new(0.0, 1.0, 0.0));
    i(&mut t, 297, 0);
    if !m.leaders.is_empty() {
        s(&mut t, 302, "LEADER{");
        i(&mut t, 290, 1);
        i(&mut t, 291, 1);
        p(&mut t, 10, m.landing);
        p(&mut t, 11, Vec3::new(dir, 0.0, 0.0));
        i(&mut t, 90, 0);
        f(&mut t, 40, m.dogleg);
        for (n, l) in m.leaders.iter().enumerate() {
            s(&mut t, 304, "LEADER_LINE{");
            for v in l {
                p(&mut t, 10, *v);
            }
            i(&mut t, 91, i64::try_from(n).unwrap_or(i64::MAX));
            s(&mut t, 305, "}");
        }
        i(&mut t, 271, 0);
        s(&mut t, 303, "}");
    }
    i(&mut t, 272, 9);
    i(&mut t, 273, 9);
    s(&mut t, 301, "}");
    s(&mut t, 340, &r.style);
    // Overridden properties: leader type, landing and dogleg, arrowhead size, content type,
    // text style and height, block content (the entity holds its own values).
    let arrow_flag = if r.arrow.is_some() { 0x100 } else { 0 };
    i(&mut t, 90, 0x1 | 0x10 | 0x40 | 0x80 | arrow_flag | 0x200 | 0x400 | 0x800 | 0x1_0000 | 0x8_0000);
    i(
        &mut t,
        170,
        if m.leaderless() {
            0
        } else if m.spline {
            2
        } else {
            1
        },
    );
    i(&mut t, 91, BYBLOCK);
    i(&mut t, 171, -2);
    i(&mut t, 290, 1);
    i(&mut t, 291, i64::from(m.dogleg > 0.0));
    f(&mut t, 41, m.dogleg);
    if let Some(a) = &r.arrow {
        s(&mut t, 342, a);
    }
    f(&mut t, 42, m.arrow_size);
    i(
        &mut t,
        172,
        if block.is_some() {
            1
        } else if text.is_some() {
            2
        } else {
            0
        },
    );
    if let Some(h) = &r.text_style {
        s(&mut t, 343, h);
    }
    i(&mut t, 173, vert);
    i(&mut t, 95, vert);
    i(&mut t, 174, 1);
    i(&mut t, 175, align);
    i(&mut t, 92, BYBLOCK);
    i(&mut t, 292, 0);
    if let Some((b, h)) = block {
        s(&mut t, 344, h);
        i(&mut t, 93, BYBLOCK);
        p(&mut t, 10, b.scale);
        f(&mut t, 43, b.rotation);
        i(&mut t, 176, 1);
        // Attribute values by their ATTDEF in the block.
        let defs: Vec<(Handle, &str)> = d
            .block(&b.block)
            .map(|blk| {
                blk.entities
                    .iter()
                    .filter_map(|e| if let EntityKind::AttDef(ad) = &e.kind { Some((e.handle, ad.tag.as_str())) } else { None })
                    .collect()
            })
            .unwrap_or_default();
        let mut n = 0i64;
        for a in &b.attribs {
            let Some((h, _)) = defs.iter().find(|(_, tag)| tag.eq_ignore_ascii_case(&a.tag)) else { continue };
            s(&mut t, 330, &h.hex());
            i(&mut t, 177, n);
            f(&mut t, 44, 0.0);
            s(&mut t, 302, &a.text.value);
            n += 1;
        }
    }
    i(&mut t, 293, 0);
    i(&mut t, 294, 0);
    i(&mut t, 178, 0);
    i(&mut t, 179, 1);
    f(&mut t, 45, 1.0);
    i(&mut t, 271, 0);
    i(&mut t, 272, 9);
    i(&mut t, 273, 9);
    t
}
