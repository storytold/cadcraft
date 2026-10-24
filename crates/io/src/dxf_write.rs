//! Drawing → DXF (AC1015 / R2000 structure, readable by every DXF consumer since 2000). Later
//! versions keep that structure (the groups they add are optional, DXF Reference) and differ in
//! `$ACADVER`, from 2004 in the newer DIMSTYLE groups, and from 2007 in UTF-8 text (the caller
//! picks the text encoding) and MULTILEADER entities (older files get LEADER + MTEXT).

use std::collections::HashMap;

use cadcraft_color::Color;
use cadcraft_doc::*;
use cadcraft_dxf::{Tag, Version};
use cadcraft_geom::{Bounds2, Vec2, Vec3};

use crate::dxf_ext::{self, DimVal, K};

struct W {
    t: Vec<Tag>,
    next: u64,
    /// Upper-case plot style name → its `ACDBPLACEHOLDER` handle (group 390 of layers and objects).
    pstyles: HashMap<String, String>,
    /// Upper-case layer name → its LAYER table record handle (group 341 of viewports).
    layers: HashMap<String, String>,
    /// Persistent reactors of boundary objects: each source object of an associative hatch →
    /// the hatches (groups `{ACAD_REACTORS 330 …}`), from [`hatch_reactors`].
    reactors: HashMap<Handle, Vec<String>>,
}

impl W {
    fn s(&mut self, c: i32, v: impl Into<String>) {
        self.t.push(Tag::s(c, v));
    }
    fn f(&mut self, c: i32, v: f64) {
        self.t.push(Tag::f(c, v));
    }
    fn i(&mut self, c: i32, v: i64) {
        self.t.push(Tag::i(c, v));
    }
    fn p(&mut self, c: i32, p: Vec3) {
        self.f(c, p.x);
        self.f(c + 10, p.y);
        self.f(c + 20, p.z);
    }
    fn p2(&mut self, c: i32, p: Vec2) {
        self.f(c, p.x);
        self.f(c + 10, p.y);
    }
    fn h(&mut self) -> String {
        let h = format!("{:X}", self.next);
        self.next = self.next.saturating_add(1);
        h
    }
    /// A `102 {NAME` … `102 }` group of handles.
    fn group(&mut self, name: &str, code: i32, handles: &[&str]) {
        self.s(102, format!("{{{name}"));
        for h in handles {
            self.s(code, *h);
        }
        self.s(102, "}");
    }
    /// A dimension variable as a group (DIMSTYLE records) or as an xdata value (DSTYLE).
    fn dimval(&mut self, code: i32, v: DimVal, xdata: bool) {
        match v {
            DimVal::Real(x) => self.f(if xdata { 1040 } else { code }, x),
            DimVal::Int(i) => self.i(if xdata { 1070 } else { code }, i),
            DimVal::Str(t) => {
                if xdata || !t.is_empty() {
                    self.s(if xdata { 1000 } else { code }, t);
                }
            }
            DimVal::Handle(h) => {
                if xdata {
                    self.s(1005, h);
                } else if h != "0" {
                    self.s(code, h);
                }
            }
        }
    }
    fn xdata(&mut self, tags: Vec<Tag>) {
        self.t.extend(tags);
    }
}

/// The `DIMASSOC` object of an associative dimension (stored in the dimension's extension
/// dictionary under `ACAD_DIMASSOC`).
struct AssocObj {
    xdict: String,
    handle: String,
    /// Associativity flag (bit per point reference).
    flags: i64,
    refs: Vec<OsnapRef>,
}

/// An `AcDbOsnapPointRef` of a DIMASSOC object.
struct OsnapRef {
    osnap: i64,
    main: Handle,
    /// The 1-based segment of a polyline the point is an end of (group 91, with subentity type
    /// 73 = 2, edge); 0 for an object made of one curve.
    segment: i64,
    param: f64,
    point: Vec3,
    other: Option<Handle>,
}

/// Handles and names the entity writer needs from the rest of the file.
#[derive(Default)]
struct Ctx {
    /// Multiline styles: MLINESTYLE handles and their dictionary.
    mlines: crate::dxf_mline::Plan,
    /// First viewport of each layout that has no paper-space viewport (id 1) → the layout's index;
    /// [`paper_view`] writes that layout's paper-space viewport ahead of it.
    paper_views: HashMap<Handle, usize>,
    /// Dimension → anonymous `*D` block name.
    dim_blocks: HashMap<Handle, String>,
    /// Upper-case text style name → STYLE record handle.
    styles: HashMap<String, String>,
    /// Upper-case arrow name (as stored in styles/overrides) → BLOCK_RECORD handle.
    arrows: HashMap<String, String>,
    /// Upper-case linetype name → LTYPE record handle (DIMLTYPE, DIMLTEX1, DIMLTEX2).
    ltypes: HashMap<String, String>,
    /// Associative dimension → its DIMASSOC object.
    assoc: HashMap<Handle, AssocObj>,
    /// Table → (anonymous `*T` block name, BLOCK_RECORD handle).
    tables: HashMap<Handle, (String, String)>,
    /// Table style name → TABLESTYLE handle (the first is the fallback).
    table_styles: Vec<(String, String)>,
    /// Multileader style name → MLEADERSTYLE handle (the first is the fallback).
    mleader_styles: Vec<(String, String)>,
    /// Upper-case block name → BLOCK_RECORD handle (multileader block content).
    block_records: HashMap<String, String>,
    /// Raster images' IMAGEDEF, IMAGEDEF_REACTOR and dictionary handles.
    images: crate::dxf_image::Plan,
    /// The version the file ends up in (the DWG version for a DWG save): it decides which
    /// entity types exist (MULTILEADER from 2007).
    target: Version,
}

impl Ctx {
    /// Whether multileaders are written as MULTILEADER (AutoCAD 2007 / AC1021 and later);
    /// older files get LEADER + MTEXT/INSERT.
    fn native_mleader(&self) -> bool {
        self.target >= Version::R2007
    }
    fn style(&self, name: &str) -> Option<String> {
        self.styles.get(&name.to_ascii_uppercase()).cloned()
    }
    fn arrow(&self, name: &str) -> Option<String> {
        self.arrows.get(&name.trim().to_ascii_uppercase()).cloned()
    }
    fn table_style(&self, name: &str) -> String {
        Self::named(&self.table_styles, name)
    }
    fn mleader_style(&self, name: &str) -> String {
        Self::named(&self.mleader_styles, name)
    }
    /// The handle of the style `name` in `styles`, or of the first one.
    fn named(styles: &[(String, String)], name: &str) -> String {
        styles.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).or_else(|| styles.first()).map(|(_, h)| h.clone()).unwrap_or_else(|| "0".into())
    }
    /// A dimension variable encoded with this file's handles; integers outside the 16-bit
    /// range of their groups are dropped.
    fn encode(&self, kind: K, v: &serde_json::Value) -> Option<DimVal> {
        let ltype = |n: &str| self.ltypes.get(&n.trim().to_ascii_uppercase()).cloned();
        dxf_ext::encode(kind, v, &|n| self.style(n), &|n| self.arrow(n), &ltype)
            .filter(|x| !matches!(x, DimVal::Int(i) if i16::try_from(*i).is_err()))
    }
}

/// The persistent reactors of boundary objects: each source object of an associative hatch in
/// the same space or block → the hatches it bounds (AutoCAD links the two both ways; without the
/// reactor the hatch no longer follows its boundary there).
fn hatch_reactors(d: &Drawing) -> HashMap<Handle, Vec<String>> {
    let mut out: HashMap<Handle, Vec<String>> = HashMap::new();
    let stores = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).chain(d.blocks.values().map(|b| &b.entities));
    for st in stores {
        for e in st.iter() {
            let EntityKind::Hatch(h) = &e.kind else { continue };
            if !h.associative {
                continue;
            }
            for s in h.loops.iter().flat_map(|l| &l.sources) {
                if *s != e.handle && st.contains(*s) {
                    let r = out.entry(*s).or_default();
                    let own = e.handle.hex();
                    if !r.contains(&own) {
                        r.push(own);
                    }
                }
            }
        }
    }
    out
}

/// `AcadAnnotative` xdata marking an annotative style.
fn annotative_xdata(w: &mut W) {
    w.s(1001, "AcadAnnotative");
    w.s(1000, "AnnotativeData");
    w.s(1002, "{");
    w.i(1070, 1);
    w.i(1070, 1);
    w.s(1002, "}");
}

/// Override (`ACAD` DSTYLE) and associativity (`CADCRAFT` ASSOC) xdata of a dimension.
fn dim_xdata(w: &mut W, dm: &Dimension, cx: &Ctx) {
    let mut ov = W { t: Vec::new(), next: 0, pstyles: HashMap::new(), layers: HashMap::new(), reactors: HashMap::new() };
    let mut sah = false;
    for (k, v) in &dm.overrides {
        let Some(field) = DimStyle::field_name(k) else { continue };
        let Some((code, kind)) = dxf_ext::dim_code(field) else { continue };
        let Some(val) = dxf_ext::canonical(field, v) else { continue };
        let Some(enc) = cx.encode(kind, &val) else { continue };
        sah |= matches!(code, 343 | 344);
        ov.i(1070, i64::from(code));
        ov.dimval(code, enc, true);
    }
    if sah {
        ov.i(1070, i64::from(dxf_ext::DIMSAH));
        ov.i(1070, 1);
    }
    if !ov.t.is_empty() {
        w.s(1001, "ACAD");
        w.s(1000, "DSTYLE");
        w.s(1002, "{");
        w.xdata(ov.t);
        w.s(1002, "}");
    }
    let mut ours = Vec::new();
    if matches!(dm.kind, DimKind::ArcLength) {
        ours.extend(dxf_ext::arclen_xdata(dm.p15));
    }
    if !dm.assoc.is_empty() {
        ours.extend(dxf_ext::assoc_xdata(&dm.assoc));
    }
    ours.extend(dxf_ext::marks_xdata(&dm.marks));
    if !ours.is_empty() {
        w.s(1001, dxf_ext::APP);
        w.xdata(ours);
    }
}

/// Group 52 of a linear or aligned dimension with oblique extension lines: their angle less the
/// dimension line's (`base`), in degrees.
fn oblique_52(w: &mut W, dm: &Dimension, base: f64) {
    if let Some(a) = dm.marks.oblique.filter(|a| a.is_finite()) {
        w.f(52, cadcraft_geom::norm_angle(a - base).to_degrees());
    }
}

/// Header variables we write, with their group codes (DXF Reference, HEADER section).
/// `P2`/`P3` = 2D/3D point.
const HEADER_VARS: &[(&str, i32)] = &[
    ("INSBASE", -3),
    ("LIMMIN", -2),
    ("LIMMAX", -2),
    ("SNAPUNIT", -2),
    ("GRIDUNIT", -2),
    ("ORTHOMODE", 70),
    ("FILLMODE", 70),
    ("MIRRTEXT", 70),
    ("LTSCALE", 40),
    ("ATTMODE", 70),
    ("TEXTSIZE", 40),
    ("TRACEWID", 40),
    ("CMLSTYLE", 2),
    ("CMLJUST", 70),
    ("CMLSCALE", 40),
    ("TEXTSTYLE", 7),
    ("CLAYER", 8),
    ("CELTYPE", 6),
    ("CECOLOR", 62),
    ("CELTSCALE", 40),
    ("DIMSCALE", 40),
    ("DIMSTYLE", 2),
    ("LUNITS", 70),
    ("LUPREC", 70),
    ("AUNITS", 70),
    ("AUPREC", 70),
    ("ELEVATION", 40),
    ("THICKNESS", 40),
    ("FILLETRAD", 40),
    ("CHAMFERA", 40),
    ("CHAMFERB", 40),
    ("ANGBASE", 50),
    ("ANGDIR", 70),
    ("PDMODE", 70),
    ("PDSIZE", 40),
    ("PLINEWID", 40),
    ("CELWEIGHT", 370),
    ("LWDISPLAY", 290),
    ("INSUNITS", 70),
    ("MEASUREMENT", 70),
    ("PSLTSCALE", 70),
];

/// The current dimension style's variables as header variables (`$DIMTXT`, `$DIMDEC`…), after
/// `$DIMSTYLE`: AutoCAD draws new and updated dimensions with the header's DIM* values, which
/// otherwise stay its defaults (0.18 text, 4 decimals) whatever the current style says (#1051).
/// `$DIMSCALE` is a header variable of its own.
fn dim_header_vars(w: &mut W, d: &Drawing) {
    let name = d.header.str("DIMSTYLE", "Standard");
    let Some(style) = d.dim_styles.iter().find(|s| s.name.eq_ignore_ascii_case(&name)) else { return };
    let v = serde_json::to_value(style).unwrap_or_default();
    let none = |_: &str| None;
    for (var, field) in cadcraft_doc::DIMVARS {
        if *var == "DIMSCALE" {
            continue;
        }
        let (Some(value), Some((_, _, kind))) = (v.get(*field), dxf_ext::DIM_CODES.iter().find(|(f, _, _)| f == field)) else { continue };
        match kind {
            // Names, not handles, in the header.
            K::TextStyle | K::Block => {
                let Some(n) = value.as_str() else { continue };
                if *kind == K::TextStyle && n.trim().is_empty() {
                    continue;
                }
                w.s(9, format!("${var}"));
                w.s(if *kind == K::TextStyle { 7 } else { 1 }, n);
            }
            K::Linetype => {}
            _ => match dxf_ext::encode(*kind, value, &none, &none, &none) {
                Some(DimVal::Real(x)) => {
                    w.s(9, format!("${var}"));
                    w.f(40, x);
                }
                Some(DimVal::Int(i)) if i16::try_from(i).is_ok() => {
                    w.s(9, format!("${var}"));
                    w.i(70, i);
                }
                Some(DimVal::Str(t)) => {
                    w.s(9, format!("${var}"));
                    w.s(1, t);
                }
                _ => {}
            },
        }
    }
}

fn header_vars(w: &mut W, d: &Drawing) {
    for (name, code) in HEADER_VARS {
        let Some(v) = d.header.get(name) else { continue };
        match (*code, v) {
            (-2 | -3, HVal::Point(p)) => {
                let p = match *name {
                    "SNAPUNIT" => d.header.snap_unit().to3(0.0),
                    "GRIDUNIT" => d.header.grid_unit().to3(0.0),
                    _ => *p,
                };
                w.s(9, format!("${name}"));
                w.f(10, p.x);
                w.f(20, p.y);
                if *code == -3 {
                    w.f(30, p.z);
                }
            }
            (-2 | -3, _) => {}
            (c @ (40 | 50), v) => {
                if let Some(f) = v.as_f64() {
                    // ANGBASE is held in radians; `$ANGBASE` is in degrees.
                    let f = if *name == "ANGBASE" { f.to_degrees() } else { f };
                    w.s(9, format!("${name}"));
                    w.f(c, f);
                }
            }
            (c @ (62 | 70 | 290 | 370), v) => {
                if let Some(i) = v.as_i64() {
                    w.s(9, format!("${name}"));
                    w.i(c, i);
                }
            }
            (c, v) => {
                if let Some(t) = v.as_str() {
                    w.s(9, format!("${name}"));
                    w.s(c, t);
                }
            }
        }
    }
    dim_header_vars(w, d);
}

/// An entity's group 440 (DXF Reference): ByBlock is `0x01000000`, a fixed transparency its alpha
/// with the "by alpha" flag `0x02000000` (the alpha mapping layers use). ByLayer writes nothing.
fn transparency_440(t: Transparency) -> Option<i64> {
    match t {
        Transparency::ByLayer => None,
        Transparency::ByBlock => Some(0x0100_0000),
        Transparency::Percent(p) => Some(i64::from(0x0200_0000 | ((100 - u32::from(p.min(90))) * 255 / 100))),
    }
}

fn common(w: &mut W, e: &Entity, owner: &str, paper: bool, subclass: &str) {
    common_x(w, e, owner, paper, subclass, None);
}

/// Common entity groups; `assoc` adds the reactor and extension dictionary of a DIMASSOC.
fn common_x(w: &mut W, e: &Entity, owner: &str, paper: bool, subclass: &str, assoc: Option<&AssocObj>) {
    w.s(5, e.handle.hex());
    let mut reactors: Vec<String> = assoc.map(|a| a.handle.clone()).into_iter().collect();
    reactors.extend(w.reactors.get(&e.handle).into_iter().flatten().cloned());
    if !reactors.is_empty() {
        let rs: Vec<&str> = reactors.iter().map(String::as_str).collect();
        w.group("ACAD_REACTORS", 330, &rs);
    }
    if let Some(a) = assoc {
        w.group("ACAD_XDICTIONARY", 360, &[&a.xdict]);
    }
    w.s(330, owner);
    w.s(100, "AcDbEntity");
    if paper {
        w.i(67, 1);
    }
    layer_and_props(w, &e.common, None);
    if (e.common.ltscale - 1.0).abs() > 1e-12 {
        w.f(48, e.common.ltscale);
    }
    if !e.common.visible {
        w.i(60, 1);
    }
    if let Some(v) = transparency_440(e.common.transparency) {
        w.i(440, v);
    }
    // A named plot style (not ByLayer/ByBlock): its placeholder object.
    if let Some(h) = w.pstyles.get(&e.common.plot_style.trim().to_ascii_uppercase()).cloned() {
        w.s(390, h);
    }
    if !subclass.is_empty() {
        w.s(100, subclass);
    }
}

/// Layer, linetype, colour and lineweight groups (8, 6, 62/420, 370). ByLayer is left out, unless
/// it replaces something else in `base` (an attribute's definition).
fn layer_and_props(w: &mut W, c: &Common, base: Option<&Common>) {
    let write = |by_layer: bool, base_by_layer: fn(&Common) -> bool| !by_layer || base.is_some_and(|b| !base_by_layer(b));
    w.s(8, &c.layer);
    if write(c.linetype.eq_ignore_ascii_case("bylayer"), |b| b.linetype.eq_ignore_ascii_case("bylayer")) {
        w.s(6, &c.linetype);
    }
    match c.color {
        Color::ByLayer if write(true, |b| b.color == Color::ByLayer) => w.i(62, 256),
        Color::ByLayer => {}
        Color::True(rgb) => {
            w.i(62, i64::from(cadcraft_color::nearest_aci(rgb)));
            w.i(420, i64::from(rgb.to_u32()));
        }
        c => w.i(62, i64::from(c.to_aci())),
    }
    if write(c.lineweight == Lineweight::ByLayer, |b| b.lineweight == Lineweight::ByLayer) {
        w.i(370, i64::from(c.lineweight.to_dxf()));
    }
}

/// The properties attribute `a` of block reference `e` (of `ins`) is drawn with: its own, else its
/// definition's; an attribute defined on layer 0 is on the reference's layer. Also the
/// definition's properties.
fn attrib_common(d: &Drawing, e: &Entity, ins: &Insert, a: &Attrib) -> (Common, Common) {
    let def = d
        .block(&ins.block)
        .and_then(|b| b.entities.iter().find(|be| matches!(&be.kind, EntityKind::AttDef(ad) if ad.tag.eq_ignore_ascii_case(&a.tag))));
    let base = def.map(|de| de.common.clone()).unwrap_or_default();
    let mut c = a.props.over(&base);
    if c.layer == "0" {
        c.layer.clone_from(&e.common.layer);
    }
    (c, base)
}

/// A multiline attribute's groups after its own (DXF 2007 and later): attribute type 2 (ATTRIB)
/// or 4 (ATTDEF), its alignment point, then its text as embedded MTEXT (`101 Embedded Object`).
/// Only what differs from unrotated text goes in the embedded object, as AutoCAD writes it.
fn multiline_tags(w: &mut W, a: &Attrib, attdef: bool) {
    let Some(m) = a.multiline_text() else { return };
    w.i(71, if attdef { 4 } else { 2 });
    w.i(72, if attdef { 0 } else { 1 });
    w.p(11, a.text.align_pt.unwrap_or(a.text.insert));
    w.s(101, "Embedded Object");
    w.p(10, m.insert);
    w.f(40, m.height);
    w.f(41, m.width);
    w.f(46, 0.0);
    w.i(71, i64::from(m.attach));
    w.i(72, 5);
    let chars: Vec<char> = crate::dxf_read::mtext_dxf::encode_contents(&m.contents).chars().collect();
    let chunks: Vec<String> = chars.chunks(250).map(|c| c.iter().collect()).collect();
    let n = chunks.len();
    for (i, c) in chunks.iter().enumerate() {
        w.s(if i + 1 == n { 1 } else { 3 }, c.clone());
    }
    if n == 0 {
        w.s(1, "");
    }
    w.s(7, &m.style);
    if m.rotation != 0.0 {
        w.p(11, Vec3::new(m.rotation.cos(), m.rotation.sin(), 0.0));
    }
    w.i(73, if m.line_spacing_exact { 2 } else { 1 });
    w.f(44, m.line_spacing);
}

/// ATTRIB/ATTDEF flags (group 70): 1 invisible, 2 constant, 4 verify, 8 preset.
fn attrib_flags(a: &Attrib) -> i64 {
    i64::from(a.invisible) | if a.constant { 2 } else { 0 } | if a.modes.verify { 4 } else { 0 } | if a.modes.preset { 8 } else { 0 }
}

/// A TEXT's groups, or an attribute's (`attrib`, in a file of version `target`) with its
/// subclass: the subclass version (`280 0`, 2010 and later), prompt, tag, flags and alignment,
/// then Lock position (`280`, 2007 and later; readers take the 280 after the tag as it).
/// The insertion point (group 10) to write for `t`. A justified text is placed by its alignment
/// point (group 11), and its insertion point is the start of its baseline, which AutoCAD reads
/// from DWG as is (it recomputes it only for DXF): writing the alignment point there drew centred
/// and right-aligned text from the wrong end in AutoCAD (#1050).
///
/// The stored point is kept when it is close to where CADCraft's font puts the baseline start:
/// then it is AutoCAD's own (exact for its fonts). It is replaced when it is clearly stale: the
/// alignment point itself (text made here) or a text whose length changed since. Close means
/// along the baseline within a tenth of the text's width plus half its height (AutoCAD's fonts,
/// or the ones CADCraft substitutes for them, differ most in width, and most on short texts) and
/// across it within a fifth of its height: Left Middle and Left Bottom text made here, whose
/// alignment point is only half or a third of a height from the baseline start, must not pass
/// for AutoCAD's.
fn text_insert(d: &Drawing, t: &Text) -> Vec3 {
    let computed = computed_text_insert(d, t);
    if t.align_pt.is_some_and(|a| a.xy() == t.insert.xy()) {
        return computed;
    }
    let h = t.height.abs();
    let off = (t.insert.xy() - computed.xy()).rotate(-t.rotation);
    let close = if t.value.contains('\n') {
        // A multiline attribute's lines are placed by its embedded MTEXT: a one-line baseline
        // start says little about them, so only the distance counts.
        let width = t.align_pt.map_or(0.0, |a| 2.0 * a.xy().dist(computed.xy()));
        off.len() <= 0.1 * width + 0.5 * h
    } else {
        let width = cadcraft_fonts::line_width(&t.value, h, t.width_factor.abs());
        off.x.abs() <= 0.1 * width + 0.5 * h && off.y.abs() <= 0.2 * h
    };
    if close { t.insert } else { computed }
}

fn computed_text_insert(d: &Drawing, t: &Text) -> Vec3 {
    let justified = !matches!((t.halign, t.valign), (HAlign::Left, VAlign::Baseline) | (HAlign::Aligned | HAlign::Fit, _));
    let Some(a) = t.align_pt.filter(|_| justified) else { return t.insert };
    let ts = d.text_style(&t.style).cloned().unwrap_or_default();
    let (h, v) = cadcraft_render::text_align(t.halign, t.valign);
    let p = cadcraft_fonts::TextParams {
        insert: t.insert.xy(),
        align_pt: Some(a.xy()),
        height: t.height,
        rotation: t.rotation,
        width_factor: t.width_factor,
        oblique: t.oblique,
        h,
        v,
        backwards: ts.backwards,
        upside_down: ts.upside_down,
    };
    let q = cadcraft_fonts::baseline_start(&cadcraft_fonts::TextFont::resolve(&ts.font), &t.value, &p);
    if q.is_finite() { Vec3::new(q.x, q.y, a.z) } else { t.insert }
}

fn text_tags(w: &mut W, d: &Drawing, t: &Text, attrib: Option<(&Attrib, Version)>, attdef_prompt: Option<&str>) {
    w.p(10, text_insert(d, t));
    w.f(40, t.height);
    w.s(1, &t.value);
    if t.rotation != 0.0 {
        w.f(50, t.rotation.to_degrees());
    }
    if (t.width_factor - 1.0).abs() > 1e-12 {
        w.f(41, t.width_factor);
    }
    if t.oblique != 0.0 {
        w.f(51, t.oblique.to_degrees());
    }
    w.s(7, &t.style);
    let h = match t.halign {
        HAlign::Left => 0,
        HAlign::Center => 1,
        HAlign::Right => 2,
        HAlign::Aligned => 3,
        HAlign::Middle => 4,
        HAlign::Fit => 5,
    };
    let v = match t.valign {
        VAlign::Baseline => 0,
        VAlign::Bottom => 1,
        VAlign::Middle => 2,
        VAlign::Top => 3,
    };
    if h != 0 {
        w.i(72, h);
    }
    if let Some(a) = t.align_pt {
        w.p(11, a);
    }
    match attrib {
        Some((a, target)) => {
            w.s(100, if attdef_prompt.is_some() { "AcDbAttributeDefinition" } else { "AcDbAttribute" });
            if target >= Version::R2010 {
                w.i(280, 0);
            }
            if let Some(prompt) = attdef_prompt {
                w.s(3, prompt);
            }
            w.s(2, &a.tag);
            w.i(70, attrib_flags(a));
            if v != 0 {
                w.i(74, v);
            }
            if target >= Version::R2007 {
                w.i(280, i64::from(a.modes.lock_position));
            }
        }
        None => {
            w.s(100, "AcDbText");
            if v != 0 {
                w.i(73, v);
            }
        }
    }
}

fn entity(w: &mut W, d: &Drawing, e: &Entity, owner: &str, paper: bool, cx: &Ctx) {
    match &e.kind {
        EntityKind::Line(l) => {
            w.s(0, "LINE");
            common(w, e, owner, paper, "AcDbLine");
            thickness(w, e);
            w.p(10, l.a);
            w.p(11, l.b);
        }
        EntityKind::Point(p) => {
            w.s(0, "POINT");
            common(w, e, owner, paper, "AcDbPoint");
            w.p(10, p.p);
            thickness(w, e);
            if p.angle != 0.0 {
                w.f(50, p.angle.to_degrees());
            }
        }
        EntityKind::Circle(c) => {
            w.s(0, "CIRCLE");
            common(w, e, owner, paper, "AcDbCircle");
            thickness(w, e);
            w.p(10, c.center);
            w.f(40, c.radius);
        }
        EntityKind::Arc(a) => {
            w.s(0, "ARC");
            common(w, e, owner, paper, "AcDbCircle");
            thickness(w, e);
            w.p(10, a.center);
            w.f(40, a.radius);
            w.s(100, "AcDbArc");
            w.f(50, a.start.to_degrees());
            w.f(51, a.end.to_degrees());
        }
        EntityKind::Ellipse(el) => {
            w.s(0, "ELLIPSE");
            common(w, e, owner, paper, "AcDbEllipse");
            w.p(10, el.center);
            w.p(11, el.major);
            w.f(40, el.ratio);
            w.f(41, el.start);
            w.f(42, el.end);
        }
        EntityKind::LwPolyline(p) => {
            w.s(0, "LWPOLYLINE");
            common(w, e, owner, paper, "AcDbPolyline");
            w.i(90, p.vertices.len() as i64);
            w.i(70, i64::from(p.closed) | if p.plinegen { 128 } else { 0 });
            if p.const_width > 0.0 {
                w.f(43, p.const_width);
            }
            if p.elevation != 0.0 {
                w.f(38, p.elevation);
            }
            thickness(w, e);
            for v in &p.vertices {
                w.p2(10, v.p);
                if v.start_width != 0.0 || v.end_width != 0.0 {
                    w.f(40, v.start_width);
                    w.f(41, v.end_width);
                }
                if v.bulge != 0.0 {
                    w.f(42, v.bulge);
                }
            }
        }
        EntityKind::MLine(m) => {
            w.s(0, "MLINE");
            common(w, e, owner, paper, "AcDbMline");
            w.t.extend(crate::dxf_mline::entity_tags(m, &cx.mlines.handle(&m.style)));
        }
        EntityKind::Polyline3d(p) => {
            w.s(0, "POLYLINE");
            common(w, e, owner, paper, "AcDb3dPolyline");
            w.i(66, 1);
            w.p(10, Vec3::ZERO);
            thickness(w, e);
            w.i(70, 8 | i64::from(p.closed));
            for q in &p.points {
                let vh = w.h();
                w.s(0, "VERTEX");
                w.s(5, vh);
                w.s(330, e.handle.hex());
                w.s(100, "AcDbEntity");
                w.s(8, &e.common.layer);
                w.s(100, "AcDbVertex");
                w.s(100, "AcDb3dPolylineVertex");
                w.p(10, *q);
                w.i(70, 32);
            }
            let sh = w.h();
            w.s(0, "SEQEND");
            w.s(5, sh);
            w.s(330, e.handle.hex());
            w.s(100, "AcDbEntity");
            w.s(8, &e.common.layer);
        }
        EntityKind::Spline(s) => {
            w.s(0, "SPLINE");
            common(w, e, owner, paper, "AcDbSpline");
            let flags = 8 | i64::from(s.closed) | if s.is_periodic() { 2 } else { 0 } | if s.weights.is_empty() { 0 } else { 4 };
            w.i(70, flags);
            w.i(71, s.degree as i64);
            w.i(72, s.knots.len() as i64);
            w.i(73, s.control.len() as i64);
            w.i(74, s.fit.len() as i64);
            w.f(42, 1e-10);
            w.f(43, 1e-10);
            // Fit data: tolerance and end tangents (the knot parametrisation is implied by the knots).
            if !s.fit.is_empty() {
                w.f(44, s.fit_opts.tolerance);
                if let Some(t) = s.fit_opts.start_tangent {
                    w.p(12, t.to3(0.0));
                }
                if let Some(t) = s.fit_opts.end_tangent {
                    w.p(13, t.to3(0.0));
                }
            }
            for k in &s.knots {
                w.f(40, *k);
            }
            for (i, c) in s.control.iter().enumerate() {
                w.p(10, c.to3(0.0));
                if let Some(wt) = s.weights.get(i) {
                    w.f(41, *wt);
                }
            }
            for f in &s.fit {
                w.p(11, f.to3(0.0));
            }
        }
        EntityKind::Helix(h) => {
            // A cubic spline through points on the helix, then the helix data (DXF Reference).
            w.s(0, "HELIX");
            common(w, e, owner, paper, "AcDbSpline");
            let (s, z) = h.spline3();
            w.i(70, 0);
            w.i(71, s.degree as i64);
            w.i(72, s.knots.len() as i64);
            w.i(73, s.control.len() as i64);
            w.i(74, 0);
            w.f(42, 1e-10);
            w.f(43, 1e-10);
            for k in &s.knots {
                w.f(40, *k);
            }
            for (c, z) in s.control.iter().zip(&z) {
                w.p(10, c.to3(*z));
            }
            w.s(100, "AcDbHelix");
            w.i(90, 29);
            w.i(91, 63);
            w.p(10, h.center);
            w.p(11, h.start_point());
            w.p(12, Vec3::Z);
            w.f(40, h.base_radius);
            w.f(41, h.turns);
            w.f(42, h.turn_height());
            w.i(290, i64::from(h.ccw));
            w.i(280, h.constrain.to_dxf());
        }
        EntityKind::Ray(r) | EntityKind::XLine(r) => {
            let ray = matches!(e.kind, EntityKind::Ray(_));
            w.s(0, if ray { "RAY" } else { "XLINE" });
            common(w, e, owner, paper, if ray { "AcDbRay" } else { "AcDbXline" });
            w.p(10, r.base);
            w.p(11, r.dir);
        }
        EntityKind::Text(t) => {
            w.s(0, "TEXT");
            common(w, e, owner, paper, "AcDbText");
            thickness(w, e);
            text_tags(w, d, t, None, None);
        }
        // Closed polylines, one per loop (`dxf_region`): REGION's own data is ACIS.
        EntityKind::Region(r) => {
            for (i, tags) in crate::dxf_region::polylines(r).into_iter().enumerate() {
                w.s(0, "LWPOLYLINE");
                if i == 0 {
                    common(w, e, owner, paper, "AcDbPolyline");
                } else {
                    // The other loops get handles of their own (only the handle and properties are written).
                    let h = Handle::parse_hex(&w.h()).unwrap_or(e.handle);
                    let part = Entity { handle: h, common: e.common.clone(), kind: EntityKind::Point(Point { p: Vec3::ZERO, angle: 0.0 }) };
                    common(w, &part, owner, paper, "AcDbPolyline");
                }
                w.t.extend(tags);
            }
        }
        EntityKind::AttDef(a) => {
            w.s(0, "ATTDEF");
            common(w, e, owner, paper, "AcDbText");
            text_tags(w, d, &a.text, Some((a, cx.target)), Some(&a.prompt));
            if cx.target >= Version::R2007 {
                multiline_tags(w, a, true);
            }
        }
        EntityKind::MText(t) => {
            w.s(0, "MTEXT");
            common(w, e, owner, paper, "AcDbMText");
            w.p(10, t.insert);
            w.f(40, t.height);
            w.f(41, t.width);
            w.i(71, i64::from(t.attach));
            w.i(72, 1);
            // Long contents go in 250-char 3-chunks, the remainder in 1.
            let chars: Vec<char> = crate::dxf_read::mtext_dxf::encode_contents(&t.contents).chars().collect();
            let chunks: Vec<String> = chars.chunks(250).map(|c| c.iter().collect()).collect();
            let n = chunks.len();
            for (i, c) in chunks.iter().enumerate() {
                w.s(if i + 1 == n { 1 } else { 3 }, c.clone());
            }
            if n == 0 {
                w.s(1, "");
            }
            w.s(7, &t.style);
            if t.rotation != 0.0 {
                w.f(50, t.rotation.to_degrees());
            }
            // Line spacing style: 1 = at least, 2 = exactly.
            w.i(73, if t.line_spacing_exact { 2 } else { 1 });
            w.f(44, t.line_spacing);
            w.t.extend(crate::dxf_read::mtext_dxf::column_tags(t));
        }
        EntityKind::Insert(i) => {
            w.s(0, "INSERT");
            common(w, e, owner, paper, "AcDbBlockReference");
            if !i.attribs.is_empty() {
                w.i(66, 1);
            }
            w.s(2, &i.block);
            w.p(10, i.insert);
            w.f(41, i.scale.x);
            w.f(42, i.scale.y);
            w.f(43, i.scale.z);
            w.f(50, i.rotation.to_degrees());
            if i.cols > 1 || i.rows > 1 {
                w.i(70, i64::from(i.cols));
                w.i(71, i64::from(i.rows));
                w.f(44, i.col_spacing);
                w.f(45, i.row_spacing);
            }
            if !i.attribs.is_empty() {
                for a in &i.attribs {
                    let ah = w.h();
                    w.s(0, "ATTRIB");
                    w.s(5, ah);
                    w.s(330, e.handle.hex());
                    w.s(100, "AcDbEntity");
                    let (c, base) = attrib_common(d, e, i, a);
                    layer_and_props(w, &c, Some(&base));
                    // Hidden with its definition (a dynamic block's visibility state hides it).
                    if !base.visible {
                        w.i(60, 1);
                    }
                    w.s(100, "AcDbText");
                    text_tags(w, d, &a.text, Some((a, cx.target)), None);
                    if cx.target >= Version::R2007 {
                        multiline_tags(w, a, false);
                    }
                }
                let sh = w.h();
                w.s(0, "SEQEND");
                w.s(5, sh);
                w.s(330, e.handle.hex());
                w.s(100, "AcDbEntity");
                w.s(8, &e.common.layer);
            }
        }
        EntityKind::Dimension(dm) => {
            w.s(0, "DIMENSION");
            common_x(w, e, owner, paper, "AcDbDimension", cx.assoc.get(&e.handle));
            let bname = cx.dim_blocks.get(&e.handle).cloned().or_else(|| dm.block.clone()).unwrap_or_default();
            w.s(2, bname);
            w.p(10, dm.defpt);
            let style = d.dim_style(&dm.style).cloned().unwrap_or_default();
            let g = cadcraft_render::dimension_geometry(dm, &style, d.header.f64("DIMSCALE", 1.0));
            w.p(11, if dm.user_text_pos { dm.text_mid } else { g.text_pos.to3(0.0) });
            let ty: i64 = match dm.kind {
                DimKind::Linear { .. } => 0,
                DimKind::Aligned => 1,
                DimKind::Angular => 2,
                DimKind::Diameter => 3,
                DimKind::Radius => 4,
                DimKind::Angular3P => 5,
                DimKind::Ordinate { x_type } => 6 | if x_type { 64 } else { 0 },
                // Written as aligned, marked by CADCraft xdata (see `dim_xdata`).
                DimKind::ArcLength => 1,
            };
            w.i(70, ty | 32 | if dm.user_text_pos { 128 } else { 0 });
            // The actual measurement (read-only in DXF): AutoLISP's entget shows it.
            w.f(42, g.measurement);
            if !dm.text.is_empty() {
                w.s(1, &dm.text);
            }
            w.s(3, &dm.style);
            if dm.text_rotation != 0.0 {
                w.f(53, dm.text_rotation.to_degrees());
            }
            match dm.kind {
                DimKind::Linear { rotation } => {
                    w.s(100, "AcDbAlignedDimension");
                    w.p(13, dm.p13);
                    w.p(14, dm.p14);
                    w.f(50, rotation.to_degrees());
                    oblique_52(w, dm, rotation);
                    w.s(100, "AcDbRotatedDimension");
                }
                DimKind::Aligned | DimKind::ArcLength => {
                    w.s(100, "AcDbAlignedDimension");
                    w.p(13, dm.p13);
                    w.p(14, dm.p14);
                    if matches!(dm.kind, DimKind::Aligned) {
                        oblique_52(w, dm, (dm.p14.xy() - dm.p13.xy()).angle());
                    }
                }
                DimKind::Radius => {
                    w.s(100, "AcDbRadialDimension");
                    w.p(15, dm.p15);
                    w.f(40, 0.0);
                }
                DimKind::Diameter => {
                    w.s(100, "AcDbDiametricDimension");
                    w.p(15, dm.p15);
                    w.f(40, 0.0);
                }
                DimKind::Angular => {
                    w.s(100, "AcDb2LineAngularDimension");
                    w.p(13, dm.p13);
                    w.p(14, dm.p14);
                    w.p(15, dm.p15);
                    w.p(16, dm.p16);
                }
                DimKind::Angular3P => {
                    w.s(100, "AcDb3PointAngularDimension");
                    w.p(13, dm.p13);
                    w.p(14, dm.p14);
                    w.p(15, dm.p15);
                }
                DimKind::Ordinate { .. } => {
                    w.s(100, "AcDbOrdinateDimension");
                    w.p(13, dm.p13);
                    w.p(14, dm.p14);
                }
            }
            dim_xdata(w, dm, cx);
        }
        EntityKind::Table(t) => {
            // Tables without a generated block (none expected) are not written.
            let Some((bname, brh)) = cx.tables.get(&e.handle) else { return };
            w.s(0, "ACAD_TABLE");
            common(w, e, owner, paper, "AcDbBlockReference");
            w.s(2, bname);
            w.p(10, t.insert);
            w.s(100, "AcDbTable");
            w.i(280, 0);
            w.s(342, cx.table_style(&t.style));
            w.s(343, brh);
            w.p(11, Vec3::new(1.0, 0.0, 0.0));
            w.i(90, 0);
            let (rows, cols) = table_size(t);
            w.i(91, rows as i64);
            w.i(92, cols as i64);
            for c in [93, 94, 95, 96] {
                w.i(c, 0);
            }
            for h in t.row_heights.iter().take(rows) {
                w.f(141, *h);
            }
            for c in t.col_widths.iter().take(cols) {
                w.f(142, *c);
            }
            let cov = cadcraft_render::table_covered(t);
            let span = |v: u32| i64::from(v.min(i16::MAX as u32));
            for r in 0..rows {
                for c in 0..cols {
                    let cell = t.cells.get(r).and_then(|x| x.get(c));
                    let covered = cov.get(r).and_then(|x| x.get(c)).copied().unwrap_or(false);
                    let merge = cell.and_then(|x| x.merged).filter(|&(a, b)| a >= 1 && b >= 1 && (a, b) != (1, 1));
                    w.i(171, 1);
                    w.i(172, 0);
                    w.i(173, i64::from(covered || merge.is_some()));
                    w.i(174, 0);
                    let (rs, cs) = if covered { (0, 0) } else { merge.map(|(a, b)| (span(a), span(b))).unwrap_or((1, 1)) };
                    w.i(175, cs);
                    w.i(176, rs);
                    w.i(91, 0);
                    w.i(178, 0);
                    w.f(145, 0.0);
                    if let Some(h) = cell.and_then(|x| x.text_height).filter(|h| h.is_finite() && *h > 0.0) {
                        w.f(140, h);
                    }
                    // Long text: 250-character code-2 chunks, the rest in code 1.
                    let chars: Vec<char> = cell.map(|x| x.text.chars().collect()).unwrap_or_default();
                    let chunks: Vec<String> = chars.chunks(250).map(|c| c.iter().collect()).collect();
                    let n = chunks.len();
                    for (i, ch) in chunks.into_iter().enumerate() {
                        w.s(if i + 1 == n { 1 } else { 2 }, ch);
                    }
                    if n == 0 {
                        w.s(1, "");
                    }
                }
            }
            w.s(1001, dxf_ext::APP);
            w.s(1000, "TABLE");
            w.i(1070, i64::from(t.title));
            w.i(1070, i64::from(t.header));
            w.f(1040, t.text_height);
            w.s(1000, &t.style);
            for c in crate::dxf_table::table_xdata(t) {
                w.s(1000, c);
            }
        }
        EntityKind::MLeader(m) if !cx.native_mleader() => mleader_fallback(w, d, e, m, owner, paper, cx),
        EntityKind::MLeader(m) => {
            w.s(0, "MULTILEADER");
            common(w, e, owner, paper, "AcDbMLeader");
            let refs = crate::dxf_mleader::Refs {
                style: cx.mleader_style(&m.style),
                text_style: m.text.as_ref().and_then(|t| cx.style(&t.style)),
                block: m.block.as_ref().and_then(|b| cx.block_records.get(&b.block.to_ascii_uppercase()).cloned()),
            };
            w.t.extend(crate::dxf_mleader::entity_tags(d, m, &refs));
        }
        EntityKind::Leader(l) => {
            w.s(0, "LEADER");
            common(w, e, owner, paper, "AcDbLeader");
            w.s(3, &l.style);
            w.i(71, i64::from(l.arrow));
            w.i(72, i64::from(l.spline));
            w.i(76, l.vertices.len() as i64);
            for v in &l.vertices {
                w.p(10, *v);
            }
        }
        EntityKind::Image(i) => {
            // Every image has its IMAGEDEF (planned in `write`); the fallback never happens.
            let Some((def, reactor)) = cx.images.by_entity.get(&e.handle) else { return };
            w.s(0, "IMAGE");
            common(w, e, owner, paper, "AcDbRasterImage");
            w.t.extend(crate::dxf_image::entity_tags(i, def, reactor));
        }
        EntityKind::Solid(s) | EntityKind::Trace(s) => {
            let solid = matches!(e.kind, EntityKind::Solid(_));
            w.s(0, if solid { "SOLID" } else { "TRACE" });
            common(w, e, owner, paper, "AcDbTrace");
            w.p(10, s.corners[0]);
            w.p(11, s.corners[1]);
            w.p(12, s.corners[2]);
            w.p(13, s.corners[3]);
            thickness(w, e);
        }
        EntityKind::Face3d(f) => {
            w.s(0, "3DFACE");
            common(w, e, owner, paper, "AcDbFace");
            w.p(10, f.corners[0]);
            w.p(11, f.corners[1]);
            w.p(12, f.corners[2]);
            w.p(13, f.corners[3]);
            w.i(70, i64::from(f.hidden_edges));
        }
        EntityKind::Hatch(h) => {
            w.s(0, "HATCH");
            common(w, e, owner, paper, "AcDbHatch");
            w.p(10, Vec3::new(0.0, 0.0, h.elevation));
            w.p(210, Vec3::Z);
            w.s(2, &h.pattern);
            w.i(70, i64::from(h.solid));
            w.i(71, i64::from(h.associative));
            w.i(91, h.loops.len() as i64);
            for l in &h.loops {
                w.i(92, 2 | if l.outer { 1 } else { 0 });
                let has_bulge = l.vertices.iter().any(|v| v.bulge != 0.0);
                w.i(72, i64::from(has_bulge));
                w.i(73, 1);
                w.i(93, l.vertices.len() as i64);
                for v in &l.vertices {
                    w.p2(10, v.p);
                    if has_bulge {
                        w.f(42, v.bulge);
                    }
                }
                // Source boundary objects of an associative hatch (the ones still beside it, in
                // its space or block, which carry the hatch as a reactor).
                let own = e.handle.hex();
                let sources: Vec<String> =
                    l.sources.iter().filter(|s| h.associative && w.reactors.get(*s).is_some_and(|r| r.contains(&own))).map(|s| s.hex()).collect();
                w.i(97, sources.len() as i64);
                for s in &sources {
                    w.s(330, s);
                }
            }
            w.i(75, i64::from(h.style));
            w.i(76, 1);
            if !h.solid {
                w.f(52, h.angle.to_degrees());
                w.f(41, h.scale);
                w.i(77, 0);
                // The lines read from the file, else the library's pattern of that name.
                let lines = if h.pattern_lines.is_empty() {
                    cadcraft_doc::library::pattern_for(d, &h.pattern).map(|p| p.lines).unwrap_or_default()
                } else {
                    h.pattern_lines.clone()
                };
                w.i(78, lines.len() as i64);
                for pl in lines {
                    let ang = pl.angle.to_radians() + h.angle;
                    w.f(53, ang.to_degrees());
                    let o = h.origin + Vec2::new(pl.origin.0, pl.origin.1).rotate(h.angle) * h.scale;
                    w.f(43, o.x);
                    w.f(44, o.y);
                    let dl = Vec2::new(pl.delta.0, pl.delta.1).rotate(ang) * h.scale;
                    w.f(45, dl.x);
                    w.f(46, dl.y);
                    w.i(79, pl.dashes.len() as i64);
                    for dsh in pl.dashes {
                        w.f(49, dsh * h.scale);
                    }
                }
            }
            w.i(98, 0);
            if let Some(g) = &h.gradient {
                gradient(w, g);
            }
            hatch_xdata(w, h);
        }
        EntityKind::Viewport(v) => {
            paper_view(w, d, e, owner, cx);
            w.s(0, "VIEWPORT");
            common(w, e, owner, paper, "AcDbViewport");
            w.p(10, v.center);
            w.f(40, v.width);
            w.f(41, v.height);
            // Status 68: a positive value is "on" (the stacking order; 1 is the active viewport), 0 off.
            w.i(68, if v.on { i64::from(v.id.clamp(1, 32767)) } else { 0 });
            w.i(69, i64::from(v.id));
            w.p2(12, v.view_center);
            w.f(45, v.view_height);
            // Status flags: 16384 = display locked, 32768 = always set, 65536 = clipped by the
            // 340 entity, 131072 = off.
            let clip = v.clip.filter(|h| *h != e.handle && d.entity(*h).is_some());
            let flags = 32768 | if v.locked { 16384 } else { 0 } | if clip.is_some() { 65536 } else { 0 } | if v.on { 0 } else { 131072 };
            w.i(90, flags);
            if let Some(h) = clip {
                w.s(340, h.hex());
            }
            // AutoCAD's frozen-layer list: one soft pointer per layer, to its LAYER record. Other
            // programs read only this; CADCraft prefers its own xdata below when present.
            for name in &v.frozen_layers {
                if let Some(h) = w.layers.get(&name.to_ascii_uppercase()).cloned() {
                    w.s(341, h);
                }
            }
            // One CADCraft xdata group holds both lists: readers stop at the first group of an app.
            if !v.frozen_layers.is_empty() || !v.layer_colors.is_empty() {
                w.s(1001, dxf_ext::APP);
                if !v.frozen_layers.is_empty() {
                    w.xdata(dxf_ext::frozen_xdata(&v.frozen_layers));
                }
                if !v.layer_colors.is_empty() {
                    w.xdata(dxf_ext::layer_colors_xdata(&v.layer_colors));
                }
            }
        }
        EntityKind::Wipeout(wo) => {
            // A 1x1 image spanning the boundary's extents. Clip vertices are in pixel space, whose origin is
            // the image's top-left corner with y pointing down: the reader maps each vertex back through
            // insert + u * (x + 0.5) + v * (0.5 - y).
            // Polygonal clip boundaries are closed, so the first vertex is repeated when needed.
            let b = Bounds2::from_points(wo.boundary.iter().copied());
            let o = if b.is_empty() { Vec2::ZERO } else { b.min };
            let (sx, sy) = (if b.width() > 0.0 { b.width() } else { 1.0 }, if b.height() > 0.0 { b.height() } else { 1.0 });
            w.s(0, "WIPEOUT");
            common(w, e, owner, paper, "AcDbWipeout");
            w.i(90, 0);
            w.p(10, Vec3::new(o.x, o.y, 0.0));
            w.p(11, Vec3::new(sx, 0.0, 0.0));
            w.p(12, Vec3::new(0.0, sy, 0.0));
            w.p2(13, Vec2::new(1.0, 1.0));
            // Display flags 7 = show image, show unaligned, use clipping boundary; clipping on; default brightness/contrast/fade.
            w.i(70, 7);
            w.i(280, 1);
            w.i(281, 50);
            w.i(282, 50);
            w.i(283, 0);
            w.i(71, 2);
            let mut pts = wo.boundary.clone();
            if let (Some(first), Some(last)) = (pts.first().copied(), pts.last().copied())
                && first != last
            {
                pts.push(first);
            }
            w.i(91, pts.len() as i64);
            for q in pts {
                w.p2(14, Vec2::new((q.x - o.x) / sx - 0.5, 0.5 - (q.y - o.y) / sy));
            }
        }
        EntityKind::Tolerance(t) => {
            w.s(0, "TOLERANCE");
            common(w, e, owner, paper, "AcDbFcf");
            w.s(3, &t.style);
            w.p(10, t.insert);
            w.s(1, &t.text);
            w.p(11, t.dir);
        }
        // Entity types CADCraft doesn't model aren't written; the save command reports them.
        EntityKind::Unknown(_) => {}
    }
}

/// `$DWGCODEPAGE`: before 2007 the code page read from the file when CADCraft knows it (the text
/// is ASCII with `\U+XXXX` escapes, right in any code page, but the file keeps its declared one);
/// otherwise, and from 2007 (UTF-8 text), `ANSI_1252`.
fn code_page(d: &Drawing, version: Version) -> String {
    let read = d.header.get("DWGCODEPAGE").and_then(HVal::as_str).map(str::trim).unwrap_or_default();
    let known = read.is_ascii() && read.len() <= 32 && cadcraft_dxf::decode_code_page(b"", read).is_some();
    if version < Version::R2007 && known { read.to_string() } else { "ANSI_1252".into() }
}

/// A multileader for a file older than 2007 (no MULTILEADER): a LEADER per leader line, ending
/// at the landing, plus its MTEXT or its block's INSERT, with the multileader's properties.
fn mleader_fallback(w: &mut W, d: &Drawing, e: &Entity, m: &MLeader, owner: &str, paper: bool, cx: &Ctx) {
    let part = |w: &mut W, kind: EntityKind| Entity { handle: Handle::parse_hex(&w.h()).unwrap_or(Handle(0)), common: e.common.clone(), kind };
    let style = d.header.str("DIMSTYLE", "Standard");
    for arm in m.leaders.iter().take(64) {
        let mut vertices = arm.clone();
        vertices.push(m.landing);
        let le = part(w, EntityKind::Leader(Leader { vertices, arrow: true, spline: m.spline, style: style.clone() }));
        entity(w, d, &le, owner, paper, cx);
    }
    if let Some(t) = &m.text {
        let te = part(w, EntityKind::MText(t.clone()));
        entity(w, d, &te, owner, paper, cx);
    }
    if let Some(ins) = &m.block {
        let ie = part(w, EntityKind::Insert(ins.clone()));
        entity(w, d, &ie, owner, paper, cx);
    }
}

/// A hatch's gradient fill (DXF Reference, HATCH group codes 450–470): two-colour gradient
/// with its rotation (radians), shift (0 = centered, 1 = shifted) and per-colour 463 records
/// carrying ACI (63) and, for true colours, the RGB value (421). Those groups belong to R2004+
/// files, so the same gradient also travels as `CADCRAFT` xdata, which survives this R2000
/// file's conversion to DWG (where the native fields don't exist).
fn gradient(w: &mut W, g: &Gradient) {
    w.i(450, 1);
    w.i(451, 0);
    w.f(460, g.angle);
    w.f(461, if g.centered { 0.0 } else { 1.0 });
    w.i(452, 0);
    w.f(462, 0.0);
    w.i(453, 2);
    for (k, c) in [(0.0, g.color1), (1.0, g.color2)] {
        w.f(463, k);
        match c {
            Color::True(rgb) => {
                w.i(63, i64::from(cadcraft_color::nearest_aci(rgb)));
                w.i(421, i64::from(rgb.to_u32()));
            }
            c => w.i(63, i64::from(c.to_aci())),
        }
    }
    w.s(470, &g.name);
    w.s(1001, dxf_ext::APP);
    w.s(1000, "GRADIENT");
    // A 1000 group holds at most 255 bytes; cut at a character boundary.
    let cut = g.name.char_indices().map(|(i, c)| i + c.len_utf8()).take_while(|end| *end <= 255).last().unwrap_or(0);
    w.s(1000, g.name.get(..cut).unwrap_or_default());
    w.f(1040, g.angle);
    w.i(1070, i64::from(g.centered));
    w.s(1000, g.color1.name());
    w.s(1000, g.color2.name());
}

/// Group 39 (thickness) of the entities whose DXF Reference record has one; only when set.
fn thickness(w: &mut W, e: &Entity) {
    if e.common.thickness != 0.0 {
        w.f(39, e.common.thickness);
    }
}

/// Hatch origin and background colour as `CADCRAFT` xdata: `1000 HATCH`, the origin as a
/// world position (`1011`, moved with the hatch by other programs) and the background colour
/// name (`1000`, empty for none). The DXF Reference has no HATCH groups for either: other
/// readers get the origin through the pattern lines' base points (43/44), the background isn't
/// carried. Appended to the gradient's `CADCRAFT` xdata when there is one.
fn hatch_xdata(w: &mut W, h: &Hatch) {
    if h.origin == Vec2::ZERO && h.background.is_none() {
        return;
    }
    if h.gradient.is_none() {
        w.s(1001, dxf_ext::APP);
    }
    w.s(1000, "HATCH");
    w.p(1011, h.origin.to3(0.0));
    w.s(1000, h.background.map(Color::name).unwrap_or_default());
}

/// What happens to the anonymous `*D` blocks read from a file (#495): a dimension that still
/// names its block regenerates its geometry under that name, keeping the block's entity
/// handles, and `*D` blocks nothing else references are not written again.
struct DimBlockPlan {
    /// Dimension (model or paper space) → the read block whose name and handles it reuses.
    reuse: HashMap<Handle, std::sync::Arc<Block>>,
    /// Upper-case names of `*D` blocks not written as ordinary blocks.
    skip: std::collections::HashSet<String>,
}

fn is_dim_block(name: &str) -> bool {
    name.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("*D"))
}

fn dim_block_plan(d: &Drawing) -> DimBlockPlan {
    let spaces: Vec<&EntityStore> = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).collect();
    // Block names other objects use: inserts and multileader contents anywhere, and dimensions
    // inside block definitions (their blocks are written as read).
    let mut used = std::collections::HashSet::new();
    let note = |e: &Entity, dims: bool, used: &mut std::collections::HashSet<String>| match &e.kind {
        EntityKind::Insert(i) => {
            used.insert(i.block.to_ascii_uppercase());
        }
        EntityKind::MLeader(m) => used.extend(m.block.as_ref().map(|b| b.block.to_ascii_uppercase())),
        EntityKind::Dimension(dm) if dims => used.extend(dm.block.as_ref().map(|n| n.to_ascii_uppercase())),
        _ => {}
    };
    for e in spaces.iter().flat_map(|s| s.iter()) {
        note(e, false, &mut used);
    }
    for e in d.blocks.values().flat_map(|b| b.entities.iter()) {
        note(e, true, &mut used);
    }
    let skip = d.blocks.keys().filter(|n| is_dim_block(n)).map(|n| n.to_ascii_uppercase()).filter(|n| !used.contains(n)).collect();
    let mut plan = DimBlockPlan { reuse: HashMap::new(), skip };
    let mut claimed = std::collections::HashSet::new();
    for e in spaces.iter().flat_map(|s| s.iter()) {
        let EntityKind::Dimension(dm) = &e.kind else { continue };
        let Some(b) = dm.block.as_deref().filter(|n| is_dim_block(n)).and_then(|n| d.block(n)) else { continue };
        let key = b.name.to_ascii_uppercase();
        if plan.skip.contains(&key) && claimed.insert(key) {
            plan.reuse.insert(e.handle, b.clone());
        }
    }
    plan
}

/// Give regenerated dimension geometry the handles of the block it replaces, in order; the rest
/// keep handle 0 and get fresh ones.
fn reuse_handles(ents: &mut [Entity], old: &Block, taken: &mut std::collections::HashSet<Handle>) {
    let mut handles = old.entities.iter().map(|e| e.handle).filter(|h| h.0 != 0);
    for e in ents.iter_mut() {
        match handles.by_ref().find(|h| taken.insert(*h)) {
            Some(h) => e.handle = h,
            None => break,
        }
    }
}

/// Anonymous dimension blocks (`*D1`…) with the rendered geometry, as consumers expect.
fn dim_block_entities(d: &Drawing, dm: &Dimension, layer: &str) -> Vec<Entity> {
    // The dimension as drawn: its overrides, DIMSCALE (0 = the drawing's) and text font.
    let style = d.dim_style(&dm.style).cloned().unwrap_or_default().with_overrides(&dm.overrides);
    let g = cadcraft_render::dimension_in(d, dm);
    let mut out = Vec::new();
    // Each part in its style colour (DIMCLRD, DIMCLRE, DIMCLRT), ByBlock when the style says so,
    // as AutoCAD writes them: readers that draw the block drew every part ByBlock, i.e. in the
    // dimension's own colour (#1052).
    let in_color = |color: Color| Common { layer: layer.into(), color, ..Common::default() };
    let (dim_c, ext_c, text_c) = (in_color(style.dim_line_color), in_color(style.ext_line_color), in_color(style.text_color));
    for (i, l) in g.lines.iter().enumerate() {
        let c = match g.line_roles.get(i) {
            Some(cadcraft_render::LineRole::Ext) => &ext_c,
            _ => &dim_c,
        };
        for seg in l.windows(2) {
            if let [a, b] = seg {
                out.push(Entity { handle: Handle(0), common: c.clone(), kind: EntityKind::Line(Line { a: a.to3(0.0), b: b.to3(0.0) }) });
            }
        }
    }
    for t in &g.fills {
        if let [a, b, cc] = t.as_slice() {
            out.push(Entity {
                handle: Handle(0),
                common: dim_c.clone(),
                kind: EntityKind::Solid(Solid { corners: [a.to3(0.0), b.to3(0.0), cc.to3(0.0), cc.to3(0.0)] }),
            });
        }
    }
    if !g.value.is_empty() {
        out.push(Entity {
            handle: Handle(0),
            common: text_c,
            kind: EntityKind::MText(MText {
                insert: g.text_pos.to3(0.0),
                height: g.text_height,
                width: 0.0,
                attach: 5,
                rotation: g.text_angle,
                style: style.text_style.clone(),
                contents: g.mtext_contents(),
                line_spacing: 1.0,
                line_spacing_exact: false,
                columns: None,
            }),
        });
    }
    out
}

/// Rows and columns written for a table (bounded like the renderer).
fn table_size(t: &Table) -> (usize, usize) {
    (t.row_heights.len().min(10_000), t.col_widths.len().min(10_000))
}

/// The anonymous `*T` block of a table: cell fills, borders and texts as its cell styles draw
/// them, for readers that only draw the block.
///
/// The block is relative to its own origin: the ACAD_TABLE reference adds the insertion point.
pub(crate) fn table_block_entities(d: &Drawing, t: &Table, layer: &str) -> Vec<Entity> {
    let at_origin = Table { insert: Vec3::ZERO, ..t.clone() };
    let g = cadcraft_render::table_geometry(d, &at_origin);
    let c = Common { layer: layer.into(), color: Color::ByBlock, ..Common::default() };
    let mut out = Vec::new();
    for f in &g.fills {
        let [a, b, cc, dd] = f.corners.map(|p| p.to3(0.0));
        // SOLID corners go 1, 2, 4, 3.
        out.push(Entity {
            handle: Handle(0),
            common: Common { color: f.color, ..c.clone() },
            kind: EntityKind::Solid(Solid { corners: [a, b, dd, cc] }),
        });
    }
    for l in &g.lines {
        out.push(Entity {
            handle: Handle(0),
            common: Common { color: l.color, lineweight: l.lineweight, ..c.clone() },
            kind: EntityKind::Line(Line { a: l.a.to3(0.0), b: l.b.to3(0.0) }),
        });
    }
    let mut texts = Vec::new();
    for cell in &g.texts {
        if !cell.text.is_empty() {
            let style = if d.text_style(&cell.style).is_some() { cell.style.clone() } else { "Standard".into() };
            texts.push(Entity {
                handle: Handle(0),
                common: Common { color: cell.color, ..c.clone() },
                kind: EntityKind::MText(MText {
                    insert: cell.at.to3(0.0),
                    height: cell.height,
                    width: 0.0,
                    attach: cell.attach,
                    rotation: 0.0,
                    style,
                    contents: cell.text.clone(),
                    line_spacing: 1.0,
                    line_spacing_exact: false,
                    columns: None,
                }),
            });
        }
    }
    out.extend(texts);
    out
}

/// Our own geometry for an arrowhead block: unit size, tip at the origin, pointing along +X.
fn arrow_block_entities(kind: cadcraft_render::Arrowhead) -> Vec<Entity> {
    let g = cadcraft_render::arrowhead(kind, Vec2::ZERO, Vec2::X, 1.0);
    let c = Common { layer: "0".into(), color: Color::ByBlock, ..Common::default() };
    let mut out = Vec::new();
    for l in &g.lines {
        for seg in l.windows(2) {
            if let [a, b] = seg {
                out.push(Entity { handle: Handle(0), common: c.clone(), kind: EntityKind::Line(Line { a: a.to3(0.0), b: b.to3(0.0) }) });
            }
        }
    }
    for t in g.tris.chunks(3) {
        if let [a, b, cc] = t {
            out.push(Entity {
                handle: Handle(0),
                common: c.clone(),
                kind: EntityKind::Solid(Solid { corners: [a.to3(0.0), b.to3(0.0), cc.to3(0.0), cc.to3(0.0)] }),
            });
        }
    }
    out
}

/// Whether the entities of the block `name` are the arrowhead block CADCraft writes for that
/// standard arrow name ([`arrow_block_entities`]): the same lines and solids with the same
/// properties (layer 0, ByBlock). Any other content (AutoCAD's own `_Dot`, a user's
/// redefinition, an older CADCraft's geometry) is the file's.
pub(crate) fn is_generated_arrow(name: &str, entities: &[&Entity]) -> bool {
    let Some((_, kind)) = dxf_ext::arrow_block_name(name) else { return false };
    let ours = arrow_block_entities(kind);
    ours.len() == entities.len() && ours.iter().zip(entities).all(|(a, b)| a.kind == b.kind && a.common == b.common)
}

/// Whether a DIMASSOC point reference to `k` names a point of it without naming a segment:
/// true for objects made of one curve. Polylines are referenced by segment ([`polyline_vertex_ref`]);
/// 3D polylines and multilines can't be yet, and references to them are left out, as AutoCAD
/// otherwise resolved them to the first segment and changed the dimension's value (#1049).
fn whole_object_ref(k: &EntityKind) -> bool {
    !matches!(k, EntityKind::LwPolyline(_) | EntityKind::Polyline3d(_) | EntityKind::MLine(_))
}

/// A polyline vertex as AutoCAD references it (#1049): an end of one of its two segments,
/// which are numbered from 1 (segment k runs from vertex k−1 to vertex k; a closed polyline's
/// closing segment is the last). Returns (osnap, parameter, segment): the end of the incoming
/// segment (osnap 1, parameter 1.0), else the start of the outgoing one (osnap 13, 0.0). Only
/// straight segments are used, whose parameters are 0 and 1; a vertex between two arcs gets no
/// reference.
pub(crate) fn polyline_vertex_ref(p: &LwPolyline, index: usize) -> Option<(i64, f64, i64)> {
    let n = p.vertices.len();
    if index >= n || n < 2 {
        return None;
    }
    let segments = if p.closed { n } else { n - 1 };
    // Segment k (1-based) starts at vertex k−1; its bulge is that vertex's.
    let straight = |k: usize| k >= 1 && k <= segments && p.vertices.get(k - 1).is_some_and(|v| v.bulge == 0.0);
    let incoming = if index == 0 { if p.closed { Some(n) } else { None } } else { Some(index) };
    let outgoing = (index < segments).then_some(index + 1);
    // Vertex 0 as the start of segment 1, the others as the end of the segment before, as
    // AutoCAD writes them when it can.
    let preferred = if index == 0 {
        [outgoing.map(|k| (13, 0.0, k)), incoming.map(|k| (1, 1.0, k))]
    } else {
        [incoming.map(|k| (1, 1.0, k)), outgoing.map(|k| (13, 0.0, k))]
    };
    preferred.into_iter().flatten().find(|(_, _, k)| straight(*k)).map(|(o, t, k)| (o, t, i64::try_from(k).unwrap_or(0)))
}

/// Standard DIMASSOC point references of an associative dimension: the extension-line
/// origins of linear/aligned dimensions, both ends of both lines of an angular dimension, the
/// feature point of ordinate dimensions and the circle of a radial/diametric dimension, linked
/// to objects that exist ([`whole_object_ref`]). CADCraft's exact links travel in xdata as well.
fn std_assoc_refs(d: &Drawing, dm: &Dimension) -> (i64, Vec<OsnapRef>) {
    if matches!(dm.kind, DimKind::Radius | DimKind::Diameter) {
        return radial_ref(d, &dm.assoc);
    }
    let slots: &[&str] = match dm.kind {
        DimKind::Linear { .. } | DimKind::Aligned => &["p13", "p14"],
        DimKind::Angular => &["p13", "p14", "p15", "p16"],
        DimKind::Angular3P => &["p13", "p14", "p15"],
        // Saved as an aligned dimension (`dim_xdata` marks it): its extension origins are the arc ends.
        DimKind::ArcLength => &["p13", "p14"],
        DimKind::Ordinate { .. } => &["p13"],
        DimKind::Radius | DimKind::Diameter => &[],
    };
    let mut flags = 0;
    let mut refs = Vec::new();
    for (slot, name) in slots.iter().enumerate() {
        let Some(a) = dm.assoc.iter().find(|a| a.point == *name) else { continue };
        let Some(ent) = d.entity(a.handle) else { continue };
        let point = match *name {
            "p13" => dm.p13,
            "p14" => dm.p14,
            "p15" => dm.p15,
            "p16" => dm.p16,
            _ => continue,
        };
        if let EntityKind::LwPolyline(p) = &ent.kind {
            let index = match &a.snap {
                AssocSnap::Vertex { index } => Some(*index),
                AssocSnap::Start => Some(0),
                AssocSnap::End => p.vertices.len().checked_sub(1),
                _ => None,
            };
            let Some((osnap, param, segment)) = index.and_then(|i| polyline_vertex_ref(p, i)) else { continue };
            flags |= 1 << slot;
            refs.push(OsnapRef { osnap, main: a.handle, segment, param, point, other: None });
            continue;
        }
        if !whole_object_ref(&ent.kind) {
            continue;
        }
        let (osnap, param, other) = match &a.snap {
            // AutoCAD marks a curve's start with osnap 13 and its end with osnap 1 (group 72).
            AssocSnap::Start => (if matches!(ent.kind, EntityKind::Point(_)) { 1 } else { 13 }, 0.0, None),
            AssocSnap::End => (1, 1.0, None),
            AssocSnap::Vertex { .. } => (1, 0.0, None),
            AssocSnap::Mid => (2, 0.0, None),
            AssocSnap::Center => (if matches!(ent.kind, EntityKind::Point(_)) { 4 } else { 3 }, 0.0, None),
            AssocSnap::OnCircle { angle } => (10, *angle, None),
            AssocSnap::Intersection { other } => {
                if !d.entity(*other).is_some_and(|o| whole_object_ref(&o.kind)) {
                    continue;
                }
                (6, 0.0, Some(*other))
            }
        };
        flags |= 1 << slot;
        refs.push(OsnapRef { osnap, main: a.handle, segment: 0, param, point, other });
    }
    (flags, refs)
}

/// The single standard reference of a radial or diametric dimension: the circle or arc it
/// measures, by its centre, with associativity flag 2. AutoCAD writes radial and diametric
/// dimensions that way: the one reference names the geometry, whose radius or diameter the
/// dimension then follows. Returns no flags and no references when the association does not name
/// a circle or arc, so such a dimension is saved unassociated rather than pointing at nothing.
fn radial_ref(d: &Drawing, assoc: &[DimAssoc]) -> (i64, Vec<OsnapRef>) {
    for a in assoc {
        let Some(ent) = d.entity(a.handle) else { continue };
        let centre = match &ent.kind {
            EntityKind::Circle(c) => c.center.xy(),
            EntityKind::Arc(arc) => arc.center.xy(),
            _ => continue,
        };
        if !centre.is_finite() {
            continue;
        }
        return (2, vec![OsnapRef { osnap: 3, main: a.handle, segment: 0, param: 0.0, point: centre.to3(0.0), other: None }]);
    }
    (0, Vec::new())
}

/// Layouts with viewports but without the paper-space viewport (id 1, the sheet itself), keyed
/// by their first viewport. AutoCAD and DWG take a layout's first viewport as its paper-space
/// viewport (DWG does not store viewport ids; readers number them in order), so a layout made in
/// CADCraft, whose paper-space viewport is implicit, gets one written ahead of its viewports.
fn paper_views(d: &Drawing) -> HashMap<Handle, usize> {
    let mut out = HashMap::new();
    'layouts: for (i, l) in d.layouts.iter().enumerate() {
        let mut first = None;
        for e in l.entities.iter() {
            if let EntityKind::Viewport(v) = &e.kind {
                if v.id == 1 {
                    continue 'layouts;
                }
                first.get_or_insert(e.handle);
            }
        }
        if let Some(h) = first {
            out.insert(h, i);
        }
    }
    out
}

/// A layout's entities with its paper-space viewport (id 1) first. DWG stores no viewport ids and
/// readers number a layout's viewports in order, so the sheet must lead even when the draw order
/// (SORTENTS) put it behind a model viewport; otherwise that viewport becomes the sheet.
fn sheet_first(l: &Layout) -> Vec<&Entity> {
    let sheet = |e: &&Entity| matches!(&e.kind, EntityKind::Viewport(v) if v.id == 1);
    let all = || l.entities.iter().map(|e| e.as_ref());
    all().filter(sheet).chain(all().filter(|e| !sheet(e))).collect()
}

/// The paper-space viewport (id 1) of the layout whose first viewport is `e`, if it needs one
/// ([`paper_views`]): the whole sheet, viewed 1:1. Marked with CADCraft xdata so that our reader
/// leaves it implicit again.
fn paper_view(w: &mut W, d: &Drawing, e: &Entity, owner: &str, cx: &Ctx) {
    let Some(l) = cx.paper_views.get(&e.handle).and_then(|i| d.layouts.get(*i)) else { return };
    let size = cadcraft_render::paper::sheet(d, &l.name).map(|s| s.size).unwrap_or(Vec2::new(12.0, 9.0));
    let center = size * 0.5;
    let h = w.h();
    w.s(0, "VIEWPORT");
    w.s(5, h);
    w.s(330, owner);
    w.s(100, "AcDbEntity");
    w.i(67, 1);
    w.s(8, "0");
    w.s(100, "AcDbViewport");
    w.p(10, center.to3(0.0));
    w.f(40, size.x);
    w.f(41, size.y);
    w.i(68, 1);
    w.i(69, 1);
    w.p2(12, center);
    w.f(45, size.y);
    w.i(90, 32768);
    w.s(1001, dxf_ext::APP);
    w.s(1000, dxf_ext::PAPER_VIEW);
}

/// The VPORT table: the `*Active` model space tiles (one viewport on the extents when there is no
/// tiled configuration) and the saved configurations, each tile a record, the current one first.
fn vport_table(w: &mut W, d: &Drawing, ext: Bounds2) {
    let single = VportTile {
        min: Vec2::ZERO,
        max: Vec2::new(1.0, 1.0),
        center: if ext.is_empty() { Vec2::new(6.0, 4.5) } else { ext.center() },
        height: if ext.is_empty() { 9.0 } else { (ext.height() * 1.1).max(1e-6) },
        aspect: 1.6,
    };
    let tiled = d.vports.iter().find(|c| c.is_active() && c.tiles.len() > 1);
    let mut records: Vec<(&str, &VportTile)> = match tiled {
        Some(c) => c.tiles.iter().map(|t| (VportConfig::ACTIVE, t)).collect(),
        None => vec![(VportConfig::ACTIVE, &single)],
    };
    records.extend(d.vports.iter().filter(|c| !c.is_active() && !c.name.is_empty()).flat_map(|c| c.tiles.iter().map(|t| (c.name.as_str(), t))));
    let th = table_head(w, "VPORT", records.len());
    let finite = |p: Vec2, or: Vec2| if p.is_finite() { p } else { or };
    for (name, t) in records {
        record_head(w, "VPORT", &th, "AcDbViewportTableRecord");
        w.s(2, name);
        w.i(70, 0);
        w.p2(10, finite(t.min, Vec2::ZERO));
        w.p2(11, finite(t.max, Vec2::new(1.0, 1.0)));
        w.p2(12, finite(t.center, Vec2::ZERO));
        // Snap grid origin and rotation (SNAPBASE, SNAPANG in degrees): R13+ files keep them here.
        w.p2(13, d.header.point("SNAPBASE").map(|p| p.xy()).filter(|p| p.is_finite()).unwrap_or(Vec2::ZERO));
        w.p2(14, d.header.snap_unit());
        w.p2(15, d.header.grid_unit());
        w.p(16, Vec3::Z);
        w.p(17, Vec3::ZERO);
        w.f(40, if t.height.is_finite() && t.height > 0.0 { t.height } else { 1.0 });
        w.f(41, if t.aspect.is_finite() && t.aspect > 0.0 { t.aspect } else { 1.0 });
        w.f(42, 50.0);
        w.f(50, d.header.f64("SNAPANG", 0.0));
    }
    w.s(0, "ENDTAB");
}

fn table_head(w: &mut W, name: &str, count: usize) -> String {
    let h = w.h();
    w.s(0, "TABLE");
    w.s(2, name);
    w.s(5, h.clone());
    w.s(330, "0");
    w.s(100, "AcDbSymbolTable");
    w.i(70, count as i64);
    h
}

fn record_head(w: &mut W, kind: &str, owner: &str, subclass: &str) -> String {
    let h = w.h();
    w.s(0, kind);
    w.s(if kind == "DIMSTYLE" { 105 } else { 5 }, h.clone());
    w.s(330, owner);
    w.s(100, "AcDbSymbolTableRecord");
    w.s(100, subclass);
    h
}

/// Write a drawing as ASCII DXF.
/// The drawing as R2000 ASCII DXF.
pub fn write(d: &Drawing) -> String {
    cadcraft_dxf::write_ascii(&tags(d, Version::R2000))
}

/// The drawing's DXF tags, declaring `version` in `$ACADVER`.
pub fn tags(d: &Drawing, version: Version) -> Vec<Tag> {
    tags_for(d, version, version)
}

/// The R2000 DXF the DWG bridge converts to a `dwg` version DWG: its entity types are the DWG
/// version's (MULTILEADER from 2007).
pub(crate) fn dwg_bridge(d: &Drawing, dwg: Version) -> String {
    let mut tags = tags_for(d, Version::R2000, dwg);
    bridge_markers(&mut tags);
    cadcraft_dxf::write_ascii(&tags)
}

/// Subclass markers the DWG bridge knows by another spelling (it matches them exactly):
/// AutoCAD writes a dynamic block lookup parameter's as `AcDbBlockLookUpParameter`, the bridge
/// reads `AcDbBlockLookupParameter`; otherwise the parameter loses its name and its lookup
/// action in the DWG, and AutoCAD's lookups stop working.
fn bridge_markers(tags: &mut [Tag]) {
    for t in tags.iter_mut().filter(|t| t.code == 100) {
        if t.str().trim().eq_ignore_ascii_case("AcDbBlockLookupParameter") {
            *t = Tag::s(100, "AcDbBlockLookupParameter");
        }
    }
}

/// The drawing's DXF tags, declaring `version` in `$ACADVER`, with what `target` (the version the
/// file ends up in: `version`, or the DWG version the bridge writes) has.
fn tags_for(d: &Drawing, version: Version, target: Version) -> Vec<Tag> {
    // An in-place block edit in progress is not written: the drawing as before it.
    let committed = d.committed();
    let d: &Drawing = &committed;
    // Structural handles start above every entity handle.
    let mut w = W {
        t: Vec::new(),
        next: d.handseed.max(0x100).saturating_add(0x1000),
        pstyles: HashMap::new(),
        layers: HashMap::new(),
        reactors: hatch_reactors(d),
    };
    // Pre-allocate handles for block records.
    let paper_layouts = d.paper_space_layouts();
    let ms_br = w.h();
    // Xrefs are saved as references: without their objects and the symbols that came with them.
    let saved_view = crate::dxf_xref::saved_view(d);
    let d = saved_view.as_ref().unwrap_or(d);
    let ps_brs: Vec<(String, String, &Layout)> = paper_layouts
        .iter()
        .enumerate()
        .map(|(i, l)| (if i == 0 { "*Paper_Space".to_string() } else { format!("*Paper_Space{}", i - 1) }, String::new(), *l))
        .collect();
    let ps_brs: Vec<(String, String, &Layout)> = ps_brs.into_iter().map(|(n, _, l)| (n, w.h(), l)).collect();
    let dim_plan = dim_block_plan(d);
    let mut user_blocks: Vec<(String, String, &Block)> = d
        .blocks
        .values()
        .filter(|b| !dim_plan.skip.contains(&b.name.to_ascii_uppercase()))
        .map(|b| (b.name.clone(), String::new(), b.as_ref()))
        .collect();
    user_blocks.iter_mut().for_each(|b| b.1 = format!("{:X}", 0));
    let user_blocks: Vec<(String, String, &Block)> = user_blocks.into_iter().map(|(n, _, b)| (n, w.h(), b)).collect();
    let mut cx = Ctx { paper_views: paper_views(d), target, ..Ctx::default() };
    cx.block_records = user_blocks.iter().map(|(n, h, _)| (n.to_ascii_uppercase(), h.clone())).collect();
    // Dimension blocks.
    let mut dim_defs: Vec<(String, String, Vec<Entity>)> = Vec::new(); // filled below
    let mut n = 1;
    let mut reused_handles = std::collections::HashSet::new();
    let all_spaces: Vec<&EntityStore> = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).collect();
    for st in &all_spaces {
        for e in st.iter() {
            if let EntityKind::Dimension(dm) = &e.kind {
                let reused = dim_plan.reuse.get(&e.handle);
                let ents: Vec<Entity>;
                let name = match reused {
                    // A dimension still linked to the block it was read with hasn't changed since
                    // (edits drop the link, and the screen draws that block): the block is written
                    // as read, so other programs show it as they drew it.
                    Some(b) if !b.entities.is_empty() => {
                        ents = b.entities.iter().map(|x| x.as_ref().clone()).collect();
                        reused_handles.extend(ents.iter().map(|x| x.handle).filter(|h| h.0 != 0));
                        b.name.clone()
                    }
                    Some(b) => {
                        let mut fresh = dim_block_entities(d, dm, &e.common.layer);
                        reuse_handles(&mut fresh, b, &mut reused_handles);
                        ents = fresh;
                        b.name.clone()
                    }
                    None => {
                        ents = dim_block_entities(d, dm, &e.common.layer);
                        let mut name = format!("*D{n}");
                        while d.block(&name).is_some() {
                            n += 1;
                            name = format!("*D{n}");
                        }
                        n += 1;
                        name
                    }
                };
                let brh = w.h();
                cx.dim_blocks.insert(e.handle, name.clone());
                dim_defs.push((name, brh, ents));
            }
        }
    }
    // Every entity, including block contents (for arrows, tables and overrides).
    let every: Vec<&Entity> = all_spaces
        .iter()
        .flat_map(|s| s.iter().map(|e| e.as_ref()))
        .chain(d.blocks.values().flat_map(|b| b.entities.iter().map(|e| e.as_ref())))
        .collect();
    // Arrowhead blocks: user blocks by name, otherwise generated from our own geometry.
    let mut arrow_names: Vec<String> = Vec::new();
    for s in &d.dim_styles {
        arrow_names.extend([s.arrow_block.clone(), s.arrow_block1.clone(), s.arrow_block2.clone(), s.leader_arrow_block.clone()]);
    }
    for e in &every {
        if let EntityKind::Dimension(dm) = &e.kind {
            for (k, v) in &dm.overrides {
                let Some(f) = DimStyle::field_name(k).filter(|f| f.starts_with("arrowBlock") || *f == "leaderArrowBlock") else { continue };
                if let Some(s) = dxf_ext::canonical(f, v).as_ref().and_then(|v| v.as_str()) {
                    arrow_names.push(s.to_string());
                }
            }
        }
    }
    let mut arrow_defs: Vec<(String, String, Vec<Entity>)> = Vec::new();
    for name in arrow_names {
        let key = name.trim().to_ascii_uppercase();
        if key.is_empty() || cx.arrows.contains_key(&key) {
            continue;
        }
        if let Some((_, h, _)) = user_blocks.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(name.trim())) {
            cx.arrows.insert(key, h.clone());
            continue;
        }
        let Some((bname, kind)) = dxf_ext::arrow_block_name(&name) else { continue };
        let existing = arrow_defs.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(&bname)).map(|(_, h, _)| h.clone());
        let existing = existing.or_else(|| user_blocks.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(&bname)).map(|(_, h, _)| h.clone()));
        let h = match existing {
            Some(h) => h,
            None => {
                let h = w.h();
                arrow_defs.push((bname, h.clone(), arrow_block_entities(kind)));
                h
            }
        };
        cx.arrows.insert(key, h);
    }
    // Table blocks (`*T`).
    let mut table_defs: Vec<(String, String, Vec<Entity>)> = Vec::new();
    let mut tn = 1;
    for e in &every {
        if let EntityKind::Table(t) = &e.kind {
            if cx.tables.contains_key(&e.handle) {
                continue;
            }
            let mut name = format!("*T{tn}");
            while d.block(&name).is_some() {
                tn += 1;
                name = format!("*T{tn}");
            }
            tn += 1;
            let brh = w.h();
            cx.tables.insert(e.handle, (name.clone(), brh.clone()));
            table_defs.push((name, brh, table_block_entities(d, t, &e.common.layer)));
        }
    }
    // Image definitions (IMAGEDEF objects) of raster images anywhere in the drawing.
    cx.images = crate::dxf_image::plan(every.iter().copied(), || w.h());
    cx.mlines = crate::dxf_mline::plan(d, every.iter().copied(), || w.h());
    // DIMASSOC objects of associative dimensions in model and paper space.
    for st in &all_spaces {
        for e in st.iter() {
            if let EntityKind::Dimension(dm) = &e.kind {
                let (flags, refs) = std_assoc_refs(d, dm);
                if !refs.is_empty() {
                    cx.assoc.insert(e.handle, AssocObj { xdict: w.h(), handle: w.h(), flags, refs });
                }
            }
        }
    }
    let default_table_style = [TableStyle::default()];
    let table_styles: &[TableStyle] = if d.table_styles.is_empty() { &default_table_style } else { &d.table_styles };
    cx.table_styles = table_styles.iter().map(|s| (s.name.clone(), w.h())).collect();
    let default_mleader_style = [MLeaderStyle::default()];
    let mleader_styles: &[MLeaderStyle] = if d.mleader_styles.is_empty() { &default_mleader_style } else { &d.mleader_styles };
    cx.mleader_styles = mleader_styles.iter().map(|s| (s.name.clone(), w.h())).collect();
    let root_dict = w.h();
    let group_dict = w.h();
    let layout_dict = w.h();
    let table_style_dict = w.h();
    // CADCraft data in XRECORDs of the named object dictionary: (key, handle, text chunks).
    let mut xrecords: Vec<(&str, String, Vec<String>)> = Vec::new();
    // The drawing's own next handle: table records and objects are numbered above it, so
    // reading it back (instead of $HANDSEED) keeps their handles across saves (#495).
    xrecords.push((dxf_ext::HANDSEED_KEY, w.h(), vec![format!("{:X}", d.handseed)]));
    if let Some(name) = d.header.get("CMLEADERSTYLE").and_then(HVal::as_str) {
        xrecords.push((dxf_ext::MLEADER_STYLE_KEY, w.h(), vec![name.to_string()]));
    }
    if let Some(chunks) = dxf_ext::constraint_chunks(&d.constraints, &d.parametric) {
        xrecords.push((dxf_ext::CONSTRAINTS_KEY, w.h(), chunks));
    }
    if let Some(chunks) = dxf_ext::layer_state_chunks(&d.layer_states) {
        xrecords.push((dxf_ext::LAYER_STATES_KEY, w.h(), chunks));
    }
    if let Some(chunks) = dxf_ext::plot_style_chunks(&d.plot_style_tables) {
        xrecords.push((dxf_ext::PLOT_STYLES_KEY, w.h(), chunks));
    }
    if let Some(chunks) = dxf_ext::anno_chunks(d) {
        xrecords.push((dxf_ext::ANNOTATION_KEY, w.h(), chunks));
    }
    // The annotation scale list: the ACAD_SCALELIST dictionary of SCALE objects.
    let scale_dict = w.h();
    let scales: Vec<(&cadcraft_doc::AnnoScale, String)> = d.scales.iter().filter(|s| s.is_valid()).map(|s| (s, w.h())).collect();
    // Plot style names used by layers and objects (Normal always), each a placeholder object.
    let pstyle_dict = w.h();
    let mut pstyle_names: Vec<String> = vec![cadcraft_doc::NORMAL_STYLE.to_string()];
    let named = d.layers.iter().map(|l| l.plot_style.as_str()).chain(every.iter().map(|e| e.common.plot_style.as_str()));
    for n in named {
        let n = n.trim();
        let logical = n.is_empty() || n.eq_ignore_ascii_case("bylayer") || n.eq_ignore_ascii_case("byblock");
        if !logical && pstyle_names.len() < dxf_ext::MAX_PLOT_STYLE_NAMES && !pstyle_names.iter().any(|x| x.eq_ignore_ascii_case(n)) {
            pstyle_names.push(n.to_string());
        }
    }
    let pstyles: Vec<(String, String)> = pstyle_names.into_iter().map(|n| (n, w.h())).collect();
    w.pstyles = pstyles.iter().map(|(n, h)| (n.to_ascii_uppercase(), h.clone())).collect();
    let model_layout = w.h();
    let layout_handles: Vec<String> = ps_brs.iter().map(|_| w.h()).collect();

    // ---------------- HEADER ----------------
    w.s(0, "SECTION");
    w.s(2, "HEADER");
    w.s(9, "$ACADVER");
    w.s(1, version.code());
    w.s(9, "$DWGCODEPAGE");
    w.s(3, code_page(d, version));
    let ext = d.extents(&Space::Model);
    if !ext.is_empty() {
        w.s(9, "$EXTMIN");
        w.p(10, ext.min.to3(0.0));
        w.s(9, "$EXTMAX");
        w.p(10, ext.max.to3(0.0));
    }
    header_vars(&mut w, d);
    let seed_pos = w.t.len();
    w.s(9, "$HANDSEED");
    w.s(5, "0");
    w.s(0, "ENDSEC");

    // ---------------- CLASSES ----------------
    w.s(0, "SECTION");
    w.s(2, "CLASSES");
    let mut classes = vec![
        ("TABLESTYLE", "AcDbTableStyle", 4095, false),
        ("MLEADERSTYLE", "AcDbMLeaderStyle", 4095, false),
        ("ACDBDICTIONARYWDFLT", "AcDbDictionaryWithDefault", 0, false),
        ("ACDBPLACEHOLDER", "AcDbPlaceHolder", 0, false),
    ];
    if !cx.tables.is_empty() {
        classes.push(("ACAD_TABLE", "AcDbTable", 1025, true));
    }
    if every.iter().any(|e| matches!(e.kind, EntityKind::Helix(_))) {
        classes.push(("HELIX", "AcDbHelix", 4095, true));
    }
    if !cx.assoc.is_empty() {
        classes.push(("DIMASSOC", "AcDbDimAssoc", 0, false));
    }
    if cx.native_mleader() && every.iter().any(|e| matches!(e.kind, EntityKind::MLeader(_))) {
        classes.push(("MULTILEADER", "AcDbMLeader", 1025, true));
    }
    if every.iter().any(|e| matches!(e.kind, EntityKind::Wipeout(_))) {
        classes.push(("WIPEOUT", "AcDbWipeout", 127, true));
    }
    if !cx.images.is_empty() {
        classes.push(("IMAGE", "AcDbRasterImage", 127, true));
        classes.push(("IMAGEDEF", "AcDbRasterImageDef", 0, false));
        classes.push(("IMAGEDEF_REACTOR", "AcDbRasterImageDefReactor", 1, false));
        classes.push(("RASTERVARIABLES", "AcDbRasterVariables", 0, false));
    }
    classes.push(("SCALE", "AcDbScale", 0, false));
    for (dxf_name, cpp, proxy, is_entity) in classes {
        w.s(0, "CLASS");
        w.s(1, dxf_name);
        w.s(2, cpp);
        w.s(3, "ObjectDBX Classes");
        w.i(90, proxy);
        w.i(280, 0);
        w.i(281, i64::from(is_entity));
    }
    w.s(0, "ENDSEC");

    // ---------------- TABLES ----------------
    w.s(0, "SECTION");
    w.s(2, "TABLES");
    vport_table(&mut w, d, ext);
    // LTYPE. Embedded text and shapes point at STYLE records (340), which get their handles
    // in the STYLE table below: (tag position, style name, is a shape) are filled in there.
    let mut ltype_handles: HashMap<String, String> = HashMap::new();
    let mut ltype_style_refs: Vec<(usize, Option<String>, bool)> = Vec::new();
    let th = table_head(&mut w, "LTYPE", d.linetypes.len());
    for lt in &d.linetypes {
        let h = record_head(&mut w, "LTYPE", &th, "AcDbLinetypeTableRecord");
        ltype_handles.entry(lt.name.to_ascii_uppercase()).or_insert(h.clone());
        cx.ltypes.entry(lt.name.to_ascii_uppercase()).or_insert(h);
        w.s(2, &lt.name);
        w.i(70, if crate::dxf_xref::dependent(&lt.name) { 48 } else { 0 });
        w.s(3, &lt.description);
        w.i(72, 65);
        w.i(73, lt.pattern.len() as i64);
        w.f(40, lt.pattern_length());
        for el in &lt.pattern {
            w.f(49, el.length);
            // 74: 2 = text, 4 = shape, + 1 when the rotation is absolute (DXF Reference, LTYPE).
            let (kind, shape) = match (&el.text, el.shape) {
                (Some(_), _) => (2, 0),
                (None, Some(n)) => (4, n),
                (None, None) => (0, 0),
            };
            if kind == 0 {
                w.i(74, 0);
                continue;
            }
            w.i(74, kind | i64::from(el.absolute));
            w.i(75, i64::from(shape));
            ltype_style_refs.push((w.t.len(), el.style.clone(), kind == 4));
            w.s(340, "0");
            w.f(46, el.scale);
            w.f(50, el.rotation);
            w.f(44, el.offset.x);
            w.f(45, el.offset.y);
            if let Some(t) = &el.text {
                w.s(9, t.clone());
            }
        }
    }
    w.s(0, "ENDTAB");
    // LAYER
    let th = table_head(&mut w, "LAYER", d.layers.len());
    for l in &d.layers {
        let lh = record_head(&mut w, "LAYER", &th, "AcDbLayerTableRecord");
        w.layers.insert(l.name.to_ascii_uppercase(), lh);
        w.s(2, &l.name);
        // 16 | 32: an xref's layer (`xref|name`), externally dependent and resolved.
        let xref = if crate::dxf_xref::dependent(&l.name) { 48 } else { 0 };
        w.i(70, i64::from(l.frozen) | if l.vp_freeze_new { 2 } else { 0 } | if l.locked { 4 } else { 0 } | xref);
        let aci = match l.color {
            Color::True(rgb) => i64::from(cadcraft_color::nearest_aci(rgb)),
            c => i64::from(c.to_aci()).clamp(1, 255),
        };
        w.i(62, if l.on { aci } else { -aci });
        if let Color::True(rgb) = l.color {
            w.i(420, i64::from(rgb.to_u32()));
        }
        w.s(6, &l.linetype);
        if !l.plot {
            w.i(290, 0);
        }
        w.i(370, i64::from(l.lineweight.to_dxf()));
        let normal = w.pstyles.get(&cadcraft_doc::NORMAL_STYLE.to_ascii_uppercase()).cloned();
        if let Some(h) = w.pstyles.get(&l.plot_style.trim().to_ascii_uppercase()).cloned().or(normal) {
            w.s(390, h);
        }
        if l.transparency > 0 {
            w.s(1001, dxf_ext::LAYER_TRANSPARENCY_APP);
            w.i(1071, dxf_ext::transparency_to_dxf(l.transparency));
        }
        if !l.description.is_empty() {
            // The first string is the layer standard's name, unused here.
            w.s(1001, dxf_ext::LAYER_DESCRIPTION_APP);
            w.s(1000, "");
            w.s(1000, dxf_ext::xdata_str(&l.description));
        }
    }
    w.s(0, "ENDTAB");
    // STYLE. Linetype shapes from a shape file that isn't a text style get an unnamed shape
    // style record (70 bit 1) naming the file.
    let mut shape_files: Vec<String> = Vec::new();
    for (_, style, shape) in &ltype_style_refs {
        let Some(f) = style.as_deref().map(str::trim).filter(|f| *shape && !f.is_empty() && d.text_style(f).is_none()) else { continue };
        if !shape_files.iter().any(|x| x.eq_ignore_ascii_case(f)) {
            shape_files.push(f.to_string());
        }
    }
    let th = table_head(&mut w, "STYLE", d.text_styles.len() + shape_files.len());
    for s in &d.text_styles {
        let h = record_head(&mut w, "STYLE", &th, "AcDbTextStyleTableRecord");
        cx.styles.entry(s.name.to_ascii_uppercase()).or_insert(h);
        w.s(2, &s.name);
        // 70 bit 4: vertical; 71 bits 2/4: backwards / upside down.
        w.i(70, if s.vertical { 4 } else { 0 });
        w.f(40, s.height);
        w.f(41, s.width_factor);
        w.f(50, s.oblique.to_degrees());
        w.i(71, if s.backwards { 2 } else { 0 } | if s.upside_down { 4 } else { 0 });
        w.f(42, 0.2);
        // The built-in font goes out as `txt`, which every reader has; CADCraft's own name
        // travels in `CADCRAFT` xdata (`1000 FONT`, name) so the style reopens with it.
        let file = if s.font == cadcraft_fonts_name() { "txt" } else { s.font.as_str() };
        w.s(3, file);
        w.s(4, &s.big_font);
        if s.annotative {
            annotative_xdata(&mut w);
        }
        if file != s.font {
            w.s(1001, dxf_ext::APP);
            w.s(1000, "FONT");
            w.s(1000, dxf_ext::xdata_str(&s.font));
        }
    }
    let mut shape_handles: HashMap<String, String> = HashMap::new();
    for f in &shape_files {
        let h = record_head(&mut w, "STYLE", &th, "AcDbTextStyleTableRecord");
        shape_handles.insert(f.to_ascii_uppercase(), h);
        w.s(2, "");
        w.i(70, 1);
        w.f(40, 0.0);
        w.f(41, 1.0);
        w.f(50, 0.0);
        w.i(71, 0);
        w.f(42, 0.2);
        w.s(3, f.clone());
        w.s(4, "");
    }
    w.s(0, "ENDTAB");
    // Linetype text and shapes without a known style use Standard (else the first style).
    let fallback_style = cx.style("Standard").or_else(|| d.text_styles.first().and_then(|s| cx.style(&s.name))).unwrap_or_else(|| "0".into());
    for (pos, style, shape) in ltype_style_refs {
        let name = style.as_deref().map(str::trim).unwrap_or_default();
        let h = shape.then(|| shape_handles.get(&name.to_ascii_uppercase()).cloned()).flatten().or_else(|| cx.style(name));
        if let Some(t) = w.t.get_mut(pos) {
            *t = Tag::s(340, h.unwrap_or_else(|| fallback_style.clone()));
        }
    }
    // VIEW: named views (DXF Reference, VIEW). A view's layer state is CADCraft xdata.
    let th = table_head(&mut w, "VIEW", d.views.len());
    for v in &d.views {
        record_head(&mut w, "VIEW", &th, "AcDbViewTableRecord");
        w.s(2, &v.name);
        w.i(70, i64::from(v.layout.is_some()));
        w.f(40, v.height);
        w.p2(10, v.center);
        w.f(41, v.width);
        w.p(11, Vec3::Z);
        w.p(12, Vec3::ZERO);
        w.f(42, 50.0);
        w.f(43, 0.0);
        w.f(44, 0.0);
        w.f(50, if v.twist.is_finite() { v.twist.to_degrees() } else { 0.0 });
        w.i(71, 0);
        w.i(281, 0);
        w.i(72, 0);
        let xdata = [("LAYERSTATE", v.layer_state.as_deref()), ("LAYOUT", v.layout.as_deref().filter(|l| !l.is_empty()))];
        if xdata.iter().any(|(_, x)| x.is_some()) {
            w.s(1001, dxf_ext::APP);
            for (key, value) in xdata {
                if let Some(value) = value {
                    w.s(1000, key);
                    w.s(1000, dxf_ext::xdata_str(value));
                }
            }
        }
    }
    w.s(0, "ENDTAB");
    // UCS: named user coordinate systems (DXF Reference, UCS).
    let th = table_head(&mut w, "UCS", d.ucss.len());
    for u in &d.ucss {
        record_head(&mut w, "UCS", &th, "AcDbUCSTableRecord");
        w.s(2, &u.name);
        w.i(70, 0);
        w.p(10, u.origin);
        w.p(11, u.x_axis);
        w.p(12, u.y_axis);
        w.i(79, 0);
        w.f(146, 0.0);
    }
    w.s(0, "ENDTAB");
    let apps = ["ACAD", dxf_ext::APP, "AcadAnnotative", dxf_ext::LAYER_TRANSPARENCY_APP, dxf_ext::LAYER_DESCRIPTION_APP];
    let th = table_head(&mut w, "APPID", apps.len());
    for app in apps {
        record_head(&mut w, "APPID", &th, "AcDbRegAppTableRecord");
        w.s(2, app);
        w.i(70, 0);
    }
    w.s(0, "ENDTAB");
    // DIMSTYLE
    let th = {
        let h = w.h();
        w.s(0, "TABLE");
        w.s(2, "DIMSTYLE");
        w.s(5, h.clone());
        w.s(330, "0");
        w.s(100, "AcDbSymbolTable");
        w.i(70, d.dim_styles.len() as i64);
        w.s(100, "AcDbDimStyleTable");
        w.i(71, 0);
        h
    };
    for s in &d.dim_styles {
        record_head(&mut w, "DIMSTYLE", &th, "AcDbDimStyleTableRecord");
        w.s(2, &s.name);
        w.i(70, 0);
        let v = serde_json::to_value(s).unwrap_or_default();
        let defaults = serde_json::to_value(DimStyle::default()).unwrap_or_default();
        let mut newer = W { t: Vec::new(), next: 0, pstyles: HashMap::new(), layers: HashMap::new(), reactors: HashMap::new() };
        for (field, code, kind) in dxf_ext::DIM_CODES {
            let Some(enc) = v.get(*field).and_then(|x| cx.encode(*kind, x)) else { continue };
            // DIMPOST is written even when empty: a reader that defaults a missing one to "<>"
            // (the DWG bridge's) would otherwise give radius and diameter dimensions no "R" or
            // "Ø" in AutoCAD (#1053).
            if *code == dxf_ext::DIMPOST
                && let DimVal::Str(t) = &enc
                && t.is_empty()
            {
                w.s(*code, "");
                continue;
            }
            if !dxf_ext::NEWER_CODES.contains(code) {
                w.dimval(*code, enc, false);
                continue;
            }
            // Table groups where the file version has them, else xdata.
            if version >= dxf_ext::NEWER_SINCE {
                w.dimval(*code, enc, false);
            } else if v.get(*field) != defaults.get(*field) {
                newer.i(1070, i64::from(*code));
                newer.dimval(*code, enc, true);
            }
        }
        if !s.arrow_block1.trim().is_empty() || !s.arrow_block2.trim().is_empty() {
            w.i(dxf_ext::DIMSAH, 1);
        }
        if s.annotative {
            annotative_xdata(&mut w);
        }
        if !newer.t.is_empty() {
            w.s(1001, dxf_ext::APP);
            w.s(1000, dxf_ext::DIMVARS_LIST);
            w.s(1002, "{");
            w.xdata(newer.t);
            w.s(1002, "}");
        }
    }
    w.s(0, "ENDTAB");
    // BLOCK_RECORD
    let th = table_head(&mut w, "BLOCK_RECORD", 1 + ps_brs.len() + user_blocks.len() + dim_defs.len() + arrow_defs.len() + table_defs.len());
    let br_rec = |w: &mut W, h: &str, name: &str, layout: Option<&str>, blk: Option<&Block>| {
        w.s(0, "BLOCK_RECORD");
        w.s(5, h);
        w.s(330, th.clone());
        w.s(100, "AcDbSymbolTableRecord");
        w.s(100, "AcDbBlockTableRecord");
        w.s(2, name);
        w.s(340, layout.unwrap_or("0"));
        // User blocks: insertion units (70) and explodability (280).
        if let Some(b) = blk {
            w.i(70, i64::from(b.units));
            w.i(280, i64::from(b.explodable));
            if let Some(a) = &b.array {
                w.s(1001, dxf_ext::APP);
                w.xdata(dxf_ext::array_xdata(a));
            }
        }
    };
    br_rec(&mut w, &ms_br, "*Model_Space", Some(&model_layout), None);
    for (i, (n, h, _)) in ps_brs.iter().enumerate() {
        br_rec(&mut w, h, n, layout_handles.get(i).map(String::as_str), None);
    }
    for (n, h, b) in &user_blocks {
        br_rec(&mut w, h, n, None, Some(b));
    }
    for (n, h, _) in dim_defs.iter().chain(&arrow_defs).chain(&table_defs) {
        br_rec(&mut w, h, n, None, None);
    }
    w.s(0, "ENDTAB");
    w.s(0, "ENDSEC");

    // ---------------- BLOCKS ----------------
    w.s(0, "SECTION");
    w.s(2, "BLOCKS");
    let block = |w: &mut W, name: &str, desc: &str, brh: &str, base: Vec3, flags: i64, ents: &mut dyn Iterator<Item = &Entity>, paper: bool| {
        let bh = w.h();
        w.s(0, "BLOCK");
        w.s(5, bh);
        w.s(330, brh);
        w.s(100, "AcDbEntity");
        if paper {
            w.i(67, 1);
        }
        w.s(8, "0");
        w.s(100, "AcDbBlockBegin");
        w.s(2, name);
        w.i(70, flags);
        w.p(10, base);
        w.s(3, name);
        w.s(1, "");
        if !desc.is_empty() {
            w.s(4, desc);
        }
        for e in ents {
            entity(w, d, e, brh, false, &cx);
        }
        let eh = w.h();
        w.s(0, "ENDBLK");
        w.s(5, eh);
        w.s(330, brh);
        w.s(100, "AcDbEntity");
        if paper {
            w.i(67, 1);
        }
        w.s(8, "0");
        w.s(100, "AcDbBlockEnd");
    };
    block(&mut w, "*Model_Space", "", &ms_br, Vec3::ZERO, 0, &mut std::iter::empty(), false);
    for (i, (n, h, l)) in ps_brs.iter().enumerate() {
        if i == 0 {
            block(&mut w, n, "", h, Vec3::ZERO, 0, &mut std::iter::empty(), true);
        } else {
            block(&mut w, n, "", h, Vec3::ZERO, 0, &mut sheet_first(l).into_iter(), true);
        }
    }
    for (n, h, b) in &user_blocks {
        block(&mut w, n, &b.description, h, b.base, if b.anonymous { 1 } else { 0 }, &mut b.entities.iter().map(|e| e.as_ref()), false);
    }
    // Generated geometry entities (dimension, arrowhead and table blocks) need handles.
    for (_, _, ents) in dim_defs.iter_mut().chain(arrow_defs.iter_mut()).chain(table_defs.iter_mut()) {
        for e in ents.iter_mut().filter(|e| e.handle.0 == 0) {
            e.handle = Handle(w.next);
            w.next = w.next.saturating_add(1);
        }
    }
    for (n, h, ents) in &dim_defs {
        block(&mut w, n, "", h, Vec3::ZERO, 1, &mut ents.iter(), false);
    }
    for (n, h, ents) in &arrow_defs {
        block(&mut w, n, "", h, Vec3::ZERO, 0, &mut ents.iter(), false);
    }
    for (n, h, ents) in &table_defs {
        block(&mut w, n, "", h, Vec3::ZERO, 1, &mut ents.iter(), false);
    }
    w.s(0, "ENDSEC");

    // ---------------- ENTITIES ----------------
    w.s(0, "SECTION");
    w.s(2, "ENTITIES");
    for e in d.model.iter() {
        entity(&mut w, d, e, &ms_br, false, &cx);
    }
    if let Some((_, h, l)) = ps_brs.first() {
        for e in sheet_first(l) {
            entity(&mut w, d, e, h, true, &cx);
        }
    }
    w.s(0, "ENDSEC");

    // ---------------- OBJECTS ----------------
    // Groups (members that still exist) and multileader styles.
    let group_owner = group_dict.clone();
    let written: std::collections::HashSet<String> = w.t.iter().filter(|t| t.code == 5).map(Tag::str).collect();
    let groups: Vec<(&Group, String)> = d.groups.iter().filter(|g| !g.name.is_empty()).map(|g| (g, w.h())).collect();
    let mleader_dict = w.h();
    let mleader_handles: Vec<String> = cx.mleader_styles.iter().map(|(_, h)| h.clone()).collect();
    w.s(0, "SECTION");
    w.s(2, "OBJECTS");
    w.s(0, "DICTIONARY");
    w.s(5, root_dict.clone());
    w.s(330, "0");
    w.s(100, "AcDbDictionary");
    w.i(281, 1);
    w.s(3, "ACAD_GROUP");
    w.s(350, group_dict.clone());
    if !cx.images.is_empty() {
        w.s(3, "ACAD_IMAGE_DICT");
        w.s(350, cx.images.dict.clone());
        w.s(3, "ACAD_IMAGE_VARS");
        w.s(350, cx.images.vars.clone());
    }
    w.s(3, "ACAD_LAYOUT");
    w.s(350, layout_dict.clone());
    w.s(3, "ACAD_MLEADERSTYLE");
    w.s(350, mleader_dict.clone());
    w.s(3, "ACAD_MLINESTYLE");
    w.s(350, cx.mlines.dict.clone());
    w.s(3, dxf_ext::PLOT_STYLE_NAMES_KEY);
    w.s(350, pstyle_dict.clone());
    w.s(3, "ACAD_TABLESTYLE");
    w.s(350, table_style_dict.clone());
    for (key, h, _) in &xrecords {
        w.s(3, *key);
        w.s(350, h);
    }
    w.s(3, "ACAD_SCALELIST");
    w.s(350, scale_dict.clone());
    w.s(0, "DICTIONARY");
    w.s(5, group_dict);
    w.s(330, root_dict.clone());
    w.s(100, "AcDbDictionary");
    w.i(281, 1);
    for (g, h) in &groups {
        w.s(3, &g.name);
        w.s(350, h);
    }
    for (g, h) in &groups {
        group_obj(&mut w, g, h, &group_owner, &written);
    }
    // Multileader styles.
    w.s(0, "DICTIONARY");
    w.s(5, mleader_dict.clone());
    w.s(330, root_dict.clone());
    w.s(100, "AcDbDictionary");
    w.i(281, 1);
    for (s, h) in mleader_styles.iter().zip(&mleader_handles) {
        w.s(3, &s.name);
        w.s(350, h);
    }
    let byblock_ltype = ltype_handles.get("BYBLOCK").cloned();
    for (s, h) in mleader_styles.iter().zip(&mleader_handles) {
        mleader_style_obj(&mut w, s, h, &mleader_dict, byblock_ltype.as_deref(), cx.style(&s.text_style));
    }
    // Multiline styles.
    w.t.extend(crate::dxf_mline::objects(&cx.mlines, &root_dict));
    // Table styles.
    w.s(0, "DICTIONARY");
    w.s(5, table_style_dict.clone());
    w.s(330, root_dict.clone());
    w.s(100, "AcDbDictionary");
    w.i(281, 1);
    for (name, h) in &cx.table_styles {
        w.s(3, name);
        w.s(350, h);
    }
    for (s, (_, h)) in table_styles.iter().zip(&cx.table_styles) {
        table_style_obj(&mut w, d, s, h, &table_style_dict);
    }
    // Plot style names: a dictionary with a default (Normal) of placeholder objects.
    w.s(0, "ACDBDICTIONARYWDFLT");
    w.s(5, pstyle_dict.clone());
    w.group("ACAD_REACTORS", 330, &[&root_dict]);
    w.s(330, root_dict.clone());
    w.s(100, "AcDbDictionary");
    w.i(281, 1);
    for (n, h) in &pstyles {
        w.s(3, n);
        w.s(350, h);
    }
    w.s(100, "AcDbDictionaryWithDefault");
    w.s(340, pstyles.first().map(|(_, h)| h.clone()).unwrap_or_default());
    for (_, h) in &pstyles {
        w.s(0, "ACDBPLACEHOLDER");
        w.s(5, h);
        w.group("ACAD_REACTORS", 330, &[&pstyle_dict]);
        w.s(330, pstyle_dict.clone());
    }
    // Parametric constraints and parameters, saved layer states, plot style tables (CADCraft
    // data).
    for (_, h, chunks) in &xrecords {
        w.s(0, "XRECORD");
        w.s(5, h);
        w.group("ACAD_REACTORS", 330, &[&root_dict]);
        w.s(330, root_dict.clone());
        w.s(100, "AcDbXrecord");
        w.i(280, 1);
        for c in chunks {
            w.s(1, c.clone());
        }
    }
    scale_list(&mut w, &scales, &scale_dict, &root_dict);
    // Dimension associativity: extension dictionary + DIMASSOC per dimension.
    let mut assoc: Vec<(&Handle, &AssocObj)> = cx.assoc.iter().collect();
    assoc.sort_by_key(|(h, _)| **h);
    for (dim, a) in assoc {
        let dh = dim.hex();
        w.s(0, "DICTIONARY");
        w.s(5, a.xdict.clone());
        w.group("ACAD_REACTORS", 330, &[&dh]);
        w.s(330, dh.clone());
        w.s(100, "AcDbDictionary");
        w.i(280, 1);
        w.i(281, 1);
        w.s(3, "ACAD_DIMASSOC");
        w.s(360, a.handle.clone());
        w.s(0, "DIMASSOC");
        w.s(5, a.handle.clone());
        w.group("ACAD_REACTORS", 330, &[&a.xdict]);
        w.s(330, a.xdict.clone());
        w.s(100, "AcDbDimAssoc");
        w.s(330, dh);
        w.i(90, a.flags);
        w.i(70, 0);
        w.i(71, 0);
        for r in &a.refs {
            w.s(1, "AcDbOsnapPointRef");
            w.i(72, r.osnap);
            w.s(331, r.main.hex());
            w.i(73, if r.segment > 0 { 2 } else { 0 });
            w.i(91, r.segment);
            w.s(301, "");
            w.f(40, r.param);
            w.p(10, r.point);
            if let Some(o) = r.other {
                w.s(332, o.hex());
                w.i(74, 0);
                w.i(92, 0);
                w.s(302, "");
            }
            w.i(75, 0);
        }
    }
    w.s(0, "DICTIONARY");
    w.s(5, layout_dict.clone());
    w.s(330, root_dict.clone());
    w.s(100, "AcDbDictionary");
    w.i(281, 1);
    w.s(3, "Model");
    w.s(350, model_layout.clone());
    for (i, (_, _, l)) in ps_brs.iter().enumerate() {
        w.s(3, &l.name);
        w.s(350, layout_handles.get(i).cloned().unwrap_or_default());
    }
    let layout_obj = |w: &mut W, h: &str, name: &str, tab: i64, brh: &str, flags: i64, page: &PageSetup, view: Option<(Vec2, f64)>| {
        // Plot scale: a standard scale when fitting or 1:1, otherwise custom (142 paper units
        // per 143 drawing units).
        let scale = if page.scale.is_finite() && page.scale > 0.0 { page.scale } else { 1.0 };
        let standard = page.scale_to_fit || scale == 1.0;
        let plot_flags = 512
            | 32
            | if standard { 16 } else { 0 }
            | if page.lineweights { 128 } else { 0 }
            | if page.center { 4 } else { 0 }
            | if page.show_plot_styles { 2 } else { 0 };
        let plot_type = match page.plot_area.as_str() {
            "display" => 0,
            "extents" => 1,
            "limits" => 2,
            "window" => 4,
            _ => 5,
        };
        w.s(0, "LAYOUT");
        w.s(5, h);
        w.s(330, layout_dict.clone());
        w.s(100, "AcDbPlotSettings");
        w.s(1, "");
        let device = page.device.trim();
        w.s(2, if device.is_empty() || device.eq_ignore_ascii_case("None") { "none_device" } else { device });
        w.s(4, page.paper.replace(' ', "_"));
        w.s(6, "");
        for (c, v) in [
            (40, page.margins_mm[0]),
            (41, page.margins_mm[1]),
            (42, page.margins_mm[2]),
            (43, page.margins_mm[3]),
            (44, page.width_mm),
            (45, page.height_mm),
            (46, 0.0),
            (47, 0.0),
            (48, page.plot_window[0]),
            (49, page.plot_window[1]),
            (140, page.plot_window[2]),
            (141, page.plot_window[3]),
            (142, scale),
            (143, 1.0),
        ] {
            w.f(c, v);
        }
        w.i(70, plot_flags);
        // Plot paper units: 1 millimetres, 0 inches (#1058).
        w.i(72, i64::from(cadcraft_render::paper::page_unit_mm(d, page) == 1.0));
        w.i(73, i64::from(page.landscape));
        w.i(74, plot_type);
        w.s(7, &page.plot_style_table);
        w.i(75, if page.scale_to_fit { 0 } else { 16 });
        w.f(147, scale);
        w.f(148, 0.0);
        w.f(149, 0.0);
        w.s(100, "AcDbLayout");
        w.s(1, name);
        w.i(70, flags);
        w.i(71, tab);
        w.p2(10, Vec2::ZERO);
        w.p2(11, Vec2::new(12.0, 9.0));
        w.p(12, Vec3::ZERO);
        w.p(14, Vec3::ZERO);
        w.p(15, Vec3::ZERO);
        w.f(146, 0.0);
        w.p(13, Vec3::ZERO);
        w.p(16, Vec3::new(1.0, 0.0, 0.0));
        w.p(17, Vec3::new(0.0, 1.0, 0.0));
        w.i(76, 0);
        w.s(330, brh);
        // The saved paper-space view (centre, height) as CADCraft xdata.
        if let Some((c, height)) = view.filter(|(c, v)| c.x.is_finite() && c.y.is_finite() && v.is_finite()) {
            w.s(1001, dxf_ext::APP);
            w.s(1000, "PSVIEW");
            w.s(1002, "{");
            w.p(1010, c.to3(0.0));
            w.f(1040, height);
            w.s(1002, "}");
        }
    };
    // Model space has no page setup of its own: write what PLOT does for it, the drawing extents
    // fitted to the paper (plot type 1, scaled to fit). The default page's "layout" area is
    // plot type 5, "Layout information" (DXF Reference, PLOTSETTINGS group 74), which means
    // nothing for model space: another program's Plot dialog then shows no plot area or scale.
    let model_page = PageSetup { plot_area: "extents".into(), scale_to_fit: true, ..PageSetup::default() };
    layout_obj(&mut w, &model_layout, "Model", 0, &ms_br, 1, &model_page, None);
    for (i, (_, brh, l)) in ps_brs.iter().enumerate() {
        let h = layout_handles.get(i).cloned().unwrap_or_default();
        layout_obj(&mut w, &h, &l.name, i64::from(l.tab_order.max(1)), brh, 1, &l.page, l.view);
    }
    // Raster image definitions.
    w.t.extend(crate::dxf_image::objects(&cx.images, &root_dict));
    w.s(0, "ENDSEC");
    w.s(0, "EOF");
    let seed = format!("{:X}", w.next);
    if let Some(t) = w.t.get_mut(seed_pos + 1) {
        *t = Tag::s(5, seed);
    }
    crate::dxf_raw::finish(&mut w.t, d);
    crate::dxf_raw::constraint_node_order(&mut w.t, version >= Version::R2013);
    crate::dxf_raw::data_store(&mut w.t, d, target >= Version::R2013);
    add_class_instance_counts(&mut w.t);
    mark_xrefs(&mut w.t, d);
    w.t
}

/// Write DXF Reference group 91 on every CLASS record. Count records after `dxf_raw::finish`
/// restores foreign classes and objects so every class gets its actual instance count.
fn add_class_instance_counts(tags: &mut Vec<Tag>) {
    let mut counts: HashMap<String, i64> = HashMap::new();
    for t in tags.iter().filter(|t| t.code == 0) {
        let kind = t.str();
        if kind != "CLASS" {
            *counts.entry(kind.to_ascii_uppercase()).or_default() += 1;
        }
    }

    let mut start = 0;
    while start < tags.len() {
        if tags.get(start).is_none_or(|t| t.code != 0 || t.str() != "CLASS") {
            start += 1;
            continue;
        }
        let end = tags[start + 1..].iter().position(|t| t.code == 0).map_or(tags.len(), |i| start + 1 + i);
        let name = tags[start + 1..end].iter().find(|t| t.code == 1).map(Tag::str).unwrap_or_default().to_ascii_uppercase();
        let count = counts.get(&name).copied().unwrap_or(0);
        let inserted = if let Some(i) = tags[start + 1..end].iter().position(|t| t.code == 91) {
            tags[start + 1 + i] = Tag::i(91, count);
            false
        } else if let Some(i) = tags[start + 1..end].iter().position(|t| t.code == 90) {
            tags.insert(start + 2 + i, Tag::i(91, count));
            true
        } else {
            false
        };
        // Inserting the group shifts the next record's start by one.
        start = end + usize::from(inserted);
    }
}

/// Xref blocks (DXF Reference, BLOCK): bit 4 of group 70 and the external drawing's path in
/// group 1, set on the written BLOCK records of blocks that have an `xref_path`.
fn mark_xrefs(tags: &mut [Tag], d: &Drawing) {
    // Overlays add bit 8.
    let xrefs: HashMap<String, (&str, i64)> = d
        .blocks
        .values()
        .filter_map(|b| Some((b.name.to_ascii_uppercase(), (b.xref_path.as_deref()?, if b.xref_overlay { 12 } else { 4 }))))
        .collect();
    if xrefs.is_empty() {
        return;
    }
    // Inside a BLOCK record: the path once its name (2) says it is an xref.
    let mut block: Option<Option<(&str, i64)>> = None;
    for t in tags.iter_mut() {
        match (t.code, block) {
            (0, _) => block = (t.str() == "BLOCK").then_some(None),
            (2, Some(None)) => block = Some(xrefs.get(&t.str().to_ascii_uppercase()).copied()),
            (70, Some(Some((_, flags)))) => *t = Tag::i(70, t.i64() | flags),
            (1, Some(Some((p, _)))) => *t = Tag::s(1, p),
            _ => {}
        }
    }
}

/// A TABLESTYLE object (DXF Reference, OBJECTS: TABLESTYLE): margins, title/header
/// suppression and per row type (data, column header, title) text and border settings.
fn table_style_obj(w: &mut W, d: &Drawing, s: &TableStyle, h: &str, dict: &str) {
    w.s(0, "TABLESTYLE");
    w.s(5, h);
    w.group("ACAD_REACTORS", 330, &[dict]);
    w.s(330, dict);
    w.s(100, "AcDbTableStyle");
    w.s(3, "");
    w.i(70, 0);
    w.i(71, 0);
    w.f(40, s.margin);
    w.f(41, s.margin);
    w.i(280, i64::from(!s.title));
    w.i(281, i64::from(!s.header));
    // Data, column header and title cell styles; CADCraft's complete cell styles in xdata.
    for kind in crate::dxf_table::CELL_TYPES {
        let cs = s.cell_style(kind);
        let style = if d.text_style(&cs.text_style).is_some() { cs.text_style.clone() } else { "Standard".to_string() };
        w.t.extend(crate::dxf_table::cell_type_tags(&cs, &style));
    }
    let x = crate::dxf_table::cell_styles_xdata(s);
    if !x.is_empty() {
        w.s(1001, dxf_ext::APP);
        for c in x {
            w.s(1000, c);
        }
    }
}

/// A GROUP object (DXF Reference, OBJECTS: GROUP): description, unnamed (`*A…`) and selectable
/// flags, and the member entities that were written to this file.
fn group_obj(w: &mut W, g: &Group, h: &str, dict: &str, written: &std::collections::HashSet<String>) {
    w.s(0, "GROUP");
    w.s(5, h);
    w.group("ACAD_REACTORS", 330, &[dict]);
    w.s(330, dict);
    w.s(100, "AcDbGroup");
    w.s(300, &g.description);
    w.i(70, i64::from(g.name.starts_with('*')));
    w.i(71, i64::from(g.selectable));
    for m in g.members.iter().map(|m| m.hex()).filter(|m| written.contains(m)) {
        w.s(340, m);
    }
}

/// A MLEADERSTYLE object (DXF Reference, OBJECTS: MLEADERSTYLE): MText content and straight
/// ByBlock leaders with the style's landing gap, dogleg length, arrow size, text style and
/// text height.
fn mleader_style_obj(w: &mut W, s: &MLeaderStyle, h: &str, dict: &str, byblock_ltype: Option<&str>, text_style: Option<String>) {
    // ByBlock as a 32-bit colour value (0xC1000000).
    const BYBLOCK: i64 = -1_056_964_608;
    w.s(0, "MLEADERSTYLE");
    w.s(5, h);
    w.group("ACAD_REACTORS", 330, &[dict]);
    w.s(330, dict);
    w.s(100, "AcDbMLeaderStyle");
    w.i(179, 2);
    w.i(170, 2);
    w.i(171, 1);
    w.i(172, 0);
    w.i(90, 2);
    w.f(40, 0.0);
    w.f(41, 0.0);
    w.i(173, 1);
    w.i(91, BYBLOCK);
    if let Some(lt) = byblock_ltype {
        w.s(340, lt);
    }
    w.i(92, -2);
    w.i(290, 1);
    w.f(42, s.landing_gap);
    w.i(291, 1);
    w.f(43, s.dogleg);
    w.s(3, "");
    w.f(44, s.arrow_size);
    w.s(300, "");
    if let Some(ts) = text_style {
        w.s(342, ts);
    }
    w.i(174, 1);
    w.i(178, 1);
    w.i(175, 1);
    w.i(176, 0);
    w.i(93, BYBLOCK);
    w.f(45, s.text_height);
    w.i(292, 0);
    w.i(297, 0);
    w.f(46, 4.0);
    w.i(94, BYBLOCK);
    for c in [47, 49, 140] {
        w.f(c, 1.0);
    }
    w.i(293, 1);
    w.f(141, 0.0);
    w.i(294, 1);
    w.i(177, 0);
    w.f(142, 1.0);
    w.i(295, 0);
    w.i(296, i64::from(s.annotative));
    w.f(143, 0.125);
    w.i(271, 0);
    w.i(272, 9);
    w.i(273, 9);
}

/// The ACAD_SCALELIST dictionary and its SCALE objects (DXF Reference, OBJECTS section): name
/// (300), paper units (140), drawing units (141), unit scale flag (290).
fn scale_list(w: &mut W, scales: &[(&cadcraft_doc::AnnoScale, String)], dict: &str, root: &str) {
    w.s(0, "DICTIONARY");
    w.s(5, dict);
    w.group("ACAD_REACTORS", 330, &[root]);
    w.s(330, root);
    w.s(100, "AcDbDictionary");
    w.i(281, 1);
    for (i, (_, h)) in scales.iter().enumerate() {
        w.s(3, format!("S{i}"));
        w.s(350, h.clone());
    }
    for (s, h) in scales {
        w.s(0, "SCALE");
        w.s(5, h.clone());
        w.group("ACAD_REACTORS", 330, &[dict]);
        w.s(330, dict);
        w.s(100, "AcDbScale");
        w.i(70, 0);
        w.s(300, dxf_ext::xdata_str(&s.name));
        w.f(140, s.paper_units);
        w.f(141, s.drawing_units);
        w.i(290, i64::from(cadcraft_doc::same_factor(s.factor(), 1.0)));
    }
}

fn cadcraft_fonts_name() -> &'static str {
    "CADCraft Stroke"
}

/// One entity's DXF groups, as the writer stores them, without its `0` group: its type name
/// and the groups of its record (an entity written as several records, a POLYLINE and its
/// VERTEXes, gives its first). Handles of objects outside the entity (owner, styles) are left
/// out. AutoLISP's `entget` is built on it.
pub fn entity_groups(d: &Drawing, e: &Entity) -> (String, Vec<Tag>) {
    let mut w = W {
        t: Vec::new(),
        next: d.handseed.max(0x100).saturating_add(0x1000),
        pstyles: HashMap::new(),
        layers: HashMap::new(),
        reactors: HashMap::new(),
    };
    let cx = Ctx { target: Version::R2018, ..Ctx::default() };
    let paper = d.space_of(e.handle).is_some_and(|s| !matches!(s, cadcraft_doc::Space::Model));
    entity(&mut w, d, e, "0", paper, &cx);
    let mut tags = w.t.into_iter();
    let kind = tags.next().filter(|t| t.code == 0).map(|t| t.value.as_str()).unwrap_or_default();
    (kind, tags.take_while(|t| t.code != 0).filter(|t| t.code != 330).collect())
}
