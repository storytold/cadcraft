//! Dimension style commands of the dimension shortcut menu: Remove Style Overrides (DIMOVERRIDE
//! Clear for the selected dimensions) and Save as New Style (a style made of a dimension's
//! settings, its overrides included, which the dimension then uses).

use cadcraft_doc::{DimStyle, EntityKind, Handle};
use serde_json::{Value, json};

use super::machines::SelectRun;
use super::*;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

fn other(m: impl Into<String>) -> EngineError {
    EngineError::Other(m.into())
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("dimoverride.clear", "Remove Style Overrides", run_clear)
            .params("{handles?} (dimensions; they draw with their style's settings again) → {changed}")
            .interactive(|_| Ok(Box::new(SelectRun::new("dimoverride.clear", "DIMOVERRIDE")))),
        CommandSpec::new("dimstyle.saveas", "Save as New Style...", run_save_as)
            .params(
                "{handle? (default: the first selected dimension), name} → {style, handle} (a new dimension style with the dimension's \
                 settings, overrides included; the dimension takes it and its overrides are cleared; the current style is unchanged)",
            )
            .interactive(|_| Ok(Box::new(SaveAsM::default()))),
    ]
}

fn run_clear(s: &mut Session, p: &Value) -> Result<Value> {
    let hs: Vec<String> = targets(s, p)?.iter().map(|h| h.hex()).collect();
    super::props::dim_override(s, &json!({ "handles": hs, "clear": true }))
}

fn is_dim(s: &Session, h: Handle) -> bool {
    s.doc().ok().and_then(|d| d.entity(h)).is_some_and(|e| matches!(e.kind, EntityKind::Dimension(_)))
}

/// A usable new dimension style name (as -DIMSTYLE takes one).
fn new_name(s: &Session, name: &str) -> Result<String> {
    let name = super::dimstyle_cli::check_name(name).map_err(|_| bad("dimstyle.saveas", "bad style name"))?;
    if s.doc()?.dim_style(&name).is_some() {
        return Err(bad("dimstyle.saveas", format!("a dimension style `{name}` already exists")));
    }
    Ok(name)
}

/// Make style `name` from dimension `h`'s settings and give it to the dimension.
fn save_as(s: &mut Session, h: Handle, name: &str) -> Result<Value> {
    let name = new_name(s, name)?;
    let d = s.doc_mut()?;
    let Some(EntityKind::Dimension(dm)) = d.entity(h).map(|e| e.kind.clone()) else { return Err(bad("dimstyle.saveas", "not a dimension")) };
    let st = d.dim_style(&dm.style).cloned().unwrap_or_default().with_overrides(&dm.overrides);
    d.dim_styles.push(DimStyle { name: name.clone(), ..st });
    d.modify_entity(h, |e| {
        if let EntityKind::Dimension(dm) = &mut e.kind {
            dm.style.clone_from(&name);
            dm.overrides.clear();
            dm.block = None;
        }
    })?;
    Ok(json!({ "style": name, "handle": h.hex() }))
}

fn run_save_as(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("dimstyle.saveas", "`name` is required"))?.to_string();
    let hs = targets(s, p)?;
    let h = hs.into_iter().find(|h| is_dim(s, *h)).ok_or_else(|| bad("dimstyle.saveas", "select a dimension"))?;
    save_as(s, h, &name)
}

/// Save as New Style: the selected dimension (or one picked), then the new style's name.
#[derive(Default)]
struct SaveAsM {
    dim: Option<Handle>,
}

impl Interactive for SaveAsM {
    fn name(&self) -> &'static str {
        "DIMSTYLE"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.dim = s.selection().into_iter().find(|h| is_dim(s, *h));
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.dim {
            None => Prompt::new("Select a dimension", Accept::POINT),
            Some(_) => Prompt::new("Enter name of new dimension style", Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.dim, i) {
            (None, Input::Point(p)) => {
                let h = curves::pick_at(s, p).filter(|h| is_dim(s, *h)).ok_or_else(|| other("Select a dimension."))?;
                self.dim = Some(h);
                Ok(Step::Continue)
            }
            (Some(h), Input::Text(t) | Input::Keyword(t)) => {
                let r = save_as(s, h, &t)?;
                s.echo(format!("Dimension style {} created from the dimension.", r["style"].as_str().unwrap_or_default()));
                Ok(Step::Done)
            }
            (_, Input::Enter) => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}
