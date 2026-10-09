//! Drafting settings and system variables: ORTHO, OSNAP, GRID, SNAP, POLAR, SETVAR/GETVAR.

use serde_json::{Value, json};

use super::*;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

fn toggle(s: &mut Session, p: &Value, get: fn(&mut crate::Settings) -> &mut bool, name: &str) -> Result<Value> {
    let v = get(&mut s.settings);
    *v = p.get("on").and_then(Value::as_bool).unwrap_or(!*v);
    let on = *v;
    s.touch();
    Ok(json!({ "on": on, "message": format!("<{name} {}>", if on { "on" } else { "off" }) }))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("ortho", "Ortho Mode", |s, p| toggle(s, p, |st| &mut st.orthomode, "Ortho"))
            .key("F8")
            .params("{on?: bool}")
            .enabled(always)
            .noundo()
            .transparent(),
        CommandSpec::new("grid", "Grid Display", |s, p| toggle(s, p, |st| &mut st.gridmode, "Grid"))
            .key("F7")
            .params("{on?: bool}")
            .enabled(always)
            .noundo()
            .transparent(),
        CommandSpec::new("snap", "Snap Mode", |s, p| toggle(s, p, |st| &mut st.snapmode, "Snap"))
            .key("F9")
            .params("{on?: bool}")
            .enabled(always)
            .noundo()
            .transparent(),
        CommandSpec::new("polar", "Polar Tracking", |s, p| toggle(s, p, |st| &mut st.polarmode, "Polar"))
            .key("F10")
            .params("{on?: bool}")
            .enabled(always)
            .noundo()
            .transparent(),
        CommandSpec::new("otrack", "Object Snap Tracking", |s, p| toggle(s, p, |st| &mut st.otrack, "Object Snap Tracking"))
            .key("F11")
            .params("{on?: bool}")
            .enabled(always)
            .noundo()
            .transparent(),
        CommandSpec::new("dynmode", "Dynamic Input", |s, p| toggle(s, p, |st| &mut st.dynmode, "Dynamic Input"))
            .key("F12")
            .params("{on?: bool}")
            .enabled(always)
            .noundo()
            .transparent(),
        CommandSpec::new("lwdisplay", "Show/Hide Lineweight", |s, p| toggle(s, p, |st| &mut st.lwdisplay, "Lineweight"))
            .params("{on?: bool}")
            .enabled(always)
            .noundo()
            .transparent(),
        CommandSpec::new("isodraft", "Isometric Drafting", |s, p| toggle(s, p, |st| &mut st.isodraft, "Isodraft"))
            .params("{on?: bool}")
            .enabled(always)
            .noundo()
            .transparent(),
        CommandSpec::new("transparencydisplay", "Show/Hide Transparency", |s, p| toggle(s, p, |st| &mut st.transparency_display, "Transparency"))
            .params("{on?: bool}")
            .enabled(always)
            .noundo(),
        CommandSpec::new("qpmode", "Quick Properties", |s, p| toggle(s, p, |st| &mut st.qpmode, "Quick Properties"))
            .params("{on?: bool}")
            .enabled(always)
            .noundo()
            .transparent(),
        CommandSpec::new("selectioncycling", "Selection Cycling", |s, p| toggle(s, p, |st| &mut st.selection_cycling, "Selection Cycling"))
            .params("{on?: bool}")
            .enabled(always)
            .noundo(),
        CommandSpec::new("osnap", "Object Snap", run_osnap)
            .key("F3")
            .alias(&["os", "ddosnap"])
            .params("{on?: bool, modes?: [\"end\",\"mid\",...] | osmode?: n}")
            .enabled(always)
            .noundo()
            .transparent(),
        CommandSpec::new("dsettings", "Drafting Settings...", run_dsettings)
            .menu(&["Tools", "Drafting Settings..."])
            .alias(&["ds", "se"])
            .params("{snapunit?: [x,y], gridunit?: [x,y], polarang? (degrees), gridmajor?}")
            .enabled(always)
            .noundo(),
        CommandSpec::new("setvar", "Set Variable", run_setvar)
            .menu(&["Tools", "Inquiry", "Set Variable"])
            .alias(&["set"])
            .params("{name, value}")
            .transparent()
            .interactive(|_| Ok(Box::new(SetvarM::default()))),
        CommandSpec::new("getvar", "Get Variable", run_getvar).params("{name}").enabled(always).noundo(),
        CommandSpec::new("sysvars", "List System Variables", |s, _| Ok(crate::sysvars::list(s))).enabled(always).noundo(),
    ]
}

fn run_osnap(s: &mut Session, p: &Value) -> Result<Value> {
    use crate::snap::mode;
    if let Some(n) = p.get("osmode").and_then(Value::as_u64) {
        s.settings.osmode = (n as u32) & 0x7fff;
    }
    if let Some(ms) = p.get("modes").and_then(Value::as_array) {
        let mut bits = 0u32;
        for m in ms.iter().filter_map(Value::as_str) {
            let b = match m.to_ascii_lowercase().get(..3).unwrap_or("") {
                "end" => mode::END,
                "mid" => mode::MID,
                "cen" => mode::CEN,
                "nod" => mode::NOD,
                "qua" => mode::QUA,
                "int" => mode::INT,
                "ins" => mode::INS,
                "per" => mode::PER,
                "tan" => mode::TAN,
                "nea" => mode::NEA,
                "gce" => mode::GCEN,
                "app" => mode::APP,
                "ext" => mode::EXT,
                "par" => mode::PAR,
                _ => 0,
            };
            bits |= b;
        }
        s.settings.osmode = bits;
    }
    if p.get("osmode").is_none() && p.get("modes").is_none() {
        let on = p.get("on").and_then(Value::as_bool).unwrap_or(s.settings.osmode & mode::OFF != 0);
        if on {
            s.settings.osmode &= !mode::OFF;
        } else {
            s.settings.osmode |= mode::OFF;
        }
    }
    s.touch();
    let on = s.settings.osmode & mode::OFF == 0;
    let active: Vec<&str> = mode::ALL.iter().filter(|(b, _)| s.settings.osmode & b != 0).map(|(_, n)| *n).collect();
    Ok(json!({ "on": on, "osmode": s.settings.osmode, "modes": active, "message": format!("<Osnap {}>", if on { "on" } else { "off" }) }))
}

fn run_dsettings(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(v) = point_param(p, "snapunit").filter(|v| v.x > 0.0 && v.y > 0.0) {
        s.settings.snapunit = v;
    }
    if let Some(v) = point_param(p, "gridunit").filter(|v| v.x > 0.0 && v.y > 0.0) {
        s.settings.gridunit = v;
    }
    if let Some(a) = p.get("polarang").and_then(Value::as_f64).filter(|a| *a > 0.0) {
        s.settings.polarang = a.to_radians();
    }
    if let Some(m) = p.get("gridmajor").and_then(Value::as_u64) {
        s.settings.gridmajor = (m as u32).clamp(1, 100);
    }
    s.touch();
    Ok(serde_json::to_value(&s.settings).unwrap_or(Value::Null))
}

fn run_setvar(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("setvar", "`name` is required"))?;
    let v = p.get("value").ok_or_else(|| bad("setvar", "`value` is required"))?;
    crate::sysvars::set(s, name, v)?;
    s.touch();
    Ok(json!({ "name": name.to_ascii_uppercase(), "value": crate::sysvars::get(s, name) }))
}

/// A system variable value as the command line shows it.
fn show_value(v: &Value) -> String {
    match v {
        Value::String(t) => t.clone(),
        Value::Array(a) => a.iter().map(show_value).collect::<Vec<_>>().join(","),
        other => other.to_string(),
    }
}

/// SETVAR at the command line: "Enter variable name or [?]", then "Enter new value for NAME <current>".
#[derive(Default)]
struct SetvarM {
    name: Option<String>,
}

impl Interactive for SetvarM {
    fn name(&self) -> &'static str {
        "SETVAR"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        // Not a free-text prompt: Space ends the input, as it does for names and numbers.
        let accept = Accept { number: true, ..Accept::TEXT };
        match &self.name {
            None => Prompt::new("Enter variable name", accept).kw(&["?"]),
            Some(n) => {
                let p = Prompt::new(format!("Enter new value for {n}"), accept);
                match crate::sysvars::get(s, n) {
                    Some(v) => p.default(show_value(&v)),
                    None => p,
                }
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.name.clone(), i) {
            (_, Input::Enter | Input::Cancel) => Ok(Step::Done),
            (None, Input::Keyword(_)) => {
                if let Value::Object(m) = crate::sysvars::list(s) {
                    for (k, v) in m {
                        s.echo(format!("{k:<12} {}", show_value(&v)));
                    }
                }
                Ok(Step::Done)
            }
            (None, Input::Text(t)) => {
                let n = t.trim().to_ascii_uppercase();
                match crate::sysvars::get(s, &n) {
                    None => {
                        s.echo(format!("Unknown variable {n}. Type SETVAR ? to list variables."));
                        Ok(Step::Done)
                    }
                    Some(v) if crate::sysvars::is_read_only(&n) => {
                        s.echo(format!("{n} = {} (read only)", show_value(&v)));
                        Ok(Step::Done)
                    }
                    Some(_) => {
                        self.name = Some(n);
                        Ok(Step::Continue)
                    }
                }
            }
            (Some(n), Input::Text(t)) => match crate::sysvars::set(s, &n, &Value::String(t)) {
                Ok(()) => {
                    s.touch();
                    Ok(Step::Done)
                }
                Err(e) => {
                    s.echo(e.to_string());
                    Ok(Step::Continue)
                }
            },
            _ => Ok(Step::Continue),
        }
    }
}

fn run_getvar(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("getvar", "`name` is required"))?;
    let v = crate::sysvars::get(s, name).ok_or_else(|| bad("getvar", format!("unknown variable `{name}`")))?;
    Ok(json!({ "name": name.to_ascii_uppercase(), "value": v }))
}
