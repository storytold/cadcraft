use super::{DimVal, Drawing, HVal, K, Version, W, dxf_ext};

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

pub(super) fn header_vars(w: &mut W, d: &Drawing) {
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

/// `$DWGCODEPAGE`: before 2007 the code page read from the file when CADCraft knows it (the text
/// is ASCII with `\U+XXXX` escapes, right in any code page, but the file keeps its declared one);
/// otherwise, and from 2007 (UTF-8 text), `ANSI_1252`.
pub(super) fn code_page(d: &Drawing, version: Version) -> String {
    let read = d.header.get("DWGCODEPAGE").and_then(HVal::as_str).map(str::trim).unwrap_or_default();
    let known = read.is_ascii() && read.len() <= 32 && cadcraft_dxf::decode_code_page(b"", read).is_some();
    if version < Version::R2007 && known { read.to_string() } else { "ANSI_1252".into() }
}
