//! Annotative scaling: the drawing's scale list (SCALELISTEDIT), the scales annotative objects
//! support (OBJECTSCALE, Add/Delete Current Scale, ANNORESET, ANNOUPDATE), which annotative
//! objects show (ANNOALLVISIBLE) and the current annotation scale (CANNOSCALE with
//! ANNOAUTOSCALE).

use cadcraft_doc::{AnnoScale, Drawing, EntityKind, Handle, MAX_SCALES, is_annotative};
use serde_json::{Value, json};

use super::machines::{SelOutcome, SelectPhase, SelectRun};
use super::{CommandSpec, bad, f64_req, targets};

use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("scalelistedit", "Scale List...", run_scalelistedit)
            .menu(&["Format", "Scale List..."])
            .alias(&["-scalelistedit"])
            .params(
                "{add?: {name, ratio: \"paper:drawing\" (e.g. \"1:50\") | paperUnits + drawingUnits}, delete?: name | [names], reset?: true | \"metric\" | \"imperial\"} → scales, current (no parameters: list only)",
            )
            .interactive(|_| Ok(Box::new(ScaleListM::default()))),
        CommandSpec::new("objectscale", "Add/Delete Scales...", run_objectscale)
            .menu(&["Modify", "Annotative Object Scale", "Add/Delete Scales..."])
            .alias(&["-objectscale"])
            .params("{handles?, add?: name | [names], delete?: name | [names]} (scales of the drawing's list; an object keeps at least one) → changed, objects")
            .interactive(|_| Ok(Box::new(ObjectScaleM::default()))),
        CommandSpec::new("aiobjectscaleadd", "Add Current Scale", run_add_current)
            .menu(&["Modify", "Annotative Object Scale", "Add Current Scale"])
            .params("{handles?} (adds the current annotation scale to the annotative objects)")
            .interactive(|_| Ok(Box::new(SelectRun::new("aiobjectscaleadd", "AIOBJECTSCALEADD")))),
        CommandSpec::new("aiobjectscaleremove", "Delete Current Scale", run_remove_current)
            .menu(&["Modify", "Annotative Object Scale", "Delete Current Scale"])
            .params("{handles?} (removes the current annotation scale from the annotative objects; an object keeps at least one)")
            .interactive(|_| Ok(Box::new(SelectRun::new("aiobjectscaleremove", "AIOBJECTSCALEREMOVE")))),
        CommandSpec::new("annoreset", "Synchronize Multiple-scale Positions", run_annoreset)
            .menu(&["Modify", "Annotative Object Scale", "Synchronize Multiple-scale Positions"])
            .params("{handles?} (every scale of an annotative object shares one position)")
            .interactive(|_| Ok(Box::new(SelectRun::new("annoreset", "ANNORESET")))),
        CommandSpec::new("annoupdate", "Annotative Update", run_annoupdate)
            .params("{handles?} (makes text, dimensions, multileaders and block references annotative or not as their style or block is)")
            .interactive(|_| Ok(Box::new(SelectRun::new("annoupdate", "ANNOUPDATE")))),
        CommandSpec::new("annovisible.current", "Current Scale Only", |s, _| set_all_visible(s, Some(false)))
            .menu(&["View", "Show Annotative Objects", "Current Scale Only"])
            .params("{} (ANNOALLVISIBLE 0: annotative objects show only at scales they support)"),
        CommandSpec::new("annovisible.all", "All Scales", |s, _| set_all_visible(s, Some(true)))
            .menu(&["View", "Show Annotative Objects", "All Scales"])
            .params("{} (ANNOALLVISIBLE 1: every annotative object shows)"),
        CommandSpec::new("annovisible.toggle", "Annotation Visibility", |s, _| set_all_visible(s, None)).params("{} (toggles ANNOALLVISIBLE)"),
    ]
}

/// The most objects one command call reports scales for.
const MAX_REPORTED: usize = 1000;

fn scale_json(sc: &AnnoScale) -> Value {
    json!({ "name": sc.name, "paperUnits": sc.paper_units, "drawingUnits": sc.drawing_units })
}

fn list_json(d: &Drawing) -> Value {
    json!({ "scales": d.scales.iter().map(scale_json).collect::<Vec<_>>(), "current": d.current_anno_scale().name })
}

/// Scale names given as one string or an array of strings.
fn names(cmd: &str, v: Option<&Value>) -> Result<Vec<String>> {
    match v {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(s)) => Ok(vec![s.trim().to_string()]),
        Some(Value::Array(a)) if a.len() <= MAX_SCALES => {
            a.iter().map(|v| v.as_str().map(|s| s.trim().to_string()).ok_or_else(|| bad(cmd, "scale names must be strings"))).collect()
        }
        Some(_) => Err(bad(cmd, "expected a scale name or an array of scale names")),
    }
}

fn find_scale(d: &Drawing, cmd: &str, name: &str) -> Result<AnnoScale> {
    d.anno_scale(name).cloned().ok_or_else(|| bad(cmd, format!("`{name}` is not in the scale list")))
}

/// A scale ratio typed as `paper:drawing` or `paper=drawing`, each side a number or distance
/// (`1:50`, `2:1`, `1/4":1'`).
pub(crate) fn parse_ratio(t: &str) -> Option<(f64, f64)> {
    let (a, b) = t.split_once(':').or_else(|| t.split_once('='))?;
    let ok = |v: f64| v.is_finite() && v > 0.0;
    let a = crate::units::parse_distance(a).filter(|v| ok(*v))?;
    let b = crate::units::parse_distance(b).filter(|v| ok(*v))?;
    Some((a, b))
}

/// Make `name` the current annotation scale (CANNOSCALE). With ANNOAUTOSCALE on, the annotative
/// objects of model space on layers that are on, thawed and unlocked get the scale too.
pub(crate) fn set_current(s: &mut Session, name: &str) -> Result<()> {
    let sc = find_scale(s.doc()?, "setvar", name)?;
    let auto = s.settings.annoautoscale;
    let d = s.doc_mut()?;
    d.header.set_str("CANNOSCALE", &sc.name);
    if auto {
        let usable = |e: &cadcraft_doc::Entity| d.layer(&e.common.layer).is_none_or(|l| l.visible() && !l.locked);
        let hs: Vec<Handle> = d.model.iter().filter(|e| is_annotative(e) && usable(e)).map(|e| e.handle).collect();
        for h in hs {
            add_scale(d, h, &sc);
        }
    }
    Ok(())
}

/// Add `sc` to annotative object `h`; whether it changed.
fn add_scale(d: &mut Drawing, h: Handle, sc: &AnnoScale) -> bool {
    let mut changed = false;
    let _ = d.modify_entity(h, |e| {
        if is_annotative(e) && !e.common.supports_scale(&sc.name) && e.common.anno_scales.len() < MAX_SCALES {
            e.common.anno_scales.push(sc.clone());
            changed = true;
        }
    });
    changed
}

/// What removing a scale from an object did.
#[derive(PartialEq)]
enum Removed {
    Yes,
    /// It is the object's only scale.
    Last,
    No,
}

fn remove_scale(d: &mut Drawing, h: Handle, name: &str) -> Removed {
    let mut r = Removed::No;
    let _ = d.modify_entity(h, |e| {
        if !is_annotative(e) || !e.common.supports_scale(name) {
            return;
        }
        if e.common.anno_scales.len() < 2 {
            r = Removed::Last;
            return;
        }
        e.common.anno_scales.retain(|s| !s.name.eq_ignore_ascii_case(name));
        r = Removed::Yes;
    });
    r
}

/// The annotative objects among the targets (an error when nothing is selected).
fn annotative_targets(s: &Session, p: &Value, cmd: &str) -> Result<(Vec<Handle>, usize)> {
    let hs = targets(s, p)?;
    if hs.is_empty() {
        return Err(bad(cmd, "nothing selected"));
    }
    let d = s.doc()?;
    let total = hs.len();
    let anno: Vec<Handle> = hs.into_iter().filter(|h| d.entity(*h).is_some_and(|e| is_annotative(e))).collect();
    Ok((anno, total))
}

fn objects_json(d: &Drawing, hs: &[Handle]) -> Value {
    json!(
        hs.iter()
            .take(MAX_REPORTED)
            .filter_map(|h| d.entity(*h))
            .map(|e| json!({ "handle": e.handle.hex(), "scales": e.common.anno_scales.iter().map(|s| s.name.clone()).collect::<Vec<_>>() }))
            .collect::<Vec<_>>()
    )
}

fn run_objectscale(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "objectscale";
    let (hs, total) = annotative_targets(s, p, CMD)?;
    let d = s.doc()?;
    let add = names(CMD, p.get("add"))?.iter().map(|n| find_scale(d, CMD, n)).collect::<Result<Vec<_>>>()?;
    let del = names(CMD, p.get("delete"))?;
    let d = s.doc_mut()?;
    let (mut changed, mut last) = (0usize, 0usize);
    for h in &hs {
        let mut any = false;
        for sc in &add {
            any |= add_scale(d, *h, sc);
        }
        for n in &del {
            match remove_scale(d, *h, n) {
                Removed::Yes => any = true,
                Removed::Last => last += 1,
                Removed::No => {}
            }
        }
        changed += usize::from(any);
    }
    let mut msg = format!("{changed} of {total} object(s) changed");
    if hs.len() < total {
        msg.push_str(&format!(", {} not annotative", total - hs.len()));
    }
    if last > 0 {
        msg.push_str(&format!(", {last} scale(s) kept as an object's only scale"));
    }
    Ok(json!({ "changed": changed, "annotative": hs.len(), "objects": objects_json(d, &hs), "message": msg }))
}

fn run_add_current(s: &mut Session, p: &Value) -> Result<Value> {
    let cur = s.doc()?.current_anno_scale();
    run_objectscale(s, &with_handles(s, p, json!({ "add": cur.name })))
}

fn run_remove_current(s: &mut Session, p: &Value) -> Result<Value> {
    let cur = s.doc()?.current_anno_scale();
    run_objectscale(s, &with_handles(s, p, json!({ "delete": cur.name })))
}

/// `extra` with the targets of `p` as `handles`.
fn with_handles(s: &Session, p: &Value, mut extra: Value) -> Value {
    let hs = targets(s, p).unwrap_or_default();
    if let Some(o) = extra.as_object_mut() {
        o.insert("handles".into(), json!(hs.iter().map(|h| h.hex()).collect::<Vec<_>>()));
    }
    extra
}

fn run_annoreset(s: &mut Session, p: &Value) -> Result<Value> {
    let (hs, _) = annotative_targets(s, p, "annoreset")?;
    // One position serves every scale of an object, so they are always in step.
    Ok(json!({ "objects": hs.len(), "message": format!("{} annotative object(s) synchronized", hs.len()) }))
}

fn run_annoupdate(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    if hs.is_empty() {
        return Err(bad("annoupdate", "nothing selected"));
    }
    let d = s.doc()?;
    let cur = d.current_anno_scale();
    // Hatches have no style: HPANNOTATIVE only sets new ones.
    let plan: Vec<(Handle, bool)> = hs
        .iter()
        .filter_map(|h| d.entity(*h))
        .filter(|e| cadcraft_doc::can_be_annotative(&e.kind) && !matches!(e.kind, EntityKind::Hatch(_)))
        .map(|e| (e.handle, d.annotative_by_style(&e.kind)))
        .filter(|(h, want)| d.entity(*h).is_some_and(|e| is_annotative(e) != *want))
        .collect();
    let d = s.doc_mut()?;
    for (h, want) in &plan {
        d.modify_entity(*h, |e| cadcraft_doc::set_annotative(e, *want, &cur))?;
    }
    Ok(json!({ "updated": plan.len(), "message": format!("{} object(s) updated", plan.len()) }))
}

fn set_all_visible(s: &mut Session, on: Option<bool>) -> Result<Value> {
    let d = s.doc_mut()?;
    let on = on.unwrap_or(!d.anno_all_visible());
    d.header.set_i64("ANNOALLVISIBLE", i64::from(on));
    Ok(json!({ "annoallvisible": i64::from(on) }))
}

// ---------------- the scale list ----------------

/// Scales objects use, and the current one: they stay when the list is reset and can't be
/// deleted.
fn in_use(d: &Drawing) -> Vec<AnnoScale> {
    let mut v = vec![d.current_anno_scale()];
    let ents = d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter()));
    for e in ents.filter(|e| is_annotative(e)) {
        for sc in &e.common.anno_scales {
            if v.len() < MAX_SCALES && !v.iter().any(|u| u.name.eq_ignore_ascii_case(&sc.name)) {
                v.push(sc.clone());
            }
        }
    }
    v
}

fn add_to_list(d: &mut Drawing, sc: AnnoScale) -> Result<()> {
    const CMD: &str = "scalelistedit";
    let name = sc.name.trim();
    if name.is_empty() || name.chars().count() > 255 {
        return Err(bad(CMD, "a scale name has 1 to 255 characters"));
    }
    if !sc.is_valid() {
        return Err(bad(CMD, "paper and drawing units must be positive"));
    }
    if d.anno_scale(name).is_some() {
        return Err(bad(CMD, format!("a scale named `{name}` already exists")));
    }
    if d.scales.len() >= MAX_SCALES {
        return Err(bad(CMD, "the scale list is full"));
    }
    d.scales.push(AnnoScale { name: name.to_string(), ..sc });
    Ok(())
}

fn delete_from_list(d: &mut Drawing, name: &str) -> Result<()> {
    const CMD: &str = "scalelistedit";
    let sc = find_scale(d, CMD, name)?;
    if d.current_anno_scale().name.eq_ignore_ascii_case(&sc.name) {
        return Err(bad(CMD, format!("`{}` is the current annotation scale", sc.name)));
    }
    if in_use(d).iter().any(|u| u.name.eq_ignore_ascii_case(&sc.name)) {
        return Err(bad(CMD, format!("`{}` is used by annotative objects", sc.name)));
    }
    d.scales.retain(|s| !s.name.eq_ignore_ascii_case(&sc.name));
    Ok(())
}

/// Replace the list with the default metric or imperial one, keeping scales in use.
fn reset_list(d: &mut Drawing, metric: bool) {
    let keep = in_use(d);
    let mut list = if metric { cadcraft_doc::metric_scales() } else { cadcraft_doc::imperial_scales() };
    for sc in keep {
        if !list.iter().any(|l| l.name.eq_ignore_ascii_case(&sc.name)) {
            list.push(sc);
        }
    }
    d.scales = list;
}

fn run_scalelistedit(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "scalelistedit";
    let metric = s.doc()?.header.i64("MEASUREMENT", 0) == 1;
    let reset = match p.get("reset") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(Value::Bool(true)) => Some(metric),
        Some(Value::String(t)) if t.eq_ignore_ascii_case("metric") => Some(true),
        Some(Value::String(t)) if t.eq_ignore_ascii_case("imperial") => Some(false),
        Some(_) => return Err(bad(CMD, "`reset` is true, \"metric\" or \"imperial\"")),
    };
    let del = names(CMD, p.get("delete"))?;
    let add = match p.get("add") {
        None | Some(Value::Null) => None,
        Some(a) => {
            let name = a.get("name").and_then(Value::as_str).ok_or_else(|| bad(CMD, "`add.name` is required"))?;
            let (pu, du) = match a.get("ratio").and_then(Value::as_str) {
                Some(r) => parse_ratio(r).ok_or_else(|| bad(CMD, format!("`{r}` is not a ratio such as 1:50")))?,
                None => (f64_req(CMD, a, "paperUnits")?, f64_req(CMD, a, "drawingUnits")?),
            };
            Some(AnnoScale::new(name, pu, du))
        }
    };
    let d = s.doc_mut()?;
    if let Some(m) = reset {
        reset_list(d, m);
    }
    for n in &del {
        delete_from_list(d, n)?;
    }
    if let Some(sc) = add {
        add_to_list(d, sc)?;
    }
    Ok(list_json(d))
}

fn echo_list(s: &mut Session) {
    let lines: Vec<String> = s
        .doc()
        .map(|d| {
            d.scales
                .iter()
                .map(|sc| format!("  {}  ({} paper = {} drawing)", sc.name, fmt_units(sc.paper_units), fmt_units(sc.drawing_units)))
                .collect()
        })
        .unwrap_or_default();
    for l in lines {
        s.echo(l);
    }
}

fn fmt_units(v: f64) -> String {
    let r = (v * 1e6).round() / 1e6;
    if r.fract() == 0.0 { format!("{r:.0}") } else { format!("{r}") }
}

#[derive(Default)]
enum ListStage {
    #[default]
    Option,
    AddName,
    AddRatio(String),
    Delete,
    Reset,
}

/// -SCALELISTEDIT: "Enter option [?/Add/Delete/Reset/Exit]", repeated until Exit or Enter.
#[derive(Default)]
struct ScaleListM {
    stage: ListStage,
}

impl Interactive for ScaleListM {
    fn name(&self) -> &'static str {
        "SCALELISTEDIT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match &self.stage {
            ListStage::Option => Prompt::new("Enter option", Accept::TEXT).kw(&["?", "Add", "Delete", "Reset", "Exit"]).default("Exit"),
            ListStage::AddName => Prompt::new("Enter name for new scale", Accept::TEXT),
            ListStage::AddRatio(_) => Prompt::new("Enter scale ratio (paper units:drawing units)", Accept::TEXT).default("1:1"),
            ListStage::Delete => Prompt::new("Enter name of scale to delete", Accept::TEXT).kw(&["?"]),
            ListStage::Reset => Prompt::new("Reset the scale list to the defaults?", Accept::TEXT).kw(&["Yes", "No"]).default("No"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let text = match i {
            Input::Cancel => return Ok(Step::Cancel),
            Input::Enter => String::new(),
            Input::Text(t) | Input::Keyword(t) => t.trim().to_string(),
            _ => return Ok(Step::Continue),
        };
        let stage = std::mem::take(&mut self.stage);
        let result = match stage {
            ListStage::Option => match text.to_ascii_lowercase().as_str() {
                "" | "e" | "exit" => return Ok(Step::Done),
                "?" => {
                    echo_list(s);
                    Ok(())
                }
                "a" | "add" => {
                    self.stage = ListStage::AddName;
                    Ok(())
                }
                "d" | "delete" => {
                    self.stage = ListStage::Delete;
                    Ok(())
                }
                "r" | "reset" => {
                    self.stage = ListStage::Reset;
                    Ok(())
                }
                _ => {
                    s.echo("Invalid option keyword.");
                    Ok(())
                }
            },
            ListStage::AddName if text.is_empty() => Ok(()),
            ListStage::AddName => {
                if s.doc()?.anno_scale(&text).is_some() {
                    s.echo(format!("A scale named \"{text}\" already exists."));
                } else {
                    self.stage = ListStage::AddRatio(text);
                }
                Ok(())
            }
            ListStage::AddRatio(name) => {
                let t = if text.is_empty() { "1:1" } else { text.as_str() };
                match parse_ratio(t) {
                    Some((pu, du)) => add_to_list(s.doc_mut()?, AnnoScale::new(&name, pu, du)),
                    None => {
                        s.echo("Enter a ratio such as 1:50.");
                        self.stage = ListStage::AddRatio(name);
                        Ok(())
                    }
                }
            }
            ListStage::Delete => match text.as_str() {
                "" => Ok(()),
                "?" => {
                    echo_list(s);
                    self.stage = ListStage::Delete;
                    Ok(())
                }
                n => delete_from_list(s.doc_mut()?, n),
            },
            ListStage::Reset => {
                if matches!(text.to_ascii_lowercase().as_str(), "y" | "yes") {
                    let metric = s.doc()?.header.i64("MEASUREMENT", 0) == 1;
                    reset_list(s.doc_mut()?, metric);
                }
                Ok(())
            }
        };
        if let Err(e) = result {
            s.echo(e.to_string());
        }
        Ok(Step::Continue)
    }
}

#[derive(Default, Clone, Copy, PartialEq)]
enum ObjStage {
    #[default]
    Select,
    Option,
    Add,
    Delete,
}

/// -OBJECTSCALE: select annotative objects, then add or delete scales by name until Enter.
#[derive(Default)]
struct ObjectScaleM {
    sel: SelectPhase,
    stage: ObjStage,
    hs: Vec<Handle>,
}

impl ObjectScaleM {
    /// Keep the annotative objects of the selection; false when there are none.
    fn take(&mut self, s: &mut Session, picked: Vec<Handle>) -> Result<bool> {
        let d = s.doc()?;
        self.hs = picked.into_iter().filter(|h| d.entity(*h).is_some_and(|e| is_annotative(e))).collect();
        if self.hs.is_empty() {
            s.echo("No annotative objects selected.");
            return Ok(false);
        }
        self.stage = ObjStage::Option;
        Ok(true)
    }
    fn apply(&self, s: &mut Session, key: &str, name: &str) {
        let mut p = json!({ "handles": self.hs.iter().map(|h| h.hex()).collect::<Vec<_>>() });
        if let Some(o) = p.as_object_mut() {
            o.insert(key.to_string(), json!(name));
        }
        match run_objectscale(s, &p) {
            Ok(r) => {
                if let Some(m) = r.get("message").and_then(Value::as_str) {
                    s.echo(m.to_string());
                }
            }
            Err(e) => s.echo(e.to_string()),
        }
    }
    fn echo_supported(&self, s: &mut Session) {
        let Ok(d) = s.doc() else { return };
        let mut names: Vec<String> = Vec::new();
        for e in self.hs.iter().filter_map(|h| d.entity(*h)) {
            for sc in &e.common.anno_scales {
                if names.len() < MAX_SCALES && !names.iter().any(|n| n.eq_ignore_ascii_case(&sc.name)) {
                    names.push(sc.name.clone());
                }
            }
        }
        s.echo(format!("Scales of the selected objects: {}", names.join(", ")));
    }
}

impl Interactive for ObjectScaleM {
    fn name(&self) -> &'static str {
        "OBJECTSCALE"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let picked = self.sel.picked.clone();
            if !self.take(s, picked)? {
                return Ok(Step::Done);
            }
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let cur = s.doc().map(|d| d.current_anno_scale().name).unwrap_or_else(|_| "1:1".into());
        match self.stage {
            ObjStage::Select => Prompt::new(if self.sel.removing { "Remove objects" } else { "Select annotative objects" }, Accept::SELECT),
            ObjStage::Option => Prompt::new("Enter an option", Accept::TEXT).kw(&["Add", "Delete", "?"]),
            ObjStage::Add => Prompt::new("Enter named scale to add", Accept::TEXT).kw(&["?"]).default(cur),
            ObjStage::Delete => Prompt::new("Enter named scale to delete", Accept::TEXT).kw(&["?"]).default(cur),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if i == Input::Cancel {
            return Ok(Step::Cancel);
        }
        if self.stage == ObjStage::Select {
            return match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => Ok(if self.take(s, hs)? { Step::Continue } else { Step::Done }),
            };
        }
        let text = match i {
            Input::Enter => String::new(),
            Input::Text(t) | Input::Keyword(t) => t.trim().to_string(),
            _ => return Ok(Step::Continue),
        };
        match self.stage {
            ObjStage::Option => match text.to_ascii_lowercase().as_str() {
                "" => return Ok(Step::Done),
                "a" | "add" => self.stage = ObjStage::Add,
                "d" | "delete" => self.stage = ObjStage::Delete,
                "?" => self.echo_supported(s),
                _ => s.echo("Invalid option keyword."),
            },
            stage @ (ObjStage::Add | ObjStage::Delete) => {
                if text == "?" {
                    echo_list(s);
                    return Ok(Step::Continue);
                }
                let name = if text.is_empty() { s.doc()?.current_anno_scale().name } else { text };
                self.apply(s, if stage == ObjStage::Add { "add" } else { "delete" }, &name);
                self.stage = ObjStage::Option;
            }
            ObjStage::Select => {}
        }
        Ok(Step::Continue)
    }
}
