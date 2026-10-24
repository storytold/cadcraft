//! FIELD (a multiline text holding a field), UPDATEFIELD (evaluate the fields of objects again)
//! and DWGPROPS (the drawing properties fields read): see `cadcraft_doc::fields` for the codes.
//!
//! Field kinds: Date (now), SaveDate (`TDUPDATE`), Filename, the drawing properties Title,
//! Subject, Author, Keywords and Comments, any system variable, and object properties (Area,
//! Length, Perimeter, Radius, Diameter, Layer, Contents) of an object by handle. A field whose
//! object is gone, or that can't be evaluated, shows `####`.

use cadcraft_doc::fields::{self, Field, FieldKind};
use cadcraft_doc::{EntityKind, Handle, MText, Prim, Space};
use cadcraft_geom::{Polyline, Vec2, Vec3};
use serde_json::{Value, json};

use super::super::machines::{SelOutcome, SelectPhase};
use super::super::*;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("field", "Field...", run_field)
            .menu(&["Insert", "Field..."])
            .params(
                "{at, field: date|savedate|filename|title|subject|author|keywords|comments|<system variable>|object, \
                 handle? (object), property?: area|length|perimeter|radius|diameter|layer|contents, format?, height?, now? (seconds since 1970)}",
            )
            .interactive(|_| Ok(Box::new(FieldM::default()))),
        CommandSpec::new("updatefield", "Update Fields", run_updatefield)
            .menu(&["Tools", "Update Fields"])
            .params("{handles?, now? (seconds since 1970)} (no handles: every object in model and paper space)")
            .interactive(|_| Ok(Box::new(UpdateFieldM::default()))),
        CommandSpec::new("dwgprops", "Drawing Properties...", run_dwgprops)
            .menu(&["File", "Drawing Properties..."])
            .params("{title?, subject?, author?, keywords?, comments?}")
            .interactive(|_| Ok(Box::new(DwgPropsM::default()))),
    ]
}

/// The drawing properties DWGPROPS keeps (header variables `CADCRAFT_DWGPROP_<NAME>`).
pub(crate) const DWGPROPS: [&str; 5] = ["Title", "Subject", "Author", "Keywords", "Comments"];

fn prop_var(name: &str) -> String {
    format!("CADCRAFT_DWGPROP_{}", name.to_ascii_uppercase())
}

/// The current time in seconds since 1970 (none on the web, where there is no clock here).
fn clock() -> Option<f64> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs_f64())
    }
    #[cfg(target_arch = "wasm32")]
    None
}

/// `now` from the JSON form (seconds since 1970) or the clock.
fn now_param(p: &Value) -> Option<f64> {
    p.get("now").and_then(Value::as_f64).filter(|v| v.is_finite() && v.abs() < 1e13).or_else(clock)
}

/// Year, month (1..=12) and day of the days since 1970-01-01 (proleptic Gregorian calendar).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/// Format a time (seconds since 1970, UTC) with a date picture: `yyyy` `yy` `MMMM` `MMM` `MM`
/// `M` `dddd` `ddd` `dd` `d` `HH` `H` `hh` `h` `mm` `ss` `tt`; anything else, and text in single
/// quotes, as it is.
pub(crate) fn format_date(secs: f64, picture: &str) -> String {
    let secs = secs.floor() as i64;
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (y, mo, d) = civil(days);
    let (hh, mi, ss) = (tod / 3600, (tod / 60) % 60, tod % 60);
    let wd = (days + 4).rem_euclid(7) as usize;
    let month = MONTHS.get(mo as usize - 1).copied().unwrap_or("");
    let day = DAYS.get(wd).copied().unwrap_or("");
    let h12 = if hh % 12 == 0 { 12 } else { hh % 12 };
    let tokens: [(&str, String); 17] = [
        ("yyyy", format!("{y:04}")),
        ("yy", format!("{:02}", y.rem_euclid(100))),
        ("MMMM", month.to_string()),
        ("MMM", month.chars().take(3).collect()),
        ("MM", format!("{mo:02}")),
        ("M", mo.to_string()),
        ("dddd", day.to_string()),
        ("ddd", day.chars().take(3).collect()),
        ("dd", format!("{d:02}")),
        ("d", d.to_string()),
        ("HH", format!("{hh:02}")),
        ("H", hh.to_string()),
        ("hh", format!("{h12:02}")),
        ("h", h12.to_string()),
        ("mm", format!("{mi:02}")),
        ("ss", format!("{ss:02}")),
        ("tt", if hh < 12 { "AM" } else { "PM" }.to_string()),
    ];
    let mut out = String::new();
    let mut rest = picture;
    'outer: while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('\'') {
            let end = r.find('\'').unwrap_or(r.len());
            out.push_str(r.get(..end).unwrap_or(""));
            rest = r.get(end + 1..).unwrap_or("");
            continue;
        }
        for (t, v) in &tokens {
            if let Some(r) = rest.strip_prefix(t) {
                out.push_str(v);
                rest = r;
                continue 'outer;
            }
        }
        let mut cs = rest.chars();
        if let Some(c) = cs.next() {
            out.push(c);
        }
        rest = cs.as_str();
    }
    out
}

/// Format a length or area: the drawing's units and precision, or `%luN` (units) and `%prN`
/// (precision) in the field's format.
fn format_number(s: &Session, v: f64, format: Option<&str>) -> Option<String> {
    let d = s.doc().ok()?;
    let opt = |key: &str| {
        let f = format?;
        let i = f.find(key)?;
        f.get(i + key.len()..)?.chars().take_while(char::is_ascii_digit).collect::<String>().parse::<i64>().ok()
    };
    let lu = opt("%lu").unwrap_or_else(|| d.header.i64("LUNITS", 2)).clamp(1, 5);
    let pr = opt("%pr").unwrap_or_else(|| d.header.i64("LUPREC", 4)).clamp(0, 8);
    Some(crate::units::format_distance(v, lu, pr))
}

/// Length of an object's curves.
fn length_of(k: &EntityKind) -> Option<f64> {
    let mut total = 0.0;
    let mut any = false;
    for p in k.prims() {
        any = true;
        total += match p {
            Prim::Seg(sg) => sg.len(),
            Prim::Circle(c) => c.radius * std::f64::consts::TAU,
            Prim::Ellipse(e) => {
                let mut pts = Vec::new();
                e.tessellate(1e-4, &mut pts);
                pts.windows(2).map(|w| w.first().zip(w.get(1)).map_or(0.0, |(a, b)| a.dist(*b))).sum()
            }
            Prim::Spline(sp) => sp.tessellate(1e-4).windows(2).map(|w| w.first().zip(w.get(1)).map_or(0.0, |(a, b)| a.dist(*b))).sum(),
            _ => return None,
        };
    }
    any.then_some(total)
}

/// Area of a closed object.
fn area_of(k: &EntityKind) -> Option<f64> {
    match k {
        EntityKind::Circle(c) => Some(c.radius * c.radius * std::f64::consts::PI),
        EntityKind::LwPolyline(pl) => Some(Polyline { vertices: pl.vertices.clone(), closed: true }.area().abs()),
        EntityKind::Ellipse(e) => {
            let a = e.major.xy().len();
            Some(std::f64::consts::PI * a * a * e.ratio)
        }
        EntityKind::Hatch(h) => Some(h.loops.iter().map(|l| Polyline { vertices: l.vertices.clone(), closed: true }.area().abs()).sum()),
        _ => None,
    }
}

/// The value of an object property, or `None` (no such object or property).
fn object_value(s: &Session, h: Handle, property: &str, format: Option<&str>) -> Option<String> {
    let d = s.doc().ok()?;
    let e = d.entity(h)?;
    let num = |v: f64| format_number(s, v, format);
    match property.to_ascii_lowercase().as_str() {
        "area" => num(area_of(&e.kind)?),
        "length" | "perimeter" | "circumference" => num(length_of(&e.kind)?),
        "radius" => match &e.kind {
            EntityKind::Circle(c) => num(c.radius),
            EntityKind::Arc(a) => num(a.radius),
            _ => None,
        },
        "diameter" => match &e.kind {
            EntityKind::Circle(c) => num(2.0 * c.radius),
            EntityKind::Arc(a) => num(2.0 * a.radius),
            _ => None,
        },
        "layer" => Some(e.common.layer.clone()),
        "contents" | "textstring" => match &e.kind {
            EntityKind::Text(t) => Some(fields::display(&t.value).into_owned()),
            EntityKind::MText(t) => Some(cadcraft_fonts::plain_mtext(&fields::display_mtext(&t.contents))),
            _ => None,
        },
        _ => None,
    }
}

/// Evaluate a field (`None`: no value).
pub(crate) fn evaluate(s: &Session, f: &Field, now: Option<f64>) -> Option<String> {
    let format = f.format.as_deref();
    match &f.kind {
        FieldKind::Var(name) => match name.to_ascii_lowercase().as_str() {
            "date" => now.map(|t| format_date(t, format.unwrap_or("yyyy-MM-dd"))),
            "savedate" => {
                // TDUPDATE: a Julian date.
                let jd = s.doc().ok()?.header.get("TDUPDATE")?.as_f64().filter(|v| v.is_finite() && *v > 0.0)?;
                Some(format_date((jd - 2_440_587.5) * 86_400.0, format.unwrap_or("yyyy-MM-dd")))
            }
            "filename" => s.state().ok().map(|st| st.title.clone()),
            n => {
                if let Some(p) = DWGPROPS.iter().find(|p| p.eq_ignore_ascii_case(n)) {
                    return Some(s.doc().ok()?.header.str(&prop_var(p), ""));
                }
                // A system variable.
                match crate::sysvars::get(s, name)? {
                    Value::String(t) => Some(t),
                    Value::Number(x) => match (x.as_i64(), x.as_f64()) {
                        (Some(i), _) => Some(i.to_string()),
                        (None, Some(v)) => format_number(s, v, format),
                        _ => None,
                    },
                    v => Some(v.to_string()),
                }
            }
        },
        FieldKind::ObjProp { handle, property } => object_value(s, (*handle)?, property, format),
        FieldKind::Other => f.value.clone(),
    }
}

/// The texts of an object that can hold fields, changed by `f`; returns the number of fields.
fn each_text(k: &mut EntityKind, f: &mut dyn FnMut(&mut String) -> usize) -> usize {
    match k {
        EntityKind::Text(t) => f(&mut t.value),
        EntityKind::MText(t) => f(&mut t.contents),
        EntityKind::Insert(i) => i.attribs.iter_mut().map(|a| f(&mut a.text.value)).sum(),
        EntityKind::MLeader(m) => m.text.as_mut().map_or(0, |t| f(&mut t.contents)),
        EntityKind::Table(t) => t.cells.iter_mut().flatten().map(|c| f(&mut c.text)).sum(),
        _ => 0,
    }
}

/// Whether an object holds a text with a field code.
fn has_field_code(k: &EntityKind) -> bool {
    let code = |t: &String| t.contains("%<");
    match k {
        EntityKind::Text(t) => code(&t.value),
        EntityKind::MText(t) => code(&t.contents),
        EntityKind::Insert(i) => i.attribs.iter().any(|a| code(&a.text.value)),
        EntityKind::MLeader(m) => m.text.as_ref().is_some_and(|t| code(&t.contents)),
        EntityKind::Table(t) => t.cells.iter().flatten().any(|c| code(&c.text)),
        _ => false,
    }
}

/// Evaluate the fields of `handles` again (all objects in every space when `None`). Returns
/// the fields found and the objects changed.
pub(crate) fn update_fields(s: &mut Session, handles: Option<&[Handle]>, now: Option<f64>) -> Result<(usize, usize)> {
    let hs: Vec<Handle> = match handles {
        Some(h) => h.to_vec(),
        None => {
            let d = s.doc()?;
            let mut spaces = vec![Space::Model];
            spaces.extend(d.layouts.iter().map(|l| Space::Paper(l.name.clone())));
            spaces.iter().filter_map(|sp| d.space(sp)).flat_map(|st| st.handles()).collect()
        }
    };
    let mut found = 0;
    let mut edits = Vec::new();
    {
        let d = s.doc()?;
        for h in hs {
            let Some(e) = d.entity(h) else { continue };
            // Look before copying: most objects (hatches, splines, ...) hold no field.
            if !has_field_code(&e.kind) {
                continue;
            }
            let mut k = e.kind.clone();
            let n = each_text(&mut k, &mut |t: &mut String| {
                if !t.contains("%<") {
                    return 0;
                }
                let (nt, n) = fields::set_values(t, |f| evaluate(s, f, now));
                *t = nt;
                n
            });
            found += n;
            if n > 0 && k != e.kind {
                edits.push((h, k));
            }
        }
    }
    let changed = edits.len();
    if changed > 0 {
        let d = s.doc_mut()?;
        for (h, k) in edits {
            d.modify_entity(h, |e| e.kind = k)?;
        }
    }
    Ok((found, changed))
}

fn run_updatefield(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = match p.get("handles") {
        None | Some(Value::Null) => None,
        Some(_) => Some(targets(s, p)?),
    };
    let (found, updated) = update_fields(s, hs.as_deref(), now_param(p))?;
    Ok(json!({ "found": found, "updated": updated, "message": format!("{found} field(s) found.  {updated} object(s) updated.") }))
}

/// The field code for a field name (and object handle and property).
fn field_code(name: &str, handle: Option<Handle>, property: Option<&str>, format: Option<&str>) -> std::result::Result<String, String> {
    let n = name.trim();
    if n.eq_ignore_ascii_case("object") {
        let h = handle.ok_or("an object field needs a `handle`")?;
        return Ok(fields::object_code(h, property.unwrap_or("Area"), format));
    }
    let canonical = ["Date", "SaveDate", "Filename"].iter().chain(DWGPROPS.iter()).find(|c| c.eq_ignore_ascii_case(n)).copied();
    match canonical {
        Some(c) => Ok(fields::var_code(c, format)),
        None if !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => Ok(fields::var_code(&n.to_ascii_uppercase(), format)),
        None => Err(format!("unknown field `{n}`")),
    }
}

/// A multiline text at `at` holding `code` with its value.
fn add_field_text(s: &mut Session, at: Vec2, code: &str, height: Option<f64>, now: Option<f64>) -> Result<Handle> {
    let (contents, _) = fields::set_values(code, |f| evaluate(s, f, now));
    let d = s.doc()?;
    let height = height.filter(|h| h.is_finite() && *h > 0.0).unwrap_or_else(|| d.header.f64("TEXTSIZE", 0.2));
    let style = d.header.str("TEXTSTYLE", "Standard");
    // Every other MTEXT property at its default.
    let m: MText = serde_json::from_value(json!({ "insert": Vec3::new(at.x, at.y, 0.0), "height": height, "style": style, "contents": contents }))
        .map_err(|e| EngineError::Other(format!("field text: {e}")))?;
    s.add_entity(EntityKind::MText(m))
}

fn run_field(s: &mut Session, p: &Value) -> Result<Value> {
    let at = point_req("field", p, "at")?;
    let name = str_param(p, "field").unwrap_or("date");
    let handle =
        p.get("handle").map(|v| v.as_str().and_then(Handle::parse_hex).ok_or_else(|| bad("field", "`handle` must be a hex handle"))).transpose()?;
    let code = field_code(name, handle, str_param(p, "property"), str_param(p, "format")).map_err(|e| bad("field", e))?;
    let h = add_field_text(s, at, &code, p.get("height").and_then(Value::as_f64), now_param(p))?;
    let shown = match s.doc()?.entity(h).map(|e| &e.kind) {
        Some(EntityKind::MText(t)) => cadcraft_fonts::plain_mtext(&fields::display_mtext(&t.contents)),
        _ => String::new(),
    };
    Ok(json!({ "handle": h.hex(), "code": code, "value": shown }))
}

fn run_dwgprops(s: &mut Session, p: &Value) -> Result<Value> {
    let mut sets = Vec::new();
    for name in DWGPROPS {
        if let Some(v) = p.get(name.to_ascii_lowercase()) {
            let v = v.as_str().ok_or_else(|| bad("dwgprops", format!("`{}` must be text", name.to_ascii_lowercase())))?;
            sets.push((prop_var(name), v.chars().take(4096).collect::<String>()));
        }
    }
    // Reading alone must not copy the drawing or record an undo step.
    if !sets.is_empty() {
        let d = s.doc_mut()?;
        for (var, v) in sets {
            d.header.set(&var, cadcraft_doc::HVal::Str(v));
        }
    }
    let d = s.doc()?;
    let mut out = serde_json::Map::new();
    for name in DWGPROPS {
        out.insert(name.to_ascii_lowercase(), json!(d.header.str(&prop_var(name), "")));
    }
    Ok(Value::Object(out))
}

fn other(m: impl Into<String>) -> EngineError {
    EngineError::Other(m.into())
}

const FIELD_NAMES: [&str; 9] = ["Date", "SaveDate", "Filename", "Title", "Subject", "Author", "Keywords", "Comments", "Object"];
const PROPERTIES: [&str; 6] = ["Area", "Length", "Radius", "Diameter", "Layer", "Contents"];

/// FIELD at the command line: the field name, for an object field the object and property,
/// then where the text goes.
#[derive(Default)]
struct FieldM {
    name: Option<String>,
    object: Option<Handle>,
    sel: Option<SelectPhase>,
    property: Option<String>,
}

impl Interactive for FieldM {
    fn name(&self) -> &'static str {
        "FIELD"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let Some(sel) = &self.sel {
            return sel.prompt();
        }
        match (&self.name, self.object, &self.property) {
            (None, _, _) => Prompt::new("Enter field name or system variable", super::super::curves::KW).kw(&FIELD_NAMES).default("Date"),
            (Some(n), Some(_), None) if n == "Object" => Prompt::new("Enter property", super::super::curves::KW).kw(&PROPERTIES).default("Area"),
            _ => Prompt::new("Specify start point", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(sel) = &mut self.sel {
            return Ok(match sel.feed(s, &i)? {
                SelOutcome::More => Step::Continue,
                SelOutcome::Empty => Step::Done,
                SelOutcome::Done(hs) => {
                    self.object = hs.first().copied();
                    self.sel = None;
                    Step::Continue
                }
            });
        }
        let word = match &i {
            Input::Keyword(k) | Input::Text(k) => Some(k.trim().to_string()),
            Input::Enter => None,
            _ => Some(String::new()),
        };
        match (&self.name, &self.property) {
            (None, _) => {
                let w = word.unwrap_or_else(|| "Date".into());
                let canon = FIELD_NAMES.iter().find(|n| n.eq_ignore_ascii_case(&w)).map(|n| n.to_string());
                let name = match canon {
                    Some(n) => n,
                    None if crate::sysvars::get(s, &w).is_some() => w.to_ascii_uppercase(),
                    None => return Err(other(format!("Unknown field or system variable \"{w}\"."))),
                };
                if name == "Object" {
                    self.sel = Some(SelectPhase::single());
                }
                self.name = Some(name);
                Ok(Step::Continue)
            }
            (Some(n), None) if n == "Object" && self.object.is_some() => {
                let w = word.unwrap_or_else(|| "Area".into());
                let p = PROPERTIES.iter().find(|p| p.eq_ignore_ascii_case(&w)).ok_or_else(|| other("Invalid option keyword."))?;
                self.property = Some(p.to_string());
                Ok(Step::Continue)
            }
            (Some(n), _) => match i {
                Input::Point(at) => {
                    let code = field_code(n, self.object, self.property.as_deref(), None).map_err(other)?;
                    add_field_text(s, at, &code, None, clock())?;
                    Ok(Step::Done)
                }
                Input::Enter => Ok(Step::Done),
                _ => Ok(Step::Continue),
            },
        }
    }
}

/// UPDATEFIELD at the command line: select objects, then their fields are evaluated again.
#[derive(Default)]
struct UpdateFieldM {
    sel: SelectPhase,
}

impl UpdateFieldM {
    fn update(s: &mut Session, hs: &[Handle]) -> Result<Step> {
        let (found, updated) = update_fields(s, Some(hs), clock())?;
        s.echo(format!("{found} field(s) found.  {updated} object(s) updated."));
        Ok(Step::Done)
    }
}

impl Interactive for UpdateFieldM {
    fn name(&self) -> &'static str {
        "UPDATEFIELD"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let hs = self.sel.picked.clone();
            return UpdateFieldM::update(s, &hs);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        self.sel.prompt()
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.sel.feed(s, &i)? {
            SelOutcome::More => Ok(Step::Continue),
            SelOutcome::Empty => Ok(Step::Done),
            SelOutcome::Done(hs) => UpdateFieldM::update(s, &hs),
        }
    }
}

/// DWGPROPS at the command line: a property, then its value.
#[derive(Default)]
struct DwgPropsM {
    which: Option<&'static str>,
}

impl Interactive for DwgPropsM {
    fn name(&self) -> &'static str {
        "DWGPROPS"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match self.which {
            None => Prompt::new("Enter property to change", super::super::curves::KW).kw(&DWGPROPS),
            Some(w) => {
                let cur = s.doc().map(|d| d.header.str(&prop_var(w), "")).unwrap_or_default();
                Prompt::new(format!("Enter {}", w.to_ascii_lowercase()), Accept::TEXT).default(cur)
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.which, i) {
            (None, Input::Keyword(k) | Input::Text(k)) => {
                self.which =
                    Some(DWGPROPS.iter().find(|p| p.eq_ignore_ascii_case(k.trim())).copied().ok_or_else(|| other("Invalid option keyword."))?);
                Ok(Step::Continue)
            }
            (Some(w), Input::Text(t)) => {
                s.doc_mut()?.header.set(&prop_var(w), cadcraft_doc::HVal::Str(t.chars().take(4096).collect()));
                Ok(Step::Done)
            }
            (_, Input::Enter) => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_format_with_pictures() {
        // 2026-10-11 08:05:09 UTC, a Sunday.
        let t = 1_791_705_909.0;
        assert_eq!(format_date(t, "yyyy-MM-dd HH:mm:ss"), "2026-10-11 08:05:09");
        assert_eq!(format_date(t, "dddd, MMMM d, yy h:mm tt"), "Sunday, October 11, 26 8:05 AM");
        assert_eq!(format_date(t, "'Plot' d.M.yyyy"), "Plot 11.10.2026");
        assert_eq!(format_date(0.0, "yyyy-MM-dd ddd"), "1970-01-01 Thu");
        assert_eq!(format_date(-86_400.0, "yyyy-MM-dd"), "1969-12-31");
    }
}
