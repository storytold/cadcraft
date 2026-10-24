//! MENUBAR and TOOLBAR: show or hide the window's menu bar and tool bar from the command line, as
//! in AutoCAD. With the menu bar hidden on Windows and Linux (where it is the only menu), typing
//! these is the way back.
//!
//! The UI owns the bars: it reports what it shows in [`Session::bars_shown`] and applies
//! [`Session::bars_request`], which these commands set.

use serde_json::{Value, json};

use super::*;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// The interface bars MENUBAR and TOOLBAR control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterfaceBars {
    pub menubar: bool,
    pub toolbar: bool,
}

impl Default for InterfaceBars {
    fn default() -> Self {
        InterfaceBars { menubar: true, toolbar: true }
    }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("menubar", "Menu Bar", run_menubar)
            .params("{value?: 0|1} (1 shows the in-window menu bar, 0 hides it; without it, reports it)")
            .enabled(always)
            .noundo()
            .interactive(|s| Ok(Box::new(MenubarM { current: s.bars_shown.menubar }))),
        CommandSpec::new("toolbar", "Tool Bar", run_toolbar)
            .alias(&["-toolbar"])
            .params("{show?: bool} (without it, reports it)")
            .enabled(always)
            .noundo()
            .interactive(|_| Ok(Box::new(ToolbarM))),
    ]
}

fn set(s: &mut Session, f: impl FnOnce(&mut InterfaceBars)) {
    let mut b = s.bars_request.unwrap_or(s.bars_shown);
    f(&mut b);
    s.bars_shown = b;
    s.bars_request = Some(b);
}

fn run_menubar(s: &mut Session, p: &Value) -> Result<Value> {
    let v = match p.get("value") {
        None | Some(Value::Null) => return Ok(json!({ "value": u8::from(s.bars_shown.menubar) })),
        Some(v) => v
            .as_u64()
            .or_else(|| v.as_str().and_then(|t| t.trim().parse().ok()))
            .filter(|v| *v <= 1)
            .ok_or_else(|| bad("menubar", "`value` must be 0 or 1"))?,
    };
    set(s, |b| b.menubar = v == 1);
    Ok(json!({ "value": v }))
}

fn run_toolbar(s: &mut Session, p: &Value) -> Result<Value> {
    let Some(show) = p.get("show").filter(|v| !v.is_null()) else { return Ok(json!({ "show": s.bars_shown.toolbar })) };
    let show = show.as_bool().ok_or_else(|| bad("toolbar", "`show` must be true or false"))?;
    set(s, |b| b.toolbar = show);
    Ok(json!({ "show": show }))
}

/// Typed MENUBAR: "Enter new value for MENUBAR <1>:".
struct MenubarM {
    current: bool,
}

impl Interactive for MenubarM {
    fn name(&self) -> &'static str {
        "MENUBAR"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Enter new value for MENUBAR", Accept::NUMBER).default(u8::from(self.current).to_string())
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Enter => Ok(Step::Done),
            Input::Text(t) => match run_menubar(s, &json!({ "value": t })) {
                Ok(_) => Ok(Step::Done),
                Err(_) => {
                    s.echo("Requires 0 or 1.");
                    Ok(Step::Continue)
                }
            },
            _ => Ok(Step::Continue),
        }
    }
}

/// Typed TOOLBAR: "Enter tool bar option [Show/Hide] <Show>:" (CADCraft has one tool bar).
struct ToolbarM;

impl Interactive for ToolbarM {
    fn name(&self) -> &'static str {
        "TOOLBAR"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Enter tool bar option", Accept::TEXT).kw(&["Show", "Hide"]).default("Show")
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let word = match &i {
            Input::Enter => "show".to_string(),
            Input::Keyword(k) | Input::Text(k) => k.trim().to_ascii_lowercase(),
            _ => return Ok(Step::Continue),
        };
        let show = if !word.is_empty() && "show".starts_with(&word) {
            true
        } else if !word.is_empty() && "hide".starts_with(&word) {
            false
        } else {
            s.echo("Invalid option keyword.");
            return Ok(Step::Continue);
        };
        run_toolbar(s, &json!({ "show": show }))?;
        Ok(Step::Done)
    }
}
