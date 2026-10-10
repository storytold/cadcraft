//! Drawing → DXF (AC1015 / R2000 structure, readable by every DXF consumer since 2000).

use std::collections::HashMap;

use cadcraft_color::Color;
use cadcraft_doc::*;
use cadcraft_dxf::Tag;
use cadcraft_geom::{Vec2, Vec3};

use crate::dxf_ext::{self, DimVal, K};

struct W {
    t: Vec<Tag>,
    next: u64,
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
        self.next += 1;
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
    param: f64,
    point: Vec3,
    other: Option<Handle>,
}

/// Handles and names the entity writer needs from the rest of the file.
#[derive(Default)]
struct Ctx {
    /// Dimension → anonymous `*D` block name.
    dim_blocks: HashMap<Handle, String>,
    /// Upper-case text style name → STYLE record handle.
    styles: HashMap<String, String>,
    /// Upper-case arrow name (as stored in styles/overrides) → BLOCK_RECORD handle.
    arrows: HashMap<String, String>,
    /// Associative dimension → its DIMASSOC object.
    assoc: HashMap<Handle, AssocObj>,
    /// Table → (anonymous `*T` block name, BLOCK_RECORD handle).
    tables: HashMap<Handle, (String, String)>,
    /// Table style name → TABLESTYLE handle (the first is the fallback).
    table_styles: Vec<(String, String)>,
}

impl Ctx {
    fn style(&self, name: &str) -> Option<String> {
        self.styles.get(&name.to_ascii_uppercase()).cloned()
    }
    fn arrow(&self, name: &str) -> Option<String> {
        self.arrows.get(&name.trim().to_ascii_uppercase()).cloned()
    }
    fn table_style(&self, name: &str) -> String {
        self.table_styles
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .or_else(|| self.table_styles.first())
            .map(|(_, h)| h.clone())
            .unwrap_or_else(|| "0".into())
    }
    /// A dimension variable encoded with this file's handles; integers outside the 16-bit
    /// range of their groups are dropped.
    fn encode(&self, kind: K, v: &serde_json::Value) -> Option<DimVal> {
        dxf_ext::encode(kind, v, &|n| self.style(n), &|n| self.arrow(n)).filter(|x| !matches!(x, DimVal::Int(i) if i16::try_from(*i).is_err()))
    }
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
    let mut ov = W { t: Vec::new(), next: 0 };
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
    if !ours.is_empty() {
        w.s(1001, dxf_ext::APP);
        w.xdata(ours);
    }
}

/// Header variables we write, with their group codes (DXF Reference, HEADER section).
/// `P2`/`P3` = 2D/3D point.
const HEADER_VARS: &[(&str, i32)] = &[
    ("INSBASE", -3),
    ("LIMMIN", -2),
    ("LIMMAX", -2),
    ("ORTHOMODE", 70),
    ("FILLMODE", 70),
    ("MIRRTEXT", 70),
    ("LTSCALE", 40),
    ("ATTMODE", 70),
    ("TEXTSIZE", 40),
    ("TRACEWID", 40),
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

fn header_vars(w: &mut W, d: &Drawing) {
    for (name, code) in HEADER_VARS {
        let Some(v) = d.header.get(name) else { continue };
        match (*code, v) {
            (-2 | -3, HVal::Point(p)) => {
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
    if let Some(a) = assoc {
        w.group("ACAD_REACTORS", 330, &[&a.handle]);
        w.group("ACAD_XDICTIONARY", 360, &[&a.xdict]);
    }
    w.s(330, owner);
    w.s(100, "AcDbEntity");
    if paper {
        w.i(67, 1);
    }
    w.s(8, &e.common.layer);
    if !e.common.linetype.eq_ignore_ascii_case("bylayer") {
        w.s(6, &e.common.linetype);
    }
    match e.common.color {
        Color::ByLayer => {}
        Color::True(rgb) => {
            w.i(62, i64::from(cadcraft_color::nearest_aci(rgb)));
            w.i(420, i64::from(rgb.to_u32()));
        }
        c => w.i(62, i64::from(c.to_aci())),
    }
    if e.common.lineweight != Lineweight::ByLayer {
        w.i(370, i64::from(e.common.lineweight.to_dxf()));
    }
    if (e.common.ltscale - 1.0).abs() > 1e-12 {
        w.f(48, e.common.ltscale);
    }
    if !e.common.visible {
        w.i(60, 1);
    }
    if let Some(v) = transparency_440(e.common.transparency) {
        w.i(440, v);
    }
    if !subclass.is_empty() {
        w.s(100, subclass);
    }
}

fn text_tags(w: &mut W, t: &Text, attrib_tag: Option<(&str, bool)>, attdef_prompt: Option<&str>) {
    w.p(10, t.insert);
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
    match attrib_tag {
        Some((tag, invisible)) => {
            w.s(100, if attdef_prompt.is_some() { "AcDbAttributeDefinition" } else { "AcDbAttribute" });
            if let Some(prompt) = attdef_prompt {
                w.s(3, prompt);
            }
            w.s(2, tag);
            w.i(70, i64::from(invisible));
            if v != 0 {
                w.i(74, v);
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
            w.p(10, l.a);
            w.p(11, l.b);
        }
        EntityKind::Point(p) => {
            w.s(0, "POINT");
            common(w, e, owner, paper, "AcDbPoint");
            w.p(10, p.p);
        }
        EntityKind::Circle(c) => {
            w.s(0, "CIRCLE");
            common(w, e, owner, paper, "AcDbCircle");
            w.p(10, c.center);
            w.f(40, c.radius);
        }
        EntityKind::Arc(a) => {
            w.s(0, "ARC");
            common(w, e, owner, paper, "AcDbCircle");
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
        EntityKind::Polyline3d(p) => {
            w.s(0, "POLYLINE");
            common(w, e, owner, paper, "AcDb3dPolyline");
            w.i(66, 1);
            w.p(10, Vec3::ZERO);
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
            let flags = 8 | i64::from(s.closed) | if s.weights.is_empty() { 0 } else { 4 };
            w.i(70, flags);
            w.i(71, s.degree as i64);
            w.i(72, s.knots.len() as i64);
            w.i(73, s.control.len() as i64);
            w.i(74, s.fit.len() as i64);
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
            text_tags(w, t, None, None);
        }
        EntityKind::AttDef(a) => {
            w.s(0, "ATTDEF");
            common(w, e, owner, paper, "AcDbText");
            text_tags(w, &a.text, Some((&a.tag, a.invisible)), Some(&a.prompt));
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
            let chars: Vec<char> = t.contents.chars().collect();
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
            w.f(44, t.line_spacing);
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
                    w.s(8, &e.common.layer);
                    w.s(100, "AcDbText");
                    text_tags(w, &a.text, Some((&a.tag, a.invisible)), None);
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
                    w.s(100, "AcDbRotatedDimension");
                }
                DimKind::Aligned | DimKind::ArcLength => {
                    w.s(100, "AcDbAlignedDimension");
                    w.p(13, dm.p13);
                    w.p(14, dm.p14);
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
        }
        EntityKind::MLeader(m) => {
            // Written as plain LEADER + MTEXT entities (MULTILEADER objects aren't supported
            // yet), so every reader shows the annotation.
            for arm in m.leaders.iter().take(64) {
                let mut vertices = arm.clone();
                vertices.push(m.landing);
                let le = Entity {
                    handle: Handle(u64::from_str_radix(&w.h(), 16).unwrap_or(0)),
                    common: e.common.clone(),
                    kind: EntityKind::Leader(cadcraft_doc::Leader { vertices, arrow: true, spline: false, style: m.style.clone() }),
                };
                entity(w, d, &le, owner, paper, cx);
            }
            if let Some(t) = &m.text {
                let te = Entity {
                    handle: Handle(u64::from_str_radix(&w.h(), 16).unwrap_or(0)),
                    common: e.common.clone(),
                    kind: EntityKind::MText(t.clone()),
                };
                entity(w, d, &te, owner, paper, cx);
            }
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
        EntityKind::Solid(s) | EntityKind::Trace(s) => {
            let solid = matches!(e.kind, EntityKind::Solid(_));
            w.s(0, if solid { "SOLID" } else { "TRACE" });
            common(w, e, owner, paper, "AcDbTrace");
            w.p(10, s.corners[0]);
            w.p(11, s.corners[1]);
            w.p(12, s.corners[2]);
            w.p(13, s.corners[3]);
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
                w.i(97, 0);
            }
            w.i(75, i64::from(h.style));
            w.i(76, 1);
            if !h.solid {
                w.f(52, h.angle.to_degrees());
                w.f(41, h.scale);
                w.i(77, 0);
                let pat = cadcraft_doc::library::pattern(&h.pattern);
                let lines = pat.map(|p| p.lines).unwrap_or_default();
                w.i(78, lines.len() as i64);
                for pl in lines {
                    let ang = pl.angle.to_radians() + h.angle;
                    w.f(53, ang.to_degrees());
                    let o = Vec2::new(pl.origin.0, pl.origin.1).rotate(h.angle) * h.scale;
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
        }
        EntityKind::Viewport(v) => {
            w.s(0, "VIEWPORT");
            common(w, e, owner, paper, "AcDbViewport");
            w.p(10, v.center);
            w.f(40, v.width);
            w.f(41, v.height);
            w.i(69, i64::from(v.id));
            w.p2(12, v.view_center);
            w.f(45, v.view_height);
            // Status flags: 16384 = display locked.
            w.i(90, if v.locked { 16384 } else { 0 });
            if !v.frozen_layers.is_empty() {
                w.s(1001, dxf_ext::APP);
                w.xdata(dxf_ext::frozen_xdata(&v.frozen_layers));
            }
        }
        // Not yet written: images, wipeouts, tables, multileaders, unknown objects.
        _ => {}
    }
}

/// Anonymous dimension blocks (`*D1`…) with the rendered geometry, as consumers expect.
fn dim_block_entities(d: &Drawing, dm: &Dimension, layer: &str) -> Vec<Entity> {
    let style = d.dim_style(&dm.style).cloned().unwrap_or_default();
    let g = cadcraft_render::dimension_geometry(dm, &style, d.header.f64("DIMSCALE", 1.0));
    let mut out = Vec::new();
    let c = Common { layer: layer.into(), color: Color::ByBlock, ..Common::default() };
    for l in &g.lines {
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
                common: c.clone(),
                kind: EntityKind::Solid(Solid { corners: [a.to3(0.0), b.to3(0.0), cc.to3(0.0), cc.to3(0.0)] }),
            });
        }
    }
    if !g.value.is_empty() {
        out.push(Entity {
            handle: Handle(0),
            common: c,
            kind: EntityKind::MText(MText {
                insert: g.text_pos.to3(0.0),
                height: style.text_height * style.scale.max(1e-9),
                width: 0.0,
                attach: 5,
                rotation: g.text_angle,
                style: style.text_style.clone(),
                contents: g.value.clone(),
                line_spacing: 1.0,
            }),
        });
    }
    out
}

/// Rows and columns written for a table (bounded like the renderer).
fn table_size(t: &Table) -> (usize, usize) {
    (t.row_heights.len().min(10_000), t.col_widths.len().min(10_000))
}

/// The anonymous `*T` block of a table: cell borders and texts, for readers that only draw
/// the block.
fn table_block_entities(d: &Drawing, t: &Table, layer: &str) -> Vec<Entity> {
    let (rows, cols) = table_size(t);
    let mut xs = vec![0.0];
    for w in t.col_widths.iter().take(cols) {
        xs.push(xs.last().copied().unwrap_or(0.0) + w);
    }
    let mut ys = vec![0.0];
    for h in t.row_heights.iter().take(rows) {
        ys.push(ys.last().copied().unwrap_or(0.0) + h);
    }
    let x_at = |c: usize| xs.get(c).copied().unwrap_or(0.0);
    let y_at = |r: usize| ys.get(r).copied().unwrap_or(0.0);
    let p = |x: f64, y: f64| Vec3::new(x, -y, 0.0);
    let cov = cadcraft_render::table_covered(t);
    let margin = d.table_styles.iter().find(|s| s.name.eq_ignore_ascii_case(&t.style)).map(|s| s.margin).unwrap_or(0.06).max(0.0);
    let c = Common { layer: layer.into(), color: Color::ByBlock, ..Common::default() };
    let mut out = Vec::new();
    let mut line = |a: Vec3, b: Vec3| out.push(Entity { handle: Handle(0), common: c.clone(), kind: EntityKind::Line(Line { a, b }) });
    let mut texts = Vec::new();
    for r in 0..rows {
        for col in 0..cols {
            if cov.get(r).and_then(|v| v.get(col)).copied().unwrap_or(false) {
                continue;
            }
            let cell = t.cells.get(r).and_then(|row| row.get(col));
            let (rs, cs) = cell.and_then(|x| x.merged).map(|(a, b)| (a.max(1) as usize, b.max(1) as usize)).unwrap_or((1, 1));
            let (r2, c2) = ((r + rs).min(rows), (col + cs).min(cols));
            let (x0, x1, y0, y1) = (x_at(col), x_at(c2), y_at(r), y_at(r2));
            line(p(x0, y1), p(x1, y1));
            line(p(x1, y1), p(x1, y0));
            if r == 0 {
                line(p(x0, y0), p(x1, y0));
            }
            if col == 0 {
                line(p(x0, y0), p(x0, y1));
            }
            let Some(cell) = cell.filter(|x| !x.text.is_empty()) else { continue };
            let title = t.title && r == 0;
            let header = t.header && r == usize::from(t.title);
            let (insert, attach) = if title || header { (p((x0 + x1) / 2.0, (y0 + y1) / 2.0), 5) } else { (p(x0 + margin, (y0 + y1) / 2.0), 4) };
            texts.push(Entity {
                handle: Handle(0),
                common: c.clone(),
                kind: EntityKind::MText(MText {
                    insert,
                    height: if title { t.text_height * 1.4 } else { t.text_height },
                    width: 0.0,
                    attach,
                    rotation: 0.0,
                    style: "Standard".into(),
                    contents: cell.text.clone(),
                    line_spacing: 1.0,
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

/// Standard DIMASSOC point references of an associative dimension: the extension-line
/// origins of linear/aligned dimensions and the feature point of ordinate dimensions, linked
/// to objects that exist. CADCraft's exact links travel in xdata as well.
fn std_assoc_refs(d: &Drawing, dm: &Dimension) -> (i64, Vec<OsnapRef>) {
    let slots: &[&str] = match dm.kind {
        DimKind::Linear { .. } | DimKind::Aligned => &["p13", "p14"],
        DimKind::Ordinate { .. } => &["p13"],
        _ => &[],
    };
    let mut flags = 0;
    let mut refs = Vec::new();
    for (slot, name) in slots.iter().enumerate() {
        let Some(a) = dm.assoc.iter().find(|a| a.point == *name) else { continue };
        let Some(ent) = d.entity(a.handle) else { continue };
        let point = if *name == "p13" { dm.p13 } else { dm.p14 };
        let (osnap, param, other) = match &a.snap {
            AssocSnap::Start | AssocSnap::End | AssocSnap::Vertex { .. } => (1, 0.0, None),
            AssocSnap::Mid => (2, 0.0, None),
            AssocSnap::Center => (if matches!(ent.kind, EntityKind::Point(_)) { 4 } else { 3 }, 0.0, None),
            AssocSnap::OnCircle { angle } => (10, *angle, None),
            AssocSnap::Intersection { other } => {
                if d.entity(*other).is_none() {
                    continue;
                }
                (6, 0.0, Some(*other))
            }
        };
        flags |= 1 << slot;
        refs.push(OsnapRef { osnap, main: a.handle, param, point, other });
    }
    (flags, refs)
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
pub fn write(d: &Drawing) -> String {
    // Structural handles start above every entity handle.
    let mut w = W { t: Vec::new(), next: d.handseed.max(0x100) + 0x1000 };
    // Pre-allocate handles for block records.
    let mut paper_layouts: Vec<&Layout> = d.layouts.iter().collect();
    paper_layouts.sort_by_key(|l| l.tab_order);
    let ms_br = w.h();
    let ps_brs: Vec<(String, String, &Layout)> = paper_layouts
        .iter()
        .enumerate()
        .map(|(i, l)| (if i == 0 { "*Paper_Space".to_string() } else { format!("*Paper_Space{}", i - 1) }, String::new(), *l))
        .collect();
    let ps_brs: Vec<(String, String, &Layout)> = ps_brs.into_iter().map(|(n, _, l)| (n, w.h(), l)).collect();
    let mut user_blocks: Vec<(String, String, &Block)> = d.blocks.values().map(|b| (b.name.clone(), String::new(), b.as_ref())).collect();
    user_blocks.iter_mut().for_each(|b| b.1 = format!("{:X}", 0));
    let user_blocks: Vec<(String, String, &Block)> = user_blocks.into_iter().map(|(n, _, b)| (n, w.h(), b)).collect();
    let mut cx = Ctx::default();
    // Dimension blocks.
    let mut dim_defs: Vec<(String, String, Vec<Entity>)> = Vec::new(); // filled below
    let mut n = 1;
    let all_spaces: Vec<&EntityStore> = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).collect();
    for st in &all_spaces {
        for e in st.iter() {
            if let EntityKind::Dimension(dm) = &e.kind {
                let mut name = format!("*D{n}");
                while d.block(&name).is_some() {
                    n += 1;
                    name = format!("*D{n}");
                }
                n += 1;
                let ents = dim_block_entities(d, dm, &e.common.layer);
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
        arrow_names.extend([s.arrow_block.clone(), s.arrow_block1.clone(), s.arrow_block2.clone()]);
    }
    for e in &every {
        if let EntityKind::Dimension(dm) = &e.kind {
            for (k, v) in &dm.overrides {
                let Some(f) = DimStyle::field_name(k).filter(|f| f.starts_with("arrowBlock")) else { continue };
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
    let constraint_chunks = dxf_ext::constraint_chunks(&d.constraints, &d.parametric);
    let root_dict = w.h();
    let group_dict = w.h();
    let layout_dict = w.h();
    let table_style_dict = w.h();
    let constraints_xrec = w.h();
    let model_layout = w.h();
    let layout_handles: Vec<String> = ps_brs.iter().map(|_| w.h()).collect();

    // ---------------- HEADER ----------------
    w.s(0, "SECTION");
    w.s(2, "HEADER");
    w.s(9, "$ACADVER");
    w.s(1, "AC1015");
    w.s(9, "$DWGCODEPAGE");
    w.s(3, "ANSI_1252");
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
    let mut classes = vec![("TABLESTYLE", "AcDbTableStyle", 4095, false)];
    if !cx.tables.is_empty() {
        classes.push(("ACAD_TABLE", "AcDbTable", 1025, true));
    }
    if !cx.assoc.is_empty() {
        classes.push(("DIMASSOC", "AcDbDimAssoc", 0, false));
    }
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
    // VPORT
    let th = table_head(&mut w, "VPORT", 1);
    record_head(&mut w, "VPORT", &th, "AcDbViewportTableRecord");
    w.s(2, "*Active");
    w.i(70, 0);
    w.p2(10, Vec2::ZERO);
    w.p2(11, Vec2::new(1.0, 1.0));
    w.p2(12, if ext.is_empty() { Vec2::new(6.0, 4.5) } else { ext.center() });
    w.p2(13, Vec2::ZERO);
    w.p2(14, Vec2::new(0.5, 0.5));
    w.p2(15, Vec2::new(0.5, 0.5));
    w.p(16, Vec3::Z);
    w.p(17, Vec3::ZERO);
    w.f(40, if ext.is_empty() { 9.0 } else { (ext.height() * 1.1).max(1e-6) });
    w.f(41, 1.6);
    w.f(42, 50.0);
    w.s(0, "ENDTAB");
    // LTYPE
    let th = table_head(&mut w, "LTYPE", d.linetypes.len());
    for lt in &d.linetypes {
        record_head(&mut w, "LTYPE", &th, "AcDbLinetypeTableRecord");
        w.s(2, &lt.name);
        w.i(70, 0);
        w.s(3, &lt.description);
        w.i(72, 65);
        w.i(73, lt.pattern.len() as i64);
        w.f(40, lt.pattern_length());
        for el in &lt.pattern {
            w.f(49, el.length);
            w.i(74, 0);
        }
    }
    w.s(0, "ENDTAB");
    // LAYER
    let th = table_head(&mut w, "LAYER", d.layers.len());
    for l in &d.layers {
        record_head(&mut w, "LAYER", &th, "AcDbLayerTableRecord");
        w.s(2, &l.name);
        w.i(70, i64::from(l.frozen) | if l.vp_freeze_new { 2 } else { 0 } | if l.locked { 4 } else { 0 });
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
    // STYLE
    let th = table_head(&mut w, "STYLE", d.text_styles.len());
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
        w.s(3, if s.font == cadcraft_fonts_name() { "txt" } else { s.font.as_str() });
        w.s(4, &s.big_font);
        if s.annotative {
            annotative_xdata(&mut w);
        }
    }
    w.s(0, "ENDTAB");
    for name in ["VIEW", "UCS"] {
        table_head(&mut w, name, 0);
        w.s(0, "ENDTAB");
    }
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
        for (field, code, kind) in dxf_ext::DIM_CODES {
            if let Some(enc) = v.get(*field).and_then(|x| cx.encode(*kind, x)) {
                w.dimval(*code, enc, false);
            }
        }
        if !s.arrow_block1.trim().is_empty() || !s.arrow_block2.trim().is_empty() {
            w.i(dxf_ext::DIMSAH, 1);
        }
        if s.annotative {
            annotative_xdata(&mut w);
        }
    }
    w.s(0, "ENDTAB");
    // BLOCK_RECORD
    let th = table_head(&mut w, "BLOCK_RECORD", 1 + ps_brs.len() + user_blocks.len() + dim_defs.len() + arrow_defs.len() + table_defs.len());
    let br_rec = |w: &mut W, h: &str, name: &str, layout: Option<&str>| {
        w.s(0, "BLOCK_RECORD");
        w.s(5, h);
        w.s(330, th.clone());
        w.s(100, "AcDbSymbolTableRecord");
        w.s(100, "AcDbBlockTableRecord");
        w.s(2, name);
        w.s(340, layout.unwrap_or("0"));
    };
    br_rec(&mut w, &ms_br, "*Model_Space", Some(&model_layout));
    for (i, (n, h, _)) in ps_brs.iter().enumerate() {
        br_rec(&mut w, h, n, layout_handles.get(i).map(String::as_str));
    }
    for (n, h, _) in &user_blocks {
        br_rec(&mut w, h, n, None);
    }
    for (n, h, _) in dim_defs.iter().chain(&arrow_defs).chain(&table_defs) {
        br_rec(&mut w, h, n, None);
    }
    w.s(0, "ENDTAB");
    w.s(0, "ENDSEC");

    // ---------------- BLOCKS ----------------
    w.s(0, "SECTION");
    w.s(2, "BLOCKS");
    let block = |w: &mut W, name: &str, brh: &str, base: Vec3, flags: i64, ents: &mut dyn Iterator<Item = &Entity>, paper: bool| {
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
    block(&mut w, "*Model_Space", &ms_br, Vec3::ZERO, 0, &mut std::iter::empty(), false);
    for (i, (n, h, l)) in ps_brs.iter().enumerate() {
        if i == 0 {
            block(&mut w, n, h, Vec3::ZERO, 0, &mut std::iter::empty(), true);
        } else {
            block(&mut w, n, h, Vec3::ZERO, 0, &mut l.entities.iter().map(|e| e.as_ref()), true);
        }
    }
    for (n, h, b) in &user_blocks {
        block(&mut w, n, h, b.base, if b.anonymous { 1 } else { 0 }, &mut b.entities.iter().map(|e| e.as_ref()), false);
    }
    // Generated geometry entities (dimension, arrowhead and table blocks) need handles.
    for (_, _, ents) in dim_defs.iter_mut().chain(arrow_defs.iter_mut()).chain(table_defs.iter_mut()) {
        for e in ents.iter_mut() {
            e.handle = Handle(w.next);
            w.next += 1;
        }
    }
    for (n, h, ents) in &dim_defs {
        block(&mut w, n, h, Vec3::ZERO, 1, &mut ents.iter(), false);
    }
    for (n, h, ents) in &arrow_defs {
        block(&mut w, n, h, Vec3::ZERO, 0, &mut ents.iter(), false);
    }
    for (n, h, ents) in &table_defs {
        block(&mut w, n, h, Vec3::ZERO, 1, &mut ents.iter(), false);
    }
    w.s(0, "ENDSEC");

    // ---------------- ENTITIES ----------------
    w.s(0, "SECTION");
    w.s(2, "ENTITIES");
    for e in d.model.iter() {
        entity(&mut w, d, e, &ms_br, false, &cx);
    }
    if let Some((_, h, l)) = ps_brs.first() {
        for e in l.entities.iter() {
            entity(&mut w, d, e, h, true, &cx);
        }
    }
    w.s(0, "ENDSEC");

    // ---------------- OBJECTS ----------------
    w.s(0, "SECTION");
    w.s(2, "OBJECTS");
    w.s(0, "DICTIONARY");
    w.s(5, root_dict.clone());
    w.s(330, "0");
    w.s(100, "AcDbDictionary");
    w.i(281, 1);
    w.s(3, "ACAD_GROUP");
    w.s(350, group_dict.clone());
    w.s(3, "ACAD_LAYOUT");
    w.s(350, layout_dict.clone());
    w.s(3, "ACAD_TABLESTYLE");
    w.s(350, table_style_dict.clone());
    if constraint_chunks.is_some() {
        w.s(3, dxf_ext::CONSTRAINTS_KEY);
        w.s(350, constraints_xrec.clone());
    }
    w.s(0, "DICTIONARY");
    w.s(5, group_dict);
    w.s(330, root_dict.clone());
    w.s(100, "AcDbDictionary");
    w.i(281, 1);
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
        table_style_obj(&mut w, s, h, &table_style_dict);
    }
    // Parametric constraints and parameters (CADCraft data).
    if let Some(chunks) = &constraint_chunks {
        w.s(0, "XRECORD");
        w.s(5, constraints_xrec.clone());
        w.group("ACAD_REACTORS", 330, &[&root_dict]);
        w.s(330, root_dict.clone());
        w.s(100, "AcDbXrecord");
        w.i(280, 1);
        for c in chunks {
            w.s(1, c.clone());
        }
    }
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
            w.i(73, 0);
            w.i(91, 0);
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
    w.s(330, root_dict);
    w.s(100, "AcDbDictionary");
    w.i(281, 1);
    w.s(3, "Model");
    w.s(350, model_layout.clone());
    for (i, (_, _, l)) in ps_brs.iter().enumerate() {
        w.s(3, &l.name);
        w.s(350, layout_handles.get(i).cloned().unwrap_or_default());
    }
    let layout_obj = |w: &mut W, h: &str, name: &str, tab: i64, brh: &str, flags: i64, page: &PageSetup| {
        w.s(0, "LAYOUT");
        w.s(5, h);
        w.s(330, layout_dict.clone());
        w.s(100, "AcDbPlotSettings");
        w.s(1, "");
        w.s(2, "none_device");
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
            (48, 0.0),
            (49, 0.0),
            (140, 0.0),
            (141, 0.0),
            (142, 1.0),
            (143, 1.0),
        ] {
            w.f(c, v);
        }
        w.i(70, 688);
        w.i(72, 0);
        w.i(73, i64::from(page.landscape));
        w.i(74, 5);
        w.s(7, "");
        w.i(75, 16);
        w.f(147, 1.0);
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
    };
    layout_obj(&mut w, &model_layout, "Model", 0, &ms_br, 1, &PageSetup::default());
    for (i, (_, brh, l)) in ps_brs.iter().enumerate() {
        let h = layout_handles.get(i).cloned().unwrap_or_default();
        layout_obj(&mut w, &h, &l.name, i64::from(l.tab_order.max(1)), brh, 1, &l.page);
    }
    w.s(0, "ENDSEC");
    w.s(0, "EOF");
    let seed = format!("{:X}", w.next);
    if let Some(t) = w.t.get_mut(seed_pos + 1) {
        *t = Tag::s(5, seed);
    }
    cadcraft_dxf::write_ascii(&w.t)
}

/// A TABLESTYLE object (DXF Reference, OBJECTS: TABLESTYLE): margins, title/header
/// suppression and per row type (data, column header, title) text and border settings.
fn table_style_obj(w: &mut W, s: &TableStyle, h: &str, dict: &str) {
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
    // Data (middle left), column header and title (middle centre).
    for (height, align) in [(s.text_height, 4), (s.text_height, 5), (s.text_height * 1.4, 5)] {
        w.s(7, "Standard");
        w.f(140, height);
        w.i(170, align);
        w.i(62, 0);
        w.i(63, 7);
        w.i(283, 0);
        for c in 274..=279 {
            w.i(c, -2);
        }
        for c in 284..=289 {
            w.i(c, 1);
        }
        for c in 64..=69 {
            w.i(c, 0);
        }
    }
}

fn cadcraft_fonts_name() -> &'static str {
    "CADCraft Stroke"
}
