//! PICKSTYLE: with group selection on (bit 1, the default), picking or windowing an object of a
//! selectable group selects the whole group. Ctrl+Shift+A (the `pickstyle` command without a
//! value) turns group selection on and off.

use std::collections::{HashMap, HashSet};

use cadcraft_doc::Handle;
use serde_json::{Value, json};

use super::super::{CommandSpec, bad};

use super::super::always;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("pickstyle", "Group Selection", run)
            .key("Cmd+Shift+A")
            .params("{value?: 0 none | 1 group selection | 2 associative hatch | 3 both} (no value: group selection on/off) → {value, message}")
            .enabled(always)
            .noundo()
            .transparent()
            .interactive(|s| Ok(Box::new(PickstyleM { current: s.settings.pickstyle }))),
    ]
}

/// The commands whose object prompts pick single members, not whole groups.
const MEMBER_PICKS: &[&str] = &["groupedit", "-group"];

/// `hs` plus the other members of the selectable groups they belong to (in the space being
/// edited, visible), when PICKSTYLE has group selection on. Order: `hs` first.
pub(crate) fn expand(s: &Session, hs: Vec<Handle>) -> Vec<Handle> {
    if s.settings.pickstyle & 1 == 0 || hs.is_empty() || s.running.as_ref().is_some_and(|r| MEMBER_PICKS.contains(&r.id.as_str())) {
        return hs;
    }
    let Ok(d) = s.doc() else { return hs };
    if !d.groups.iter().any(|g| g.selectable) {
        return hs;
    }
    let space = s.space();
    let Some(store) = d.space(&space) else { return hs };
    // Member → its selectable groups, built once (window selections may hold many objects).
    let mut of: HashMap<Handle, Vec<usize>> = HashMap::new();
    for (i, g) in d.groups.iter().enumerate().filter(|(_, g)| g.selectable) {
        for m in &g.members {
            of.entry(*m).or_default().push(i);
        }
    }
    let mut seen: HashSet<Handle> = hs.iter().copied().collect();
    let mut done: HashSet<usize> = HashSet::new();
    let mut out = hs.clone();
    for h in &hs {
        for gi in of.get(h).into_iter().flatten() {
            if !done.insert(*gi) {
                continue;
            }
            let Some(g) = d.groups.get(*gi) else { continue };
            for m in &g.members {
                if store.get(*m).is_some_and(|e| d.is_visible(e)) && seen.insert(*m) {
                    out.push(*m);
                }
            }
        }
    }
    out
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let v = match p.get("value") {
        None | Some(Value::Null) => s.settings.pickstyle ^ 1,
        Some(v) => v
            .as_u64()
            .or_else(|| v.as_str().and_then(|t| t.trim().parse().ok()))
            .filter(|v| *v <= 3)
            .and_then(|v| u8::try_from(v).ok())
            .ok_or_else(|| bad("pickstyle", "`value` must be 0, 1, 2 or 3"))?,
    };
    s.settings.pickstyle = v;
    let msg = format!("<Group selection {}>", if v & 1 != 0 { "on" } else { "off" });
    Ok(json!({ "value": v, "message": msg }))
}

/// Typed PICKSTYLE: "Enter new value for PICKSTYLE <1>:".
struct PickstyleM {
    current: u8,
}

impl Interactive for PickstyleM {
    fn name(&self) -> &'static str {
        "PICKSTYLE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Enter new value for PICKSTYLE", Accept::NUMBER).default(self.current.to_string())
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Enter => Ok(Step::Done),
            Input::Text(t) => match run(s, &json!({ "value": t })) {
                Ok(_) => Ok(Step::Done),
                Err(_) => {
                    s.echo("Requires an integer between 0 and 3.");
                    Ok(Step::Continue)
                }
            },
            _ => Ok(Step::Continue),
        }
    }
}
