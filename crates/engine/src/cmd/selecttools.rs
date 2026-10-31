//! Selection tools: SELECTSIMILAR (objects like the selected ones, per SELECTSIMILARMODE) and
//! object isolation — ISOLATEOBJECTS, HIDEOBJECTS, UNISOLATEOBJECTS.
//!
//! Isolation is temporary (OBJECTISOLATIONMODE 0): hidden objects are kept in
//! [`Drawing::hidden_objects`], which display, selection, snapping and plotting skip and which
//! is never written to a file. Hiding and showing are undoable like any other change.

use cadcraft_doc::{Drawing, Entity, EntityKind, Handle};
use serde_json::{Value, json};

use super::machines::{SelOutcome, SelectPhase, SelectRun};
use super::{CommandSpec, bad, targets};

use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// Most objects one call looks at (a hostile drawing can be huge).
const MAX_SCAN: usize = 2_000_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("selectsimilar", "Select Similar", run_selectsimilar)
            .params(
                "{handles? (default the selection), mode?: SELECTSIMILARMODE bits (1 colour, 2 layer, 4 linetype, 8 linetype scale, \
                 16 lineweight, 32 plot style, 64 object style, 128 name; default the variable, 130)} → selects every visible object \
                 in the current space of the same type with those properties: {selected, handles}",
            )
            .noundo()
            .interactive(|_| Ok(Box::new(SelectSimilarM::default()))),
        CommandSpec::new("isolateobjects", "Isolate Objects", run_isolate)
            .menu(&["Tools", "Isolate", "Isolate Objects"])
            .params("{handles? (default the selection)} → hides every other object of the current space for this session: {hidden}")
            .interactive(|_| Ok(Box::new(SelectRun::new("isolateobjects", "ISOLATEOBJECTS")))),
        CommandSpec::new("hideobjects", "Hide Objects", run_hide)
            .menu(&["Tools", "Isolate", "Hide Objects"])
            .params("{handles? (default the selection)} → hides these objects for this session: {hidden}")
            .interactive(|_| Ok(Box::new(SelectRun::new("hideobjects", "HIDEOBJECTS")))),
        CommandSpec::new("unisolateobjects", "End Object Isolation", run_unisolate)
            .menu(&["Tools", "Isolate", "End Object Isolation"])
            .params("{} → shows every object ISOLATEOBJECTS or HIDEOBJECTS hid: {shown}"),
    ]
}

// ---------------- SELECTSIMILAR ----------------

/// SELECTSIMILARMODE bits.
const COLOR: u32 = 1;
const LAYER: u32 = 2;
const LINETYPE: u32 = 4;
const LTSCALE: u32 = 8;
const LINEWEIGHT: u32 = 16;
const PLOT_STYLE: u32 = 32;
const OBJECT_STYLE: u32 = 64;
const NAME: u32 = 128;

/// The style an object is drawn with: text, dimension, leader, multileader or table style, or a
/// hatch pattern.
fn object_style(e: &Entity) -> Option<&str> {
    Some(match &e.kind {
        EntityKind::Text(t) => &t.style,
        EntityKind::MText(t) => &t.style,
        EntityKind::AttDef(a) => &a.text.style,
        EntityKind::Dimension(d) => &d.style,
        EntityKind::Leader(l) => &l.style,
        EntityKind::MLeader(m) => &m.style,
        EntityKind::Table(t) => &t.style,
        EntityKind::Hatch(h) => &h.pattern,
        _ => return None,
    })
}

/// The name an object references: a block reference's block, a raster image's definition.
fn object_name(e: &Entity) -> Option<&str> {
    match &e.kind {
        EntityKind::Insert(i) => Some(&i.block),
        EntityKind::Image(i) => Some(if i.name.is_empty() { &i.path } else { &i.name }),
        _ => None,
    }
}

fn same_text(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        (None, None) => true,
        _ => false,
    }
}

/// Is `e` like `r`: the same object type and, for each bit of `mode`, the same property?
pub(crate) fn similar(e: &Entity, r: &Entity, mode: u32) -> bool {
    let (a, b) = (&e.common, &r.common);
    e.kind.dxf_name().eq_ignore_ascii_case(r.kind.dxf_name())
        && (mode & COLOR == 0 || a.color == b.color)
        && (mode & LAYER == 0 || a.layer.eq_ignore_ascii_case(&b.layer))
        && (mode & LINETYPE == 0 || a.linetype.eq_ignore_ascii_case(&b.linetype))
        && (mode & LTSCALE == 0 || (a.ltscale - b.ltscale).abs() <= 1e-9 * a.ltscale.abs().max(b.ltscale.abs()).max(1.0))
        && (mode & LINEWEIGHT == 0 || a.lineweight == b.lineweight)
        && (mode & PLOT_STYLE == 0 || a.plot_style.eq_ignore_ascii_case(&b.plot_style))
        && (mode & OBJECT_STYLE == 0 || same_text(object_style(e), object_style(r)))
        && (mode & NAME == 0 || same_text(object_name(e), object_name(r)))
}

/// The visible objects of the current space like any of `refs`.
fn similar_objects(s: &Session, d: &Drawing, refs: &[Handle], mode: u32) -> Vec<Handle> {
    let refs: Vec<&Entity> = refs.iter().filter_map(|h| d.entity(*h).map(|e| e.as_ref())).collect();
    if refs.is_empty() {
        return Vec::new();
    }
    let Some(store) = d.space(&s.space()) else { return Vec::new() };
    store.iter().take(MAX_SCAN).filter(|e| d.is_visible(e) && refs.iter().any(|r| similar(e, r, mode))).map(|e| e.handle).collect()
}

fn mode_param(s: &Session, p: &Value) -> Result<u32> {
    match p.get("mode") {
        None | Some(Value::Null) => Ok(s.settings.selectsimilarmode),
        Some(v) => v
            .as_u64()
            .filter(|m| *m <= 255)
            .map(|m| m as u32)
            .ok_or_else(|| bad("selectsimilar", "`mode` must be a SELECTSIMILARMODE bit code from 0 to 255")),
    }
}

fn run_selectsimilar(s: &mut Session, p: &Value) -> Result<Value> {
    let refs = targets(s, p)?;
    let mode = mode_param(s, p)?;
    if refs.is_empty() {
        return Err(bad("selectsimilar", "select objects to match"));
    }
    let found = similar_objects(s, s.doc()?, &refs, mode);
    if found.is_empty() {
        return Err(bad("selectsimilar", "none of the given objects exist in the current space"));
    }
    s.set_selection(found);
    let sel = s.selection();
    let n = sel.len();
    Ok(json!({ "selected": n, "handles": sel.iter().map(|h| h.hex()).collect::<Vec<_>>(), "message": format!("{n} found") }))
}

/// SELECTSIMILAR typed: "Select objects or [SEttings]:"; Enter selects the similar objects.
/// SEttings asks for the SELECTSIMILARMODE bit code (the command line has no dialog).
#[derive(Default)]
struct SelectSimilarM {
    sel: SelectPhase,
    settings: bool,
}

impl SelectSimilarM {
    fn finish(&self, s: &mut Session, hs: &[Handle]) -> Result<Step> {
        let r = run_selectsimilar(s, &json!({ "handles": hs.iter().map(|h| h.hex()).collect::<Vec<_>>() }))?;
        if let Some(m) = r.get("message").and_then(Value::as_str) {
            s.echo(m.to_string());
        }
        Ok(Step::Done)
    }
}

impl Interactive for SelectSimilarM {
    fn name(&self) -> &'static str {
        "SELECTSIMILAR"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let hs = self.sel.picked.clone();
            return self.finish(s, &hs);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        if self.settings {
            return Prompt::new("Enter properties to match (SELECTSIMILARMODE bit code)", Accept::NUMBER)
                .default(s.settings.selectsimilarmode.to_string());
        }
        self.sel.prompt().kw(&["SEttings"])
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.settings {
            match &i {
                Input::Enter => {}
                Input::Text(t) | Input::Keyword(t) => crate::sysvars::set(s, "SELECTSIMILARMODE", &json!(t.trim()))?,
                _ => return Ok(Step::Continue),
            }
            self.settings = false;
            return Ok(Step::Continue);
        }
        if let Input::Keyword(k) | Input::Text(k) = &i
            && k.trim().eq_ignore_ascii_case("settings")
        {
            self.settings = true;
            return Ok(Step::Continue);
        }
        match self.sel.feed(s, &i)? {
            SelOutcome::More => Ok(Step::Continue),
            SelOutcome::Empty => Ok(Step::Done),
            SelOutcome::Done(hs) => self.finish(s, &hs),
        }
    }
}

// ---------------- isolation ----------------

/// Hide `hs` (objects that exist and aren't hidden yet); returns how many were hidden.
fn hide(s: &mut Session, hs: Vec<Handle>) -> Result<usize> {
    let d = s.doc()?;
    let hs: Vec<Handle> = hs.into_iter().filter(|h| d.entity(*h).is_some() && !d.is_hidden_object(*h)).collect();
    if !hs.is_empty() {
        s.doc_mut()?.hidden_objects.extend(hs.iter().copied());
    }
    // Hidden objects can't stay selected.
    s.set_selection(Vec::new());
    Ok(hs.len())
}

fn hidden_message(n: usize) -> String {
    if n == 1 { "1 object hidden".to_string() } else { format!("{n} objects hidden") }
}

fn run_isolate(s: &mut Session, p: &Value) -> Result<Value> {
    let keep = targets(s, p)?;
    if keep.is_empty() {
        return Err(bad("isolateobjects", "select objects to isolate"));
    }
    let d = s.doc()?;
    if !keep.iter().any(|h| d.entity(*h).is_some()) {
        return Err(bad("isolateobjects", "none of the given objects exist"));
    }
    let keep_set: std::collections::HashSet<Handle> = keep.iter().copied().collect();
    let others: Vec<Handle> = d
        .space(&s.space())
        .map(|st| st.iter().take(MAX_SCAN).filter(|e| !keep_set.contains(&e.handle)).map(|e| e.handle).collect())
        .unwrap_or_default();
    let n = hide(s, others)?;
    Ok(json!({ "hidden": n, "message": hidden_message(n) }))
}

fn run_hide(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    if hs.is_empty() {
        return Err(bad("hideobjects", "select objects to hide"));
    }
    let n = hide(s, hs)?;
    Ok(json!({ "hidden": n, "message": hidden_message(n) }))
}

fn run_unisolate(s: &mut Session, _p: &Value) -> Result<Value> {
    let n = s.doc()?.hidden_objects.len();
    if n == 0 {
        return Ok(json!({ "shown": 0, "message": "No objects are isolated or hidden." }));
    }
    s.doc_mut()?.hidden_objects.clear();
    let msg = if n == 1 { "1 object shown".to_string() } else { format!("{n} objects shown") };
    Ok(json!({ "shown": n, "message": msg }))
}
