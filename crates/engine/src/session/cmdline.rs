use super::{EngineError, Input, Result, Session, Value, Vec2, cmd, lisp, prompt, snap, units};

impl Session {
    /// Process one line typed at the command line (Enter pressed). Spaces separate inputs as in a
    /// script line (see [`Session::script`]), so `MOVE L  0,0 5,5` ends the selection with the
    /// second space and `ERASE L ` with the trailing one. Input after the command ends is ignored.
    pub fn cmdline(&mut self, text: &str) -> Result<()> {
        let t = text.trim_end_matches(['\r', '\n']);
        // The command line types spaces literally at free-text prompts: the line is one input.
        if self.current_prompt().is_some_and(|p| p.accept.text && !p.accept.point && !p.accept.number) {
            return self.typed(t);
        }
        // At `Command:` leading blanks before a command name are not Enter (`" LINE"` from the
        // control channel starts LINE); a line of only blanks still is.
        let t = if self.running.is_none() && !t.trim().is_empty() { t.trim_start() } else { t };
        self.input_line(t, false)
    }

    /// One line of input: each space ends an input and an empty input (two spaces in a row, a
    /// trailing space, an empty line) is Enter, which at `Command:` repeats the last command. In a
    /// script, a free-text prompt (TEXT/MTEXT contents) takes the rest of the line, a prompt with
    /// keywords still splits so `I 15` reaches the keyword and the value, and the line goes on
    /// after its command ends; at the command line the rest after the command ends is ignored.
    fn input_line(&mut self, line: &str, script: bool) -> Result<()> {
        let mut rest = line;
        let mut first = true;
        loop {
            if self.running.is_none() {
                if !script && !first {
                    return Ok(());
                }
                // `cmd {json}`: programmatic call with parameters.
                if let Some((name, json)) = rest.trim().split_once(' ')
                    && json.trim_start().starts_with('{')
                {
                    let params: Value =
                        serde_json::from_str(json.trim()).map_err(|e| EngineError::BadParams { cmd: name.into(), msg: e.to_string() })?;
                    let id = cmd::resolve_alias(&name.to_ascii_lowercase());
                    let r = self.execute(&id, &params)?;
                    if !r.is_null() {
                        self.echo(r.to_string());
                    }
                    return Ok(());
                }
            } else if script && self.current_prompt().is_some_and(|p| p.accept.text && !p.accept.point && !p.accept.number && p.keywords.is_empty()) {
                return self.typed(rest);
            }
            first = false;
            let (tok, more) = next_token(rest);
            if self.running.is_some() || self.lisp_prompt().is_some() {
                self.typed(tok)?;
            } else if tok.trim().is_empty() {
                self.input(Input::Enter)?;
            } else if lisp_expression(tok) {
                // `(expression)` or `!variable` at Command: evaluates LISP and shows the value.
                let src = tok.trim().strip_prefix('!').unwrap_or(tok.trim()).to_string();
                if let Err(e) = self.lisp_eval(&src, true, false, Vec::new()) {
                    self.echo(e.to_string());
                }
            } else {
                self.start(tok)?;
            }
            match more {
                Some(more) => rest = more,
                None => return Ok(()),
            }
        }
    }

    /// One token typed while a command runs.
    pub(crate) fn typed(&mut self, t: &str) -> Result<()> {
        let Some(prompt) = self.current_prompt() else { return Ok(()) };
        let tt = t.trim();
        if tt.is_empty() {
            return self.input(Input::Enter);
        }
        // FROM, M2P, point filters, `<a` (before `'`: `'_from` is a modifier).
        if let Some(r) = self.typed_modifier(&prompt, tt) {
            return r;
        }
        // Transparent command.
        if tt.starts_with('\'') {
            return self.start(tt);
        }
        // A LISP expression typed at a command's prompt: its value is the answer.
        // Not at a free-text prompt: there the text is the answer (CAL takes "(3+4)*2").
        if lisp_expression(tt) && !prompt.accept.text && self.lisp_prompt().is_none() && !self.lisp_groups_undo() {
            let src = tt.strip_prefix('!').unwrap_or(tt).to_string();
            let v = match self.lisp_eval(&src, false, true, Vec::new()) {
                Ok(Some(Ok(v))) => v,
                Ok(_) => return Ok(()),
                Err(e) => {
                    self.echo(e.to_string());
                    return Ok(());
                }
            };
            return match v {
                lisp::value::Value::Nil => Ok(()),
                lisp::value::Value::Int(i) => self.typed(&i.to_string()),
                lisp::value::Value::Real(r) => self.typed(&format!("{r}")),
                lisp::value::Value::Str(s) => self.typed(&s),
                other => match other.as_point() {
                    Some(p) => self.input(Input::Point(Vec2::new(p[0], p[1]))),
                    None => {
                        self.echo(format!("Invalid input: {}", lisp::value::to_text(&other, true)));
                        Ok(())
                    }
                },
            };
        }
        // At "Select objects" prompts the selection modes (Previous, Last, ALL) win over a command
        // keyword sharing the letter (HATCH's "picK internal point" against `P`).
        let selection_mode = prompt.accept.select
            && matches!(tt.strip_prefix('_').unwrap_or(tt).to_ascii_lowercase().as_str(), "all" | "l" | "last" | "p" | "previous");
        // A leading underscore asks for a keyword by its English name, whatever the interface
        // language (`_M`, `_Yes`): scripts and LISP routines write them that way. (Snap
        // overrides such as `_cen` were taken above.)
        if !selection_mode
            && let Some(k) = prompt.match_keyword(tt).or_else(|| tt.strip_prefix('_').and_then(|r| prompt.match_keyword(r)))
            && !(prompt.accept.number && tt.parse::<f64>().is_ok())
        {
            return self.input(Input::Keyword(k));
        }
        if prompt.accept.point {
            if let Some(p) = prompt::parse_point_with(tt, self.last_point, &self.angle_settings()) {
                self.last_polar = prompt::parse_polar_text(tt, &self.angle_settings());
                return self.input(Input::Point(p));
            }
            // Direct distance entry along the tracking path the cursor is on, from the point it
            // comes from (an acquired or temporary tracking point needs no base point).
            if let Some(dist) = units::parse_distance(tt).filter(|_| !prompt.accept.number)
                && let Some((from, dir)) = self.tracking.last.as_ref().filter(|t| t.point.near(self.cursor, 1e-9)).and_then(|t| t.along())
            {
                return self.input(Input::Point(from + dir * dist));
            }
            // Direct distance entry along the rubber band.
            if let (Some(base), Some(dist)) = (prompt.base, units::parse_distance(tt))
                && !prompt.accept.number
            {
                let mut c = self.cursor;
                if self.settings.orthomode && !prompt.corner {
                    c = snap::iso::constrain(&self.drafting_settings(), base, c);
                }
                let dir = (c - base).normalized();
                let dir = if dir == Vec2::ZERO { Vec2::X } else { dir };
                return self.input(Input::Point(base + dir * dist));
            }
        }
        self.input(Input::Text(tt.to_string()))
    }

    /// Run a script, as AutoCAD reads one: a space or a line end ends an input, and an empty
    /// input (two spaces in a row, a space at the end of a line, an empty line) is Enter. At
    /// free-text prompts spaces are part of the text. Lines starting with `;` are comments.
    ///
    /// A script cannot run a script: the SCRIPT command (or another `cmd {json}` call) inside a
    /// running one is refused, so two files that run each other cannot recurse until the stack dies.
    pub fn script(&mut self, text: &str) -> Result<()> {
        if self.in_script {
            return Err(EngineError::BadParams { cmd: "script".into(), msg: "a script is already running; nested scripts are not supported".into() });
        }
        self.in_script = true;
        let r = self.script_lines(text);
        self.in_script = false;
        r
    }

    fn script_lines(&mut self, text: &str) -> Result<()> {
        for line in text.lines() {
            // Trailing spaces are inputs (Enter); other trailing whitespace (`\r`) is not.
            let line = line.trim_end_matches(|c: char| c.is_whitespace() && c != ' ');
            if line.trim_start().starts_with(';') {
                continue;
            }
            self.input_line(line, true)?;
        }
        Ok(())
    }
}

/// Typed text that is LISP: `(expression)` or `!variable`.
fn lisp_expression(t: &str) -> bool {
    let t = t.trim();
    t.starts_with('(') || (t.starts_with('!') && t.len() > 1)
}

/// The next command-line input in `rest` and what follows it: up to a space or tab, except that
/// a LISP expression runs to its closing parenthesis (spaces and quoted strings inside it).
fn next_token(rest: &str) -> (&str, Option<&str>) {
    let lead = rest.len() - rest.trim_start_matches('!').len();
    if rest.get(lead..).is_some_and(|r| r.starts_with('(')) {
        let (mut depth, mut in_str, mut escaped) = (0i32, false, false);
        for (i, c) in rest.char_indices().skip(lead) {
            if in_str {
                match (escaped, c) {
                    (true, _) => escaped = false,
                    (false, '\\') => escaped = true,
                    (false, '"') => in_str = false,
                    _ => {}
                }
                continue;
            }
            match c {
                '"' => in_str = true,
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        let end = i + 1;
                        // As after any input, a space ends it and a trailing one is Enter.
                        let more = rest.get(end..).filter(|m| !m.is_empty()).map(|m| m.strip_prefix([' ', '\t']).unwrap_or(m));
                        return (&rest[..end], more);
                    }
                }
                _ => {}
            }
        }
        // Unbalanced: all of it (the reader reports the error).
        return (rest, None);
    }
    match rest.split_once([' ', '\t']) {
        Some((tok, more)) => (tok, Some(more)),
        None => (rest, None),
    }
}
