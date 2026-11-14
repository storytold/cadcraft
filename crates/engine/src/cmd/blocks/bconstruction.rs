//! BCONSTRUCTION marks block geometry and controls its editor-only display.

use std::collections::HashSet;

use cadcraft_doc::{EntityKind, Handle};
use serde_json::{Value, json};

use super::bedit;
use crate::cmd::curves::KW;
use crate::cmd::machines::{SelOutcome, SelectPhase};
use crate::cmd::{CommandSpec, bad, str_param, targets};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// The command id.
const COMMAND: &str = "bconstruction";
/// The most objects one call accepts, which bounds the work of a single undo step.
const MAX_TARGETS: usize = 65_536;
/// The operation keywords of the first prompt.
const OPTIONS: &[&str] = &["Convert", "Revert", "Show all", "Hide all"];

/// The BCONSTRUCTION command, enabled only while the Block Editor is open.
pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new(COMMAND, "Construction Geometry", run)
            .menu(&["Tools", "Construction Geometry"])
            .params("{mode: convert|revert|show|hide, handles?: [objects] (convert/revert: default selection)} -> {mode, changed?, construction}. Convert marks ordinary geometry as construction; Revert removes that mark and moves it to the current layer. Show/Hide changes the construction view in the Block Editor.")
            .enabled(bedit::editing)
            .interactive(|_| Ok(Box::new(Construction::default()))),
    ]
}

/// One BCONSTRUCTION operation.
#[derive(Clone, Copy)]
enum Mode {
    Convert,
    Revert,
    Show,
    Hide,
}

impl Mode {
    /// The operation `word` names: its full name (case, spaces, dashes and underscores ignored) or its first letter.
    fn parse(word: &str) -> Option<Self> {
        if word.len() > 32 {
            return None;
        }
        match word.trim().to_ascii_lowercase().replace([' ', '-', '_'], "").as_str() {
            "convert" | "c" => Some(Self::Convert),
            "revert" | "r" => Some(Self::Revert),
            "show" | "showall" | "s" => Some(Self::Show),
            "hide" | "hideall" | "h" => Some(Self::Hide),
            _ => None,
        }
    }

    /// The operation name used in the command result and in the params.
    fn name(self) -> &'static str {
        match self {
            Self::Convert => "convert",
            Self::Revert => "revert",
            Self::Show => "show",
            Self::Hide => "hide",
        }
    }
}

/// How many objects of the edited block are construction geometry.
fn construction_count(s: &Session) -> Result<usize> {
    Ok(s.doc()?.model.iter().filter(|e| e.common.is_construction()).count())
}

/// Validate the whole selection before any drawing, selection or history mutation.
fn selected(s: &Session, p: &Value) -> Result<Vec<Handle>> {
    if p.get("handles").and_then(Value::as_array).is_some_and(|a| a.len() > MAX_TARGETS) {
        return Err(bad(COMMAND, "at most 65536 object handles are accepted"));
    }
    if p.get("handles").is_none_or(Value::is_null) && p.get("handle").is_none_or(Value::is_null) && s.state()?.selection.len() > MAX_TARGETS {
        return Err(bad(COMMAND, "at most 65536 selected objects are accepted"));
    }
    let handles = targets(s, p)?;
    if handles.is_empty() || handles.len() > MAX_TARGETS {
        return Err(bad(COMMAND, "select between 1 and 65536 objects"));
    }
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for h in handles {
        let e = s.doc()?.model.get(h).ok_or_else(|| bad(COMMAND, format!("object {} is not in the Block Editor", h.hex())))?;
        if matches!(e.kind, EntityKind::AttDef(_) | EntityKind::Unknown(_)) {
            return Err(bad(COMMAND, format!("object {} is not supported construction geometry", h.hex())));
        }
        if s.doc()?.layer(&e.common.layer).is_some_and(|layer| layer.locked) {
            return Err(bad(COMMAND, format!("object {} is on a locked layer", h.hex())));
        }
        if seen.insert(h) {
            unique.push(h);
        }
    }
    Ok(unique)
}

/// Marks (`on`) or unmarks the selected objects as construction geometry and returns how many changed. All or nothing: the whole selection is checked first, and Revert also moves the objects to the current layer.
fn convert(s: &mut Session, p: &Value, on: bool) -> Result<usize> {
    let handles = selected(s, p)?;
    let d = s.doc()?;
    let changed: Vec<_> = handles.into_iter().filter(|h| d.model.get(*h).is_some_and(|e| e.common.is_construction() != on)).collect();
    if changed.is_empty() {
        return Ok(0);
    }
    let current = d.header.str("CLAYER", "0");
    if !on && d.layer(&current).is_none() {
        return Err(bad(COMMAND, "the current layer does not exist"));
    }
    let mut d = d.clone();
    for h in &changed {
        d.modify_entity(*h, |e| {
            e.common.set_construction(on);
            if !on {
                e.common.layer = current.clone();
            }
        })?;
    }
    *s.doc_mut()? = d;
    Ok(changed.len())
}

/// Hiding construction geometry is only the drawing's `block_construction_hidden` flag, which
/// display, selection, snapping and the extents already read through `Drawing::is_visible`.
/// Object isolation (HIDEOBJECTS) is the user's and stays untouched, so Show all can never
/// reveal an object the user hid. What the view no longer shows leaves the selection; a true
/// no-op changes nothing.
fn refresh(s: &mut Session) -> Result<()> {
    let st = s.state()?;
    if st.block_edit.is_none() {
        return Ok(());
    }
    let selection: Vec<_> = st.selection.iter().copied().filter(|h| st.doc.entity(*h).is_some_and(|e| st.doc.is_visible(e))).collect();
    if selection != st.selection {
        s.set_selection(selection);
    }
    Ok(())
}

/// Runs one operation from its params: Convert and Revert edit the selected objects, Show and Hide switch the editor view.
fn run(s: &mut Session, p: &Value) -> Result<Value> {
    // The interactive body can be called after the editor was closed by another command.
    bedit::editing(s).map_err(EngineError::Other)?;
    let mode = str_param(p, "mode").and_then(Mode::parse).ok_or_else(|| bad(COMMAND, "mode must be convert, revert, show or hide"))?;
    let changed = match mode {
        Mode::Convert => Some(convert(s, p, true)?),
        Mode::Revert => Some(convert(s, p, false)?),
        Mode::Show | Mode::Hide => {
            let hide = matches!(mode, Mode::Hide);
            if s.doc()?.block_construction_hidden != hide {
                s.doc_mut()?.block_construction_hidden = hide;
            }
            None
        }
    };
    refresh(s)?;
    let count = construction_count(s)?;
    s.echo(match mode {
        Mode::Convert => format!("{} object(s) marked as construction.", changed.unwrap_or(0)),
        Mode::Revert => format!("{} object(s) returned to ordinary geometry.", changed.unwrap_or(0)),
        Mode::Show => format!("{count} construction object(s) shown."),
        Mode::Hide => format!("{count} construction object(s) hidden."),
    });
    Ok(match changed {
        Some(changed) => json!({ "mode": mode.name(), "changed": changed, "construction": count }),
        None => json!({ "mode": mode.name(), "construction": count }),
    })
}

/// Runs `mode` on the objects the interactive selection picked.
fn selected_run(s: &mut Session, mode: Mode, handles: &[Handle]) -> Result<Step> {
    run(s, &json!({ "mode": mode.name(), "handles": handles.iter().map(|h| h.hex()).collect::<Vec<_>>() }))?;
    Ok(Step::Done)
}

/// The interactive command: the operation keyword first, then the objects to convert or revert.
#[derive(Default)]
struct Construction {
    mode: Option<Mode>,
    selection: Option<SelectPhase>,
}

impl Interactive for Construction {
    /// The command name shown on the prompt.
    fn name(&self) -> &'static str {
        "BCONSTRUCTION"
    }

    /// The operation keywords first, then the selection prompt.
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.mode {
            None => Prompt::new("Choose construction operation", KW).kw(OPTIONS).default("Convert"),
            Some(_) => self.selection.as_ref().map_or_else(|| Prompt::new("Select objects", Accept::SELECT), SelectPhase::prompt),
        }
    }

    /// Takes the operation, then the selection. Show all and Hide all need no objects and finish at once.
    fn input(&mut self, s: &mut Session, input: Input) -> Result<Step> {
        if input == Input::Cancel {
            return Ok(Step::Cancel);
        }
        let Some(mode) = self.mode else {
            let mode = match input {
                Input::Enter => Some(Mode::Convert),
                Input::Text(word) | Input::Keyword(word) => Mode::parse(&word),
                _ => None,
            }
            .ok_or_else(|| EngineError::Other("Choose Convert, Revert, Show all or Hide all.".into()))?;
            if matches!(mode, Mode::Show | Mode::Hide) {
                run(s, &json!({ "mode": mode.name() }))?;
                return Ok(Step::Done);
            }
            let selection = SelectPhase::begin(s);
            if selection.done {
                return selected_run(s, mode, &selection.picked);
            }
            self.mode = Some(mode);
            self.selection = Some(selection);
            return Ok(Step::Continue);
        };
        let selection = self.selection.get_or_insert_with(|| SelectPhase::begin(s));
        match selection.feed(s, &input)? {
            SelOutcome::More => Ok(Step::Continue),
            SelOutcome::Empty => Ok(Step::Done),
            SelOutcome::Done(handles) => selected_run(s, mode, &handles),
        }
    }
}
