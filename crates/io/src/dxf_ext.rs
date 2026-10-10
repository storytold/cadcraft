//! DXF data shared by the reader and the writer beyond plain entity geometry:
//! dimension-variable group codes (DIMSTYLE records and the `ACAD` "DSTYLE" override xdata,
//! both per the DXF Reference), arrowhead block names, extended-data helpers and the
//! CADCraft-owned payloads (exact associativity, table flags, parametric constraints).
//!
//! CADCraft's own data lives under the registered application `CADCRAFT` (xdata) and the
//! named-object-dictionary entry `CADCRAFT_CONSTRAINTS` (an XRECORD). Other readers keep or
//! ignore both.

use std::collections::HashMap;

use cadcraft_color::Color;
use cadcraft_doc::{AssocSnap, Constraint, DimAssoc, DimStyle, Handle, Parametric};
use cadcraft_dxf::Tag;
use cadcraft_render::Arrowhead;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Registered application name of CADCraft's extended data.
pub(crate) const APP: &str = "CADCRAFT";
/// Named-object-dictionary key of the constraint XRECORD.
pub(crate) const CONSTRAINTS_KEY: &str = "CADCRAFT_CONSTRAINTS";

/// Caps for hostile input.
pub(crate) const MAX_XDATA_ITEMS: usize = 4096;
pub(crate) const MAX_PAYLOAD: usize = 64 << 20;
pub(crate) const MAX_CONSTRAINTS: usize = 1_000_000;

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
    /// Handle of an arrowhead BLOCK_RECORD (DIMBLK, DIMBLK1, DIMBLK2).
    Block,
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
    ("angularUnit", 275, K::Int),
    ("fractionFormat", 276, K::Int),
    ("linearUnit", 277, K::Int),
    ("decimalSeparator", 278, K::Char),
    ("textJust", 280, K::Int),
    ("textStyle", 340, K::TextStyle),
    ("arrowBlock", 342, K::Block),
    ("arrowBlock1", 343, K::Block),
    ("arrowBlock2", 344, K::Block),
];

/// DIMSAH: separate arrow blocks in use.
pub(crate) const DIMSAH: i32 = 173;

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

/// Encode a canonical field value. `style` / `block` map names to handles.
pub(crate) fn encode(kind: K, v: &Value, style: &dyn Fn(&str) -> Option<String>, block: &dyn Fn(&str) -> Option<String>) -> Option<DimVal> {
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
    })
}

/// Decode a group value into the JSON a [`DimStyle`] field takes. `styles` / `blocks` map
/// upper-case handles to names.
pub(crate) fn decode(kind: K, t: &Tag, styles: &HashMap<String, String>, blocks: &HashMap<String, String>) -> Option<Value> {
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
    let norm: String = n.trim_start_matches('_').chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    if kind == Arrowhead::ClosedFilled && !matches!(norm.as_str(), "closedfilled" | "filled") {
        return None;
    }
    let std_name = ARROWS.iter().find(|(k, _)| *k == kind).map(|(_, s)| s.to_string())?;
    let keep = n.starts_with('_') && n.chars().skip(1).all(|c| c.is_ascii_alphanumeric());
    Some((if keep { n.to_string() } else { std_name }, kind))
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
pub(crate) fn read_dstyle(tags: &[Tag], styles: &HashMap<String, String>, blocks: &HashMap<String, String>) -> serde_json::Map<String, Value> {
    let list = xdata_list(xdata(tags, "ACAD"), "DSTYLE");
    let mut out = serde_json::Map::new();
    let mut it = list.iter().take(MAX_XDATA_ITEMS * 2);
    while let Some(t) = it.next() {
        if t.code != 1070 {
            continue;
        }
        let code = t.i64();
        let Some(v) = it.next() else { break };
        let Some((field, kind)) = i32::try_from(code).ok().and_then(dim_field) else { continue };
        if let Some(val) = decode(kind, v, styles, blocks).and_then(|j| canonical(field, &j)) {
            out.insert(field.to_string(), val);
        }
    }
    out
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
/// no parametric data. Backslashes are written as `\` so no chunk contains a DXF
/// `\U+` escape.
pub(crate) fn constraint_chunks(constraints: &[Constraint], parametric: &Parametric) -> Option<Vec<String>> {
    if constraints.is_empty() && *parametric == Parametric::default() {
        return None;
    }
    let json = serde_json::to_string(&Payload { version: 1, constraints: constraints.to_vec(), parametric: parametric.clone() }).ok()?;
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
    Some(chars.chunks(250).map(|c| c.iter().collect()).collect())
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
