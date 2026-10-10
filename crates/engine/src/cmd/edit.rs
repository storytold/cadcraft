//! Edit menu: undo/redo, clipboard, selection.

use cadcraft_doc::{Entity, Handle, entity_bounds};
use cadcraft_geom::{Bounds2, Mat3, Vec2};
use serde_json::{Value, json};

use super::*;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("undo", "Undo", run_undo).menu(&["Edit", "Undo"]).key("Cmd+Z").alias(&["u"]).params("{count?: n}").noundo(),
        CommandSpec::new("redo", "Redo", run_redo).menu(&["Edit", "Redo"]).key("Cmd+Shift+Z").alias(&["mredo"]).params("{count?: n}").noundo(),
        CommandSpec::new("cutclip", "Cut", run_cut).menu(&["Edit", "Cut"]).key("Cmd+X").params("{handles?}"),
        CommandSpec::new("copyclip", "Copy", run_copyclip).menu(&["Edit", "Copy"]).key("Cmd+C").params("{handles?}").noundo(),
        CommandSpec::new("copybase", "Copy with Base Point", run_copybase)
            .menu(&["Edit", "Copy with Base Point"])
            .key("Cmd+Shift+C")
            .params("{handles?, base: [x,y]}")
            .noundo(),
        CommandSpec::new("pasteclip", "Paste", run_paste)
            .menu(&["Edit", "Paste"])
            .key("Cmd+V")
            .params("{at?: [x,y]} (default: same offset as copied)")
            .interactive(|_| Ok(Box::new(PasteM { orig: false }))),
        CommandSpec::new("pasteorig", "Paste to Original Coordinates", run_pasteorig).menu(&["Edit", "Paste to Original Coordinates"]).params("{}"),
        CommandSpec::new("erase.selection", "Clear", run_clear).menu(&["Edit", "Clear"]).key("Delete").enabled(has_selection).params("{}"),
        CommandSpec::new("selectall", "Select All", run_selectall).menu(&["Edit", "Select All"]).key("Cmd+A").alias(&["ai_selall"]).noundo(),
        CommandSpec::new("select", "Select", run_select)
            .params("{handles: [hex]} | {window: [[x,y],[x,y]], crossing?: bool} | {at: [x,y]} | {clear: true} | {add?: bool}")
            .noundo()
            .interactive(|_| Ok(Box::new(SelectM::default()))),
    ]
}

/// A programmatic undo/redo (`engine.execute`, MCP, control channel, scripts) first ends a running
/// command the way Esc does, exactly as `Session::start` does for Cmd+Z, the menu or the command
/// line. Otherwise it would swap the drawing under the command and pop the previous command's step.
fn end_running_command(s: &mut Session) {
    if s.running.is_some() {
        s.cancel();
    }
}

fn run_undo(s: &mut Session, p: &Value) -> Result<Value> {
    end_running_command(s);
    let n = p.get("count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 1000);
    let mut labels = Vec::new();
    for _ in 0..n {
        match s.undo()? {
            Some(l) => labels.push(l),
            None => break,
        }
    }
    if labels.is_empty() {
        return Ok(json!({ "message": "Everything has been undone" }));
    }
    Ok(json!({ "undone": labels, "message": labels.join(", ").to_string() }))
}

fn run_redo(s: &mut Session, p: &Value) -> Result<Value> {
    end_running_command(s);
    let n = p.get("count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 1000);
    let mut labels = Vec::new();
    for _ in 0..n {
        match s.redo()? {
            Some(l) => labels.push(l),
            None => break,
        }
    }
    Ok(json!({ "redone": labels, "message": if labels.is_empty() { "Nothing to redo".to_string() } else { labels.join(", ") } }))
}

fn copy_to_clip(s: &mut Session, hs: &[Handle], base: Option<Vec2>) -> Result<usize> {
    let d = s.doc()?;
    let ents: Vec<Entity> = hs.iter().filter_map(|h| d.entity(*h).map(|e| (**e).clone())).collect();
    let b = ents.iter().fold(Bounds2::EMPTY, |acc, e| acc.union(&entity_bounds(d, e, 0)));
    s.clipboard_base = base.unwrap_or(b.min);
    let n = ents.len();
    s.clipboard = ents;
    Ok(n)
}

fn run_copyclip(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let n = copy_to_clip(s, &hs, None)?;
    Ok(json!({ "copied": n }))
}

fn run_copybase(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let base = point_param(p, "base");
    let n = copy_to_clip(s, &hs, base)?;
    Ok(json!({ "copied": n }))
}

fn run_cut(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let n = copy_to_clip(s, &hs, None)?;
    let d = s.doc_mut()?;
    for h in &hs {
        d.remove_entity(*h);
    }
    s.set_selection(Vec::new());
    Ok(json!({ "cut": n }))
}

pub(crate) fn paste(s: &mut Session, offset: Vec2) -> Result<Vec<Handle>> {
    let clip = s.clipboard.clone();
    let space = s.space();
    let m = Mat3::translate(offset);
    let d = s.doc_mut()?;
    let mut out = Vec::new();
    for mut e in clip {
        e.handle = d.new_handle();
        e.kind.transform(&m);
        d.ensure_layer(&e.common.layer);
        out.push(e.handle);
        if let Some(st) = d.space_mut(&space) {
            st.push(e);
        }
    }
    s.set_selection(out.clone());
    Ok(out)
}

fn run_paste(s: &mut Session, p: &Value) -> Result<Value> {
    if s.clipboard.is_empty() {
        return Err(bad("pasteclip", "the clipboard is empty"));
    }
    let off = match point_param(p, "at") {
        Some(at) => at - s.clipboard_base,
        None => Vec2::ZERO,
    };
    let r = paste(s, off)?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

fn run_pasteorig(s: &mut Session, _p: &Value) -> Result<Value> {
    let r = paste(s, Vec2::ZERO)?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

fn run_clear(s: &mut Session, _p: &Value) -> Result<Value> {
    let hs = s.selection();
    let d = s.doc_mut()?;
    let mut n = 0;
    for h in hs {
        if d.remove_entity(h).is_some() {
            n += 1;
        }
    }
    s.set_selection(Vec::new());
    Ok(json!({ "erased": n }))
}

fn run_selectall(s: &mut Session, _p: &Value) -> Result<Value> {
    let space = s.space();
    let d = s.doc()?;
    let hs: Vec<Handle> = d
        .space(&space)
        .map(|st| st.iter().filter(|e| d.is_visible(e) && d.layer(&e.common.layer).is_none_or(|l| !l.locked)).map(|e| e.handle).collect())
        .unwrap_or_default();
    let n = hs.len();
    s.set_selection(hs);
    Ok(json!({ "selected": n }))
}

fn run_select(s: &mut Session, p: &Value) -> Result<Value> {
    if bool_or(p, "clear", false) {
        s.set_selection(Vec::new());
        return Ok(json!({ "selected": 0 }));
    }
    let space = s.space();
    let add = bool_or(p, "add", false);
    let picked: Vec<Handle> = if let Some(w) = points_param(p, "window") {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { return Err(bad("select", "window needs 2 corners")) };
        crate::select::select_window(s.doc()?, &space, Bounds2::new(*a, *b), bool_or(p, "crossing", false))
    } else if let Some(at) = point_param(p, "at") {
        let ap = f64_or(p, "aperture", s.pixel_size() * s.settings.pickbox * 1.5);
        crate::select::pick(s.doc()?, &space, at, ap).into_iter().collect()
    } else if let Some(f) = points_param(p, "fence") {
        crate::select::select_fence(s.doc()?, &space, &f)
    } else {
        targets(s, p)?
    };
    let mut sel = if add { s.selection() } else { Vec::new() };
    sel.extend(picked);
    s.set_selection(sel);
    let sel = s.selection();
    Ok(json!({ "selected": sel.len(), "handles": sel.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

struct PasteM {
    orig: bool,
}

impl Interactive for PasteM {
    fn name(&self) -> &'static str {
        "PASTECLIP"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        if s.clipboard.is_empty() {
            s.echo("Clipboard is empty.");
            return Ok(Step::Done);
        }
        if self.orig {
            paste(s, Vec2::ZERO)?;
            return Ok(Step::Done);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Specify insertion point", Accept::POINT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                let base = s.clipboard_base;
                paste(s, p - base)?;
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<cadcraft_doc::EntityKind> {
        let m = Mat3::translate(c - s.clipboard_base);
        s.clipboard
            .iter()
            .take(500)
            .map(|e| {
                let mut k = e.kind.clone();
                k.transform(&m);
                k
            })
            .collect()
    }
}

#[derive(Default)]
struct SelectM {
    sel: super::machines::SelectPhase,
}

impl Interactive for SelectM {
    fn name(&self) -> &'static str {
        "SELECT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        self.sel.prompt()
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.sel.feed(s, &i)? {
            super::machines::SelOutcome::More => Ok(Step::Continue),
            _ => Ok(Step::Done),
        }
    }
}
