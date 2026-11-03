//! DXF data shared by the reader and the writer beyond plain entity geometry:
//! dimension-variable group codes (DIMSTYLE records and the `ACAD` "DSTYLE" override xdata,
//! both per the DXF Reference), arrowhead block names, extended-data helpers and the
//! CADCraft-owned payloads (exact associativity, table flags, hatch gradients, parametric
//! constraints).
//!
//! CADCraft's own data lives under the registered application `CADCRAFT` (xdata) and the
//! named-object-dictionary entries `CADCRAFT_CONSTRAINTS`, `CADCRAFT_LAYERSTATES`,
//! `CADCRAFT_PLOTSTYLES`, `CADCRAFT_HANDSEED` and `CADCRAFT_MLEADERSTYLE` (XRECORDs). Other
//! readers keep or ignore them.

use std::collections::HashMap;

use cadcraft_color::Color;
use cadcraft_doc::{ArrayParams, AssocSnap, Constraint, DimAssoc, DimStyle, Handle, LayerState, Parametric, PlotStyleTable};
use cadcraft_dxf::{Tag, Version};
use cadcraft_render::Arrowhead;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Registered application name of CADCraft's extended data.
pub(crate) const APP: &str = "CADCRAFT";
/// Named-object-dictionary key of the constraint XRECORD.
pub(crate) const CONSTRAINTS_KEY: &str = "CADCRAFT_CONSTRAINTS";
/// Named-object-dictionary key of the saved layer states XRECORD.
pub(crate) const LAYER_STATES_KEY: &str = "CADCRAFT_LAYERSTATES";
/// Named-object-dictionary key of the drawing's plot style tables XRECORD.
pub(crate) const PLOT_STYLES_KEY: &str = "CADCRAFT_PLOTSTYLES";
/// Named-object-dictionary key of the annotative scaling XRECORD.
pub(crate) const ANNOTATION_KEY: &str = "CADCRAFT_ANNOTATION";
/// Named-object-dictionary key of the plot style names dictionary (DXF Reference): its entries
/// name `ACDBPLACEHOLDER` objects, and layers and objects point at them with group 390.
pub(crate) const PLOT_STYLE_NAMES_KEY: &str = "ACAD_PLOTSTYLENAME";
/// Named-object-dictionary key of the XRECORD holding the drawing's next entity handle (hex).
pub(crate) const HANDSEED_KEY: &str = "CADCRAFT_HANDSEED";
/// Named-object-dictionary key of the XRECORD holding the current multileader style.
pub(crate) const MLEADER_STYLE_KEY: &str = "CADCRAFT_MLEADERSTYLE";
/// Most plot style tables and plot style names read from a file.
pub(crate) const MAX_PLOT_STYLE_TABLES: usize = 1000;
pub(crate) const MAX_PLOT_STYLE_NAMES: usize = 100_000;

/// Caps for hostile input.
pub(crate) const MAX_XDATA_ITEMS: usize = 4096;
pub(crate) const MAX_PAYLOAD: usize = 64 << 20;
pub(crate) const MAX_CONSTRAINTS: usize = 1_000_000;
pub(crate) const MAX_LAYER_STATES: usize = 100_000;

/// How a dimension variable is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum K {
    Real,
    Int,
    Bool,
    Str,
    /// ACI colour number (0 ByBlock, 256 ByLayer).
    Color,
    /// A character stored as its code (DIMDSEP).
    Char,
    /// Handle of a STYLE record (DIMTXSTY).
    TextStyle,
    /// Handle of an arrowhead BLOCK_RECORD (DIMBLK, DIMBLK1, DIMBLK2, DIMLDRBLK).
    Block,
    /// Handle of an LTYPE record (DIMLTYPE, DIMLTEX1, DIMLTEX2); "" (ByBlock) isn't written.
    Linetype,
    /// An angle: degrees in the style, radians in the file (DIMJOGANG).
    Angle,
}

/// [`DimStyle`] field (camelCase) → DIMSTYLE group code (DXF Reference, DIMSTYLE table). The
/// same codes identify variables in the DSTYLE override xdata.
pub(crate) const DIM_CODES: &[(&str, i32, K)] = &[
    ("post", 3, K::Str),
    ("altPost", 4, K::Str),
    ("scale", 40, K::Real),
    ("arrowSize", 41, K::Real),
    ("extOffset", 42, K::Real),
    ("baselineSpacing", 43, K::Real),
    ("extExtend", 44, K::Real),
    ("round", 45, K::Real),
    ("dimLineExtend", 46, K::Real),
    ("tolPlus", 47, K::Real),
    ("tolMinus", 48, K::Real),
    ("textHeight", 140, K::Real),
    ("centerMark", 141, K::Real),
    ("tickSize", 142, K::Real),
    ("altFactor", 143, K::Real),
    ("linearFactor", 144, K::Real),
    ("tolScale", 146, K::Real),
    ("textGap", 147, K::Real),
    ("altRound", 148, K::Real),
    ("tolerance", 71, K::Bool),
    ("limits", 72, K::Bool),
    ("textInsideHorizontal", 73, K::Bool),
    ("textOutsideHorizontal", 74, K::Bool),
    ("suppressExt1", 75, K::Bool),
    ("suppressExt2", 76, K::Bool),
    ("textAbove", 77, K::Int),
    ("zeroSuppression", 78, K::Int),
    ("alt", 170, K::Bool),
    ("altDecimals", 171, K::Int),
    ("dimLineColor", 176, K::Color),
    ("extLineColor", 177, K::Color),
    ("textColor", 178, K::Color),
    ("angularDecimals", 179, K::Int),
    ("decimals", 271, K::Int),
    ("tolDecimals", 272, K::Int),
    ("altUnit", 273, K::Int),
    ("angularUnit", 275, K::Int),
    ("fractionFormat", 276, K::Int),
    ("linearUnit", 277, K::Int),
    ("decimalSeparator", 278, K::Char),
    ("textJust", 280, K::Int),
    ("tolZeroSuppression", 284, K::Int),
    ("altZeroSuppression", 285, K::Int),
    ("textStyle", 340, K::TextStyle),
    ("arrowBlock", 342, K::Block),
    ("arrowBlock1", 343, K::Block),
    ("arrowBlock2", 344, K::Block),
    ("fixedExtLength", 49, K::Real),
    ("jogAngle", 50, K::Angle),
    // DIMTFILL and DIMTFILLCLR (Open Design Specification for .dwg files 5.4.1, DIMSTYLE). In a
    // DIMSTYLE record 70 also holds the record's flags, which come first.
    ("textFill", 69, K::Int),
    ("textFillColor", 70, K::Color),
    ("angularZeroSuppression", 79, K::Int),
    ("arcSymbol", 90, K::Int),
    ("textVerticalPos", 145, K::Real),
    ("forceLineInside", 172, K::Bool),
    ("textInside", 174, K::Bool),
    ("suppressOutsideArrows", 175, K::Bool),
    ("textMove", 279, K::Int),
    ("suppressDim1", 281, K::Bool),
    ("suppressDim2", 282, K::Bool),
    ("tolJust", 283, K::Int),
    ("fit", 289, K::Int),
    ("fixedExt", 290, K::Bool),
    ("leaderArrowBlock", 341, K::Block),
    ("dimLinetype", 345, K::Linetype),
    ("extLinetype1", 346, K::Linetype),
    ("extLinetype2", 347, K::Linetype),
    ("dimLineweight", 371, K::Int),
    ("extLineweight", 372, K::Int),
];

/// DIMSAH: separate arrow blocks in use.
pub(crate) const DIMSAH: i32 = 173;
/// DIMPOST's group in DIMSTYLE records.
pub(crate) const DIMPOST: i32 = 3;

/// Variables newer than R2000: DIMFXL, DIMJOGANG, DIMTFILL, DIMTFILLCLR, DIMARCSYM, DIMFXLON,
/// DIMLTYPE, DIMLTEX1, DIMLTEX2. Files of a variable's [`newer_since`] version or later get it as
/// a DIMSTYLE group (R2004, and R2007 for DIMTFILL and DIMTFILLCLR); older files get it in a
/// style's `CADCRAFT` "DIMVARS" xdata (`1070 <code>` then the value, like DSTYLE) instead of a
/// group an older reader doesn't expect. The reader takes both.
pub(crate) const NEWER_CODES: &[i32] = &[49, 50, 69, 70, 90, 290, 345, 346, 347];
/// The first version with [`NEWER_CODES`] as DIMSTYLE groups, except DIMTFILL and DIMTFILLCLR
/// (see [`newer_since`]): the others came with AutoCAD 2006, whose files are the 2004 format
/// (AC1018).
pub(crate) const NEWER_SINCE: Version = Version::R2004;

/// The first version that has `code` (one of [`NEWER_CODES`]) as a DIMSTYLE group: R2007 for
/// DIMTFILL and DIMTFILLCLR (Open Design Specification for .dwg files 5.4.1), else
/// [`NEWER_SINCE`].
pub(crate) fn newer_since(code: i32) -> Version {
    if matches!(code, 69 | 70) { Version::R2007 } else { NEWER_SINCE }
}
/// Xdata list name of [`NEWER_CODES`] values.
pub(crate) const DIMVARS_LIST: &str = "DIMVARS";

pub(crate) fn dim_code(field: &str) -> Option<(i32, K)> {
    DIM_CODES.iter().find(|(f, _, _)| *f == field).map(|(_, c, k)| (*c, *k))
}
pub(crate) fn dim_field(code: i32) -> Option<(&'static str, K)> {
    DIM_CODES.iter().find(|(_, c, _)| *c == code).map(|(f, _, k)| (*f, *k))
}

/// The canonical JSON of a dimension-variable value (as a serialised [`DimStyle`] holds it),
/// or `None` when the value is unusable for the field.
pub(crate) fn canonical(field: &str, v: &Value) -> Option<Value> {
    let field = DimStyle::field_name(field)?;
    let mut st = DimStyle::default();
    let mut m = serde_json::Map::new();
    m.insert(field.to_string(), v.clone());
    if !st.apply_fields(&m).is_empty() {
        return None;
    }
    serde_json::to_value(&st).ok()?.get(field).cloned()
}

/// A dimension variable encoded for a group: real, integer, string or handle.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DimVal {
    Real(f64),
    Int(i64),
    Str(String),
    Handle(String),
}

/// Name → handle lookups of the writer: text styles, arrowhead blocks and linetypes.
pub(crate) type Lookup<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Encode a canonical field value. `style` / `block` / `ltype` map names to handles.
pub(crate) fn encode(kind: K, v: &Value, style: Lookup, block: Lookup, ltype: Lookup) -> Option<DimVal> {
    Some(match kind {
        K::Real => DimVal::Real(v.as_f64().filter(|x| x.is_finite())?),
        K::Int => DimVal::Int(v.as_i64()?),
        K::Bool => DimVal::Int(i64::from(v.as_bool()?)),
        K::Str => DimVal::Str(v.as_str()?.to_string()),
        K::Color => DimVal::Int(i64::from(serde_json::from_value::<Color>(v.clone()).ok()?.to_aci())),
        K::Char => DimVal::Int(i64::from(u32::from(v.as_str()?.chars().next().unwrap_or('.')))),
        K::TextStyle => DimVal::Handle(style(v.as_str()?)?),
        K::Block => {
            let name = v.as_str()?;
            DimVal::Handle(if name.trim().is_empty() { "0".to_string() } else { block(name)? })
        }
        K::Linetype => DimVal::Handle(ltype(v.as_str().filter(|n| !n.trim().is_empty())?)?),
        K::Angle => DimVal::Real(v.as_f64().filter(|x| x.is_finite())?.to_radians()),
    })
}

/// Decode a group value into the JSON a [`DimStyle`] field takes. `styles` / `blocks` /
/// `ltypes` map upper-case handles to names.
pub(crate) fn decode(
    kind: K,
    t: &Tag,
    styles: &HashMap<String, String>,
    blocks: &HashMap<String, String>,
    ltypes: &HashMap<String, String>,
) -> Option<Value> {
    Some(match kind {
        K::Real => {
            let x = t.f64();
            if !x.is_finite() {
                return None;
            }
            serde_json::json!(x)
        }
        K::Int => serde_json::json!(t.i64()),
        K::Bool => Value::Bool(t.i64() != 0),
        K::Str => Value::String(t.str()),
        K::Color => serde_json::to_value(Color::from_aci(i16::try_from(t.i64()).ok()?)).ok()?,
        K::Char => Value::String(char::from_u32(u32::try_from(t.i64()).ok()?).filter(|c| !c.is_control())?.to_string()),
        K::TextStyle => Value::String(styles.get(&t.str().trim().to_ascii_uppercase())?.clone()),
        K::Block => {
            let h = t.str().trim().to_ascii_uppercase();
            if h.is_empty() || h == "0" { Value::String(String::new()) } else { Value::String(blocks.get(&h)?.clone()) }
        }
        K::Linetype => {
            let h = t.str().trim().to_ascii_uppercase();
            if h.is_empty() || h == "0" { Value::String(String::new()) } else { Value::String(ltypes.get(&h)?.clone()) }
        }
        K::Angle => {
            // Rounded so that whole degrees come back whole.
            let x = (t.f64().to_degrees() * 1e9).round() / 1e9;
            if !x.is_finite() {
                return None;
            }
            serde_json::json!(x)
        }
    })
}

/// Block names of the arrowheads (the DIMBLK names of the DXF Reference); geometry is ours.
pub(crate) const ARROWS: &[(Arrowhead, &str)] = &[
    (Arrowhead::ClosedFilled, "_ClosedFilled"),
    (Arrowhead::ClosedBlank, "_ClosedBlank"),
    (Arrowhead::Closed, "_Closed"),
    (Arrowhead::Dot, "_Dot"),
    (Arrowhead::DotSmall, "_DotSmall"),
    (Arrowhead::DotBlank, "_DotBlank"),
    (Arrowhead::Oblique, "_Oblique"),
    (Arrowhead::ArchTick, "_ArchTick"),
    (Arrowhead::Open, "_Open"),
    (Arrowhead::Open90, "_Open90"),
    (Arrowhead::Open30, "_Open30"),
    (Arrowhead::Origin, "_Origin"),
    (Arrowhead::Origin2, "_Origin2"),
    (Arrowhead::Small, "_Small"),
    (Arrowhead::BoxFilled, "_BoxFilled"),
    (Arrowhead::BoxBlank, "_BoxBlank"),
    (Arrowhead::DatumFilled, "_DatumFilled"),
    (Arrowhead::DatumBlank, "_DatumBlank"),
    (Arrowhead::Integral, "_Integral"),
    (Arrowhead::None, "_None"),
];

/// True for a standard arrowhead block name (case-insensitive).
pub(crate) fn is_standard_arrow(name: &str) -> bool {
    ARROWS.iter().any(|(_, n)| n.eq_ignore_ascii_case(name))
}

/// The block an arrow name is written as when no user block has that name: names starting
/// with `_` are kept (block names are case-insensitive), other recognised names become the
/// standard name; unrecognised names (which draw as closed filled) give `None`.
pub(crate) fn arrow_block_name(name: &str) -> Option<(String, Arrowhead)> {
    let n = name.trim();
    if n.is_empty() {
        return None;
    }
    let kind = Arrowhead::parse(n);
    let norm: String = n.trim_start_matches('_').chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase();
    if kind == Arrowhead::ClosedFilled && !matches!(norm.as_str(), "closedfilled" | "filled") {
        return None;
    }
    let std_name = ARROWS.iter().find(|(k, _)| *k == kind).map(|(_, s)| s.to_string())?;
    let keep = n.starts_with('_') && n.chars().skip(1).all(|c| c.is_ascii_alphanumeric());
    Some((if keep { n.to_string() } else { std_name }, kind))
}

/// Applications whose xdata carries layer properties: transparency and description.
pub(crate) const LAYER_TRANSPARENCY_APP: &str = "AcCmTransparency";
pub(crate) const LAYER_DESCRIPTION_APP: &str = "AcAecLayerStandard";

/// A transparency percentage (0..=90) as an `AcCmTransparency` value: the alpha with the
/// "by alpha" flag (0x02 in the top byte).
pub(crate) fn transparency_to_dxf(percent: u8) -> i64 {
    let t = u32::from(percent.min(90));
    i64::from(0x0200_0000 | ((100 - t) * 255 / 100))
}

/// The percentage of an `AcCmTransparency` value; `None` unless it is "by alpha".
pub(crate) fn transparency_from_dxf(v: i64) -> Option<u8> {
    if (v >> 24) & 0xff != 2 {
        return None;
    }
    let alpha = u32::try_from(v & 0xff).ok()?;
    let opaque = (alpha * 100 + 127) / 255;
    u8::try_from(100u32.saturating_sub(opaque).min(90)).ok()
}

/// An xdata string cut to the 255 bytes a `1000` group may hold, at a character boundary.
pub(crate) fn xdata_str(s: &str) -> &str {
    let mut end = s.len().min(255);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or_default()
}

/// The extended data of one application: the groups after its `1001` up to the next `1001`.
pub(crate) fn xdata<'a>(tags: &'a [Tag], app: &str) -> &'a [Tag] {
    let Some(start) = tags.iter().position(|t| t.code == 1001 && t.str().trim().eq_ignore_ascii_case(app)) else { return &[] };
    let rest = tags.get(start + 1..).unwrap_or(&[]);
    let end = rest.iter().position(|t| t.code == 1001).unwrap_or(rest.len());
    rest.get(..end).unwrap_or(&[])
}

/// The groups inside `1000 <name>` `1002 {` … `1002 }` of an xdata run (nested braces kept).
pub(crate) fn xdata_list<'a>(x: &'a [Tag], name: &str) -> &'a [Tag] {
    let Some(at) = x.iter().position(|t| t.code == 1000 && t.str().trim().eq_ignore_ascii_case(name)) else { return &[] };
    let Some(open) = x.get(at + 1).filter(|t| t.code == 1002 && t.str().trim() == "{") else { return &[] };
    let _ = open;
    let body = x.get(at + 2..).unwrap_or(&[]);
    let mut depth = 0usize;
    for (i, t) in body.iter().enumerate() {
        if t.code == 1002 {
            match t.str().trim() {
                "{" => depth += 1,
                "}" if depth == 0 => return body.get(..i).unwrap_or(&[]),
                "}" => depth -= 1,
                _ => {}
            }
        }
    }
    body
}

/// Owner handle of a record: the first 330 outside `102 {…}` groups.
pub(crate) fn owner(tags: &[Tag]) -> Option<String> {
    let mut in_group = false;
    for t in tags {
        match t.code {
            102 => in_group = t.str().trim_start().starts_with('{'),
            330 if !in_group => return Some(t.str()),
            c if c >= 1000 => return None,
            _ => {}
        }
    }
    None
}

/// Per-dimension overrides from the `ACAD` "DSTYLE" xdata: `1070 <code>` followed by the value.
pub(crate) fn read_dstyle(
    tags: &[Tag],
    styles: &HashMap<String, String>,
    blocks: &HashMap<String, String>,
    ltypes: &HashMap<String, String>,
) -> serde_json::Map<String, Value> {
    read_dimvar_list(xdata_list(xdata(tags, "ACAD"), "DSTYLE"), styles, blocks, ltypes)
}

/// A style's [`NEWER_CODES`] variables from its `CADCRAFT` "DIMVARS" xdata.
pub(crate) fn read_newer_dimvars(
    tags: &[Tag],
    styles: &HashMap<String, String>,
    blocks: &HashMap<String, String>,
    ltypes: &HashMap<String, String>,
) -> serde_json::Map<String, Value> {
    read_dimvar_list(xdata_list(xdata(tags, APP), DIMVARS_LIST), styles, blocks, ltypes)
}

/// `1070 <code>` / value pairs of dimension variables.
fn read_dimvar_list(
    list: &[Tag],
    styles: &HashMap<String, String>,
    blocks: &HashMap<String, String>,
    ltypes: &HashMap<String, String>,
) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    let mut it = list.iter().take(MAX_XDATA_ITEMS * 2);
    while let Some(t) = it.next() {
        if t.code != 1070 {
            continue;
        }
        let code = t.i64();
        let Some(v) = it.next() else { break };
        let Some((field, kind)) = i32::try_from(code).ok().and_then(dim_field) else { continue };
        if let Some(val) = decode(kind, v, styles, blocks, ltypes).and_then(|j| canonical(field, &j)) {
            out.insert(field.to_string(), val);
        }
    }
    out
}

/// Xdata marker of the paper-space viewport (id 1) the writer adds to layouts that have none;
/// CADCraft keeps that viewport implicit, so the reader drops it.
pub(crate) const PAPER_VIEW: &str = "PAPERVIEW";

/// True for a VIEWPORT record carrying the [`PAPER_VIEW`] marker as the first string of its CADCraft
/// xdata, where the writer puts it. Later strings are layer names (VPFROZEN, VPCOLORS), so a layer
/// called PAPERVIEW does not count.
pub(crate) fn is_paper_view(tags: &[Tag]) -> bool {
    xdata(tags, APP).iter().find(|t| t.code == 1000).is_some_and(|t| t.str().trim().eq_ignore_ascii_case(PAPER_VIEW))
}

/// Snap kinds in CADCraft's ASSOC xdata.
fn snap_code(s: &AssocSnap) -> i64 {
    match s {
        AssocSnap::Start => 0,
        AssocSnap::End => 1,
        AssocSnap::Mid => 2,
        AssocSnap::Center => 3,
        AssocSnap::OnCircle { .. } => 4,
        AssocSnap::Intersection { .. } => 5,
        AssocSnap::Vertex { .. } => 6,
    }
}

/// Exact associativity as CADCraft xdata groups: per link `1000 point`, `1005 object`,
/// `1070 snap` and the snap's argument (`1040 angle`, `1005 other object`, `1071 vertex`).
pub(crate) fn assoc_xdata(assoc: &[DimAssoc]) -> Vec<Tag> {
    let mut v = vec![Tag::s(1000, "ASSOC"), Tag::s(1002, "{")];
    for a in assoc {
        v.push(Tag::s(1002, "{"));
        v.push(Tag::s(1000, a.point.clone()));
        v.push(Tag::s(1005, a.handle.hex()));
        v.push(Tag::i(1070, snap_code(&a.snap)));
        match &a.snap {
            AssocSnap::OnCircle { angle } => v.push(Tag::f(1040, *angle)),
            AssocSnap::Intersection { other } => v.push(Tag::s(1005, other.hex())),
            AssocSnap::Vertex { index } => v.push(Tag::i(1071, i64::try_from(*index).unwrap_or(i64::from(i32::MAX)).min(i64::from(i32::MAX)))),
            _ => {}
        }
        v.push(Tag::s(1002, "}"));
    }
    v.push(Tag::s(1002, "}"));
    v
}

/// An arc-length dimension as CADCraft xdata: `1000 ARCLEN`, `1002 {`, the arc centre as an xdata
/// point (`1010`/`1020`/`1030`), `1002 }`. The record itself stays a standard aligned dimension
/// (DXF has no arc-length DIMENSION type), so other programs still read it.
pub(crate) fn arclen_xdata(center: cadcraft_geom::Vec3) -> Vec<Tag> {
    vec![Tag::s(1000, "ARCLEN"), Tag::s(1002, "{"), Tag::f(1010, center.x), Tag::f(1020, center.y), Tag::f(1030, center.z), Tag::s(1002, "}")]
}

/// Dimension marks as CADCraft xdata (`1000 MARKS`, `1002 {`, items, `1002 }`): `1000 JOGLINE`
/// and a point; `1000 JOGGED`, the centre and the jog point; `1000 BREAK`, a point, `1040` size
/// and `1070` auto; `1000 INSPECT`, `1070` shape, `1000` label and `1000` rate. Points are
/// `1010/1020/1030`. The oblique angle is the standard group 52 instead.
pub(crate) fn marks_xdata(m: &cadcraft_doc::DimMarks) -> Vec<Tag> {
    let pt = |v: &mut Vec<Tag>, p: cadcraft_geom::Vec3| v.extend([Tag::f(1010, p.x), Tag::f(1020, p.y), Tag::f(1030, p.z)]);
    let mut v = Vec::new();
    if let Some(j) = m.jog_line {
        v.push(Tag::s(1000, "JOGLINE"));
        pt(&mut v, j);
    }
    if let Some(j) = &m.jogged {
        v.push(Tag::s(1000, "JOGGED"));
        pt(&mut v, j.center);
        pt(&mut v, j.jog);
    }
    for b in m.breaks.iter().take(MAX_XDATA_ITEMS / 8) {
        v.push(Tag::s(1000, "BREAK"));
        pt(&mut v, b.at);
        v.extend([Tag::f(1040, b.size), Tag::i(1070, i64::from(b.auto))]);
    }
    if let Some(i) = &m.inspect {
        v.extend([Tag::s(1000, "INSPECT"), Tag::i(1070, i64::from(i.shape)), Tag::s(1000, i.label.clone()), Tag::s(1000, i.rate.clone())]);
    }
    if v.is_empty() {
        return v;
    }
    let mut out = vec![Tag::s(1000, "MARKS"), Tag::s(1002, "{")];
    out.extend(v);
    out.push(Tag::s(1002, "}"));
    out
}

/// The marks written by [`marks_xdata`] (no oblique angle); empty for other writers. Damaged
/// items are skipped.
pub(crate) fn read_marks(tags: &[Tag]) -> cadcraft_doc::DimMarks {
    let list = xdata_list(xdata(tags, APP), "MARKS");
    let list = list.get(..list.len().min(MAX_XDATA_ITEMS)).unwrap_or(&[]);
    let mut m = cadcraft_doc::DimMarks::default();
    let num = |i: usize, code: i32| list.get(i).filter(|t| t.code == code).map(Tag::f64).filter(|x| x.is_finite());
    let point = |i: usize| Some(cadcraft_geom::Vec3::new(num(i, 1010)?, num(i + 1, 1020)?, num(i + 2, 1030)?));
    let text = |i: usize| list.get(i).filter(|t| t.code == 1000).map(Tag::str);
    for (i, t) in list.iter().enumerate() {
        if t.code != 1000 {
            continue;
        }
        match t.str().as_str() {
            "JOGLINE" => m.jog_line = point(i + 1).or(m.jog_line),
            "JOGGED" => {
                if let (Some(center), Some(jog)) = (point(i + 1), point(i + 4)) {
                    m.jogged = Some(cadcraft_doc::JoggedRadius { center, jog });
                }
            }
            "BREAK" => {
                let size = list.get(i + 4).filter(|t| t.code == 1040).map(Tag::f64).filter(|x| x.is_finite() && *x > 0.0);
                let auto = list.get(i + 5).filter(|t| t.code == 1070).is_some_and(|t| t.i64() != 0);
                if let (Some(at), Some(size)) = (point(i + 1), size) {
                    m.breaks.push(cadcraft_doc::DimBreak { at, size, auto });
                }
            }
            "INSPECT" => {
                let shape = list.get(i + 1).filter(|t| t.code == 1070).and_then(|t| u8::try_from(t.i64()).ok()).filter(|s| *s <= 2);
                if let (Some(shape), Some(label), Some(rate)) = (shape, text(i + 2), text(i + 3)) {
                    m.inspect = Some(cadcraft_doc::Inspection { shape, label, rate });
                }
            }
            _ => {}
        }
    }
    m
}

/// The arc centre written by [`arclen_xdata`]; None for other dimensions and other writers.
pub(crate) fn read_arclen(tags: &[Tag]) -> Option<cadcraft_geom::Vec3> {
    let list = xdata_list(xdata(tags, APP), "ARCLEN");
    let g = |c: i32| list.iter().find(|t| t.code == c).map(Tag::f64).filter(|v| v.is_finite());
    Some(cadcraft_geom::Vec3::new(g(1010)?, g(1020)?, g(1030).unwrap_or(0.0)))
}

/// A viewport's frozen layer names as CADCraft xdata: `1000 VPFROZEN`, `1002 {`, one `1000` per layer, `1002 }`.
pub(crate) fn frozen_xdata(layers: &[String]) -> Vec<Tag> {
    let mut v = vec![Tag::s(1000, "VPFROZEN"), Tag::s(1002, "{")];
    v.extend(layers.iter().map(|l| Tag::s(1000, l.clone())));
    v.push(Tag::s(1002, "}"));
    v
}

/// The frozen layer names written by [`frozen_xdata`]; empty for files from other writers.
pub(crate) fn read_frozen(tags: &[Tag]) -> Vec<String> {
    let list = xdata_list(xdata(tags, APP), "VPFROZEN");
    list.iter().take(MAX_XDATA_ITEMS).filter(|t| t.code == 1000).map(Tag::str).collect()
}

/// A viewport's layer colour overrides as CADCraft xdata: `1000 VPCOLORS`, `1002 {`, then per override
/// `1000 layer` and `1000 colour` (the [`Color::name`] text, which keeps true colours), `1002 }`.
pub(crate) fn layer_colors_xdata(colors: &[(String, Color)]) -> Vec<Tag> {
    let mut v = vec![Tag::s(1000, "VPCOLORS"), Tag::s(1002, "{")];
    for (layer, color) in colors {
        v.push(Tag::s(1000, layer.clone()));
        v.push(Tag::s(1000, color.name()));
    }
    v.push(Tag::s(1002, "}"));
    v
}

/// The overrides written by [`layer_colors_xdata`]; empty for files from other writers, bad pairs are skipped.
pub(crate) fn read_layer_colors(tags: &[Tag]) -> Vec<(String, Color)> {
    let list = xdata_list(xdata(tags, APP), "VPCOLORS");
    let names: Vec<String> = list.iter().take(MAX_XDATA_ITEMS * 2).filter(|t| t.code == 1000).map(Tag::str).collect();
    names.as_chunks::<2>().0.iter().filter_map(|[name, color]| Some((name.clone(), Color::parse(color)?))).collect()
}

const POINT_NAMES: [&str; 5] = ["defpt", "p13", "p14", "p15", "p16"];

/// Associativity links from CADCraft xdata; malformed links are skipped.
pub(crate) fn read_assoc(tags: &[Tag]) -> Vec<DimAssoc> {
    let list = xdata_list(xdata(tags, APP), "ASSOC");
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(t) = list.get(i) {
        if out.len() >= MAX_XDATA_ITEMS {
            break;
        }
        i += 1;
        if !(t.code == 1002 && t.str().trim() == "{") {
            continue;
        }
        let end = list.get(i..).and_then(|r| r.iter().position(|x| x.code == 1002)).map(|p| i + p).unwrap_or(list.len());
        let body = list.get(i..end).unwrap_or(&[]);
        i = end + 1;
        let point = body.iter().find(|x| x.code == 1000).map(Tag::str).unwrap_or_default();
        let Some(point) = POINT_NAMES.iter().find(|p| **p == point.trim()) else { continue };
        let handles: Vec<Handle> = body.iter().filter(|x| x.code == 1005).filter_map(|x| Handle::parse_hex(&x.str())).collect();
        let Some(&handle) = handles.first() else { continue };
        let Some(code) = body.iter().find(|x| x.code == 1070).map(Tag::i64) else { continue };
        let snap = match code {
            0 => AssocSnap::Start,
            1 => AssocSnap::End,
            2 => AssocSnap::Mid,
            3 => AssocSnap::Center,
            4 => match body.iter().find(|x| x.code == 1040).map(Tag::f64).filter(|a| a.is_finite()) {
                Some(angle) => AssocSnap::OnCircle { angle },
                None => continue,
            },
            5 => match handles.get(1) {
                Some(&other) => AssocSnap::Intersection { other },
                None => continue,
            },
            6 => match body.iter().find(|x| x.code == 1071).and_then(|x| usize::try_from(x.i64()).ok()) {
                Some(index) => AssocSnap::Vertex { index },
                None => continue,
            },
            _ => continue,
        };
        out.push(DimAssoc { point: point.to_string(), handle, snap });
    }
    out
}

/// The parametric payload stored in the `CADCRAFT_CONSTRAINTS` XRECORD.
#[derive(Serialize, Deserialize)]
struct Payload {
    version: u32,
    #[serde(default)]
    constraints: Vec<Constraint>,
    #[serde(default)]
    parametric: Parametric,
}

/// JSON chunks (≤ 250 characters) for the constraint XRECORD, or `None` when the drawing has
/// no parametric data.
pub(crate) fn constraint_chunks(constraints: &[Constraint], parametric: &Parametric) -> Option<Vec<String>> {
    if constraints.is_empty() && *parametric == Parametric::default() {
        return None;
    }
    let json = serde_json::to_string(&Payload { version: 1, constraints: constraints.to_vec(), parametric: parametric.clone() }).ok()?;
    Some(json_chunks(&json))
}

/// JSON text split into XRECORD strings of at most 250 characters. Backslashes are written as
/// `\u005c` so no chunk contains a DXF `\U+` escape.
fn json_chunks(json: &str) -> Vec<String> {
    let mut safe = String::with_capacity(json.len());
    let mut it = json.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('\\') => safe.push_str("\\u005c"),
                Some(n) => {
                    safe.push('\\');
                    safe.push(n);
                }
                None => safe.push('\\'),
            }
        } else {
            safe.push(c);
        }
    }
    let chars: Vec<char> = safe.chars().collect();
    chars.chunks(250).map(|c| c.iter().collect()).collect()
}

/// An associative array's parameters as CADCraft xdata on its block record: `1000 ARRAY`,
/// `1002 {`, the JSON text in `1000` strings short enough for the 255-byte limit even when
/// characters are written as `\U+XXXX` escapes, `1002 }`.
pub(crate) fn array_xdata(p: &ArrayParams) -> Vec<Tag> {
    let Ok(json) = serde_json::to_string(p) else { return Vec::new() };
    let safe = json_chunks(&json).concat();
    let mut v = vec![Tag::s(1000, "ARRAY"), Tag::s(1002, "{")];
    let (mut cur, mut cost) = (String::new(), 0usize);
    for c in safe.chars() {
        let n = if c.is_ascii() { 1 } else { 7 * c.len_utf16() };
        if cost + n > 240 {
            v.push(Tag::s(1000, std::mem::take(&mut cur)));
            cost = 0;
        }
        cur.push(c);
        cost += n;
    }
    if !cur.is_empty() {
        v.push(Tag::s(1000, cur));
    }
    v.push(Tag::s(1002, "}"));
    v
}

/// The array parameters written by [`array_xdata`]; `None` for other records and for
/// parameters that are not usable.
pub(crate) fn read_array(tags: &[Tag]) -> Option<ArrayParams> {
    let list = xdata_list(xdata(tags, APP), "ARRAY");
    let mut text = String::new();
    for t in list.iter().take(MAX_XDATA_ITEMS).filter(|t| t.code == 1000) {
        text.push_str(&t.str());
    }
    let p: ArrayParams = serde_json::from_str(&text).ok()?;
    p.is_valid().then_some(p)
}

/// The saved layer states stored in the `CADCRAFT_LAYERSTATES` XRECORD.
#[derive(Serialize, Deserialize)]
struct LayerStatesPayload {
    version: u32,
    #[serde(default)]
    states: Vec<LayerState>,
}

/// JSON chunks for the layer states XRECORD, or `None` when the drawing has no saved layer
/// states.
pub(crate) fn layer_state_chunks(states: &[LayerState]) -> Option<Vec<String>> {
    if states.is_empty() {
        return None;
    }
    let json = serde_json::to_string(&LayerStatesPayload { version: 1, states: states.to_vec() }).ok()?;
    Some(json_chunks(&json))
}

/// Parse the layer states XRECORD text; `None` when it is not a payload we understand.
pub(crate) fn parse_layer_states(text: &str) -> Option<Vec<LayerState>> {
    if text.len() > MAX_PAYLOAD {
        return None;
    }
    let p: LayerStatesPayload = serde_json::from_str(text).ok()?;
    if p.version != 1 || p.states.len() > MAX_LAYER_STATES {
        return None;
    }
    Some(p.states)
}

/// The drawing's plot style tables stored in the `CADCRAFT_PLOTSTYLES` XRECORD.
#[derive(Serialize, Deserialize)]
struct PlotStylesPayload {
    version: u32,
    #[serde(default)]
    tables: Vec<PlotStyleTable>,
}

/// JSON chunks for the plot style tables XRECORD, or `None` when the drawing keeps none.
pub(crate) fn plot_style_chunks(tables: &[PlotStyleTable]) -> Option<Vec<String>> {
    if tables.is_empty() {
        return None;
    }
    let json = serde_json::to_string(&PlotStylesPayload { version: 1, tables: tables.to_vec() }).ok()?;
    Some(json_chunks(&json))
}

/// Parse the plot style tables XRECORD text (tables sanitized); `None` when it is not a
/// payload we understand.
pub(crate) fn parse_plot_styles(text: &str) -> Option<Vec<PlotStyleTable>> {
    if text.len() > MAX_PAYLOAD {
        return None;
    }
    let p: PlotStylesPayload = serde_json::from_str(text).ok()?;
    if p.version != 1 || p.tables.len() > MAX_PLOT_STYLE_TABLES {
        return None;
    }
    Some(p.tables.into_iter().map(PlotStyleTable::sanitized).filter(|t| !t.name.is_empty()).collect())
}

/// Annotative scaling stored in the `CADCRAFT_ANNOTATION` XRECORD: the current annotation scale
/// (CANNOSCALE), ANNOALLVISIBLE, the annotative blocks and the scales each annotative object
/// supports, by handle. The DXF Reference doesn't document how objects refer to their scales,
/// so this is CADCraft's own record; the scale list itself is the standard `ACAD_SCALELIST`.
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct AnnoPayload {
    version: u32,
    #[serde(default)]
    cannoscale: Option<String>,
    #[serde(default)]
    annoallvisible: Option<i64>,
    #[serde(default)]
    blocks: Vec<String>,
    #[serde(default)]
    objects: Vec<AnnoObject>,
}

#[derive(Serialize, Deserialize)]
struct AnnoObject {
    handle: String,
    scales: Vec<cadcraft_doc::AnnoScale>,
}

/// Most annotative objects read from one file.
const MAX_ANNO_OBJECTS: usize = 1_000_000;

/// JSON chunks for the annotative scaling XRECORD, or `None` when nothing differs from a new
/// drawing (1:1, ANNOALLVISIBLE 1, no annotative objects or blocks).
pub(crate) fn anno_chunks(d: &cadcraft_doc::Drawing) -> Option<Vec<String>> {
    let cannoscale = d.header.str("CANNOSCALE", "1:1");
    let annoallvisible = d.header.i64("ANNOALLVISIBLE", 1);
    let blocks: Vec<String> = d.blocks.values().filter(|b| b.annotative).map(|b| b.name.clone()).collect();
    let objects: Vec<AnnoObject> = d
        .model
        .iter()
        .chain(d.layouts.iter().flat_map(|l| l.entities.iter()))
        .filter(|e| cadcraft_doc::is_annotative(e))
        .map(|e| AnnoObject { handle: e.handle.hex(), scales: e.common.anno_scales.clone() })
        .collect();
    if cannoscale == "1:1" && annoallvisible == 1 && blocks.is_empty() && objects.is_empty() {
        return None;
    }
    let p = AnnoPayload { version: 1, cannoscale: Some(cannoscale), annoallvisible: Some(annoallvisible), blocks, objects };
    Some(json_chunks(&serde_json::to_string(&p).ok()?))
}

/// Parse the annotative scaling XRECORD text; `None` when it is not a payload we understand.
pub(crate) fn parse_anno(text: &str) -> Option<AnnoPayload> {
    if text.len() > MAX_PAYLOAD {
        return None;
    }
    let p: AnnoPayload = serde_json::from_str(text).ok()?;
    (p.version == 1 && p.objects.len() <= MAX_ANNO_OBJECTS && p.blocks.len() <= MAX_ANNO_OBJECTS).then_some(p)
}

/// Apply a parsed annotative scaling record: variables, annotative blocks and each object's
/// scales (invalid scales and objects that can't be annotative are skipped).
pub(crate) fn apply_anno(d: &mut cadcraft_doc::Drawing, p: AnnoPayload) {
    if let Some(c) = p.cannoscale.filter(|c| !c.trim().is_empty() && c.len() <= 255) {
        d.header.set_str("CANNOSCALE", &c);
    }
    if let Some(v) = p.annoallvisible {
        d.header.set_i64("ANNOALLVISIBLE", i64::from(v != 0));
    }
    // Block names compare case-insensitively; one map keeps a long hostile list linear.
    let by_upper: std::collections::HashMap<String, String> = d.blocks.keys().map(|k| (k.to_ascii_uppercase(), k.clone())).collect();
    for name in &p.blocks {
        let key = by_upper.get(&name.to_ascii_uppercase()).cloned();
        if let Some(b) = key.and_then(|k| d.blocks.get_mut(&k)) {
            std::sync::Arc::make_mut(b).annotative = true;
        }
    }
    for o in p.objects {
        let Some(h) = Handle::parse_hex(&o.handle) else { continue };
        let mut scales: Vec<cadcraft_doc::AnnoScale> = Vec::new();
        for s in o.scales.into_iter().filter(|s| s.is_valid() && !s.name.trim().is_empty()) {
            if scales.len() < cadcraft_doc::MAX_SCALES && !scales.iter().any(|x| x.name.eq_ignore_ascii_case(&s.name)) {
                scales.push(s);
            }
        }
        let _ = d.modify_entity(h, |e| {
            if cadcraft_doc::can_be_annotative(&e.kind) {
                e.common.anno_scales = scales;
            }
        });
    }
}

/// A SCALE object (DXF Reference, OBJECTS section): name (300), paper units (140), drawing
/// units (141); `None` when unusable.
pub(crate) fn read_scale(tags: &[Tag]) -> Option<cadcraft_doc::AnnoScale> {
    let name = tags.iter().find(|t| t.code == 300).map(Tag::str)?;
    let num = |c: i32| tags.iter().find(|t| t.code == c).map(Tag::f64);
    let s = cadcraft_doc::AnnoScale::new(name.trim(), num(140)?, num(141)?);
    (s.is_valid() && !s.name.is_empty() && s.name.len() <= 255).then_some(s)
}

/// Parse the constraint XRECORD text; `None` when it is not a payload we understand.
pub(crate) fn parse_constraints(text: &str) -> Option<(Vec<Constraint>, Parametric)> {
    if text.len() > MAX_PAYLOAD {
        return None;
    }
    let p: Payload = serde_json::from_str(text).ok()?;
    if p.version != 1 || p.constraints.len() > MAX_CONSTRAINTS {
        return None;
    }
    Some((p.constraints, p.parametric))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_dimvar_has_a_code() {
        for (_, field) in cadcraft_doc::DIMVARS {
            assert!(dim_code(field).is_some(), "{field} has no DXF group code");
        }
    }

    #[test]
    fn arrow_names() {
        assert_eq!(arrow_block_name("_DOT").map(|x| x.0).as_deref(), Some("_DOT"));
        assert_eq!(arrow_block_name("Architectural tick").map(|x| x.0).as_deref(), Some("_ArchTick"));
        assert!(arrow_block_name("").is_none());
        assert!(arrow_block_name("nonsense").is_none());
        assert!(is_standard_arrow("_dot"));
    }
}
