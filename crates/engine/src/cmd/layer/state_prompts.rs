//! -LAYER stAte: save, restore, edit, rename, delete and list the drawing's layer states, with
//! AutoCAD 2024's prompts and messages (measured). Save, Restore, Name and Delete run the
//! `layerstate.*` commands; Edit changes the state's mask in place.

use cadcraft_doc::LayerState as LS;
use serde_json::{Value, json};

use super::prompts::{KW, NAME, call, message, retry};
use super::{state_index, valid_name};
use crate::{Input, Prompt, Result, Session};

/// The options after stAte. Import and EXport (layer state files) are not available yet.
const OPTIONS: &[&str] = &["?", "Save", "Restore", "Edit", "Name", "Delete", "Import", "EXport"];

/// "Enter states to change": each keyword, the name AutoCAD 2024 lists it by and its mask bit, in
/// AutoCAD's order.
const TOGGLES: [(&str, &str, u32); 10] = [
    ("On", "On", LS::ON),
    ("Frozen", "Frozen", LS::FROZEN),
    ("Lock", "Lock", LS::LOCKED),
    ("Plot", "Plot", LS::PLOT),
    ("Newvpfreeze", "Newvpfreeze", LS::VP_FREEZE_NEW),
    ("Color", "Color", LS::COLOR),
    ("lineType", "lineType", LS::LINETYPE),
    ("lineWeight", "lineWeight", LS::LINEWEIGHT),
    ("TRansparency", "Transparency", LS::TRANSPARENCY),
    ("plotStyle", "plotStyle", LS::PLOT_STYLE),
];

/// The options that ask for a saved state's name.
#[derive(Clone, Copy, Debug)]
pub(super) enum Pick {
    Restore,
    Edit,
    Name,
    Delete,
}

impl Pick {
    fn verb(self) -> &'static str {
        match self {
            Pick::Restore => "restore",
            Pick::Edit => "edit",
            Pick::Name => "rename",
            Pick::Delete => "delete",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(super) enum Stage {
    #[default]
    Option,
    /// Save: the new state's name.
    SaveName,
    /// Restore, Edit, Name or Delete: the saved state's name.
    Pick(Pick),
    /// Name: the new name of the state called so.
    RenameTo(String),
    /// "Enter states to change" for the state `name`: one Save makes on Enter, or one Edit
    /// changes (`edit`).
    Toggles { name: String, mask: u32, edit: bool },
}

impl Stage {
    pub(super) fn prompt(&self) -> Prompt {
        match self {
            Stage::Option => Prompt::new("Enter an option", KW).kw(OPTIONS),
            Stage::SaveName | Stage::RenameTo(_) => Prompt::new("Enter new layer state name", NAME),
            Stage::Pick(p) => Prompt::new(format!("Enter name of layer state to {}", p.verb()), NAME).kw(&["?"]),
            Stage::Toggles { .. } => Prompt::new("Enter states to change", KW).kw(&TOGGLES.map(|t| t.0)),
        }
    }

    /// One input. `Ok(false)`: back to -LAYER's options. An invalid input leaves the stage as it
    /// is, so the prompt is asked again.
    pub(super) fn input(&mut self, s: &mut Session, i: &Input) -> Result<bool> {
        let text = match i {
            Input::Text(t) | Input::Keyword(t) => Some(t.trim().to_string()).filter(|t| !t.is_empty()),
            _ => None,
        };
        let next = match std::mem::take(self) {
            Stage::Option => match i {
                Input::Enter => return Ok(false),
                Input::Keyword(k) => option(s, k),
                _ => return Err(retry("Invalid option keyword.")),
            },
            Stage::SaveName => match text {
                None => Stage::Option,
                Some(n) => save_name(s, n),
            },
            Stage::Pick(p) => match (i, text) {
                (Input::Keyword(k), _) if k == "?" => {
                    list(s);
                    Stage::Pick(p)
                }
                (_, None) => Stage::Option,
                (_, Some(n)) => pick(s, p, &n),
            },
            Stage::RenameTo(from) => {
                if let Some(to) = text {
                    rename(s, &from, &to);
                }
                Stage::Option
            }
            Stage::Toggles { name, mut mask, edit } => match i {
                Input::Keyword(k) => {
                    if let Some(&(_, _, bit)) = TOGGLES.iter().find(|t| t.0 == k) {
                        mask ^= bit;
                    }
                    show(s, mask);
                    Stage::Toggles { name, mask, edit }
                }
                Input::Enter => {
                    finish(s, &name, mask, edit);
                    Stage::Option
                }
                _ => {
                    *self = Stage::Toggles { name, mask, edit };
                    return Err(retry("Invalid option keyword."));
                }
            },
        };
        *self = next;
        Ok(true)
    }
}

fn option(s: &mut Session, k: &str) -> Stage {
    match k {
        "?" => {
            list(s);
            Stage::Option
        }
        "Save" => Stage::SaveName,
        "Restore" => Stage::Pick(Pick::Restore),
        "Edit" => Stage::Pick(Pick::Edit),
        "Name" => Stage::Pick(Pick::Name),
        "Delete" => Stage::Pick(Pick::Delete),
        other => {
            s.echo(format!("-LAYER stAte {other} is not available yet."));
            Stage::Option
        }
    }
}

/// Whether a saved state has the name `n` in any case.
fn taken(s: &Session, n: &str) -> bool {
    s.doc().is_ok_and(|d| d.layer_states.iter().any(|st| st.name.eq_ignore_ascii_case(n)))
}

/// Save's name: refused (asked again) when it has a character AutoCAD 2024 refuses (the layer
/// name ones, measured one by one), is longer than 255 characters (measured 256), or is a saved
/// state's name in any case (measured `s1` after `S1`).
fn save_name(s: &mut Session, n: String) -> Stage {
    if !valid_name(&n) {
        s.echo(format!("\"{n}\" is an invalid layer state name."));
        return Stage::SaveName;
    }
    if taken(s, &n) {
        s.echo(format!("Layer state \"{n}\" already exists."));
        return Stage::SaveName;
    }
    let mask = s.settings.layer_state_mask;
    show(s, mask);
    Stage::Toggles { name: n, mask, edit: false }
}

fn pick(s: &mut Session, p: Pick, n: &str) -> Stage {
    let found = s.doc().ok().and_then(|d| state_index(d, n).and_then(|i| d.layer_states.get(i)).map(|st| (st.name.clone(), st.mask)));
    let Some((name, mask)) = found else {
        s.echo("Unknown layer state name.");
        return Stage::Pick(p);
    };
    match p {
        Pick::Restore => {
            run(s, "layerstate.restore", &json!({ "name": name }));
            Stage::Option
        }
        Pick::Delete => {
            run(s, "layerstate.delete", &json!({ "name": name }));
            Stage::Option
        }
        Pick::Edit => {
            show(s, mask);
            Stage::Toggles { name, mask, edit: true }
        }
        Pick::Name => Stage::RenameTo(name),
    }
}

/// Name's new name. A name some state has in any case, the state's own too, is refused with
/// AutoCAD 2024's two lines (measured `S2` onto `S2`, `S1` onto `s1`). A name with a character
/// AutoCAD refuses leaves the state as it is with no message, as AutoCAD does (measured `a,b`,
/// `a<b`); AutoCAD renames to a name starting with "*" as an anonymous `*A1`, and CADCraft leaves
/// that state as it is too.
fn rename(s: &mut Session, from: &str, to: &str) {
    if taken(s, to) {
        s.echo("Layer state could not be renamed.");
        s.echo(format!("{to} already exists."));
    } else if valid_name(to) {
        run(s, "layerstate.rename", &json!({ "from": from, "to": to }));
    }
}

/// Enter at "Enter states to change": Save makes the state with this mask and keeps the toggles
/// for the next Save; Edit sets the state's mask and keeps the bits no toggle reaches (AutoCAD's
/// 65536 after a restore). Edit leaves the next Save's toggles alone (measured).
fn finish(s: &mut Session, name: &str, mask: u32, edit: bool) {
    if !edit {
        if run(s, "layerstate.save", &json!({ "name": name, "mask": mask & LS::ALL })) {
            s.settings.layer_state_mask = mask & LS::ALL;
        }
        return;
    }
    // Only a changed mask touches the drawing.
    let Ok(d) = s.doc() else { return };
    let Some(i) = state_index(d, name).filter(|&i| d.layer_states.get(i).is_some_and(|st| st.mask != mask)) else { return };
    if let Ok(d) = s.doc_mut()
        && let Some(st) = d.layer_states.get_mut(i)
    {
        st.mask = mask;
    }
}

/// Run a `layerstate.*` command, reporting a refusal; `true` when it ran.
fn run(s: &mut Session, id: &str, p: &Value) -> bool {
    match call(s, id, p) {
        Ok(_) => true,
        Err(e) => {
            s.echo(message(&e));
            false
        }
    }
}

/// The toggles as AutoCAD 2024 lists them before "Enter states to change".
fn show(s: &mut Session, mask: u32) {
    s.echo("States");
    for (_, label, bit) in TOGGLES {
        s.echo(format!("{label:<20}{}", if mask & bit != 0 { "Yes" } else { "No" }));
    }
}

/// `?`: every saved state by name without regard to case, as AutoCAD 2024 lists them (measured:
/// `S2`, `b`, `S1` saved in that order list as `b`, `S1`, `S2`; a name starting with "*", its
/// `*A1`, is listed too).
fn list(s: &mut Session) {
    let mut names: Vec<String> = s.doc().map(|d| d.layer_states.iter().map(|st| st.name.clone()).collect()).unwrap_or_default();
    names.sort_by_cached_key(|n| n.to_ascii_lowercase());
    s.echo("Layer state");
    s.echo("-".repeat(29));
    for n in names {
        s.echo(n);
    }
}
