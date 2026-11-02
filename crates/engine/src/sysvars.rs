//! System variables: session settings plus drawing header variables, by name.

use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

mod table;

const SESSION_VARS: &[&str] = &[
    "AUDITCTL",
    "ATTREQ",
    "OSMODE",
    "OSNAPHATCH",
    "ANNOAUTOSCALE",
    "ORTHOMODE",
    "POLARMODE",
    "POLARANG",
    "POLARADDANG",
    "POLARDIST",
    "SNAPTYPE",
    "AUTOSNAP",
    "QNEWTEMPLATE",
    "GRIDMODE",
    "SNAPMODE",
    "SNAPSTYL",
    "SNAPISOPAIR",
    "SNAPUNIT",
    "GRIDUNIT",
    "UNDOCTL",
    "UNDOMARKS",
    "GRIDMAJOR",
    "DYNDIGRIP",
    "DYNDIVIS",
    "DYNMODE",
    "DYNPIFORMAT",
    "DYNPICOORDS",
    "DYNPROMPT",
    "LWDISPLAY",
    "PLOTTRANSPARENCYOVERRIDE",
    "SELECTIONCYCLING",
    "SELECTSIMILARMODE",
    "OBJECTISOLATIONMODE",
    "QPMODE",
    "HIGHLIGHT",
    "SELECTIONEFFECT",
    "SELECTIONPREVIEW",
    "PREVIEWEFFECT",
    "PREVIEWFILTER",
    "GRIPS",
    "GRIPOBJLIMIT",
    "SELECTIONAREA",
    "WINDOWAREACOLOR",
    "CROSSINGAREACOLOR",
    "SELECTIONAREAOPACITY",
    "PICKBOX",
    "APERTURE",
    "PICKFIRST",
    "ACADLSPASDOC",
    "CMDECHO",
    "FILEDIA",
    "PICKADD",
    "PICKDRAG",
    "PICKSTYLE",
    "GRIPSIZE",
    "CURSORSIZE",
    "MAXARRAY",
    "ZOOMFACTOR",
    "MEASUREINIT",
    "LASTPOINT",
    "FONTALT",
    "FONTFALLBACK",
    "SAVETIME",
    "SAVEFILEPATH",
    "ISAVEBAK",
];

pub fn get(s: &Session, name: &str) -> Option<Value> {
    let n = name.trim().to_ascii_uppercase();
    let st = &s.settings;
    let v = match n.as_str() {
        "OSMODE" => json!(st.osmode),
        "OSNAPHATCH" => json!(i32::from(st.osnaphatch)),
        // AutoCAD's values: positive on, negative off (4 = every annotative object).
        "ANNOAUTOSCALE" => json!(if st.annoautoscale { 4 } else { -4 }),
        // Paper units per drawing unit of the current annotation scale (1:50 → 0.02).
        "CANNOSCALEVALUE" => json!(1.0 / s.doc().ok()?.current_anno_scale().factor()),
        "ORTHOMODE" => json!(i32::from(st.orthomode)),
        "POLARMODE" => json!(st.polar_flags),
        "POLARADDANG" => json!(format_angle_list(&st.polaraddang)),
        "POLARDIST" => json!(st.polardist),
        "SNAPTYPE" => json!(i32::from(st.polarsnap)),
        // Marker, tooltip and tracking tooltip always on (1 + 4 + 32); polar 8, tracking 16.
        "AUTOSNAP" => json!(37 + 8 * u32::from(st.polarmode) + 16 * u32::from(st.otrack)),
        "POLARANG" => json!(st.polarang.to_degrees()),
        "GRIDMODE" => json!(i32::from(st.gridmode)),
        "SNAPMODE" => json!(i32::from(st.snapmode)),
        "SNAPSTYL" => json!(i32::from(st.isodraft)),
        "SNAPISOPAIR" => json!(st.isoplane.index()),
        "SNAPUNIT" | "GRIDUNIT" => {
            let settings = s.drafting_settings();
            let unit = if n == "SNAPUNIT" { settings.snapunit } else { settings.gridunit };
            json!([unit.x, unit.y])
        }
        "GRIDMAJOR" => json!(st.gridmajor),
        "DYNDIGRIP" => json!(st.dyndigrip),
        "DYNDIVIS" => json!(st.dyndivis),
        "DYNMODE" => json!(i32::from(st.dynmode)),
        "DYNPIFORMAT" => json!(i32::from(st.dynpi_cartesian)),
        "DYNPICOORDS" => json!(i32::from(st.dynpi_absolute)),
        "DYNPROMPT" => json!(i32::from(st.dynprompt)),
        "LWDISPLAY" => json!(i32::from(st.lwdisplay)),
        "PLOTTRANSPARENCYOVERRIDE" => json!(st.plot_transparency_override),
        // Negative while turned off, so the toggle keeps the display it turns back on.
        "SELECTIONCYCLING" => {
            let d = i32::from(st.selection_cycling_display);
            json!(if st.selection_cycling { d } else { -d })
        }
        "SELECTSIMILARMODE" => json!(st.selectsimilarmode),
        // Isolation is temporary: hidden objects show again when the drawing is reopened.
        "OBJECTISOLATIONMODE" => json!(0),
        "QPMODE" => json!(i32::from(st.qpmode)),
        "HIGHLIGHT" => json!(i32::from(st.highlight)),
        "SELECTIONEFFECT" => json!(st.selectioneffect),
        "SELECTIONPREVIEW" => json!(st.selectionpreview),
        "PREVIEWEFFECT" => json!(st.previeweffect),
        "PREVIEWFILTER" => json!(st.previewfilter),
        "GRIPS" => json!(st.grips),
        "GRIPOBJLIMIT" => json!(st.gripobjlimit),
        "SELECTIONAREA" => json!(i32::from(st.selectionarea)),
        "WINDOWAREACOLOR" => json!(st.windowareacolor),
        "CROSSINGAREACOLOR" => json!(st.crossingareacolor),
        "SELECTIONAREAOPACITY" => json!(st.selectionareaopacity),
        "PICKBOX" => json!(st.pickbox),
        "APERTURE" => json!(st.aperture),
        "PICKFIRST" => json!(i32::from(st.pickfirst)),
        "ACADLSPASDOC" => json!(i32::from(st.acadlspasdoc)),
        "CMDECHO" => json!(i32::from(st.cmdecho)),
        "FILEDIA" => json!(i32::from(st.filedia)),
        "PICKADD" => json!(i32::from(st.pickadd)),
        "PICKDRAG" => json!(i32::from(st.pickdrag) * 2),
        "PICKSTYLE" => json!(st.pickstyle),
        "GRIPSIZE" => json!(st.gripsize),
        "CURSORSIZE" => json!(st.cursorsize),
        "MAXARRAY" => json!(st.maxarray),
        "ZOOMFACTOR" => json!(st.zoomfactor),
        "MEASUREINIT" => json!(i32::from(st.measureinit)),
        "LASTPOINT" => json!([s.last_point.x, s.last_point.y, 0.0]),
        // Held in radians (`$ANGBASE` in DXF is in degrees); reported in degrees, like POLARANG.
        "ANGBASE" => json!(s.doc().ok()?.header.f64("ANGBASE", 0.0).to_degrees()),
        "CONSTRAINTINFER" => json!(i32::from(s.doc().ok()?.parametric.settings.infer)),
        // Text, as AutoCAD reads it back; the header keeps the index (and CECOLOR_RGB) for DXF.
        "CECOLOR" => json!(crate::cmd::current_color_value(s.doc().ok()?)),
        // Process-wide font substitution (profile settings, not saved in the drawing).
        "FONTALT" => json!(cadcraft_fonts::ttf::font_alt()),
        "FONTFALLBACK" => json!(cadcraft_fonts::ttf::fallback_fonts()),
        // Autosave (profile settings too).
        "SAVETIME" => json!(crate::autosave::savetime()),
        "SAVEFILEPATH" => json!(crate::autosave::savefilepath()),
        "ISAVEBAK" => json!(i32::from(crate::autosave::isavebak())),
        "CMDNAMES" => json!(s.running.as_ref().map(|r| r.id.to_ascii_uppercase()).unwrap_or_default()),
        "DWGNAME" => json!(s.state().map(|d| d.title.clone()).unwrap_or_default()),
        "DBMOD" => json!(s.state().map(|d| i32::from(d.is_dirty())).unwrap_or(0)),
        // The drawing's folder with a trailing separator ("" while it has no file); DWGTITLED is 1 once it has one.
        "DWGPREFIX" => json!(s.state().ok().and_then(|d| d.path.as_deref()).map(dwg_prefix).unwrap_or_default()),
        "DWGTITLED" => json!(s.state().map(|d| i32::from(d.path.is_some())).unwrap_or(0)),
        "CMDACTIVE" => json!(i32::from(s.running.is_some())),
        "TILEMODE" => json!(i32::from(s.state().is_ok_and(|d| d.space.layout().is_none()))),
        "PLATFORM" => json!(std::env::consts::OS),
        "BLOCKEDITOR" => json!(i32::from(s.state().is_ok_and(|d| d.block_edit.is_some()))),
        // The template NEW/QNEW start from (a profile setting, like FONTALT; "" = none).
        "QNEWTEMPLATE" => json!(crate::cmd::file::qnew_template()),
        "CTAB" => json!(s.state().map(|d| d.space.tab_name().to_string()).unwrap_or_default()),
        // UNDO state of the drawing: Control/Auto/group bits and the number of marks.
        "UNDOCTL" => json!(s.state().ok()?.undo_ctl.bits()),
        "UNDOMARKS" => json!(s.state().ok()?.undo_marks()),
        "VIEWCTR" => {
            let v = s.state().ok()?.view();
            json!([v.center.x, v.center.y, 0.0])
        }
        "VIEWSIZE" => json!(s.state().ok()?.view().height),
        "EXTMIN" | "EXTMAX" => {
            let d = s.doc().ok()?;
            let e = d.extents(&s.space());
            let p = if n == "EXTMIN" { e.min } else { e.max };
            json!([p.x, p.y, 0.0])
        }
        _ => {
            if let Some(v) = table::get(s, &n) {
                return Some(v);
            }
            let d = s.doc().ok()?;
            match d.header.get(&n) {
                Some(h) => serde_json::to_value(h).ok()?,
                // A dimension variable the header doesn't carry reads from the current style.
                None => {
                    let field = cadcraft_doc::DIMVARS.iter().find(|(v, _)| *v == n)?.1;
                    let st = d.dim_style(&d.header.str("DIMSTYLE", "Standard"))?;
                    serde_json::to_value(st).ok()?.get(field)?.clone()
                }
            }
        }
    };
    Some(v)
}

/// DWGPREFIX: the folder of the drawing file, made absolute, with a trailing separator.
fn dwg_prefix(path: &str) -> String {
    let full = std::path::absolute(path).unwrap_or_else(|_| std::path::PathBuf::from(path));
    match full.parent() {
        Some(dir) => {
            let mut text = dir.to_string_lossy().into_owned();
            if !text.ends_with(std::path::MAIN_SEPARATOR) {
                text.push(std::path::MAIN_SEPARATOR);
            }
            text
        }
        None => String::new(),
    }
}

fn as_bool(v: &Value) -> Option<bool> {
    v.as_bool().or_else(|| v.as_i64().map(|i| i != 0)).or_else(|| v.as_str().and_then(|s| s.trim().parse::<i64>().ok()).map(|i| i != 0))
}
/// QPMODE is -1/0 (off), 1 or 2 (on) in AutoCAD; booleans work too.
fn as_i(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_bool().map(i64::from)).or_else(|| v.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
}
/// A whole number from 0 to `max` (a mode or a small bitcode).
fn small(v: &Value, max: u8) -> Option<u8> {
    as_i(v).filter(|n| (0..=i64::from(max)).contains(n)).and_then(|n| u8::try_from(n).ok())
}
fn as_f64(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(crate::units::parse_distance)).filter(|f| f.is_finite())
}
fn as_pt(v: &Value) -> Option<cadcraft_geom::Vec2> {
    crate::cmd::point_value(v).or_else(|| as_f64(v).map(|f| cadcraft_geom::Vec2::new(f, f)))
}

/// Variables `get` reports from session or drawing state that `set` can't change.
const READ_ONLY: &[&str] = &[
    "CMDNAMES",
    "DWGNAME",
    "DWGPREFIX",
    "DWGTITLED",
    "CMDACTIVE",
    "TILEMODE",
    "PLATFORM",
    "DBMOD",
    "BLOCKEDITOR",
    "CTAB",
    "LASTPOINT",
    "VIEWCTR",
    "VIEWSIZE",
    "EXTMIN",
    "EXTMAX",
    "UNDOCTL",
    "UNDOMARKS",
];

pub fn is_read_only(name: &str) -> bool {
    READ_ONLY.iter().any(|r| r.eq_ignore_ascii_case(name.trim())) || table::is_read_only(&name.trim().to_ascii_uppercase())
}

/// POLARADDANG as typed: up to ten angles in degrees separated by `;` (radians out); `None` when
/// any is not a number or there are too many.
pub fn parse_angle_list(text: &str) -> Option<Vec<f64>> {
    let angles: Option<Vec<f64>> =
        text.split(';').map(str::trim).filter(|t| !t.is_empty()).map(|t| t.parse::<f64>().ok().filter(|a| a.is_finite())).collect();
    Some(angles.filter(|a| a.len() <= 10)?.into_iter().map(f64::to_radians).collect())
}

/// POLARADDANG as shown: degrees separated by `;`.
pub fn format_angle_list(angles: &[f64]) -> String {
    angles.iter().map(|a| format!("{}", (a.to_degrees() * 1e6).round() / 1e6)).collect::<Vec<_>>().join(";")
}

pub fn set(s: &mut Session, name: &str, v: &Value) -> Result<()> {
    let n = name.trim().to_ascii_uppercase();
    if is_read_only(&n) {
        return Err(EngineError::BadParams { cmd: "setvar".into(), msg: format!("{n} is read-only") });
    }
    let bad = || EngineError::BadParams { cmd: "setvar".into(), msg: format!("invalid value for {n}") };
    let st = &mut s.settings;
    match n.as_str() {
        "OSMODE" => {
            st.osmode = v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).filter(|x| *x <= 0x7fff).ok_or_else(bad)? as u32
        }
        // The `color` command's input: ByLayer, ByBlock, a name, an index or a true colour.
        "CECOLOR" => crate::cmd::set_current_color_value(s, v).ok_or_else(bad)?,
        "OSNAPHATCH" => st.osnaphatch = as_bool(v).ok_or_else(bad)?,
        "ANNOAUTOSCALE" => st.annoautoscale = as_i(v).filter(|i| (-4..=4).contains(i)).ok_or_else(bad)? > 0,
        // A scale of the drawing's list; ANNOAUTOSCALE adds it to the annotative objects.
        "CANNOSCALE" => crate::cmd::annoscale::set_current(s, v.as_str().ok_or_else(bad)?)?,
        "CANNOSCALEVALUE" => return Err(EngineError::BadParams { cmd: "setvar".into(), msg: format!("{n} is read-only") }),
        "ORTHOMODE" => st.orthomode = as_bool(v).ok_or_else(bad)?,
        "POLARMODE" => st.polar_flags = as_i(v).filter(|b| (0..=15).contains(b)).ok_or_else(bad)? as u32,
        "POLARADDANG" => {
            let text = v.as_str().map(str::to_string).or_else(|| as_f64(v).map(|f| f.to_string())).ok_or_else(bad)?;
            st.polaraddang = parse_angle_list(&text).ok_or_else(bad)?;
        }
        "POLARDIST" => st.polardist = as_f64(v).filter(|d| *d >= 0.0 && *d < 1e12).ok_or_else(bad)?,
        "SNAPTYPE" => st.polarsnap = as_i(v).filter(|b| (0..=1).contains(b)).ok_or_else(bad)? == 1,
        "AUTOSNAP" => {
            let bits = as_i(v).filter(|b| (0..=63).contains(b)).ok_or_else(bad)?;
            st.polarmode = bits & 8 != 0;
            st.otrack = bits & 16 != 0;
        }
        "QNEWTEMPLATE" => crate::cmd::file::set_qnew_template(v.as_str().ok_or_else(bad)?),
        "POLARANG" => st.polarang = as_f64(v).filter(|a| *a > 0.0).ok_or_else(bad)?.to_radians(),
        "GRIDMODE" => st.gridmode = as_bool(v).ok_or_else(bad)?,
        "SNAPMODE" => st.snapmode = as_bool(v).ok_or_else(bad)?,
        "SNAPSTYL" => st.isodraft = as_i(v).filter(|b| (0..=1).contains(b)).ok_or_else(bad)? == 1,
        "SNAPISOPAIR" => st.isoplane = as_i(v).and_then(crate::snap::iso::IsoPlane::from_index).ok_or_else(bad)?,
        "SNAPUNIT" | "GRIDUNIT" => {
            let unit = as_pt(v)
                .filter(|p| p.is_finite() && if n == "GRIDUNIT" { p.x >= 0.0 && p.y >= 0.0 } else { p.x > 0.0 && p.y > 0.0 })
                .ok_or_else(bad)?;
            s.doc_mut()?.header.set(&n, cadcraft_doc::HVal::Point(unit.to3(0.0)));
        }
        "GRIDMAJOR" => st.gridmajor = as_f64(v).ok_or_else(bad)?.clamp(1.0, 100.0) as u32,
        "DYNDIGRIP" => st.dyndigrip = as_i(v).filter(|i| (0..=31).contains(i)).and_then(|i| u32::try_from(i).ok()).ok_or_else(bad)?,
        "DYNDIVIS" => st.dyndivis = as_i(v).filter(|i| (0..=2).contains(i)).and_then(|i| u8::try_from(i).ok()).ok_or_else(bad)?,
        "DYNMODE" => st.dynmode = as_bool(v).ok_or_else(bad)?,
        "DYNPIFORMAT" => st.dynpi_cartesian = as_bool(v).ok_or_else(bad)?,
        "DYNPICOORDS" => st.dynpi_absolute = as_bool(v).ok_or_else(bad)?,
        "DYNPROMPT" => st.dynprompt = as_bool(v).ok_or_else(bad)?,
        "LWDISPLAY" => st.lwdisplay = as_bool(v).ok_or_else(bad)?,
        "PLOTTRANSPARENCYOVERRIDE" => {
            st.plot_transparency_override = as_i(v).filter(|i| (0..=2).contains(i)).and_then(|i| u8::try_from(i).ok()).ok_or_else(bad)?
        }
        "SELECTIONCYCLING" => {
            let i = as_i(v).filter(|i| (-2..=2).contains(i)).ok_or_else(bad)?;
            st.selection_cycling = i > 0;
            st.selection_cycling_display = u8::try_from(i.unsigned_abs()).map_err(|_| bad())?;
        }
        "SELECTSIMILARMODE" => st.selectsimilarmode = as_i(v).filter(|b| (0..=255).contains(b)).ok_or_else(bad)? as u32,
        "OBJECTISOLATIONMODE" => match as_i(v) {
            Some(0) => {}
            Some(1) => return Err(EngineError::Other("OBJECTISOLATIONMODE 1 (isolation saved with the drawing) is not available yet".into())),
            _ => return Err(bad()),
        },
        "QPMODE" => st.qpmode = as_i(v).ok_or_else(bad)? > 0,
        "HIGHLIGHT" => st.highlight = as_bool(v).ok_or_else(bad)?,
        "SELECTIONEFFECT" => st.selectioneffect = small(v, 1).ok_or_else(bad)?,
        "SELECTIONPREVIEW" => st.selectionpreview = small(v, 3).ok_or_else(bad)?,
        "PREVIEWEFFECT" => st.previeweffect = small(v, 2).ok_or_else(bad)?,
        "PREVIEWFILTER" => st.previewfilter = small(v, 63).ok_or_else(bad)?,
        "GRIPS" => st.grips = small(v, 2).ok_or_else(bad)?,
        "GRIPOBJLIMIT" => st.gripobjlimit = as_i(v).filter(|n| (0..=32_767).contains(n)).and_then(|n| u32::try_from(n).ok()).ok_or_else(bad)?,
        "SELECTIONAREA" => st.selectionarea = as_bool(v).ok_or_else(bad)?,
        "WINDOWAREACOLOR" => st.windowareacolor = small(v, 255).filter(|c| *c >= 1).ok_or_else(bad)?,
        "CROSSINGAREACOLOR" => st.crossingareacolor = small(v, 255).filter(|c| *c >= 1).ok_or_else(bad)?,
        "SELECTIONAREAOPACITY" => st.selectionareaopacity = small(v, 100).ok_or_else(bad)?,
        "PICKBOX" => st.pickbox = as_f64(v).ok_or_else(bad)?.clamp(0.0, 50.0),
        "APERTURE" => st.aperture = as_f64(v).ok_or_else(bad)?.clamp(1.0, 50.0),
        "PICKFIRST" => st.pickfirst = as_bool(v).ok_or_else(bad)?,
        "ACADLSPASDOC" => st.acadlspasdoc = as_bool(v).ok_or_else(bad)?,
        "CMDECHO" => st.cmdecho = small(v, 1).ok_or_else(bad)? == 1,
        "FILEDIA" => st.filedia = small(v, 1).ok_or_else(bad)? == 1,
        "PICKADD" => st.pickadd = as_bool(v).ok_or_else(bad)?,
        "PICKDRAG" => st.pickdrag = as_i(v).filter(|b| matches!(b, 0 | 2)).map(|b| b == 2).ok_or_else(bad)?,
        "PICKSTYLE" => st.pickstyle = as_i(v).filter(|b| (0..=3).contains(b)).and_then(|b| u8::try_from(b).ok()).ok_or_else(bad)?,
        "GRIPSIZE" => st.gripsize = as_f64(v).ok_or_else(bad)?.clamp(1.0, 255.0),
        "CURSORSIZE" => st.cursorsize = as_f64(v).ok_or_else(bad)?.clamp(1.0, 100.0),
        "MAXARRAY" => st.maxarray = as_f64(v).ok_or_else(bad)?.clamp(100.0, 10_000_000.0) as u64,
        "ZOOMFACTOR" => st.zoomfactor = as_i(v).filter(|z| (3..=100).contains(z)).ok_or_else(bad)? as u32,
        "MEASUREINIT" => st.measureinit = small(v, 1).ok_or_else(bad)? == 1,
        "FONTALT" => cadcraft_fonts::ttf::set_font_alt(v.as_str().ok_or_else(bad)?),
        "FONTFALLBACK" => cadcraft_fonts::ttf::set_fallback_fonts(v.as_str().ok_or_else(bad)?),
        "SAVETIME" => crate::autosave::set_savetime(
            as_f64(v).filter(|m| *m >= 0.0 && *m <= f64::from(crate::autosave::MAX_SAVETIME)).ok_or_else(bad)?.round() as u32,
        ),
        "SAVEFILEPATH" => crate::autosave::set_savefilepath(v.as_str().ok_or_else(bad)?),
        "ISAVEBAK" => crate::autosave::set_isavebak(as_bool(v).ok_or_else(bad)?),
        // A number is in degrees; text is an angle typed in AUNITS (SETVAR at the command line).
        "ANGBASE" => {
            let a = match v {
                Value::String(t) => s.angle_settings().amount(t),
                _ => as_f64(v).map(f64::to_radians),
            };
            let a = a.filter(|a| a.is_finite()).ok_or_else(bad)?;
            s.doc_mut()?.header.set_f64("ANGBASE", cadcraft_geom::norm_angle(a));
        }
        // Whole numbers inside the range UNITS clamps to.
        "LUNITS" | "LUPREC" | "AUNITS" | "AUPREC" => {
            let (_, lo, hi) = crate::cmd::UNIT_RANGES.iter().find(|(k, ..)| k.eq_ignore_ascii_case(&n)).copied().ok_or_else(bad)?;
            let i = as_i(v)
                .or_else(|| as_f64(v).filter(|f| f.fract() == 0.0 && f.abs() < 1e15).map(|f| f as i64))
                .filter(|i| (lo..=hi).contains(i))
                .ok_or_else(bad)?;
            s.doc_mut()?.header.set_i64(&n, i);
        }
        "CONSTRAINTINFER" => s.doc_mut()?.parametric.settings.infer = as_bool(v).ok_or_else(bad)?,
        _ => {
            if let Some(r) = table::set(s, &n, v) {
                return r;
            }
            let d = s.doc_mut()?;
            // A dimension variable takes only the values its style field takes, as in DIMSTYLE and DIMOVERRIDE.
            if let Some((_, field)) = cadcraft_doc::DIMVARS.iter().find(|(var, _)| *var == n) {
                let mut st = d.dim_style(&d.header.str("DIMSTYLE", "Standard")).cloned().unwrap_or_default();
                if !st.apply_fields(&serde_json::Map::from_iter([((*field).to_string(), v.clone())])).is_empty() {
                    return Err(bad());
                }
                // A real style field (DIMJOGANG, DIMSCALE...) is a real variable, even set to a
                // whole number first, so a later fraction still fits. It takes the value the style
                // took.
                if let Some(f) = serde_json::to_value(&st).ok().and_then(|j| j.get(*field).filter(|f| f.is_f64()).and_then(|f| f.as_f64())) {
                    d.header.set_f64(&n, f);
                    return Ok(());
                }
            }
            let val = match v {
                Value::Number(x) if x.is_i64() => cadcraft_doc::HVal::Int(x.as_i64().unwrap_or(0)),
                Value::Number(x) => cadcraft_doc::HVal::Real(x.as_f64().filter(|f| f.is_finite()).ok_or_else(bad)?),
                Value::String(t) => match t.trim().parse::<i64>() {
                    Ok(i) => cadcraft_doc::HVal::Int(i),
                    Err(_) => match t.trim().parse::<f64>() {
                        Ok(f) if f.is_finite() => cadcraft_doc::HVal::Real(f),
                        _ => cadcraft_doc::HVal::Str(t.clone()),
                    },
                },
                Value::Bool(b) => cadcraft_doc::HVal::Int(i64::from(*b)),
                Value::Array(_) | Value::Object(_) => cadcraft_doc::HVal::Point(as_pt(v).ok_or_else(bad)?.to3(0.0)),
                _ => return Err(bad()),
            };
            // Keep the header type stable for known numeric vars.
            match d.header.get(&n) {
                Some(cadcraft_doc::HVal::Real(_)) => {
                    d.header.set_f64(&n, val.as_f64().filter(|f| f.is_finite()).ok_or_else(bad)?);
                    return Ok(());
                }
                Some(cadcraft_doc::HVal::Int(_)) => {
                    let i = match val {
                        cadcraft_doc::HVal::Int(i) => i,
                        cadcraft_doc::HVal::Real(f) if f.fract() == 0.0 && f.abs() < 1e15 => f as i64,
                        _ => return Err(bad()),
                    };
                    d.header.set_i64(&n, i);
                    return Ok(());
                }
                // A text variable (CLAYER...) keeps what was typed, so a layer named `10` stays a string.
                Some(cadcraft_doc::HVal::Str(_)) => {
                    let text = match v {
                        Value::String(t) => t.clone(),
                        Value::Number(x) => x.to_string(),
                        _ => return Err(bad()),
                    };
                    d.header.set(&n, cadcraft_doc::HVal::Str(text));
                    return Ok(());
                }
                // A point variable takes `x,y` typed at the command line too.
                Some(cadcraft_doc::HVal::Point(_)) => {
                    d.header.set(&n, cadcraft_doc::HVal::Point(as_pt(v).ok_or_else(bad)?.to3(0.0)));
                    return Ok(());
                }
                _ => {}
            }
            d.header.set(&n, val);
        }
    }
    Ok(())
}

pub fn list(s: &Session) -> Value {
    let mut m = serde_json::Map::new();
    for n in SESSION_VARS {
        if let Some(v) = get(s, n) {
            m.insert((*n).to_string(), v);
        }
    }
    if let Ok(d) = s.doc() {
        for (k, v) in &d.header.vars {
            if matches!(k.as_str(), "ANGBASE" | "SNAPUNIT" | "GRIDUNIT" | "CECOLOR") {
                m.insert(k.clone(), get(s, k).unwrap_or(Value::Null));
            } else if !k.starts_with("CADCRAFT_") {
                m.insert(k.clone(), serde_json::to_value(v).unwrap_or(Value::Null));
            }
        }
    }
    table::list(s, &mut m);
    Value::Object(m)
}
