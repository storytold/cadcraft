//! Interactive commands: prompts, inputs and the command-line coordinate grammar.

use cadcraft_doc::{EntityKind, Handle};
use cadcraft_geom::Vec2;
use serde::Serialize;

use crate::{Result, Session};

/// What the current prompt accepts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Accept {
    pub point: bool,
    pub number: bool,
    pub text: bool,
    pub select: bool,
    /// Enter is meaningful (ends the command or takes the default).
    pub enter: bool,
}

impl Accept {
    pub const POINT: Accept = Accept { point: true, number: false, text: false, select: false, enter: true };
    pub const POINT_OR_NUMBER: Accept = Accept { point: true, number: true, text: false, select: false, enter: true };
    pub const NUMBER: Accept = Accept { point: false, number: true, text: false, select: false, enter: true };
    pub const TEXT: Accept = Accept { point: false, number: false, text: true, select: false, enter: true };
    pub const SELECT: Accept = Accept { point: true, number: false, text: false, select: true, enter: true };
}

/// A prompt shown on the command line.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub message: String,
    /// Keyword options, written with their shortcut letters capitalised (e.g. "Undo", "Close").
    pub keywords: Vec<String>,
    pub default: Option<String>,
    pub accept: Accept,
    /// Rubber-band origin (for direct distance entry, ortho and polar).
    pub base: Option<Vec2>,
    /// The command resolves deferred tangent/perpendicular snaps ([`Input::Deferred`]) at this
    /// prompt (a point with no base yet, e.g. the first point of a line). Elsewhere a deferred
    /// pick arrives as the plain point it was picked at.
    pub deferred: bool,
}

impl Prompt {
    pub fn new(message: impl Into<String>, accept: Accept) -> Self {
        Prompt { message: message.into(), keywords: Vec::new(), default: None, accept, base: None, deferred: false }
    }
    pub fn kw(mut self, k: &[&str]) -> Self {
        self.keywords = k.iter().map(|s| s.to_string()).collect();
        self
    }
    pub fn base(mut self, p: Vec2) -> Self {
        self.base = Some(p);
        self
    }
    pub fn base_opt(mut self, p: Option<Vec2>) -> Self {
        self.base = p;
        self
    }
    /// Accept deferred tangent/perpendicular snaps (the `deferred` field).
    pub fn deferred(mut self) -> Self {
        self.deferred = true;
        self
    }
    pub fn default(mut self, d: impl Into<String>) -> Self {
        self.default = Some(d.into());
        self
    }
    /// The prompt picks objects: a "Select objects" prompt, or a point prompt whose click selects
    /// one object ("Select object to offset", "Select first line"…, the commands that pick a
    /// single object themselves). The UI shows the pick box there.
    pub fn picks_objects(&self) -> bool {
        self.accept.select || (self.accept.point && self.message.starts_with("Select"))
    }
    /// "Specify next point or [Undo/Close] <default>:".
    pub fn display(&self) -> String {
        let mut s = self.message.clone();
        if !self.keywords.is_empty() {
            s.push_str(if self.message.is_empty() { "[" } else { " or [" });
            s.push_str(&self.keywords.join("/"));
            s.push(']');
        }
        if let Some(d) = &self.default {
            s.push_str(&format!(" <{d}>"));
        }
        s.push(':');
        s
    }
    /// Match typed text against keywords: full word or the capitalised shortcut, case-insensitive.
    pub fn match_keyword(&self, text: &str) -> Option<String> {
        let t = text.trim();
        if t.is_empty() {
            return None;
        }
        let tl = t.to_ascii_lowercase();
        for k in &self.keywords {
            let shortcut: String = k.chars().filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit()).collect::<String>().to_ascii_lowercase();
            let shortcut =
                if shortcut.is_empty() { k.chars().next().map(|c| c.to_ascii_lowercase().to_string()).unwrap_or_default() } else { shortcut };
            let kl = k.to_ascii_lowercase();
            if tl == kl || tl == shortcut || (tl.len() >= shortcut.len() && kl.starts_with(&tl)) {
                return Some(k.clone());
            }
        }
        None
    }
}

/// An input to the active command.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum Input {
    Point(Vec2),
    /// A deferred tangent/perpendicular snap, only sent to prompts that accept it
    /// (`Prompt.deferred`).
    Deferred(crate::snap::Deferred),
    Keyword(String),
    /// Free text or a number/distance/angle to be parsed by the command.
    Text(String),
    /// Objects picked during a selection prompt.
    Pick(Vec<Handle>),
    Enter,
    Cancel,
}

/// What happens after an input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Continue,
    Done,
    Cancel,
}

/// A prompt-driven command.
pub trait Interactive: Send {
    fn name(&self) -> &'static str;
    fn prompt(&self, s: &Session) -> Prompt;
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step>;
    /// Called once when the command starts (may finish at once, e.g. with a pickfirst selection).
    fn begin(&mut self, _s: &mut Session) -> Result<Step> {
        Ok(Step::Continue)
    }
    /// Rubber-band geometry for the cursor position.
    fn preview(&self, _s: &Session, _cursor: Vec2) -> Vec<EntityKind> {
        Vec::new()
    }
}

/// Parse a typed point: `x,y[,z]`, `@dx,dy`, `@dist<angle`, `dist<angle`, `#x,y` (absolute).
/// Relative forms use `last`.
pub fn parse_point(text: &str, last: Vec2) -> Option<Vec2> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let (rel, body) = if let Some(b) = t.strip_prefix('@') {
        (true, b.trim())
    } else if let Some(b) = t.strip_prefix('#') {
        (false, b.trim())
    } else {
        (false, t)
    };
    if rel && body.is_empty() {
        return Some(last);
    }
    let p = if let Some((d, a)) = body.split_once('<') {
        let dist = crate::units::parse_distance(d)?;
        let ang = crate::units::parse_angle(a)?;
        Vec2::from_angle(ang) * dist
    } else {
        let parts: Vec<&str> = body.split(',').collect();
        if parts.len() < 2 || parts.len() > 3 {
            return None;
        }
        let x = crate::units::parse_distance(parts.first()?)?;
        let y = crate::units::parse_distance(parts.get(1)?)?;
        if let Some(z) = parts.get(2) {
            crate::units::parse_distance(z)?;
        }
        Vec2::new(x, y)
    };
    let r = if rel { last + p } else { p };
    r.is_finite().then_some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points() {
        let last = Vec2::new(10.0, 10.0);
        assert_eq!(parse_point("3,4", last), Some(Vec2::new(3.0, 4.0)));
        assert_eq!(parse_point("3,4,5", last), Some(Vec2::new(3.0, 4.0)));
        assert_eq!(parse_point("@1,2", last), Some(Vec2::new(11.0, 12.0)));
        assert!(parse_point("@5<90", last).unwrap().near(Vec2::new(10.0, 15.0), 1e-9));
        assert!(parse_point("5<0", last).unwrap().near(Vec2::new(5.0, 0.0), 1e-9));
        assert_eq!(parse_point("#1,1", last), Some(Vec2::new(1.0, 1.0)));
        assert_eq!(parse_point("@", last), Some(last));
        assert_eq!(parse_point("1", last), None);
        assert_eq!(parse_point("a,b", last), None);
        assert_eq!(parse_point("1,2,3,4", last), None);
    }

    #[test]
    fn keywords() {
        let p = Prompt::new("Specify next point", Accept::POINT).kw(&["Undo", "Close"]);
        assert_eq!(p.display(), "Specify next point or [Undo/Close]:");
        assert_eq!(p.match_keyword("u").as_deref(), Some("Undo"));
        assert_eq!(p.match_keyword("CL").as_deref(), Some("Close"));
        assert_eq!(p.match_keyword("x"), None);
        let q = Prompt::new("", Accept::POINT).kw(&["3P", "2P", "Ttr"]);
        assert_eq!(q.match_keyword("3p").as_deref(), Some("3P"));
        assert_eq!(q.match_keyword("t").as_deref(), Some("Ttr"));
    }
}
