use super::ctx::{AssocObj, Ctx};
use super::{
    Attrib, Color, Common, DimKind, DimStyle, Dimension, Drawing, Entity, EntityKind, HAlign, HashMap, Insert, Lineweight, Text, Transparency,
    VAlign, Vec3, Version, W, dxf_ext,
};

/// `AcadAnnotative` xdata marking an annotative style.
pub(super) fn annotative_xdata(w: &mut W) {
    w.s(1001, "AcadAnnotative");
    w.s(1000, "AnnotativeData");
    w.s(1002, "{");
    w.i(1070, 1);
    w.i(1070, 1);
    w.s(1002, "}");
}

/// Override (`ACAD` DSTYLE) and associativity (`CADCRAFT` ASSOC) xdata of a dimension.
pub(super) fn dim_xdata(w: &mut W, dm: &Dimension, cx: &Ctx) {
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
pub(super) fn oblique_52(w: &mut W, dm: &Dimension, base: f64) {
    if let Some(a) = dm.marks.oblique.filter(|a| a.is_finite()) {
        w.f(52, cadcraft_geom::norm_angle(a - base).to_degrees());
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

pub(super) fn common(w: &mut W, e: &Entity, owner: &str, paper: bool, subclass: &str) {
    common_x(w, e, owner, paper, subclass, None);
}

/// Common entity groups; `assoc` adds the reactor and extension dictionary of a DIMASSOC.
pub(super) fn common_x(w: &mut W, e: &Entity, owner: &str, paper: bool, subclass: &str, assoc: Option<&AssocObj>) {
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
pub(super) fn layer_and_props(w: &mut W, c: &Common, base: Option<&Common>) {
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
pub(super) fn attrib_common(d: &Drawing, e: &Entity, ins: &Insert, a: &Attrib) -> (Common, Common) {
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
pub(super) fn multiline_tags(w: &mut W, a: &Attrib, attdef: bool) {
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
    // Justification recalculates the baseline in XY; group 10 keeps its own elevation.
    if q.is_finite() { Vec3::new(q.x, q.y, t.insert.z) } else { t.insert }
}

/// `attdef_prompt` is `Some` for an ATTDEF (its prompt, which may be empty) and `None` for an ATTRIB.
pub(super) fn text_tags(w: &mut W, d: &Drawing, t: &Text, attrib: Option<(&Attrib, Version)>, attdef_prompt: Option<&str>) {
    let attdef = attdef_prompt.is_some();
    // An attribute definition with no default value is drawn as its tag, whatever its prompt: the
    // baseline start AutoCAD stored is judged against the tag's width too. A stored point that is
    // close to the empty text's own start is kept as it is, so the font's width of the tag never
    // moves it.
    let own = text_insert(d, t);
    let insert = match attrib {
        Some((a, _)) if attdef && t.value.is_empty() && own != t.insert => text_insert(d, &Text { value: a.tag.clone(), ..t.clone() }),
        _ => own,
    };
    w.p(10, insert);
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
            w.s(100, if attdef { "AcDbAttributeDefinition" } else { "AcDbAttribute" });
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
