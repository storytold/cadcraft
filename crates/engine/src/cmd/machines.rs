//! Building blocks for interactive commands: object selection and simple point sequences.

use cadcraft_doc::Handle;
use serde_json::{Value, json};

use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// A "Select objects:" phase. Uses the pickfirst selection when there is one.
#[derive(Clone, Debug, Default)]
pub struct SelectPhase {
    pub picked: Vec<Handle>,
    pub done: bool,
    pub single: bool,
    pub removing: bool,
}

pub enum SelOutcome {
    /// Keep prompting.
    More,
    /// Selection finished with these objects.
    Done(Vec<Handle>),
    /// Selection finished empty: the command should end.
    Empty,
}

impl SelectPhase {
    /// Start with the pickfirst selection, if any (then the phase is already done).
    pub fn begin(s: &mut Session) -> Self {
        let pre = s.selection();
        if !pre.is_empty() && s.settings.pickfirst {
            s.remember_selection(&pre);
            SelectPhase { picked: pre, done: true, single: false, removing: false }
        } else {
            SelectPhase::default()
        }
    }
    pub fn single() -> Self {
        SelectPhase { single: true, ..Default::default() }
    }
    pub fn prompt(&self) -> Prompt {
        let msg = if self.removing {
            "Remove objects"
        } else if self.single {
            "Select object"
        } else {
            "Select objects"
        };
        Prompt::new(msg, Accept::SELECT)
    }
    pub fn feed(&mut self, s: &mut Session, i: &Input) -> Result<SelOutcome> {
        match i {
            Input::Pick(hs) => {
                let before = self.picked.len();
                if self.removing {
                    self.picked.retain(|h| !hs.contains(h));
                } else {
                    for h in hs {
                        if !self.picked.contains(h) {
                            self.picked.push(*h);
                        }
                    }
                }
                let n = hs.len();
                let _ = before;
                if n > 0 {
                    s.echo(format!("{n} found, {} total", self.picked.len()));
                } else {
                    s.echo("0 found");
                }
                s.set_selection(self.picked.clone());
                if self.single && !self.picked.is_empty() {
                    return Ok(self.finish(s));
                }
                Ok(SelOutcome::More)
            }
            Input::Text(t) | Input::Keyword(t) => {
                match t.trim().to_ascii_lowercase().as_str() {
                    "r" | "remove" => self.removing = true,
                    "a" | "add" => self.removing = false,
                    _ => s.echo("*Invalid selection*"),
                }
                Ok(SelOutcome::More)
            }
            Input::Enter => Ok(self.finish(s)),
            _ => Ok(SelOutcome::More),
        }
    }
    fn finish(&mut self, s: &mut Session) -> SelOutcome {
        self.done = true;
        if self.picked.is_empty() {
            SelOutcome::Empty
        } else {
            s.remember_selection(&self.picked);
            SelOutcome::Done(self.picked.clone())
        }
    }
}

/// "Select objects:", then run a JSON command on them as `{handles}`. With a pickfirst selection it
/// runs at once, exactly like the JSON form does on the current selection.
pub struct SelectRun {
    id: &'static str,
    name: &'static str,
    /// Single-object prompt (e.g. LAYFRZ's "Select an object on the layer to be frozen").
    single: Option<&'static str>,
    sel: SelectPhase,
}

impl SelectRun {
    pub fn new(id: &'static str, name: &'static str) -> Self {
        SelectRun { id, name, single: None, sel: SelectPhase::default() }
    }
    /// Pick one object, with a command-specific prompt.
    pub fn single(id: &'static str, name: &'static str, msg: &'static str) -> Self {
        SelectRun { single: Some(msg), ..SelectRun::new(id, name) }
    }
    fn run(&self, s: &mut Session, hs: &[Handle]) -> Result<Step> {
        let spec = super::find_command(self.id).ok_or_else(|| EngineError::UnknownCommand(self.id.into()))?;
        // Call the command body directly: the running command records the single undo step.
        let r = (spec.run)(s, &json!({ "handles": hs.iter().map(|h| h.hex()).collect::<Vec<_>>() }))?;
        if let Some(m) = r.get("message").and_then(Value::as_str) {
            s.echo(m.to_string());
        }
        Ok(Step::Done)
    }
}

impl Interactive for SelectRun {
    fn name(&self) -> &'static str {
        self.name
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let hs = self.sel.picked.clone();
            return self.run(s, &hs);
        }
        self.sel.single = self.single.is_some();
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.single {
            Some(msg) if !self.sel.removing => Prompt::new(msg, Accept::SELECT),
            _ => self.sel.prompt(),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.sel.feed(s, &i)? {
            SelOutcome::More => Ok(Step::Continue),
            SelOutcome::Empty => Ok(Step::Done),
            SelOutcome::Done(hs) => {
                // Objects picked at the prompt don't stay selected afterwards.
                let step = self.run(s, &hs)?;
                s.set_selection(Vec::new());
                Ok(step)
            }
        }
    }
}

/// Parse a number from text input.
pub fn number(t: &str) -> Option<f64> {
    crate::units::parse_distance(t)
}

#[cfg(test)]
mod tests {
    use cadcraft_doc::Handle;
    use cadcraft_geom::Vec2;
    use serde_json::json;

    use crate::{Input, Session};

    /// Menu/typed invocation of selection commands prompts "Select objects" when nothing is selected
    /// (#126), and acts on the picked objects in one undo step.
    #[test]
    fn selection_commands_prompt_without_pickfirst() {
        let mut s = Session::new();
        s.execute("layer.new", &json!({ "name": "A", "current": true })).unwrap();
        s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
        s.execute("layer.current", &json!({ "name": "0" })).unwrap();
        s.execute("line", &json!({ "points": [[0, 5], [10, 5]] })).unwrap();
        let hs: Vec<Handle> = s.doc().unwrap().model.iter().map(|e| e.handle).collect();
        let undo_before = s.state().unwrap().undo.len();

        s.start("cutclip").unwrap();
        assert!(s.prompt_text().contains("Select objects"), "{}", s.prompt_text());
        s.input(Input::Pick(vec![hs[1]])).unwrap();
        s.input(Input::Enter).unwrap();
        assert!(s.running.is_none());
        assert_eq!(s.doc().unwrap().model.len(), 1);
        assert_eq!(s.clipboard.len(), 1);
        assert_eq!(s.state().unwrap().undo.len(), undo_before + 1, "one undo step");

        // COPYBASE asks for the base point first, then for objects.
        s.start("copybase").unwrap();
        assert!(s.prompt_text().contains("Specify base point"), "{}", s.prompt_text());
        s.input(Input::Point(Vec2::new(3.0, 4.0))).unwrap();
        assert!(s.prompt_text().contains("Select objects"), "{}", s.prompt_text());
        s.input(Input::Pick(vec![hs[0]])).unwrap();
        s.input(Input::Enter).unwrap();
        assert!(s.running.is_none());
        assert_eq!(s.clipboard_base, Vec2::new(3.0, 4.0));

        // LAYFRZ picks one object and freezes its layer.
        s.start("layfrz").unwrap();
        assert!(s.prompt_text().contains("layer to be frozen"), "{}", s.prompt_text());
        s.input(Input::Pick(vec![hs[0]])).unwrap();
        assert!(s.running.is_none());
        assert!(s.doc().unwrap().layer("A").is_some_and(|l| l.frozen));
    }

    /// With a pickfirst selection the commands run at once; JSON calls never prompt.
    #[test]
    fn selection_commands_pickfirst_and_json_unchanged() {
        let mut s = Session::new();
        s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
        let h = s.doc().unwrap().model.iter().next().unwrap().handle;
        s.set_selection(vec![h]);
        s.start("copyclip").unwrap();
        assert!(s.running.is_none(), "pickfirst: no prompt");
        assert_eq!(s.clipboard.len(), 1);

        s.set_selection(Vec::new());
        for id in ["cutclip", "copyclip", "zoom.object", "draworder.front", "laycur", "layoff"] {
            let _ = s.execute(id, &json!({}));
            assert!(s.running.is_none(), "{id}: JSON form must not prompt");
        }
        assert_eq!(s.doc().unwrap().model.len(), 1);
        assert!(s.doc().unwrap().layer("0").is_some_and(|l| l.on));
    }
}
