//! -GROUP: the command-line group editor, "Enter a group option
//! [?/Order/Add/Remove/Explode/REName/Selectable/Create] <Create>:". The JSON form takes the option
//! as `option` and never prompts.

use cadcraft_doc::{Drawing, Handle};
use serde_json::{Value, json};

use super::super::machines::{SelOutcome, SelectPhase, number};
use super::super::qselect::wildcard;
use super::super::{CommandSpec, bad, bool_or, str_param, targets};

use super::{add_members, create, echo_list, find, remove_members, rename, ungroup, valid_name};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("-group", "Group Options", run)
            .alias(&["-g"])
            .params(
                "{option?: \"create\" (default) | \"?\" | \"order\" | \"add\" | \"remove\" | \"explode\" | \"rename\" | \"selectable\", name?, ...}: \
                 create {name?, description?, handles?, selectable?}; ? {name?: \"A*,B\" (default *)} → {groups}; \
                 order {name, from, to, count?: 1} (0-based positions); add/remove {name, handles? (default the selection)}; \
                 explode {name}; rename {name, newName}; selectable {name, selectable?: bool (default: toggle)}",
            )
            .interactive(|_| Ok(Box::new(DashGroupM::default()))),
    ]
}

/// Groups whose names match `pattern` (comma-separated wildcards; empty is `*`).
fn matching<'a>(d: &'a Drawing, pattern: &str) -> impl Iterator<Item = &'a cadcraft_doc::Group> + 'a {
    let pats: Vec<String> = pattern.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
    d.groups.iter().filter(move |g| pats.is_empty() || pats.iter().any(|p| wildcard(p, &g.name)))
}

/// The group named by `name` (required), as an index.
fn named(s: &Session, p: &Value) -> Result<usize> {
    let name = str_param(p, "name").ok_or_else(|| bad("-group", "`name` is required"))?;
    find(s.doc()?, name).ok_or_else(|| bad("-group", format!("no group named \"{name}\"")))
}

/// Move `count` members starting at `from` so that they start at `to` (0-based positions).
fn reorder(s: &mut Session, i: usize, from: usize, to: usize, count: usize) -> Result<()> {
    let n = s.doc()?.groups.get(i).map_or(0, |g| g.members.len());
    let end = from
        .checked_add(count)
        .filter(|e| *e <= n && count > 0)
        .ok_or_else(|| bad("-group", format!("positions must be within 0-{}", n.saturating_sub(1))))?;
    if to >= n {
        return Err(bad("-group", format!("positions must be within 0-{}", n.saturating_sub(1))));
    }
    if let Some(g) = s.doc_mut()?.groups.get_mut(i) {
        let moved: Vec<Handle> = g.members.drain(from..end).collect();
        let at = to.min(g.members.len());
        g.members.splice(at..at, moved);
    }
    Ok(())
}

fn usize_param(p: &Value, key: &str) -> Result<Option<usize>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            v.as_u64().and_then(|u| usize::try_from(u).ok()).map(Some).ok_or_else(|| bad("-group", format!("`{key}` must be a whole number from 0")))
        }
    }
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let option = str_param(p, "option").unwrap_or("create").trim().to_ascii_lowercase();
    match option.as_str() {
        "create" => {
            let hs = targets(s, p)?;
            let name = create(s, str_param(p, "name"), str_param(p, "description").unwrap_or_default(), &hs, bool_or(p, "selectable", true))?;
            Ok(json!({ "name": name }))
        }
        "?" | "list" => {
            let d = s.doc()?;
            let groups: Vec<Value> = matching(d, str_param(p, "name").unwrap_or("*"))
                .map(|g| json!({ "name": g.name, "description": g.description, "selectable": g.selectable, "members": g.members.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
                .collect();
            Ok(json!({ "groups": groups }))
        }
        "order" => {
            let i = named(s, p)?;
            let from = usize_param(p, "from")?.ok_or_else(|| bad("-group", "`from` is required"))?;
            let to = usize_param(p, "to")?.ok_or_else(|| bad("-group", "`to` is required"))?;
            reorder(s, i, from, to, usize_param(p, "count")?.unwrap_or(1))?;
            let g = s.doc()?.groups.get(i).ok_or_else(|| bad("-group", "no such group"))?;
            Ok(json!({ "name": g.name, "members": g.members.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
        }
        "add" | "remove" => {
            let i = named(s, p)?;
            let hs = targets(s, p)?;
            if option == "add" {
                let n = add_members(s, i, &hs)?;
                Ok(json!({ "added": n }))
            } else {
                let (n, deleted) = remove_members(s, i, &hs)?;
                Ok(json!({ "removed": n, "deleted": deleted }))
            }
        }
        "explode" => {
            let i = named(s, p)?;
            Ok(json!({ "ungrouped": ungroup(s, &[i])? }))
        }
        "rename" => {
            let i = named(s, p)?;
            let new = str_param(p, "newName").ok_or_else(|| bad("-group", "`newName` is required"))?;
            rename(s, i, new)?;
            Ok(json!({ "name": new.trim() }))
        }
        "selectable" => {
            let i = named(s, p)?;
            let current = s.doc()?.groups.get(i).is_some_and(|g| g.selectable);
            let on = bool_or(p, "selectable", !current);
            if let Some(g) = s.doc_mut()?.groups.get_mut(i) {
                g.selectable = on;
            }
            Ok(json!({ "selectable": on }))
        }
        other => Err(bad("-group", format!("unknown option `{other}`"))),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Op {
    #[default]
    Create,
    Order,
    Add,
    Remove,
    Explode,
    Rename,
    Selectable,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Stage {
    #[default]
    Option,
    List,
    Name,
    Description,
    Select,
    NewName,
    Selectable,
    From,
    To,
    Count,
}

#[derive(Default)]
struct DashGroupM {
    stage: Stage,
    op: Op,
    /// Create: the new name (None: unnamed). Other options: the group's name.
    name: Option<String>,
    description: String,
    from: usize,
    to: usize,
    sel: SelectPhase,
}

impl DashGroupM {
    fn index(&self, s: &Session) -> Result<usize> {
        let n = self.name.clone().unwrap_or_default();
        find(s.doc()?, &n).ok_or_else(|| bad("-group", format!("no group named \"{n}\"")))
    }
    fn members(&self, s: &Session) -> usize {
        self.index(s).ok().and_then(|i| s.doc().ok()?.groups.get(i).map(|g| g.members.len())).unwrap_or(0)
    }
    /// The group was named: go on with the option.
    fn named(&mut self, s: &mut Session) -> Result<Step> {
        self.stage = match self.op {
            Op::Create => Stage::Description,
            Op::Add | Op::Remove => Stage::Select,
            Op::Rename => Stage::NewName,
            Op::Selectable => Stage::Selectable,
            Op::Order => Stage::From,
            Op::Explode => {
                let i = self.index(s)?;
                for n in ungroup(s, &[i])? {
                    s.echo(format!("Group {n} ungrouped."));
                }
                return Ok(Step::Done);
            }
        };
        Ok(Step::Continue)
    }
    fn selected(&mut self, s: &mut Session, hs: &[Handle]) -> Result<Step> {
        s.set_selection(Vec::new());
        let r = match self.op {
            Op::Create => {
                create(s, self.name.as_deref(), &self.description, hs, true).map(|n| format!("Group {n} created with {} object(s).", hs.len()))
            }
            Op::Add => self.index(s).and_then(|i| add_members(s, i, hs)).map(|n| format!("{n} object(s) added.")),
            _ => self
                .index(s)
                .and_then(|i| remove_members(s, i, hs))
                .map(|(n, gone)| if gone { "The group has no objects left and was deleted.".to_string() } else { format!("{n} object(s) removed.") }),
        };
        s.echo(r.unwrap_or_else(|e| e.to_string()));
        Ok(Step::Done)
    }
    /// A typed position (0-based) within the group, or None after reporting a bad one.
    fn position(&self, s: &mut Session, t: &str) -> Option<usize> {
        let n = self.members(s);
        match number(t).filter(|v| v.fract() == 0.0 && *v >= 0.0 && *v < n as f64) {
            Some(v) => Some(v as usize),
            None => {
                s.echo(format!("Requires a whole number between 0 and {}.", n.saturating_sub(1)));
                None
            }
        }
    }
}

impl Interactive for DashGroupM {
    fn name(&self) -> &'static str {
        "-GROUP"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let last = self.members(s).saturating_sub(1);
        let g = self.name.clone().unwrap_or_default();
        match self.stage {
            Stage::Option => Prompt::new("Enter a group option", super::super::curves::KW)
                .kw(&["?", "Order", "Add", "Remove", "Explode", "REName", "Selectable", "Create"])
                .default("Create"),
            Stage::List => Prompt::new("Enter group name(s) to list", Accept::TEXT).default("*"),
            Stage::Name if self.op == Op::Create => Prompt::new("Enter a group name", Accept::TEXT).kw(&["?"]),
            Stage::Name if self.op == Op::Rename => Prompt::new("Enter old group name", Accept::TEXT).kw(&["?"]),
            Stage::Name => Prompt::new("Enter a group name", Accept::TEXT).kw(&["?"]),
            Stage::Description => Prompt::new("Enter a group description", Accept::TEXT),
            Stage::Select if self.sel.removing => self.sel.prompt(),
            Stage::Select => Prompt::new(
                match self.op {
                    Op::Add => "Select objects to add to the group",
                    Op::Remove => "Select objects to remove from the group",
                    _ => "Select objects",
                },
                Accept::SELECT,
            ),
            Stage::NewName => Prompt::new("Enter new group name", Accept::TEXT).kw(&["?"]),
            Stage::Selectable => {
                let on = s.doc().ok().and_then(|d| d.groups.iter().find(|x| x.name.eq_ignore_ascii_case(&g))).is_some_and(|x| x.selectable);
                let msg = if on {
                    format!("Group {g} is selectable. Make it unselectable?")
                } else {
                    format!("Group {g} is not selectable. Make it selectable?")
                };
                Prompt::new(msg, super::super::curves::KW).kw(&["Yes", "No"]).default("Y")
            }
            Stage::From => Prompt::new(format!("Enter the position of the first object to move (0-{last})"), Accept::NUMBER),
            Stage::To => Prompt::new(format!("Enter the position to move it to (0-{last})"), Accept::NUMBER),
            Stage::Count => Prompt::new("Number of objects to move", Accept::NUMBER).default("1"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let text = match &i {
            Input::Text(t) => Some(t.trim().to_string()),
            _ => None,
        };
        let keyword = match &i {
            Input::Keyword(k) => Some(k.to_ascii_lowercase()),
            _ => None,
        };
        match self.stage {
            Stage::Option => {
                let k = if i == Input::Enter { Some("create".to_string()) } else { keyword };
                let (op, stage) = match k.as_deref() {
                    Some("?") => (Op::Create, Stage::List),
                    Some("order") => (Op::Order, Stage::Name),
                    Some("add") => (Op::Add, Stage::Name),
                    Some("remove") => (Op::Remove, Stage::Name),
                    Some("explode") => (Op::Explode, Stage::Name),
                    Some("rename") => (Op::Rename, Stage::Name),
                    Some("selectable") => (Op::Selectable, Stage::Name),
                    Some("create") => (Op::Create, Stage::Name),
                    _ => {
                        s.echo("Invalid option keyword.");
                        return Ok(Step::Continue);
                    }
                };
                self.op = op;
                self.stage = stage;
            }
            Stage::List => {
                let pattern = text.unwrap_or_default();
                let d = s.doc()?;
                let lines: Vec<String> = matching(d, &pattern)
                    .map(|g| {
                        let sel = if g.selectable { "selectable" } else { "not selectable" };
                        format!(
                            "{}: {}{} object(s), {sel}",
                            g.name,
                            if g.description.is_empty() { String::new() } else { format!("{}, ", g.description) },
                            g.members.len()
                        )
                    })
                    .collect();
                if lines.is_empty() {
                    s.echo("No matching groups found.");
                }
                for l in lines {
                    s.echo(l);
                }
                return Ok(Step::Done);
            }
            Stage::Name => {
                if keyword.as_deref() == Some("?") {
                    echo_list(s)?;
                    return Ok(Step::Continue);
                }
                if self.op == Op::Create {
                    // `*` or Enter makes an unnamed group.
                    match text.as_deref() {
                        None | Some("*") if matches!(i, Input::Enter | Input::Text(_)) => self.name = None,
                        Some(n) if !valid_name(n) => {
                            s.echo(format!("\"{n}\" is not a valid group name."));
                            return Ok(Step::Continue);
                        }
                        Some(n) if find(s.doc()?, n).is_some() => {
                            s.echo(format!("A group named \"{n}\" already exists."));
                            return Ok(Step::Continue);
                        }
                        Some(n) => self.name = Some(n.to_string()),
                        None => return Ok(Step::Continue),
                    }
                    return self.named(s);
                }
                match text {
                    Some(t) => match find(s.doc()?, &t) {
                        Some(g) => {
                            self.name = s.doc()?.groups.get(g).map(|x| x.name.clone());
                            return self.named(s);
                        }
                        None => s.echo(format!("No group named \"{t}\".")),
                    },
                    None if i == Input::Enter => return Ok(Step::Done),
                    None => {}
                }
            }
            Stage::Description => {
                self.description = text.unwrap_or_default();
                self.stage = Stage::Select;
                self.sel = SelectPhase::begin(s);
                if self.sel.done {
                    let hs = self.sel.picked.clone();
                    return self.selected(s, &hs);
                }
            }
            Stage::Select => match self.sel.feed(s, &i)? {
                SelOutcome::More => {}
                SelOutcome::Empty => return Ok(Step::Done),
                SelOutcome::Done(hs) => return self.selected(s, &hs),
            },
            Stage::NewName => {
                if keyword.as_deref() == Some("?") {
                    echo_list(s)?;
                    return Ok(Step::Continue);
                }
                let Some(new) = text else { return Ok(if i == Input::Enter { Step::Done } else { Step::Continue }) };
                let g = self.index(s)?;
                match rename(s, g, &new) {
                    Ok(()) => {
                        s.echo(format!("Group {} renamed to {new}.", self.name.clone().unwrap_or_default()));
                        return Ok(Step::Done);
                    }
                    Err(e) => s.echo(e.to_string()),
                }
            }
            Stage::Selectable => {
                let yes = match (&i, keyword.as_deref()) {
                    (Input::Enter, _) | (_, Some("yes")) => true,
                    (_, Some("no")) => false,
                    _ => return Ok(Step::Continue),
                };
                if yes {
                    let g = self.index(s)?;
                    if let Some(x) = s.doc_mut()?.groups.get_mut(g) {
                        x.selectable = !x.selectable;
                    }
                }
                return Ok(Step::Done);
            }
            Stage::From | Stage::To => {
                let Some(t) = text else { return Ok(if i == Input::Enter { Step::Done } else { Step::Continue }) };
                if let Some(v) = self.position(s, &t) {
                    if self.stage == Stage::From {
                        self.from = v;
                        self.stage = Stage::To;
                    } else {
                        self.to = v;
                        self.stage = Stage::Count;
                    }
                }
            }
            Stage::Count => {
                let count = match text {
                    None if i == Input::Enter => 1,
                    None => return Ok(Step::Continue),
                    Some(t) => match number(&t).filter(|v| v.fract() == 0.0 && *v >= 1.0 && *v <= self.members(s).saturating_sub(self.from) as f64) {
                        Some(v) => v as usize,
                        None => {
                            s.echo(format!("Requires a whole number between 1 and {}.", self.members(s).saturating_sub(self.from)));
                            return Ok(Step::Continue);
                        }
                    },
                };
                let g = self.index(s)?;
                match reorder(s, g, self.from, self.to, count) {
                    Ok(()) => s.echo("Group reordered."),
                    Err(e) => s.echo(e.to_string()),
                }
                return Ok(Step::Done);
            }
        }
        Ok(Step::Continue)
    }
}
