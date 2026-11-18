//! LAYOUT / -LAYOUT typed at the command line: one option, then the layout name(s) it needs.
//! Every option runs the `layout.*` command bodies, so validation matches the JSON form and the
//! command is one undo step.

use cadcraft_doc::Space;
use serde_json::{Value, json};

use super::super::find_command;
use super::super::qselect::wildcard;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

const OPTIONS: &[&str] = &["Copy", "Delete", "New", "Template", "Rename", "SAveas", "Set", "?"];

/// Keyword-only: spaces separate inputs.
const KW: Accept = Accept { point: false, number: false, text: false, select: false, enter: true };

#[derive(Clone, Debug, Default)]
enum Stage {
    #[default]
    Option,
    CopyFrom,
    CopyTo(String),
    Delete,
    New,
    RenameFrom,
    RenameTo(String),
    Set,
    SaveasLayout,
    SaveasFile(String),
    List,
}

#[derive(Default)]
pub struct LayoutM {
    stage: Stage,
}

/// The layout the name prompts default to: the current one, else the first tab.
pub(super) fn current_layout(s: &Session) -> Option<String> {
    if let Space::Paper(n) = s.layout_space() {
        return Some(n);
    }
    let d = s.doc().ok()?;
    d.layouts.iter().min_by_key(|l| l.tab_order).map(|l| l.name.clone())
}

/// The name typed at a name prompt (`None` for Enter, which takes the default).
fn typed_name(i: &Input) -> Option<String> {
    match i {
        Input::Text(t) | Input::Keyword(t) => Some(t.trim().to_string()).filter(|t| !t.is_empty()),
        _ => None,
    }
}

/// An engine refusal as a re-prompt message (no `invalid parameters for …` prefix).
fn retry(e: EngineError) -> EngineError {
    match e {
        EngineError::BadParams { msg, .. } => EngineError::Other(msg),
        other => other,
    }
}

fn need(name: Option<String>) -> Result<String> {
    name.ok_or_else(|| EngineError::Other("Requires a layout name.".into()))
}

impl LayoutM {
    fn option(&mut self, s: &mut Session, k: &str) -> Result<Step> {
        self.stage = match k {
            "Copy" => Stage::CopyFrom,
            "Delete" => Stage::Delete,
            "New" => Stage::New,
            "Rename" => Stage::RenameFrom,
            "Set" => Stage::Set,
            "SAveas" => Stage::SaveasLayout,
            "?" => Stage::List,
            other => {
                // Template: report and end, leaving the next input to the command line.
                s.echo(format!("LAYOUT {other} is not available yet."));
                return Ok(Step::Done);
            }
        };
        Ok(Step::Continue)
    }

    fn list(s: &mut Session, pat: &str) -> Result<()> {
        let d = s.doc()?;
        let mut ls: Vec<_> = d.layouts.iter().collect();
        ls.sort_by_key(|l| l.tab_order);
        let pats: Vec<&str> = pat.split(',').map(str::trim).filter(|p| !p.is_empty()).take(1000).collect();
        let pats = if pats.is_empty() { vec!["*"] } else { pats };
        let names: Vec<String> = ls.iter().filter(|l| pats.iter().any(|p| wildcard(p, &l.name))).map(|l| l.name.clone()).collect();
        let current = s.layout_space().tab_name().to_string();
        s.echo(format!("Active Layout: {current}"));
        if names.is_empty() {
            s.echo(format!("No layout matches \"{pat}\"."));
        }
        for n in names {
            s.echo(format!("Layout: {n}"));
        }
        Ok(())
    }
}

impl Interactive for LayoutM {
    fn name(&self) -> &'static str {
        "LAYOUT"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let cur = current_layout(s);
        let with_cur = |p: Prompt| match &cur {
            Some(c) => p.default(c.clone()),
            None => p,
        };
        match &self.stage {
            Stage::Option => Prompt::new("Enter layout option", KW).kw(OPTIONS).default("set"),
            Stage::CopyFrom => with_cur(Prompt::new("Enter name of layout to copy", Accept::TEXT)),
            Stage::CopyTo(from) => {
                let next = s.doc().ok().and_then(|d| (2..=10_000).map(|i| format!("{from} ({i})")).find(|n| d.layout(n).is_none()));
                let p = Prompt::new("Enter layout name for copy", Accept::TEXT);
                match next {
                    Some(n) => p.default(n),
                    None => p,
                }
            }
            Stage::Delete => with_cur(Prompt::new("Enter name of layout to delete", Accept::TEXT)),
            Stage::New => {
                let next = s.doc().map(super::next_layout_name).unwrap_or_else(|_| "Layout1".into());
                Prompt::new("Enter name of new layout", Accept::TEXT).default(next)
            }
            Stage::RenameFrom => with_cur(Prompt::new("Enter layout to rename", Accept::TEXT)),
            Stage::RenameTo(_) => Prompt::new("Enter new layout name", Accept::TEXT),
            Stage::Set => {
                let tab = s.layout_space().tab_name().to_string();
                Prompt::new("Enter layout to make current", Accept::TEXT).default(tab)
            }
            Stage::SaveasLayout => with_cur(Prompt::new("Enter layout to save to template", Accept::TEXT)),
            Stage::SaveasFile(layout) => Prompt::new("Enter template file name", Accept::TEXT).default(super::saveas::default_path(s, layout)),
            Stage::List => Prompt::new("Enter layout name(s) to list", Accept::TEXT).default("*"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if i == Input::Cancel {
            return Ok(Step::Cancel);
        }
        let name = typed_name(&i);
        match self.stage.clone() {
            Stage::Option => match i {
                Input::Enter => {
                    self.stage = Stage::Set;
                    Ok(Step::Continue)
                }
                Input::Keyword(k) => self.option(s, &k),
                _ => Err(EngineError::Other("Invalid option keyword.".into())),
            },
            Stage::CopyFrom => {
                let from = need(name.or_else(|| current_layout(s)))?;
                let from = super::layout_name("layout.copy", s.doc()?, &from).map_err(retry)?;
                self.stage = Stage::CopyTo(from);
                Ok(Step::Continue)
            }
            Stage::CopyTo(from) => {
                let mut p = json!({ "from": from });
                if let (Some(n), Some(o)) = (name, p.as_object_mut()) {
                    o.insert("to".into(), json!(n));
                }
                let r = super::run_copy(s, &p).map_err(retry)?;
                let to = r.get("name").and_then(Value::as_str).unwrap_or_default();
                s.echo(format!("Layout \"{from}\" copied to \"{to}\"."));
                Ok(Step::Done)
            }
            Stage::Delete => {
                let n = need(name.or_else(|| current_layout(s)))?;
                let r = super::run_delete(s, &json!({ "name": n })).map_err(retry)?;
                let gone = r.get("deleted").and_then(Value::as_str).unwrap_or_default();
                s.echo(format!("Layout \"{gone}\" deleted."));
                Ok(Step::Done)
            }
            Stage::New => {
                let p = match name {
                    Some(n) => json!({ "name": n }),
                    None => json!({}),
                };
                super::run_new(s, &p).map_err(retry)?;
                Ok(Step::Done)
            }
            Stage::RenameFrom => {
                let from = need(name.or_else(|| current_layout(s)))?;
                let from = super::layout_name("layout.rename", s.doc()?, &from).map_err(retry)?;
                self.stage = Stage::RenameTo(from);
                Ok(Step::Continue)
            }
            Stage::RenameTo(from) => {
                let to = need(name)?;
                super::run_rename(s, &json!({ "from": from, "to": to })).map_err(retry)?;
                Ok(Step::Done)
            }
            Stage::Set => {
                let n = match name {
                    Some(n) => n,
                    None => s.layout_space().tab_name().to_string(),
                };
                let set = find_command("layout.set").ok_or_else(|| EngineError::UnknownCommand("layout.set".into()))?;
                (set.run)(s, &json!({ "name": n })).map_err(retry)?;
                Ok(Step::Done)
            }
            Stage::SaveasLayout => {
                let n = need(name.or_else(|| current_layout(s)))?;
                let n = super::layout_name("layout.saveas", s.doc()?, &n).map_err(retry)?;
                self.stage = Stage::SaveasFile(n);
                Ok(Step::Continue)
            }
            Stage::SaveasFile(layout) => {
                let path = name.unwrap_or_else(|| super::saveas::default_path(s, &layout));
                let r = super::saveas::run_saveas(s, &json!({ "name": layout, "path": path })).map_err(retry)?;
                let path = r.get("path").and_then(Value::as_str).unwrap_or_default();
                s.echo(format!("Layout \"{layout}\" saved to template \"{path}\"."));
                Ok(Step::Done)
            }
            Stage::List => {
                Self::list(s, name.as_deref().unwrap_or("*"))?;
                Ok(Step::Done)
            }
        }
    }
}
