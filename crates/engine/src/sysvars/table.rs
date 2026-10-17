//! System variables the engine already keeps in the drawing header (or in [`crate::Settings`]) and
//! reads with a default, described once in a table: their type, allowed values and default. GETVAR
//! reports the default until a value is set, SETVAR checks the value against the table, and
//! `SETVAR ?` lists them. Variables CADCraft doesn't model are not in the table.

use serde_json::{Value, json};

use crate::{EngineError, Result, Session, Settings};

/// What a header variable holds and which values it takes.
#[derive(Clone, Copy, Debug)]
enum Kind {
    /// An integer from the list.
    Choice(&'static [i64]),
    /// An integer in `min..=max`.
    Int(i64, i64),
    /// A real number of at least `min` (`strict`: greater than `min`).
    Real { min: f64, strict: bool },
    /// Any finite real number.
    AnyReal,
    /// An angle: kept in radians, shown and typed in degrees (a typed text follows AUNITS).
    Angle,
    /// A hatch pattern name from the library.
    Pattern,
    /// A block name; `.` clears it.
    Name,
    /// Text set by its own command, not by SETVAR.
    ReadOnlyStr,
}

/// A default that may depend on the drawing's units (MEASUREMENT 1 = metric).
#[derive(Clone, Copy, Debug)]
enum Def {
    Int(i64),
    Real(f64),
    /// Imperial, metric.
    Units(f64, f64),
    Str(&'static str),
}

/// Header variables: name, kind, the default the commands use while the header has none.
const HEADER: &[(&str, Kind, Def)] = &[
    ("CENTEREXE", Kind::Real { min: 0.0, strict: false }, Def::Units(0.12, 3.5)),
    ("CHAMFERA", Kind::Real { min: 0.0, strict: false }, Def::Real(0.0)),
    ("CHAMFERB", Kind::Real { min: 0.0, strict: false }, Def::Real(0.0)),
    ("CHAMFERC", Kind::Real { min: 0.0, strict: false }, Def::Real(0.0)),
    ("CHAMFERD", Kind::Angle, Def::Real(0.0)),
    ("CHAMMODE", Kind::Choice(&[0, 1]), Def::Int(0)),
    ("CIRCLERAD", Kind::Real { min: 0.0, strict: false }, Def::Real(0.0)),
    ("CMLJUST", Kind::Choice(&[0, 1, 2]), Def::Int(0)),
    ("CMLSCALE", Kind::AnyReal, Def::Units(1.0, 20.0)),
    ("CPLOTSTYLE", Kind::ReadOnlyStr, Def::Str("ByLayer")),
    ("DIMASSOC", Kind::Choice(&[0, 1, 2]), Def::Int(2)),
    ("DONUTID", Kind::Real { min: 0.0, strict: false }, Def::Real(0.5)),
    ("DONUTOD", Kind::Real { min: 0.0, strict: true }, Def::Real(1.0)),
    ("EDGEMODE", Kind::Choice(&[0, 1]), Def::Int(0)),
    ("HPANG", Kind::Angle, Def::Real(0.0)),
    // 1: HATCH and GRADIENT typed or from a menu open the Hatch and Gradient dialog; 0: the command line.
    ("HPDLGMODE", Kind::Choice(&[0, 1]), Def::Int(1)),
    ("HPNAME", Kind::Pattern, Def::Str("ANSI31")),
    ("HPSCALE", Kind::Real { min: 0.0, strict: true }, Def::Real(1.0)),
    ("INSNAME", Kind::Name, Def::Str("")),
    ("LWDEFAULT", Kind::Choice(&LINEWEIGHTS), Def::Int(25)),
    // Negative: Through (OFFSET picks a point).
    ("OFFSETDIST", Kind::AnyReal, Def::Real(-1.0)),
    ("OFFSETERASE", Kind::Choice(&[0, 1]), Def::Int(0)),
    ("OFFSETLAYER", Kind::Choice(&[0, 1]), Def::Int(0)),
    ("POLYSIDES", Kind::Int(3, 1024), Def::Int(4)),
    ("PROJMODE", Kind::Choice(&[0, 1, 2]), Def::Int(1)),
    ("REVCLOUDARCLEN", Kind::Real { min: 0.0, strict: true }, Def::Units(0.5, 15.0)),
    ("SPLINESEGS", Kind::Int(-32768, 32767), Def::Int(8)),
    ("SPLKNOTS", Kind::Choice(&[0, 1, 2]), Def::Int(0)),
    ("TEXTFILL", Kind::Choice(&[0, 1]), Def::Int(1)),
    ("TRIMEXTENDMODE", Kind::Choice(&[0, 1]), Def::Int(1)),
    ("TRIMMODE", Kind::Choice(&[0, 1]), Def::Int(1)),
    ("WIPEOUTFRAME", Kind::Choice(&[0, 1, 2]), Def::Int(1)),
];

/// The standard lineweights in hundredths of a millimetre.
const LINEWEIGHTS: [i64; 24] = [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211];

/// On/off session settings: name, read, write.
type Toggle = (&'static str, fn(&Settings) -> bool, fn(&mut Settings, bool));
const SETTINGS: &[Toggle] =
    &[("AUDITCTL", |s| s.auditctl, |s, v| s.auditctl = v), ("TRANSPARENCYDISPLAY", |s| s.transparency_display, |s, v| s.transparency_display = v)];

fn header_var(name: &str) -> Option<&'static (&'static str, Kind, Def)> {
    HEADER.iter().find(|(n, _, _)| *n == name)
}

fn toggle(name: &str) -> Option<&'static Toggle> {
    SETTINGS.iter().find(|(n, _, _)| *n == name)
}

/// Whether `name` (upper case) is a table variable SETVAR can't change.
pub(super) fn is_read_only(name: &str) -> bool {
    header_var(name).is_some_and(|(_, k, _)| matches!(k, Kind::ReadOnlyStr))
}

/// The value of a table variable (upper-case `name`), or None when it isn't one.
pub(super) fn get(s: &Session, name: &str) -> Option<Value> {
    if let Some((_, read, _)) = toggle(name) {
        return Some(json!(i32::from(read(&s.settings))));
    }
    let (_, kind, def) = header_var(name)?;
    let d = s.doc().ok()?;
    let h = &d.header;
    let real_def = match *def {
        Def::Real(f) => f,
        Def::Units(imperial, metric) => {
            if h.i64("MEASUREMENT", 0) == 1 {
                metric
            } else {
                imperial
            }
        }
        Def::Int(i) => i as f64,
        Def::Str(_) => 0.0,
    };
    Some(match (kind, def) {
        (Kind::Choice(_) | Kind::Int(..), Def::Int(i)) => json!(h.i64(name, *i)),
        (Kind::Angle, _) => json!(h.f64(name, real_def).to_degrees()),
        (_, Def::Str(t)) => json!(h.str(name, t)),
        _ => json!(h.f64(name, real_def)),
    })
}

fn int_value(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().filter(|f| f.fract() == 0.0 && f.abs() < 1e15).map(|f| f as i64)),
        Value::Bool(b) => Some(i64::from(*b)),
        Value::String(t) => t.trim().parse().ok(),
        _ => None,
    }
}

/// Set a table variable (upper-case `name`); None when it isn't one.
pub(super) fn set(s: &mut Session, name: &str, v: &Value) -> Option<Result<()>> {
    let bad = |allowed: String| EngineError::BadParams { cmd: "setvar".into(), msg: format!("invalid value for {name} ({allowed})") };
    if let Some((_, _, write)) = toggle(name) {
        let r = match int_value(v) {
            Some(i @ (0 | 1)) => {
                write(&mut s.settings, i == 1);
                Ok(())
            }
            _ => Err(bad("0 or 1".into())),
        };
        return Some(r);
    }
    let (_, kind, _) = header_var(name)?;
    Some(set_header(s, name, *kind, v, bad))
}

fn set_header(s: &mut Session, name: &str, kind: Kind, v: &Value, bad: impl Fn(String) -> EngineError) -> Result<()> {
    let real = || super::as_f64(v);
    match kind {
        Kind::Choice(allowed) => {
            let i = int_value(v).filter(|i| allowed.contains(i));
            let list = || allowed.iter().map(i64::to_string).collect::<Vec<_>>().join(", ");
            let i = i.ok_or_else(|| bad(list()))?;
            s.doc_mut()?.header.set_i64(name, i);
        }
        Kind::Int(min, max) => {
            let i = int_value(v).filter(|i| (min..=max).contains(i)).ok_or_else(|| bad(format!("{min} to {max}")))?;
            s.doc_mut()?.header.set_i64(name, i);
        }
        Kind::Real { min, strict } => {
            let f = real().filter(|f| if strict { *f > min } else { *f >= min });
            let f = f.ok_or_else(|| bad(format!("a number {} {min}", if strict { "above" } else { "of at least" })))?;
            s.doc_mut()?.header.set_f64(name, f);
        }
        Kind::AnyReal => {
            let f = real().ok_or_else(|| bad("a number".into()))?;
            s.doc_mut()?.header.set_f64(name, f);
        }
        Kind::Angle => {
            let a = match v {
                Value::String(t) => s.angle_settings().amount(t),
                _ => v.as_f64().map(f64::to_radians),
            };
            let a = a.filter(|a| a.is_finite()).ok_or_else(|| bad("an angle".into()))?;
            s.doc_mut()?.header.set_f64(name, cadcraft_geom::norm_angle(a));
        }
        Kind::Pattern => {
            let p = v.as_str().map(|t| t.trim().to_ascii_uppercase()).filter(|p| cadcraft_doc::library::pattern(p).is_some());
            let p = p.ok_or_else(|| bad("a hatch pattern name".into()))?;
            s.doc_mut()?.header.set_str(name, &p);
        }
        Kind::Name => {
            let t = v.as_str().map(str::trim).filter(|t| t.chars().count() <= 255).ok_or_else(|| bad("a name, or . for none".into()))?;
            let t = if t == "." { "" } else { t };
            s.doc_mut()?.header.set_str(name, t);
        }
        Kind::ReadOnlyStr => return Err(EngineError::BadParams { cmd: "setvar".into(), msg: format!("{name} is read-only") }),
    }
    Ok(())
}

/// Add the table variables and the dimension variables of the current style to a `SETVAR ?` list.
pub(super) fn list(s: &Session, m: &mut serde_json::Map<String, Value>) {
    let names = SETTINGS.iter().map(|(n, _, _)| *n).chain(HEADER.iter().map(|(n, _, _)| *n));
    for n in names.chain(cadcraft_doc::DIMVARS.iter().map(|(n, _)| *n)) {
        if !m.contains_key(n)
            && let Some(v) = super::get(s, n)
        {
            m.insert(n.to_string(), v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_names_are_upper_case_and_unique() {
        let mut names: Vec<&str> = HEADER.iter().map(|(n, _, _)| *n).chain(SETTINGS.iter().map(|(n, _, _)| *n)).collect();
        assert!(names.iter().all(|n| n.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())));
        let len = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), len);
    }
}
