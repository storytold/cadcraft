//! OPTIONS: the user-profile settings (the system variables a user keeps across sessions and
//! drawings, as AutoCAD keeps them in the user profile rather than in the drawing).
//!
//! The JSON form reads and changes them without a dialog: `{}` returns every profile variable by
//! name, `{"PICKBOX": 5, "OSMODE": 7}` sets those (all or nothing), `{"reset": true}` restores the
//! defaults first. The desktop app opens its Options dialog when OPTIONS is typed or chosen from
//! the menu, and saves [`profile`] between runs ([`restore`] reads it back).

use serde_json::{Map, Value, json};

use super::{CommandSpec, always, bad};
use crate::{Result, Session, Settings};

/// The profile variables, in the order [`profile`] lists them.
pub const PROFILE_VARS: &[&str] = &[
    "OSMODE",
    "AUTOSNAP",
    "POLARMODE",
    "POLARANG",
    "POLARADDANG",
    "ORTHOMODE",
    "SNAPMODE",
    "SNAPUNIT",
    "GRIDMODE",
    "GRIDUNIT",
    "GRIDMAJOR",
    "DYNMODE",
    "DYNPIFORMAT",
    "DYNPICOORDS",
    "DYNPROMPT",
    "DYNDIGRIP",
    "DYNDIVIS",
    "OSNAPHATCH",
    "LWDISPLAY",
    "QPMODE",
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
    "PICKSTYLE",
    "PICKBOX",
    "APERTURE",
    "GRIPSIZE",
    "CURSORSIZE",
    "PICKFIRST",
    "PICKADD",
    "PICKDRAG",
    "MAXARRAY",
    "MEASUREINIT",
];

/// The most profile entries [`restore`] reads (a saved profile is untrusted input).
const MAX_ENTRIES: usize = 256;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("options", "Options...", run_options)
            .menu(&["Tools", "Options..."])
            .alias(&["op", "preferences"])
            .params("{reset?: bool, OSMODE?, PICKBOX?, APERTURE?, GRIPSIZE?, CURSORSIZE?, PICKFIRST?, PICKADD?, PICKDRAG?: 0 (two clicks) | 2 (clicks or drag), …: any profile variable}")
            .enabled(always)
            .noundo(),
    ]
}

/// Whether `name` is a profile variable (any case).
pub fn is_profile_var(name: &str) -> bool {
    PROFILE_VARS.iter().any(|v| v.eq_ignore_ascii_case(name.trim()))
}

/// Every profile variable and its value, by name.
pub fn profile(s: &Session) -> Value {
    let mut m = Map::new();
    for name in PROFILE_VARS {
        if let Some(v) = crate::sysvars::get(s, name) {
            m.insert((*name).to_string(), v);
        }
    }
    Value::Object(m)
}

/// The profile variables the drawing keeps in its header rather than in the session's settings.
const DRAWING_VARS: &[&str] = &["SNAPUNIT", "GRIDUNIT"];

/// Set the profile variables named in `values` (an object; other keys are refused). All or
/// nothing: one bad name or value leaves every setting, and the drawing, as they were.
pub fn apply(s: &mut Session, values: &Value) -> Result<()> {
    let Some(obj) = values.as_object() else { return Err(bad("options", "expected an object of profile variables")) };
    let before = s.settings.clone();
    // When one of the values is a drawing variable, the drawing as it is: put back whole on a
    // refusal, it is the same drawing again, so not marked as changed either.
    let touches_drawing = obj.keys().any(|k| DRAWING_VARS.iter().any(|v| v.eq_ignore_ascii_case(k.trim())));
    let drawing = if touches_drawing { s.state().ok().map(|st| st.doc.clone()) } else { None };
    let put_back = |s: &mut Session| {
        s.settings = before.clone();
        if let (Some(doc), Ok(st)) = (&drawing, s.state_mut()) {
            st.doc = doc.clone();
        }
    };
    for (name, v) in obj {
        if !is_profile_var(name) {
            put_back(s);
            return Err(bad("options", format!("{name} is not a profile setting")));
        }
        if let Err(e) = crate::sysvars::set(s, name, v) {
            put_back(s);
            return Err(bad("options", format!("invalid value for {}: {e}", name.to_ascii_uppercase())));
        }
    }
    Ok(())
}

/// Restore a saved [`profile`]: known variables with valid values are applied, anything else is
/// skipped (an old or hand-edited profile never stops the app from starting).
pub fn restore(s: &mut Session, saved: &Value) {
    let Some(obj) = saved.as_object() else { return };
    for (name, v) in obj.iter().take(MAX_ENTRIES) {
        if is_profile_var(name) {
            let before = s.settings.clone();
            if crate::sysvars::set(s, name, v).is_err() {
                s.settings = before;
            }
        }
    }
}

/// Put every profile variable back to its default, except [`DRAWING_VARS`]: a reset doesn't change
/// the drawing.
pub fn reset(s: &mut Session) {
    let d = Settings::default();
    let st = &mut s.settings;
    st.osmode = d.osmode;
    st.polarmode = d.polarmode;
    st.otrack = d.otrack;
    st.polar_flags = d.polar_flags;
    st.polarang = d.polarang;
    st.polaraddang = d.polaraddang;
    st.orthomode = d.orthomode;
    st.snapmode = d.snapmode;
    st.gridmode = d.gridmode;
    st.gridmajor = d.gridmajor;
    st.dynmode = d.dynmode;
    st.dynpi_cartesian = d.dynpi_cartesian;
    st.dynpi_absolute = d.dynpi_absolute;
    st.osnaphatch = d.osnaphatch;
    st.lwdisplay = d.lwdisplay;
    st.qpmode = d.qpmode;
    st.pickbox = d.pickbox;
    st.aperture = d.aperture;
    st.gripsize = d.gripsize;
    st.cursorsize = d.cursorsize;
    st.pickfirst = d.pickfirst;
    st.pickadd = d.pickadd;
    st.pickdrag = d.pickdrag;
    st.maxarray = d.maxarray;
    st.dynprompt = d.dynprompt;
    st.dyndigrip = d.dyndigrip;
    st.dyndivis = d.dyndivis;
    st.selectioneffect = d.selectioneffect;
    st.selectionpreview = d.selectionpreview;
    st.previeweffect = d.previeweffect;
    st.previewfilter = d.previewfilter;
    st.grips = d.grips;
    st.gripobjlimit = d.gripobjlimit;
    st.selectionarea = d.selectionarea;
    st.windowareacolor = d.windowareacolor;
    st.crossingareacolor = d.crossingareacolor;
    st.selectionareaopacity = d.selectionareaopacity;
    st.pickstyle = d.pickstyle;
    st.measureinit = d.measureinit;
}

fn run_options(s: &mut Session, p: &Value) -> Result<Value> {
    // Typed without the desktop app's dialog (the CLI, scripts): report how to change them.
    if p.is_null() {
        let mut v = profile(s);
        if let Some(m) = v.as_object_mut() {
            m.insert("message".into(), json!("Options: change profile settings with SETVAR, or OPTIONS {\"NAME\": value}"));
        }
        return Ok(v);
    }
    let Some(obj) = p.as_object() else { return Err(bad("options", "expected an object")) };
    let reset_first = match obj.get("reset") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad("options", "`reset` must be true or false")),
    };
    let values: Map<String, Value> = obj.iter().filter(|(k, _)| k.as_str() != "reset").map(|(k, v)| (k.clone(), v.clone())).collect();
    let before = s.settings.clone();
    if reset_first {
        reset(s);
    }
    if let Err(e) = apply(s, &Value::Object(values)) {
        s.settings = before;
        return Err(e);
    }
    s.touch();
    Ok(profile(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_profile_var_round_trips_through_restore() {
        let mut a = Session::new();
        apply(&mut a, &json!({"PICKBOX": 7, "OSMODE": 3, "AUTOSNAP": 37, "POLARANG": 30, "SNAPUNIT": [2, 3], "DYNPIFORMAT": 1})).unwrap();
        let saved = profile(&a);
        for name in PROFILE_VARS {
            assert!(saved.get(*name).is_some(), "{name} is reported");
        }
        let mut b = Session::new();
        restore(&mut b, &saved);
        assert_eq!(profile(&b), saved);
    }

    #[test]
    fn a_refused_change_leaves_the_drawings_snap_and_grid_too() {
        let mut s = Session::new();
        let before = profile(&s);
        let refused = [
            json!({ "SNAPUNIT": [2, 3], "GRIDUNIT": [4, 5], "WINDOWAREACOLOR": 0 }),
            json!({ "GRIDUNIT": [4, 5], "LUNITS": 2 }),
            json!({ "WINDOWAREACOLOR": 0 }),
        ];
        for bad_one in refused {
            assert!(apply(&mut s, &bad_one).is_err(), "{bad_one}");
            assert_eq!(profile(&s), before, "{bad_one}");
            // The same drawing, so there is nothing new to save.
            assert!(!s.state().unwrap().is_dirty(), "{bad_one}");
        }
        assert!(s.execute("options", &json!({ "reset": true, "SNAPUNIT": [2, 3], "WINDOWAREACOLOR": 0 })).is_err());
        assert_eq!(profile(&s), before);
        // The header entries come back exactly: a zero grid spacing (which reads as the snap
        // spacing), and a snap spacing the drawing doesn't have. The refused value comes last, so
        // both are written first (the keys are taken in name order).
        let header = |s: &Session, n: &str| s.doc().unwrap().header.get(n).cloned();
        s.doc_mut().unwrap().header.set("GRIDUNIT", cadcraft_doc::HVal::Point(cadcraft_geom::Vec3::new(0.0, 0.0, 0.0)));
        s.doc_mut().unwrap().header.vars.remove("SNAPUNIT");
        let (grid, snap) = (header(&s, "GRIDUNIT"), header(&s, "SNAPUNIT"));
        assert!(apply(&mut s, &json!({ "GRIDUNIT": [4, 5], "SNAPUNIT": [2, 3], "WINDOWAREACOLOR": 0 })).is_err());
        assert_eq!((header(&s, "GRIDUNIT"), header(&s, "SNAPUNIT")), (grid, snap));
    }

    #[test]
    fn reset_puts_every_profile_var_back_except_the_drawings() {
        let defaults = profile(&Session::new());
        let mut s = Session::new();
        for name in PROFILE_VARS.iter().filter(|n| !DRAWING_VARS.contains(n)) {
            // A value the variable takes that isn't its default: 0 and 1 swapped, or a number one
            // more or less, or an angle.
            let was = &defaults[*name];
            let n = was.as_f64().unwrap_or(0.0);
            let tries = [json!(if n == 0.0 { 1 } else { 0 }), json!(n + 1.0), json!(n - 1.0), json!("15")];
            let changed = tries.iter().any(|v| apply(&mut s, &json!({ *name: v })).is_ok() && profile(&s)[*name] != *was);
            assert!(changed, "no other value found for {name} (default {was})");
        }
        assert!(PROFILE_VARS.iter().filter(|n| !DRAWING_VARS.contains(n)).all(|n| profile(&s)[*n] != defaults[*n]));
        apply(&mut s, &json!({ "SNAPUNIT": [2, 3], "GRIDUNIT": [4, 5] })).unwrap();
        reset(&mut s);
        let after = profile(&s);
        for name in PROFILE_VARS.iter().filter(|n| !DRAWING_VARS.contains(n)) {
            assert_eq!(after[*name], defaults[*name], "{name}");
        }
        // The snap and grid spacing are the drawing's, and stay.
        assert_eq!((after["SNAPUNIT"].clone(), after["GRIDUNIT"].clone()), (json!([2.0, 3.0]), json!([4.0, 5.0])));
    }
}
