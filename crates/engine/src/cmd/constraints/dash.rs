//! -PARAMETERS: the Parameters Manager at the command line (#2579). Each run takes one option:
//! New, Edit, Rename, Delete, or `?` to list. The JSON form is PARAMETERS' and never prompts.

use std::collections::HashSet;

use cadcraft_constraints as cs;
use cadcraft_doc::Drawing;

use super::super::curves::KW;
use super::super::qselect::wildcard;
use super::{delete_parameter, parameter_line, rename_parameter, set_parameter};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// What happens to the parameter named at [`Stage::Existing`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Edit,
    Rename,
    Delete,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Stage {
    #[default]
    Option,
    /// New: the name of the user parameter to make.
    NewName,
    /// Edit, Rename and Delete: the parameter to work on.
    Existing(Op),
    /// New and Edit: the parameter's expression.
    Expression,
    /// Rename: the parameter's new name.
    Rename,
    /// `?`: which parameters to list.
    List,
}

#[derive(Default)]
pub(super) struct DashParametersM {
    stage: Stage,
    /// The parameter being made, edited or renamed.
    name: String,
    /// New: the name is not in the drawing yet.
    new: bool,
}

/// The first `userN` name not in use, as the Parameters Manager names a new user parameter.
fn free_user_name(d: &Drawing) -> String {
    let used: HashSet<&str> = d.parametric.parameters.iter().map(|p| p.name.as_str()).chain(d.constraints.iter().map(|c| c.name.as_str())).collect();
    // One of the first `used.len() + 1` names is free, so the search is bounded by the drawing.
    (1..=used.len().saturating_add(1)).map(|n| format!("user{n}")).find(|n| !used.contains(n.as_str())).unwrap_or_else(|| "user1".into())
}

/// Typed text, or `None` for Enter (and for anything else).
fn typed(i: &Input) -> Option<&str> {
    match i {
        Input::Text(t) => Some(t.trim()).filter(|t| !t.is_empty()),
        _ => None,
    }
}

impl DashParametersM {
    /// The current expression of the parameter being edited.
    fn current_expr(&self, s: &Session) -> Option<String> {
        cs::parameter_table(s.doc().ok()?).into_iter().find(|r| r.name == self.name).map(|r| r.expr)
    }

    fn list(s: &mut Session, pattern: &str) -> Result<Step> {
        let pats: Vec<&str> = pattern.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
        let lines: Vec<String> = cs::parameter_table(s.doc()?)
            .iter()
            .filter(|r| pats.is_empty() || pats.iter().any(|p| wildcard(p, &r.name)))
            .map(parameter_line)
            .collect();
        if lines.is_empty() {
            s.echo("No matching parameters found.");
        }
        for l in lines {
            s.echo(l);
        }
        Ok(Step::Done)
    }
}

impl Interactive for DashParametersM {
    fn name(&self) -> &'static str {
        "-PARAMETERS"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match self.stage {
            Stage::Option => Prompt::new("Enter a parameter option", KW).kw(&["New", "Edit", "Rename", "Delete", "?"]),
            Stage::NewName => {
                let free = s.doc().map(free_user_name).unwrap_or_else(|_| "user1".into());
                Prompt::new("Enter a name for the new parameter", Accept::TEXT).default(free)
            }
            Stage::Existing(Op::Edit) => Prompt::new("Enter the name of the parameter to edit", Accept::TEXT),
            Stage::Existing(Op::Rename) => Prompt::new("Enter the name of the parameter to rename", Accept::TEXT),
            Stage::Existing(Op::Delete) => Prompt::new("Enter the name of the parameter to delete", Accept::TEXT),
            Stage::Expression if self.new => Prompt::new("Enter an expression for the new parameter", Accept::TEXT),
            Stage::Expression => {
                let p = Prompt::new(format!("Enter a new expression for {}", self.name), Accept::TEXT);
                match self.current_expr(s) {
                    Some(e) => p.default(e),
                    None => p,
                }
            }
            Stage::Rename => Prompt::new(format!("Enter a new name for {}", self.name), Accept::TEXT),
            Stage::List => Prompt::new("Enter the parameters to list", Accept::TEXT).default("*"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.stage {
            Stage::Option => {
                self.stage = match &i {
                    Input::Enter => return Ok(Step::Done),
                    Input::Keyword(k) => match k.to_ascii_lowercase().as_str() {
                        "new" => Stage::NewName,
                        "edit" => Stage::Existing(Op::Edit),
                        "rename" => Stage::Existing(Op::Rename),
                        "delete" => Stage::Existing(Op::Delete),
                        "?" => Stage::List,
                        _ => return Err(EngineError::Other("Invalid option keyword.".into())),
                    },
                    _ => return Err(EngineError::Other("Invalid option keyword.".into())),
                };
                Ok(Step::Continue)
            }
            Stage::NewName => {
                let d = s.doc()?;
                let name = typed(&i).map_or_else(|| free_user_name(d), str::to_string);
                if !cs::expr::valid_name(&name) {
                    return Err(EngineError::Other(format!("`{name}` is not a valid parameter name: a letter or `_`, then letters, digits or `_`.")));
                }
                if cs::name_in_use(d, &name) {
                    return Err(EngineError::Other(format!("`{name}` is already in use.")));
                }
                (self.name, self.new, self.stage) = (name, true, Stage::Expression);
                Ok(Step::Continue)
            }
            Stage::Existing(op) => {
                let Some(name) = typed(&i) else { return Ok(Step::Done) };
                if !cs::name_in_use(s.doc()?, name) {
                    return Err(EngineError::Other(format!("No parameter named `{name}`.")));
                }
                self.name = name.to_string();
                match op {
                    Op::Edit => self.stage = Stage::Expression,
                    Op::Rename => self.stage = Stage::Rename,
                    Op::Delete => {
                        delete_parameter(s, name)?;
                        s.echo(format!("Parameter {name} deleted."));
                        return Ok(Step::Done);
                    }
                }
                Ok(Step::Continue)
            }
            Stage::Expression => {
                // Enter keeps an edited parameter as it is; a new one needs an expression.
                let Some(expr) = typed(&i) else {
                    return if self.new { Err(EngineError::Other("An expression is required.".into())) } else { Ok(Step::Done) };
                };
                set_parameter(s, &self.name, expr)?;
                let row = cs::parameter_table(s.doc()?).into_iter().find(|r| r.name == self.name);
                if let Some(r) = row {
                    s.echo(parameter_line(&r));
                }
                Ok(Step::Done)
            }
            Stage::Rename => {
                let Some(new) = typed(&i) else { return Ok(Step::Done) };
                rename_parameter(s, &self.name, new)?;
                s.echo(format!("Parameter {} renamed to {new}.", self.name));
                Ok(Step::Done)
            }
            Stage::List => Self::list(s, typed(&i).unwrap_or("*")),
        }
    }
}
