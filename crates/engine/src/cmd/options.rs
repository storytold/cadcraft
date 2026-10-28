//! OPTIONS: the user-profile settings (the system variables a user keeps across sessions and
//! drawings, as AutoCAD keeps them in the user profile rather than in the drawing).
//!
//! The JSON form reads and changes them without a dialog: `{}` returns every profile variable by
//! name, `{"PICKBOX": 5, "OSMODE": 7}` sets those (all or nothing), `{"reset": true}` restores the
//! defaults first. The desktop app opens its Options dialog when OPTIONS is typed or chosen from
//! the menu, and saves [`profile`] between runs ([`restore`] reads it back).

use serde_json::{Map, Value, json};

use super::*;
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

/// Set the profile variables named in `values` (an object; other keys are refused). All or
/// nothing: one bad name or value leaves every setting as it was.
pub fn apply(s: &mut Session, values: &Value) -> Result<()> {
    let Some(obj) = values.as_object() else { return Err(bad("options", "expected an object of profile variables")) };
    let before = s.settings.clone();
    for (name, v) in obj {
        if !is_profile_var(name) {
            s.settings = before;
            return Err(bad("options", format!("{name} is not a profile setting")));
        }
        if let Err(e) = crate::sysvars::set(s, name, v) {
            s.settings = before;
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

/// Put every profile variable back to its default.
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
}
