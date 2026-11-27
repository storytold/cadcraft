//! IMAGE's options on the images a drawing already has: `path` points a definition at another
//! file, `reload` reads its file again and `detach` removes it with the images that show it. An
//! image is named by its definition (the name IMAGEATTACH gave it); a definition is one file path.

use std::collections::BTreeSet;
use std::sync::Arc;

use cadcraft_doc::{Drawing, EntityKind, EntityStore, Handle, Image};
use serde_json::{Value, json};

use super::{MAX_PATH, load_missing, paths, run_image};
use crate::cmd::qselect::wildcard;
use crate::cmd::{bad, str_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Snapshot, Step};

/// `{option, names?, path?}` of IMAGE and -IMAGE; `cmd` names the command in errors. `path` and
/// `detach` change the drawing and are one undo step each; the listing and `reload` only read, and
/// reading pixels again is no change to the drawing (it stays as saved, with no step to undo).
pub(super) fn run(s: &mut Session, p: &Value, cmd: &str) -> Result<Value> {
    let st = s.state()?;
    let before = (st.doc.clone(), st.selection.clone());
    let out = dispatch(s, p, cmd)?;
    let edits =
        p.get("option").and_then(Value::as_str).is_some_and(|o| matches!(o.trim().to_ascii_lowercase().as_str(), "path" | "p" | "detach" | "d"));
    if edits {
        let snap = Snapshot { label: "Raster Images".into(), doc: before.0, selection: before.1 };
        if s.lisp_groups_undo() {
            s.lisp_record_step(snap);
        } else {
            s.state_mut()?.push_undo(snap);
        }
        s.state_mut()?.revision += 1;
    }
    Ok(out)
}

fn dispatch(s: &mut Session, p: &Value, cmd: &str) -> Result<Value> {
    let option = match p.get("option") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(t)) => t.trim().to_ascii_lowercase(),
        Some(_) => return Err(bad(cmd, "`option` must be text")),
    };
    match option.as_str() {
        "" | "?" | "list" => {
            let mut list = run_image(s, &json!({}))?;
            if let Some(pats) = patterns(p, cmd)? {
                let keep = resolve(&definitions(s.doc()?), &pats, cmd)?;
                if let Some(rows) = list["images"].as_array_mut() {
                    rows.retain(|r| keep.iter().any(|(n, f)| r["name"] == json!(n) && r["path"] == json!(f)));
                }
            }
            Ok(list)
        }
        "reload" | "r" => {
            let pats = patterns(p, cmd)?.unwrap_or_else(|| vec!["*".into()]);
            let found = resolve(&definitions(s.doc()?), &pats, cmd)?;
            let files: Vec<String> = found.iter().map(|(_, f)| f.clone()).collect();
            load(s, &files, true)?;
            report(s, &found)
        }
        "path" | "p" => {
            let path = str_param(p, "path").map(str::trim).filter(|t| !t.is_empty()).ok_or_else(|| bad(cmd, "`path` is required"))?.to_string();
            if path.len() > MAX_PATH {
                return Err(bad(cmd, "`path` is too long"));
            }
            let pats = patterns(p, cmd)?.ok_or_else(|| bad(cmd, "`names` is required"))?;
            let found = resolve(&definitions(s.doc()?), &pats, cmd)?;
            let old: BTreeSet<&str> = found.iter().map(|(_, f)| f.as_str()).collect();
            let d = s.doc_mut()?;
            for_images(d, &|i| old.contains(i.path.as_str()), &mut |store, hs| {
                for h in hs {
                    store.modify(*h, |e| {
                        if let EntityKind::Image(i) = &mut e.kind {
                            i.path.clone_from(&path);
                        }
                    });
                }
            });
            drop_unused(d, &old);
            load(s, std::slice::from_ref(&path), false)?;
            let moved: Vec<(String, String)> = found.iter().map(|(n, _)| (n.clone(), path.clone())).collect();
            report(s, &moved)
        }
        "detach" | "d" => {
            let pats = patterns(p, cmd)?.ok_or_else(|| bad(cmd, "`names` is required"))?;
            let found = resolve(&definitions(s.doc()?), &pats, cmd)?;
            let old: BTreeSet<&str> = found.iter().map(|(_, f)| f.as_str()).collect();
            let d = s.doc_mut()?;
            let erased = for_images(d, &|i| old.contains(i.path.as_str()), &mut |store, hs| {
                for h in hs {
                    store.remove(*h);
                }
            });
            drop_unused(d, &old);
            s.set_selection(Vec::new());
            let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
            for n in &names {
                s.echo(format!("Image \"{n}\" detached."));
            }
            Ok(json!({ "detached": names, "erased": erased }))
        }
        _ => Err(bad(cmd, format!("unknown option \"{option}\""))),
    }
}

/// The definitions of `d`: name and file, in the order the drawing first uses them.
fn definitions(d: &Drawing) -> Vec<(String, String)> {
    let name_of = |path: &str| super::images(d).find(|i| i.path == path).map(|i| i.name.clone()).unwrap_or_default();
    paths(d).into_iter().map(|f| (name_of(&f), f)).collect()
}

/// `names` as patterns (`A,B*`, or a list of them): `None` when omitted.
fn patterns(p: &Value, cmd: &str) -> Result<Option<Vec<String>>> {
    let texts: Vec<&str> = match p.get("names") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(t)) => vec![t.as_str()],
        Some(Value::Array(a)) => a.iter().map(|v| v.as_str().ok_or_else(|| bad(cmd, "`names` must be text"))).collect::<Result<_>>()?,
        Some(_) => return Err(bad(cmd, "`names` must be text")),
    };
    let pats: Vec<String> = texts.iter().flat_map(|t| t.split(',')).map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect();
    if pats.is_empty() {
        return Err(bad(cmd, "`names` is empty"));
    }
    Ok(Some(pats))
}

/// The definitions of `defs` each pattern names, each once; a pattern that names none is an error.
fn resolve(defs: &[(String, String)], pats: &[String], cmd: &str) -> Result<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> = Vec::new();
    for pat in pats {
        let mut any = false;
        for d in defs.iter().filter(|(n, _)| wildcard(pat, n)) {
            any = true;
            if !out.contains(d) {
                out.push(d.clone());
            }
        }
        if !any {
            return Err(bad(cmd, format!("no image matches \"{pat}\"")));
        }
    }
    Ok(out)
}

/// Run `act` on the images of `d` that `hit` picks, store by store (model space, the layouts, the
/// blocks). Returns how many images that is.
fn for_images(d: &mut Drawing, hit: &dyn Fn(&Image) -> bool, act: &mut dyn FnMut(&mut EntityStore, &[Handle])) -> usize {
    let find =
        |st: &EntityStore| -> Vec<Handle> { st.iter().filter(|e| matches!(&e.kind, EntityKind::Image(i) if hit(i))).map(|e| e.handle).collect() };
    let mut n = 0;
    let hs = find(&d.model);
    if !hs.is_empty() {
        n += hs.len();
        act(&mut d.model, &hs);
    }
    for l in d.layouts.iter_mut() {
        let hs = find(&l.entities);
        if !hs.is_empty() {
            n += hs.len();
            act(&mut l.entities, &hs);
        }
    }
    for b in d.blocks.values_mut() {
        let hs = find(&b.entities);
        if !hs.is_empty() {
            n += hs.len();
            act(&mut Arc::make_mut(b).entities, &hs);
        }
    }
    n
}

/// Forget the pixels of the files of `old` that no image names any more.
fn drop_unused(d: &mut Drawing, old: &BTreeSet<&str>) {
    let used: BTreeSet<String> = paths(d).into_iter().collect();
    for f in old.iter().filter(|f| !used.contains(**f)) {
        d.rasters.remove(f);
    }
}

/// Read the files `files` into the drawing: those not loaded yet, or with `again` all of them (a
/// file that can't be read any more is dropped). Problems are reported; the status says the rest.
fn load(s: &mut Session, files: &[String], again: bool) -> Result<()> {
    let host = s.state()?.path.clone();
    let mut rasters = s.doc()?.rasters.clone();
    if again {
        for f in files {
            rasters.remove(f);
        }
    }
    let notes = load_missing(files, host.as_deref(), &mut rasters);
    // Pixels are a cache of the files, not part of the drawing: a drawing that was as saved stays so.
    let st = s.state_mut()?;
    let unchanged = Arc::ptr_eq(&st.doc, &st.saved);
    Arc::make_mut(&mut st.doc).rasters = rasters;
    if unchanged {
        st.saved = st.doc.clone();
    }
    st.revision += 1;
    for n in notes {
        s.echo(n);
    }
    Ok(())
}

/// The listing rows (as IMAGE lists them) of the definitions `found` (name, file), echoed.
fn report(s: &mut Session, found: &[(String, String)]) -> Result<Value> {
    let all = run_image(s, &json!({}))?;
    let rows: Vec<Value> = all["images"]
        .as_array()
        .map(|a| a.iter().filter(|r| found.iter().any(|(n, f)| r["name"] == json!(n) && r["path"] == json!(f))).cloned().collect())
        .unwrap_or_default();
    for r in &rows {
        let f = |k: &str| r[k].as_str().unwrap_or_default().to_string();
        s.echo(format!("Image \"{}\": {} ({})", f("name"), f("status"), f("path")));
    }
    Ok(json!({ "images": rows }))
}

/// Typed text without the quotes around it, or `None` for Enter.
fn text(i: &Input) -> Option<String> {
    match i {
        Input::Text(t) | Input::Keyword(t) => {
            let t = t.trim();
            Some(t.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(t).trim().to_string()).filter(|t| !t.is_empty())
        }
        _ => None,
    }
}

/// Where -IMAGE is in its prompts.
#[derive(Default)]
enum Stage {
    #[default]
    Option,
    /// Names for the option (its JSON name).
    Names(&'static str),
    /// The new file for these definitions.
    NewPath(String),
}

#[derive(Default)]
pub(super) struct DashImageM {
    stage: Stage,
}

/// Run an option chosen at the command line; a refusal is reported, not raised.
fn run_option(s: &mut Session, p: &Value) -> Result<Option<Value>> {
    match run(s, p, "-image") {
        Ok(v) => Ok(Some(v)),
        Err(EngineError::BadParams { msg, .. }) => {
            s.echo(msg);
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

impl Interactive for DashImageM {
    fn name(&self) -> &'static str {
        "-IMAGE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match &self.stage {
            Stage::Option => Prompt::new("Enter an option", crate::cmd::curves::KW).kw(&["?", "Path", "Reload", "Detach"]).default("?"),
            Stage::Names("?") => Prompt::new("Enter image name(s) to list", Accept::TEXT).default("*"),
            Stage::Names(op) => Prompt::new(format!("Enter image name(s) to {op}"), Accept::TEXT),
            Stage::NewPath(names) => Prompt::new(format!("Enter new path for \"{names}\""), Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if matches!(i, Input::Cancel) {
            return Ok(Step::Cancel);
        }
        match &self.stage {
            Stage::Option => {
                let k = match &i {
                    Input::Enter => "?".to_string(),
                    Input::Keyword(k) => k.clone(),
                    Input::Text(t) => match self.prompt(s).match_keyword(t) {
                        Some(k) => k,
                        None => {
                            s.echo("Invalid option keyword.");
                            return Ok(Step::Continue);
                        }
                    },
                    _ => return Ok(Step::Continue),
                };
                self.stage = Stage::Names(match k.as_str() {
                    "Path" => "path",
                    "Reload" => "reload",
                    "Detach" => "detach",
                    _ => "?",
                });
                Ok(Step::Continue)
            }
            Stage::Names(op) => {
                let op = *op;
                let names = match text(&i) {
                    Some(t) => t,
                    None if op == "?" => "*".into(),
                    None => return Ok(Step::Done),
                };
                if op == "path" {
                    self.stage = Stage::NewPath(names);
                    return Ok(Step::Continue);
                }
                if let Some(r) = run_option(s, &json!({ "option": op, "names": names }))?
                    && op == "?"
                {
                    let rows = r["images"].as_array().cloned().unwrap_or_default();
                    s.echo(format!("{:<24} {:<10} Path", "Image name", "Status"));
                    for x in &rows {
                        let f = |k: &str| x[k].as_str().unwrap_or_default().to_string();
                        s.echo(format!("{:<24} {:<10} {}", f("name"), f("status"), f("path")));
                    }
                    s.echo(format!("Total image(s): {}", rows.len()));
                }
                Ok(Step::Done)
            }
            Stage::NewPath(names) => {
                if let Some(t) = text(&i) {
                    run_option(s, &json!({ "option": "path", "names": names, "path": t }))?;
                }
                Ok(Step::Done)
            }
        }
    }
}
