//! Interactive commands: prompts, inputs and the command-line coordinate grammar.

use cadcraft_doc::{EntityKind, Handle};
use cadcraft_geom::{Mat3, Vec2};
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
    /// The base is a rectangle's first corner (ZOOM Window, PLOTWINDOW, RECTANG, ARRAYRECT's unit
    /// cell): ortho does not bend the opposite corner, so the rectangle keeps its area.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub corner: bool,
    /// The grip this prompt stretches (one hot grip at the STRETCH prompt), as (object, grip
    /// index): Dynamic Input shows its dimensions ([`crate::grip_dims`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grip: Option<(Handle, usize)>,
    /// A visual chooser the UI can show for this prompt; a choice made in it is entered as
    /// [`Input::Text`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picker: Option<Picker>,
    /// Shown as written, without keywords, a default or a colon added: an AutoLISP `get…`
    /// prompt (the routine writes all of it).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub raw: bool,
    /// The prompt asks for a distance from `base` (a radius, a diameter, a length), and a point
    /// answers with its distance from it: Dynamic Input shows one distance field instead of the
    /// pointer boxes.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub distance: bool,
    /// The prompt picks one object whatever it says: an AutoLISP `entsel`/`nentsel`, whose message
    /// the routine writes, so it cannot be read for the word "Select" (#1598).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub picks_one: bool,
}

/// Parse `@distance<angle` (relative polar), returning `(distance, angle in radians)`.
/// Returns `None` for any other form.
pub fn parse_polar_text(text: &str, angles: &crate::units::AngleSettings) -> Option<(f64, f64)> {
    let body = text.trim().strip_prefix('@')?; // relative form only
    let (d, a) = body.split_once('<')?;
    let dist = d.trim().parse::<f64>().ok()?;
    let angle = angles.amount(a.trim())?;
    (dist.is_finite() && angle.is_finite()).then_some((dist, angle))
}

/// A visual chooser for a prompt ([`Prompt::picker`]).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Picker {
    /// A hatch pattern by name (see [`crate::pattern_choices`]); holds the current pattern.
    HatchPattern(String),
}

impl Prompt {
    pub fn new(message: impl Into<String>, accept: Accept) -> Self {
        Prompt { message: message.into(), accept, ..Default::default() }
    }
    /// The grip being stretched (the `grip` field).
    pub fn grip(mut self, g: (Handle, usize)) -> Self {
        self.grip = Some(g);
        self
    }
    /// The visual chooser the UI can show (the `picker` field).
    pub fn picker(mut self, p: Picker) -> Self {
        self.picker = Some(p);
        self
    }
    pub fn kw(mut self, k: &[&str]) -> Self {
        self.keywords = k.iter().map(ToString::to_string).collect();
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
    /// The base is a window's first corner (the `corner` field).
    pub fn corner(mut self, p: Vec2) -> Self {
        self.base = Some(p);
        self.corner = true;
        self
    }
    /// Asks for a distance from the base (the `distance` field).
    pub fn distance(mut self) -> Self {
        self.distance = true;
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
    /// The prompt picks objects: a "Select objects" prompt, one that says it picks a single object
    /// ([`Prompt::picks_one`]), or a point prompt whose click selects one ("Select object to
    /// offset", "Select first line"…, the commands that pick a single object themselves). The UI
    /// shows the pick box there. The wording is only read where the engine wrote it: a routine
    /// writes its own message (`raw`), which may open with "Select" and still ask for a point,
    /// and says through `picks_one` when it picks instead (#1635 review).
    pub fn picks_objects(&self) -> bool {
        self.picks_one || self.accept.select || (self.accept.point && !self.raw && self.message.starts_with("Select"))
    }
    /// "Specify next point or [Undo/Close] <default>:".
    pub fn display(&self) -> String {
        if self.raw {
            return self.message.trim_matches(['\n', '\r']).to_string();
        }
        let mut s = self.message.clone();
        if !self.keywords.is_empty() {
            // A question offers its answers ("Fix any errors detected? [Yes/No]"); anything else
            // offers options besides what it asks for ("Specify next point or [Undo]").
            s.push_str(if self.message.is_empty() {
                "["
            } else if self.message.ends_with('?') {
                " ["
            } else {
                " or ["
            });
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
        let keys = self.keywords.iter().map(|k| {
            let shortcut: String = k.chars().filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit()).collect::<String>().to_ascii_lowercase();
            let shortcut =
                if shortcut.is_empty() { k.chars().next().map(|c| c.to_ascii_lowercase().to_string()).unwrap_or_default() } else { shortcut };
            (k, k.to_ascii_lowercase(), shortcut)
        });
        // A whole keyword or its capital-letter shortcut wins over a prefix of another keyword
        // (`M` is Multiple, not a prefix of mEthod). A prefix reaches past the keyword's last
        // capital, as in AutoCAD: `L` is none of LAyer, LType, ltScale or LWeight (AutoCAD 2024's
        // CHPROP: "Ambiguous response, please clarify..."), `LTS` is ltScale.
        let past_capitals = |k: &str| k.char_indices().rfind(|(_, c)| c.is_ascii_uppercase() || c.is_ascii_digit()).map_or(1, |(i, _)| i + 1);
        keys.clone()
            .find(|(_, kl, sc)| tl == *kl || tl == *sc)
            .or_else(|| keys.clone().find(|(k, kl, _)| tl.len() >= past_capitals(k) && kl.starts_with(&tl)))
            .map(|(k, _, _)| k.clone())
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
    /// Called when the command is cancelled (Esc), for one that moved the view or the space and
    /// puts it back. The drawing itself is restored by the session.
    fn cancelled(&mut self, _s: &mut Session) {}
    /// Rubber-band geometry for the cursor position.
    fn preview(&self, _s: &Session, _cursor: Vec2) -> Vec<EntityKind> {
        Vec::new()
    }
    /// Preview objects drawn with their own properties (colour, layer, linetype) rather than
    /// the current layer's, as [`Interactive::preview`]'s are: a block reference re-evaluated
    /// under the cursor, drawn as it will look. Drawn under the rubber band.
    fn preview_objects(&self, _s: &Session, _cursor: Vec2) -> Vec<cadcraft_doc::Entity> {
        Vec::new()
    }
    /// The preview as existing objects under one transform, when it is that (MOVE, COPY,
    /// ROTATE, SCALE). The UI then draws them from geometry uploaded once instead of rebuilding
    /// the transformed copies every frame; `None` falls back to [`Interactive::preview`].
    fn drag(&self, _s: &Session, _cursor: Vec2) -> Option<Drag> {
        None
    }
    /// The multiline text being typed (MTEXT, once its box is known) with empty contents: where
    /// the UI places its in-place editor and with which height, width and justification.
    fn editing_mtext(&self) -> Option<cadcraft_doc::MText> {
        None
    }
}

/// See [`Interactive::drag`].
#[derive(Clone, Debug)]
pub struct Drag {
    pub handles: Vec<Handle>,
    pub matrix: Mat3,
    /// Rubber-band geometry drawn as well (the base-to-cursor line), and dragged objects the
    /// matrix can't draw (text MIRROR keeps readable), already transformed.
    pub extra: Vec<EntityKind>,
    /// The dragged objects are the clipboard's ([`Session::clipboard`]), not `handles`.
    pub clipboard: bool,
}

/// Parse a typed point: `x,y[,z]`, `@dx,dy`, `@dist<angle`, `dist<angle`, `#x,y` (absolute),
/// `*x,y` and `*@dx,dy`/`@*dx,dy` (world coordinates: the same as the plain forms, as there is no
/// UCS yet).
/// Relative forms use `last`. Polar angles read the default angle settings (degrees from +X,
/// counterclockwise); `parse_point_with` reads a drawing's.
pub fn parse_point(text: &str, last: Vec2) -> Option<Vec2> {
    parse_point_with(text, last, &crate::units::AngleSettings::default())
}

/// `parse_point` with polar angles read per `angles` (AUNITS, ANGBASE, ANGDIR); `d<<a` and
/// `d<<<a` measure from +X counterclockwise (in degrees and in AUNITS).
pub fn parse_point_with(text: &str, last: Vec2, angles: &crate::units::AngleSettings) -> Option<Vec2> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let t = t.strip_prefix('*').map_or(t, str::trim_start);
    let (rel, body) = if let Some(b) = t.strip_prefix('@') {
        let b = b.trim();
        (true, b.strip_prefix('*').map_or(b, str::trim_start))
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
        // `d<<a` / `d<<<a`: the override prefixes keep their `<<` / `<<<`.
        let ang = if a.starts_with('<') { angles.direction(&format!("<{a}"))? } else { angles.direction(a)? };
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
        assert_eq!(parse_point("*3,4", last), Some(Vec2::new(3.0, 4.0)));
        assert_eq!(parse_point("*@1,2", last), Some(Vec2::new(11.0, 12.0)));
        assert_eq!(parse_point("@*1,2", last), Some(Vec2::new(11.0, 12.0)));
        assert_eq!(parse_point("*", last), None);
        assert_eq!(parse_point("1", last), None);
        assert_eq!(parse_point("a,b", last), None);
        assert_eq!(parse_point("1,2,3,4", last), None);
    }

    #[test]
    fn keywords() {
        let p = Prompt::new("Specify next point", Accept::POINT).kw(&["Undo", "Close"]);
        assert_eq!(p.display(), "Specify next point or [Undo/Close]:");
        let q = Prompt::new("Erase polyline?", Accept::TEXT).kw(&["Yes", "No"]).default("No");
        assert_eq!(q.display(), "Erase polyline? [Yes/No] <No>:");
        assert_eq!(p.match_keyword("u").as_deref(), Some("Undo"));
        assert_eq!(p.match_keyword("CL").as_deref(), Some("Close"));
        assert_eq!(p.match_keyword("x"), None);
        let q = Prompt::new("", Accept::POINT).kw(&["3P", "2P", "Ttr"]);
        assert_eq!(q.match_keyword("3p").as_deref(), Some("3P"));
        assert_eq!(q.match_keyword("t").as_deref(), Some("Ttr"));
        // A prefix reaches past the last capital (AutoCAD 2024's CHPROP: `L` is ambiguous).
        let c =
            Prompt::new("Enter property to change", Accept::TEXT).kw(&["Color", "LAyer", "LType", "ltScale", "LWeight", "Thickness", "TRansparency"]);
        assert_eq!(c.match_keyword("l"), None);
        assert_eq!(c.match_keyword("la").as_deref(), Some("LAyer"));
        assert_eq!(c.match_keyword("lts").as_deref(), Some("ltScale"));
        assert_eq!(c.match_keyword("s").as_deref(), Some("ltScale"));
        assert_eq!(c.match_keyword("tr").as_deref(), Some("TRansparency"));
        // Even when no other keyword starts with it: AutoCAD 2024 answers QDIM's `DA` with "Point or
        // option keyword required." and PEDIT's vertex `E` with "Invalid option keyword.".
        let q = Prompt::new("", Accept::POINT).kw(&[
            "Continuous",
            "Staggered",
            "Baseline",
            "Ordinate",
            "Radius",
            "Diameter",
            "datumPoint",
            "Edit",
            "seTtings",
        ]);
        assert_eq!(q.match_keyword("da"), None);
        assert_eq!(q.match_keyword("datump").as_deref(), Some("datumPoint"));
        assert_eq!(q.match_keyword("p").as_deref(), Some("datumPoint"));
        let v = Prompt::new("", Accept::TEXT).kw(&["Next", "Previous", "Break", "Insert", "Move", "Regen", "Straighten", "Tangent", "Width", "eXit"]);
        assert_eq!(v.match_keyword("e"), None);
        assert_eq!(v.match_keyword("ex").as_deref(), Some("eXit"));
    }
}
