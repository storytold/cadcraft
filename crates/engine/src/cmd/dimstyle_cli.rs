//! -DIMSTYLE, dimension styles at the command line (scripts, the keyboard): save the current
//! settings as a style (annotative or not), make a style current, list the current settings or
//! a style's or a dimension's, compare a style with the current one, give the current settings
//! to dimensions, list the styles. Also `dimstyle.overrides`, which sets the current style's
//! overrides (what the Dimension Style Manager's Override edits), in one undo step.

use cadcraft_doc::{DimStyle, Drawing, EntityKind, Handle};
use serde_json::{Value, json};

use super::machines::{SelOutcome, SelectPhase};
use super::*;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

const CMD: &str = "-dimstyle";

fn other(m: impl Into<String>) -> EngineError {
    EngineError::Other(m.into())
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("-dimstyle", "Dimension Style", run)
            .params(
                "{option: save | annotative | restore | status | variables | apply | list, name?, handle?, handles?, replace?: bool, pattern?} \
                 → save (annotative: an annotative style): the current settings (style and its overrides) as style `name`, made current \
                 (`replace` redefines one); restore: style `name` (or dimension `handle`'s style) made current; status: {variables} of the \
                 current settings; variables: {variables} of style `name` or dimension `handle`; apply: the current settings given to \
                 `handles` (default: the selection); list: {styles} matching `pattern` (*, ? and #)",
            )
            .interactive(|_| Ok(Box::new(DashDimStyle::default()))),
        CommandSpec::new("dimstyle.overrides", "Dimension Style Overrides", run_overrides).params(
            "{<style fields or DIM* variables: values>, clear?: bool} → {overrides} (the current style's overrides, which new \
                 dimensions take; `clear` drops them all first)",
        ),
    ]
}

/// What new dimensions are drawn with: the current style with its overrides.
fn current_settings(d: &Drawing) -> DimStyle {
    let name = d.header.str("DIMSTYLE", "Standard");
    let mut st = d.dim_style(&name).cloned().unwrap_or_default().with_overrides(&d.dim_overrides());
    st.name = name;
    st
}

/// A style's variables (DIM* name → value) as `dimstyle.list` reports them.
fn variables(st: &DimStyle) -> serde_json::Map<String, Value> {
    let v = serde_json::to_value(st).unwrap_or(Value::Null);
    cadcraft_doc::DIMVARS.iter().filter_map(|(var, field)| v.get(*field).map(|x| ((*var).to_string(), x.clone()))).collect()
}

/// A variable's value as the command line shows it.
fn shown(v: &Value) -> String {
    if v.is_object()
        && let Ok(c) = serde_json::from_value::<cadcraft_color::Color>(v.clone())
    {
        return c.name();
    }
    match v {
        Value::String(s) if s.is_empty() => "\"\"".into(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => (if *b { "On" } else { "Off" }).into(),
        other => other.to_string(),
    }
}

/// One line per variable: name, value and what it is.
fn listing(vars: &serde_json::Map<String, Value>) -> Vec<String> {
    cadcraft_doc::DIMVARS
        .iter()
        .filter_map(|(var, field)| {
            let v = vars.get(*var)?;
            let label = crate::properties::dim_prop(field).map_or(*field, |p| p.label);
            Some(format!("{var:<10} {:<20} {label}", shown(v)))
        })
        .collect()
}

/// The variables of `b` that differ from `a`'s: (name, a's value, b's value).
fn differences(a: &DimStyle, b: &DimStyle) -> Vec<(String, Value, Value)> {
    let (va, vb) = (variables(a), variables(b));
    cadcraft_doc::DIMVARS
        .iter()
        .filter_map(|(var, _)| {
            let (x, y) = (va.get(*var)?, vb.get(*var)?);
            (x != y).then(|| ((*var).to_string(), x.clone(), y.clone()))
        })
        .collect()
}

/// `name` against a pattern with `*` (any run), `?` (any character) and `#` (a digit), ignoring
/// case: QSELECT's matcher, which takes time in proportion to the lengths (a pattern that backs
/// up at every `*` can't hang the command).
fn matches(pattern: &str, name: &str) -> bool {
    let p = pattern.trim();
    p.is_empty() || super::qselect::wildcard(p, name)
}

fn styles_matching(d: &Drawing, pattern: &str) -> Vec<String> {
    d.dim_styles.iter().filter(|s| matches(pattern, &s.name)).map(|s| s.name.clone()).collect()
}

/// A usable dimension style name, trimmed: for -DIMSTYLE and `dimstyle.saveas` alike. No `~`,
/// which Restore reads as "compare with".
pub(super) fn check_name(name: &str) -> Result<String> {
    let name = name.trim();
    if !usable_dim_style_name(name) {
        return Err(bad(CMD, "bad style name"));
    }
    Ok(name.to_string())
}

/// Whether `name` (trimmed) is one a new dimension style can have: what -DIMSTYLE,
/// `dimstyle.saveas`, `dimstyle` and `dimstyle.rename` take, for dialogs to check before OK.
pub fn usable_dim_style_name(name: &str) -> bool {
    let name = name.trim();
    !(name.is_empty() || name.len() > 255 || name.contains(['<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', ',', '=', '`', '~']))
}

/// Save the current settings as style `name` and make it current.
fn save(s: &mut Session, name: &str, annotative: Option<bool>, replace: bool) -> Result<Value> {
    let name = check_name(name)?;
    // Refused before the drawing is touched: a failed call leaves no (empty) undo step.
    if !replace && s.doc()?.dim_style(&name).is_some() {
        return Err(bad(CMD, format!("a dimension style `{name}` already exists")));
    }
    let d = s.doc_mut()?;
    let mut st = current_settings(d);
    if let Some(a) = annotative {
        st.annotative = a;
    }
    let name = match d.dim_styles.iter_mut().find(|x| x.name.eq_ignore_ascii_case(&name)) {
        Some(existing) if replace => {
            let kept = existing.name.clone();
            *existing = DimStyle { name: kept.clone(), ..st };
            kept
        }
        Some(_) => return Err(bad(CMD, format!("a dimension style `{name}` already exists"))),
        None => {
            d.dim_styles.push(DimStyle { name: name.clone(), ..st });
            name
        }
    };
    d.header.set_str("DIMSTYLE", &name);
    d.sync_dim_vars();
    // A redefined style redraws the dimensions that use it.
    super::props::regen_dims(d, |dm| dm.style.eq_ignore_ascii_case(&name))?;
    Ok(json!({ "style": name }))
}

/// Make style `name` current (its overrides go).
fn restore(s: &mut Session, name: &str) -> Result<Value> {
    let n = s.doc()?.dim_style(name).map(|t| t.name.clone()).ok_or_else(|| bad(CMD, format!("no dimension style `{name}`")))?;
    let d = s.doc_mut()?;
    d.header.set_str("DIMSTYLE", &n);
    d.sync_dim_vars();
    Ok(json!({ "current": n }))
}

/// Give dimensions the current settings: the current style and its overrides.
fn apply(s: &mut Session, hs: &[Handle]) -> Result<Value> {
    // The dimensions among them, found before the drawing is touched: none leaves no undo step.
    let d = s.doc()?;
    let dims: Vec<Handle> = hs.iter().copied().filter(|h| matches!(d.entity(*h).map(|e| &e.kind), Some(EntityKind::Dimension(_)))).collect();
    if dims.is_empty() {
        return Ok(json!({ "changed": 0 }));
    }
    let d = s.doc_mut()?;
    let style = d.header.str("DIMSTYLE", "Standard");
    let overrides = d.dim_overrides();
    let mut n = 0;
    for h in dims {
        d.modify_entity(h, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                dm.style.clone_from(&style);
                dm.overrides.clone_from(&overrides);
                dm.block = None;
                n += 1;
            }
        })?;
    }
    Ok(json!({ "changed": n }))
}

/// The style and effective settings of dimension `h`.
fn dimension_settings(s: &Session, h: Handle) -> Result<DimStyle> {
    let d = s.doc()?;
    match d.entity(h).map(|e| &e.kind) {
        Some(EntityKind::Dimension(dm)) => Ok(d.dim_style(&dm.style).cloned().unwrap_or_default().with_overrides(&dm.overrides)),
        _ => Err(bad(CMD, "not a dimension")),
    }
}

fn handle_param(p: &Value) -> Option<Handle> {
    p.get("handle").and_then(|v| v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle)))
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let option = str_param(p, "option").unwrap_or("restore").trim().to_ascii_lowercase();
    match option.as_str() {
        "save" | "annotative" => {
            let name = str_param(p, "name").ok_or_else(|| bad(CMD, "`name` is required"))?.to_string();
            let anno = if option == "annotative" { Some(true) } else { p.get("annotative").and_then(Value::as_bool) };
            save(s, &name, anno, bool_or(p, "replace", false))
        }
        "restore" => match handle_param(p) {
            Some(h) => {
                let name = dimension_settings(s, h)?.name;
                restore(s, &name)
            }
            None => restore(s, str_param(p, "name").ok_or_else(|| bad(CMD, "`name` or `handle` is required"))?),
        },
        "status" => Ok(json!({ "variables": variables(&current_settings(s.doc()?)) })),
        "variables" => {
            let st = match handle_param(p) {
                Some(h) => dimension_settings(s, h)?,
                None => {
                    let name = str_param(p, "name").ok_or_else(|| bad(CMD, "`name` or `handle` is required"))?;
                    s.doc()?.dim_style(name).cloned().ok_or_else(|| bad(CMD, format!("no dimension style `{name}`")))?
                }
            };
            Ok(json!({ "style": st.name, "variables": variables(&st) }))
        }
        "apply" => {
            let hs = targets(s, p)?;
            apply(s, &hs)
        }
        "list" | "?" => Ok(json!({ "styles": styles_matching(s.doc()?, str_param(p, "pattern").unwrap_or("*")) })),
        _ => Err(bad(CMD, "`option` is save, annotative, restore, status, variables, apply or list")),
    }
}

/// `dimstyle.overrides`: set the current style's overrides (the header DIM* variables).
fn run_overrides(s: &mut Session, p: &Value) -> Result<Value> {
    let clear = bool_or(p, "clear", false);
    let mut given = serde_json::Map::new();
    for (k, v) in p.as_object().into_iter().flatten() {
        if k == "clear" {
            continue;
        }
        let field = DimStyle::field_name(k)
            .filter(|f| !matches!(*f, "name" | "annotative"))
            .ok_or_else(|| bad("dimstyle.overrides", format!("`{k}` is not a dimension variable")))?;
        given.insert(field.to_string(), v.clone());
    }
    // Checked first (against the style alone when its overrides go), so nothing changes when
    // one value is unusable: a command's error doesn't undo what it did.
    let d = s.doc()?;
    let mut base = current_settings(d);
    if clear {
        base = d.dim_style(&base.name).cloned().unwrap_or_default();
    }
    let rejected = base.apply_fields(&given);
    if !rejected.is_empty() {
        return Err(bad("dimstyle.overrides", format!("unusable values for {}", rejected.join(", "))));
    }
    // The values as the style took them (a lineweight named ByLayer is -1, ...): what the
    // header variables hold, so writing them can't fail halfway.
    let taken = serde_json::to_value(&base).unwrap_or(Value::Null);
    if clear {
        s.doc_mut()?.sync_dim_vars();
    }
    for (field, v) in &given {
        let Some((var, _)) = cadcraft_doc::DIMVARS.iter().find(|(_, f)| f == field) else { continue };
        let v = taken.get(field.as_str()).unwrap_or(v);
        // Colors are stored as color numbers in the header.
        let v = match serde_json::from_value::<cadcraft_color::Color>(v.clone()).ok().or_else(|| v.as_str().and_then(cadcraft_color::Color::parse)) {
            Some(c) if field.ends_with("Color") => json!(c.to_aci()),
            _ => v.clone(),
        };
        crate::sysvars::set(s, var, &v)?;
    }
    Ok(json!({ "overrides": s.doc()?.dim_overrides() }))
}

/// What -DIMSTYLE asks next.
#[derive(Clone, Default)]
enum Ask {
    #[default]
    Option,
    Annotative,
    SaveName {
        annotative: Option<bool>,
    },
    Redefine {
        name: String,
        annotative: Option<bool>,
    },
    RestoreName,
    RestorePick,
    VariablesName,
    VariablesPick,
    Apply,
    ListPattern,
}

#[derive(Default)]
struct DashDimStyle {
    ask: Ask,
    sel: SelectPhase,
}

impl DashDimStyle {
    fn list(&self, s: &mut Session, pattern: &str) -> Result<()> {
        let names = styles_matching(s.doc()?, pattern);
        s.echo(format!("Named dimension styles: {}", if names.is_empty() { "none".into() } else { names.join(", ") }));
        Ok(())
    }
    fn show_variables(&self, s: &mut Session, st: &DimStyle) {
        s.echo(format!("Dimension style: {}", st.name));
        for line in listing(&variables(st)) {
            s.echo(line);
        }
    }
    fn answer(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let ask = std::mem::take(&mut self.ask);
        let text = match &i {
            Input::Text(t) | Input::Keyword(t) => Some(t.trim().to_string()),
            _ => None,
        };
        match ask {
            Ask::Option => {
                let k = text.unwrap_or_else(|| "Restore".into());
                self.ask = match k.to_ascii_lowercase().as_str() {
                    "annotative" | "an" => Ask::Annotative,
                    "save" | "s" => Ask::SaveName { annotative: None },
                    "restore" | "r" | "" => Ask::RestoreName,
                    "status" | "st" => {
                        let st = current_settings(s.doc()?);
                        self.show_variables(s, &st);
                        return Ok(Step::Done);
                    }
                    "variables" | "v" => Ask::VariablesName,
                    "apply" | "a" => Ask::Apply,
                    "?" => Ask::ListPattern,
                    _ => return Err(other("Enter one of the options.")),
                };
                Ok(Step::Continue)
            }
            Ask::Annotative => {
                let yes = !matches!(text.as_deref().map(str::to_ascii_lowercase).as_deref(), Some("no" | "n"));
                self.ask = Ask::SaveName { annotative: Some(yes) };
                Ok(Step::Continue)
            }
            Ask::SaveName { annotative } => match text.as_deref() {
                None | Some("") => Ok(Step::Done),
                Some("?") => {
                    self.list(s, "*")?;
                    self.ask = Ask::SaveName { annotative };
                    Ok(Step::Continue)
                }
                Some(name) => {
                    let name = match check_name(name) {
                        Ok(n) => n,
                        Err(e) => {
                            self.ask = Ask::SaveName { annotative };
                            return Err(e);
                        }
                    };
                    if s.doc()?.dim_style(&name).is_some() {
                        self.ask = Ask::Redefine { name, annotative };
                        return Ok(Step::Continue);
                    }
                    save(s, &name, annotative, false)?;
                    Ok(Step::Done)
                }
            },
            Ask::Redefine { name, annotative } => {
                if matches!(text.as_deref().map(str::to_ascii_lowercase).as_deref(), Some("yes" | "y")) {
                    save(s, &name, annotative, true)?;
                    Ok(Step::Done)
                } else {
                    self.ask = Ask::SaveName { annotative };
                    Ok(Step::Continue)
                }
            }
            Ask::RestoreName => match text.as_deref() {
                None | Some("") => {
                    self.ask = Ask::RestorePick;
                    Ok(Step::Continue)
                }
                Some("?") => {
                    self.list(s, "*")?;
                    self.ask = Ask::RestoreName;
                    Ok(Step::Continue)
                }
                // ~name: how style `name` differs from the current settings.
                Some(t) if t.starts_with('~') => {
                    let d = s.doc()?;
                    let other_style = d.dim_style(t.trim_start_matches('~').trim()).cloned().ok_or_else(|| other("No such dimension style."))?;
                    let current = current_settings(d);
                    let diff = differences(&current, &other_style);
                    s.echo(format!("Differences between {} and current settings:", other_style.name));
                    if diff.is_empty() {
                        s.echo("None".to_string());
                    }
                    for (var, cur, theirs) in diff {
                        s.echo(format!("{var:<10} {:<20} {}", shown(&theirs), shown(&cur)));
                    }
                    self.ask = Ask::RestoreName;
                    Ok(Step::Continue)
                }
                Some(name) => {
                    if let Err(e) = restore(s, name) {
                        self.ask = Ask::RestoreName;
                        return Err(e);
                    }
                    Ok(Step::Done)
                }
            },
            Ask::RestorePick | Ask::VariablesPick => {
                let restoring = matches!(ask, Ask::RestorePick);
                let Input::Point(p) = i else {
                    if matches!(i, Input::Enter) {
                        return Ok(Step::Done);
                    }
                    // Anything else asks for the dimension again.
                    self.ask = ask;
                    return Ok(Step::Continue);
                };
                let picked = curves::pick_at(s, p).and_then(|h| dimension_settings(s, h).ok());
                let Some(st) = picked else {
                    self.ask = if restoring { Ask::RestorePick } else { Ask::VariablesPick };
                    return Err(other("Select a dimension."));
                };
                if restoring {
                    restore(s, &st.name)?;
                } else {
                    self.show_variables(s, &st);
                }
                Ok(Step::Done)
            }
            Ask::VariablesName => match text.as_deref() {
                None | Some("") => {
                    self.ask = Ask::VariablesPick;
                    Ok(Step::Continue)
                }
                Some("?") => {
                    self.list(s, "*")?;
                    self.ask = Ask::VariablesName;
                    Ok(Step::Continue)
                }
                Some(name) => {
                    let Some(st) = s.doc()?.dim_style(name).cloned() else {
                        self.ask = Ask::VariablesName;
                        return Err(other("No such dimension style."));
                    };
                    self.show_variables(s, &st);
                    Ok(Step::Done)
                }
            },
            Ask::Apply => match self.sel.feed(s, &i)? {
                SelOutcome::More => {
                    self.ask = Ask::Apply;
                    Ok(Step::Continue)
                }
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    let r = apply(s, &hs)?;
                    s.echo(format!("{} dimension(s) take the current settings.", r["changed"]));
                    s.set_selection(Vec::new());
                    Ok(Step::Done)
                }
            },
            Ask::ListPattern => {
                self.list(s, text.as_deref().filter(|t| !t.is_empty()).unwrap_or("*"))?;
                Ok(Step::Done)
            }
        }
    }
}

impl Interactive for DashDimStyle {
    fn name(&self) -> &'static str {
        "-DIMSTYLE"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let d = s.doc()?;
        let current = d.header.str("DIMSTYLE", "Standard");
        let annotative = d.dim_style(&current).is_some_and(|st| st.annotative);
        let overridden = if d.dim_overrides().is_empty() { "" } else { " (with overrides)" };
        s.echo(format!("Current dimension style: {current}{overridden}  Annotative: {}", if annotative { "Yes" } else { "No" }));
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match &self.ask {
            Ask::Option => Prompt::new("Enter a dimension style option", curves::KW)
                .kw(&["ANnotative", "Save", "Restore", "STatus", "Variables", "Apply", "?"])
                .default("Restore"),
            Ask::Annotative => Prompt::new("Create annotative dimension style", curves::KW).kw(&["Yes", "No"]).default("Yes"),
            Ask::SaveName { .. } => Prompt::new("Enter name for new dimension style or [?]", Accept::TEXT).kw(&["?"]),
            Ask::Redefine { .. } => Prompt::new("That name is already in use, redefine it?", curves::KW).kw(&["Yes", "No"]).default("No"),
            Ask::RestoreName => Prompt::new("Enter a dimension style name, [?] or <select dimension>", Accept::TEXT).kw(&["?"]),
            Ask::RestorePick | Ask::VariablesPick => Prompt::new("Select dimension", Accept::POINT),
            Ask::VariablesName => Prompt::new("Enter a dimension style name, [?] or <select dimensions>", Accept::TEXT).kw(&["?"]),
            Ask::Apply => self.sel.prompt(),
            Ask::ListPattern => Prompt::new("Enter dimension style(s) to list", Accept::TEXT).default("*"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        // A refused answer asks the same question again.
        let asked = self.ask.clone();
        let r = self.answer(s, i);
        if r.is_err() {
            self.ask = asked;
        }
        r
    }
}
