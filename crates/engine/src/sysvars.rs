//! System variables: session settings plus drawing header variables, by name.

use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

const SESSION_VARS: &[&str] = &[
    "OSMODE",
    "ORTHOMODE",
    "POLARMODE",
    "POLARANG",
    "GRIDMODE",
    "SNAPMODE",
    "SNAPUNIT",
    "GRIDUNIT",
    "GRIDMAJOR",
    "DYNMODE",
    "LWDISPLAY",
    "QPMODE",
    "PICKBOX",
    "APERTURE",
    "PICKFIRST",
    "PICKADD",
    "GRIPSIZE",
    "CURSORSIZE",
    "LASTPOINT",
    "FONTALT",
    "FONTFALLBACK",
];

pub fn get(s: &Session, name: &str) -> Option<Value> {
    let n = name.trim().to_ascii_uppercase();
    let st = &s.settings;
    let v = match n.as_str() {
        "OSMODE" => json!(st.osmode),
        "ORTHOMODE" => json!(i32::from(st.orthomode)),
        "POLARMODE" => json!(i32::from(st.polarmode)),
        "POLARANG" => json!(st.polarang.to_degrees()),
        "GRIDMODE" => json!(i32::from(st.gridmode)),
        "SNAPMODE" => json!(i32::from(st.snapmode)),
        "SNAPUNIT" => json!([st.snapunit.x, st.snapunit.y]),
        "GRIDUNIT" => json!([st.gridunit.x, st.gridunit.y]),
        "GRIDMAJOR" => json!(st.gridmajor),
        "DYNMODE" => json!(i32::from(st.dynmode)),
        "LWDISPLAY" => json!(i32::from(st.lwdisplay)),
        "QPMODE" => json!(i32::from(st.qpmode)),
        "PICKBOX" => json!(st.pickbox),
        "APERTURE" => json!(st.aperture),
        "PICKFIRST" => json!(i32::from(st.pickfirst)),
        "PICKADD" => json!(i32::from(st.pickadd)),
        "GRIPSIZE" => json!(st.gripsize),
        "CURSORSIZE" => json!(st.cursorsize),
        "LASTPOINT" => json!([s.last_point.x, s.last_point.y, 0.0]),
        // Process-wide font substitution (profile settings, not saved in the drawing).
        "FONTALT" => json!(cadcraft_fonts::ttf::font_alt()),
        "FONTFALLBACK" => json!(cadcraft_fonts::ttf::fallback_fonts()),
        "CMDNAMES" => json!(s.running.as_ref().map(|r| r.id.to_ascii_uppercase()).unwrap_or_default()),
        "DWGNAME" => json!(s.state().map(|d| d.title.clone()).unwrap_or_default()),
        "DBMOD" => json!(s.state().map(|d| i32::from(d.is_dirty())).unwrap_or(0)),
        "CTAB" => json!(
            s.state()
                .map(|d| match &d.space {
                    cadcraft_doc::Space::Model => "Model".to_string(),
                    cadcraft_doc::Space::Paper(n) => n.clone(),
                })
                .unwrap_or_default()
        ),
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

fn as_bool(v: &Value) -> Option<bool> {
    v.as_bool().or_else(|| v.as_i64().map(|i| i != 0)).or_else(|| v.as_str().and_then(|s| s.trim().parse::<i64>().ok()).map(|i| i != 0))
}
/// QPMODE is -1/0 (off), 1 or 2 (on) in AutoCAD; booleans work too.
fn as_i(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_bool().map(i64::from)).or_else(|| v.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
}
fn as_f64(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(crate::units::parse_distance)).filter(|f| f.is_finite())
}
fn as_pt(v: &Value) -> Option<cadcraft_geom::Vec2> {
    crate::cmd::point_value(v).or_else(|| as_f64(v).map(|f| cadcraft_geom::Vec2::new(f, f)))
}

pub fn set(s: &mut Session, name: &str, v: &Value) -> Result<()> {
    let n = name.trim().to_ascii_uppercase();
    let bad = || EngineError::BadParams { cmd: "setvar".into(), msg: format!("invalid value for {n}") };
    let st = &mut s.settings;
    match n.as_str() {
        "OSMODE" => st.osmode = v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).ok_or_else(bad)? as u32 & 0x7fff,
        "ORTHOMODE" => st.orthomode = as_bool(v).ok_or_else(bad)?,
        "POLARMODE" => st.polarmode = as_bool(v).ok_or_else(bad)?,
        "POLARANG" => st.polarang = as_f64(v).filter(|a| *a > 0.0).ok_or_else(bad)?.to_radians(),
        "GRIDMODE" => st.gridmode = as_bool(v).ok_or_else(bad)?,
        "SNAPMODE" => st.snapmode = as_bool(v).ok_or_else(bad)?,
        "SNAPUNIT" => st.snapunit = as_pt(v).filter(|p| p.x > 0.0 && p.y > 0.0).ok_or_else(bad)?,
        "GRIDUNIT" => st.gridunit = as_pt(v).filter(|p| p.x > 0.0 && p.y > 0.0).ok_or_else(bad)?,
        "GRIDMAJOR" => st.gridmajor = as_f64(v).ok_or_else(bad)?.clamp(1.0, 100.0) as u32,
        "DYNMODE" => st.dynmode = as_bool(v).ok_or_else(bad)?,
        "LWDISPLAY" => st.lwdisplay = as_bool(v).ok_or_else(bad)?,
        "QPMODE" => st.qpmode = as_i(v).ok_or_else(bad)? > 0,
        "PICKBOX" => st.pickbox = as_f64(v).ok_or_else(bad)?.clamp(0.0, 50.0),
        "APERTURE" => st.aperture = as_f64(v).ok_or_else(bad)?.clamp(1.0, 50.0),
        "PICKFIRST" => st.pickfirst = as_bool(v).ok_or_else(bad)?,
        "PICKADD" => st.pickadd = as_bool(v).ok_or_else(bad)?,
        "GRIPSIZE" => st.gripsize = as_f64(v).ok_or_else(bad)?.clamp(1.0, 255.0),
        "CURSORSIZE" => st.cursorsize = as_f64(v).ok_or_else(bad)?.clamp(1.0, 100.0),
        "FONTALT" => cadcraft_fonts::ttf::set_font_alt(v.as_str().ok_or_else(bad)?),
        "FONTFALLBACK" => cadcraft_fonts::ttf::set_fallback_fonts(v.as_str().ok_or_else(bad)?),
        _ => {
            let d = s.doc_mut()?;
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
                Value::Array(_) => cadcraft_doc::HVal::Point(as_pt(v).ok_or_else(bad)?.to3(0.0)),
                _ => return Err(bad()),
            };
            // Keep the header type stable for known numeric vars.
            if let Some(old) = d.header.get(&n)
                && matches!(old, cadcraft_doc::HVal::Real(_))
                && let Some(f) = val.as_f64()
            {
                d.header.set_f64(&n, f);
                return Ok(());
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
            if !k.starts_with("CADCRAFT_") {
                m.insert(k.clone(), serde_json::to_value(v).unwrap_or(Value::Null));
            }
        }
    }
    Value::Object(m)
}
