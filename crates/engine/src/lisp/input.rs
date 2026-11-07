//! User input from LISP: `getpoint`, `getcorner`, `getdist`, `getangle`, `getorient`,
//! `getreal`, `getint`, `getstring`, `getkword`, `entsel`, `nentsel`, and `initget`, which sets
//! what the next one accepts (bits: 1 no Enter, 2 no zero, 4 no negatives, 128 any text; and
//! keywords).

use cadcraft_geom::Vec2;

use super::eval::{Ctx, Flow, Need};
use super::value::Value;
use super::{LispError, builtins::args_err};
use crate::{Accept, Input, Prompt, Session};

/// `initget` settings.
#[derive(Clone, Debug, Default)]
pub struct Initget {
    pub bits: i32,
    pub keywords: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GetKind {
    Point,
    Corner,
    Dist,
    /// `getangle` (relative to ANGBASE) or `getorient` (absolute).
    Angle {
        orient: bool,
    },
    Real,
    Int,
    /// `getstring`; `spaces`: a space is part of the text (non-nil first argument).
    String {
        spaces: bool,
    },
    Kword,
    /// `getfiled`: a file name typed; `new`: it may name a file that doesn't exist yet.
    File {
        new: bool,
    },
    /// `entsel` / `nentsel`: an object picked.
    EntSel,
}

/// A `get…` call waiting for the user.
#[derive(Clone, Debug)]
pub struct GetRequest {
    pub kind: GetKind,
    pub message: String,
    pub base: Option<[f64; 3]>,
    pub initget: Initget,
    /// `getdist`/`getangle` without a base point: the first point given, the second to come.
    pub first: Option<[f64; 3]>,
    /// `getfiled`'s extension, default and folder (boxed: it keeps [`Need`](super::eval::Need) small).
    pub file: Box<FileAsk>,
}

/// `getfiled`: the extension a bare name gets, the name Enter gives, and the folder a relative
/// name is in.
#[derive(Clone, Debug, Default)]
pub struct FileAsk {
    pub ext: String,
    pub default: String,
    /// The default folder (a default ending in a path delimiter, or any default with flag 16).
    pub dir: String,
    /// New-file mode asks before an existing file is replaced (flag 32 clear).
    pub confirm: bool,
    /// A new file's name that already exists, waiting for Yes to replace it.
    pub replace: Option<String>,
}

impl FileAsk {
    /// `name` with the extension when it has none, in the default folder when relative (`None`
    /// for an empty name).
    fn complete(&self, name: &str) -> Option<String> {
        let name = name.trim().trim_matches('"');
        if name.is_empty() {
            return None;
        }
        let has_ext = std::path::Path::new(name).extension().is_some();
        let name = if has_ext || self.ext.is_empty() { name.to_string() } else { format!("{name}.{}", self.ext) };
        Some(if self.dir.is_empty() || std::path::Path::new(&name).is_absolute() {
            name
        } else {
            std::path::Path::new(&self.dir).join(name).to_string_lossy().into_owned()
        })
    }
}

/// `getfiled`'s answer `name`, typed or the default taken with Enter: open mode (`new` false)
/// asks again for a file that isn't there, and new-file mode asks before replacing one that is
/// (unless flag 32 says not to), where files can be checked. In the browser (wasm32) there is no
/// file system to look in, so the name is taken as given, and opening it then fails the way every
/// file function there does (`open` gives nil).
fn file_answer(req: &GetRequest, name: String, new: bool) -> Result<Value, Again> {
    if cfg!(not(target_arch = "wasm32")) {
        let exists = super::builtins::file_exists(&name);
        if !new && !exists {
            return Err(format!("Can't find file {name}.").as_str().into());
        }
        if new && exists && req.file.confirm {
            let file = FileAsk { replace: Some(name), ..(*req.file).clone() };
            return Err(Again::Ask(Box::new(GetRequest { file: Box::new(file), ..req.clone() })));
        }
    }
    Ok(Value::str(&name))
}

impl GetRequest {
    /// The prompt the command line shows.
    pub fn prompt(&self) -> Prompt {
        let any = self.initget.bits & 128 != 0 || !self.initget.keywords.is_empty();
        let accept = match self.kind {
            GetKind::Point | GetKind::Corner | GetKind::EntSel => Accept { text: any, ..Accept::POINT },
            GetKind::Dist | GetKind::Angle { .. } => Accept { text: any, ..Accept::POINT_OR_NUMBER },
            GetKind::Real | GetKind::Int => Accept { text: any, ..Accept::NUMBER },
            GetKind::String { .. } | GetKind::Kword | GetKind::File { .. } => Accept::TEXT,
        };
        let message = match (&self.file.replace, &self.first, self.message.is_empty()) {
            (Some(name), _, _) => format!("{name} already exists. Replace it? [Yes/No] <No>: "),
            (None, Some(_), _) => "Specify second point: ".to_string(),
            (None, None, true) => default_message(self.kind).to_string(),
            (None, None, false) => self.message.clone(),
        };
        let base = self.first.or(self.base).map(|p| Vec2::new(p[0], p[1]));
        let mut p = Prompt::new(message, accept).base_opt(base);
        p.keywords = self.initget.keywords.clone();
        p.raw = true;
        // `entsel`/`nentsel` pick one object. The routine writes the message, so the prompt says
        // so itself rather than being read for the word "Select" -- with a message of its own
        // ("\nSelect the second line: ", or no mention of selecting at all) the pick box went
        // missing, while the same call without one got it from the default "Select object: "
        // (#1598).
        p.picks_one = matches!(self.kind, GetKind::EntSel);
        p
    }

    /// What an input gives: a value, a reason to ask again, or (for a distance or an angle
    /// without a base point) the first of two points.
    pub fn answer(&self, s: &mut Session, input: Input) -> Answer {
        match self.answer_value(s, input) {
            Ok(v) => Answer::Value(v),
            Err(Again::Second(p)) => Answer::Second(p),
            Err(Again::Msg(m)) => Answer::Again(m),
            Err(Again::Ask(r)) => Answer::Ask(r),
        }
    }

    fn answer_value(&self, s: &mut Session, input: Input) -> Result<Value, Again> {
        if let (GetKind::File { .. }, Some(name)) = (self.kind, &self.file.replace) {
            // Replace the file that is there? Yes gives its name; No (the default) asks for a name again.
            let yes = |t: &str| matches!(t.trim().to_ascii_lowercase().as_str(), "y" | "yes");
            let no = |t: &str| matches!(t.trim().to_ascii_lowercase().as_str(), "n" | "no");
            let again = || Again::Ask(Box::new(GetRequest { file: Box::new(FileAsk { replace: None, ..(*self.file).clone() }), ..self.clone() }));
            return match input {
                Input::Text(t) | Input::Keyword(t) if yes(&t) => Ok(Value::str(name)),
                Input::Text(t) | Input::Keyword(t) if no(&t) => Err(again()),
                Input::Enter => Err(again()),
                Input::Cancel => Err("*Cancel*".into()),
                _ => Err("Answer Yes or No.".into()),
            };
        }
        let ig = &self.initget;
        let no_null = ig.bits & 1 != 0;
        match input {
            Input::Enter => {
                if matches!(self.kind, GetKind::String { .. }) {
                    return Ok(Value::str(""));
                }
                if let GetKind::File { new } = self.kind {
                    // Enter takes the default name (checked as a typed one is), or cancels the choice
                    // (nil) without one.
                    return match self.file.complete(&self.file.default) {
                        Some(name) => file_answer(self, name, new),
                        None => Ok(Value::Nil),
                    };
                }
                if no_null {
                    return Err(match self.kind {
                        GetKind::Int => "Requires an integer value.",
                        GetKind::Real | GetKind::Dist | GetKind::Angle { .. } => "Requires numeric value.",
                        GetKind::Kword => "Invalid option keyword.",
                        _ => "Invalid point.",
                    }
                    .into());
                }
                Ok(Value::Nil)
            }
            Input::Keyword(k) => Ok(Value::str(&k)),
            Input::Point(p) => self.point(s, [p.x, p.y, 0.0]),
            // A deferred tangent/perpendicular snap is the point it was picked at here.
            Input::Deferred(d) => self.point(s, [d.at.x, d.at.y, 0.0]),
            Input::Pick(hs) => match (self.kind, hs.first()) {
                (GetKind::EntSel, Some(h)) => Ok(Value::list(vec![Value::Ename(*h), Value::point(&[s.last_point.x, s.last_point.y, 0.0])])),
                _ => Err("Invalid point.".into()),
            },
            Input::Text(t) => self.text(s, &t),
            Input::Cancel => Err("*Cancel*".into()),
        }
    }

    fn point(&self, s: &mut Session, p: [f64; 3]) -> Result<Value, Again> {
        match self.kind {
            GetKind::Point | GetKind::Corner => Ok(Value::point(&p)),
            GetKind::Dist | GetKind::Angle { .. } => {
                let Some(base) = self.first.or(self.base) else {
                    // The first of two points: ask for the second.
                    return Err(Again::Second(p));
                };
                let (dx, dy) = (p[0] - base[0], p[1] - base[1]);
                if let GetKind::Angle { orient } = self.kind {
                    let mut a = dy.atan2(dx);
                    if !orient {
                        a -= s.lisp_angbase();
                    }
                    return self.check_number(a.rem_euclid(std::f64::consts::TAU), false);
                }
                let d = if self.initget.bits & 64 != 0 { (dx * dx + dy * dy).sqrt() } else { (dx * dx + dy * dy + (p[2] - base[2]).powi(2)).sqrt() };
                self.check_number(d, false)
            }
            GetKind::EntSel => {
                let ap = s.settings.pickbox.max(1.0) * s.pixel_size();
                let space = s.state().map(|st| st.edit_space()).map_err(|e| Again::Msg(e.to_string()))?;
                let d = s.doc().map_err(|e| Again::Msg(e.to_string()))?;
                match crate::select::pick(d, &space, Vec2::new(p[0], p[1]), ap) {
                    Some(h) => Ok(Value::list(vec![Value::Ename(h), Value::point(&p)])),
                    None => Err("Missed, try again.".into()),
                }
            }
            _ => Err(match self.kind {
                GetKind::Int => "Requires an integer value.",
                GetKind::Kword => "Invalid option keyword.",
                GetKind::String { .. } => "Requires a string.",
                GetKind::File { .. } => "Requires a file name.",
                _ => "Requires numeric value.",
            }
            .into()),
        }
    }

    fn text(&self, s: &mut Session, t: &str) -> Result<Value, Again> {
        let ig = &self.initget;
        if let Some(k) = keyword(&ig.keywords, t) {
            return Ok(Value::str(&k));
        }
        let any = ig.bits & 128 != 0;
        match self.kind {
            GetKind::String { spaces } => {
                let t = if spaces { t } else { t.split(' ').next().unwrap_or("") };
                Ok(Value::str(t))
            }
            GetKind::Kword if any => Ok(Value::str(t)),
            GetKind::Kword => Err("Invalid option keyword.".into()),
            GetKind::File { new } => {
                let Some(name) = self.file.complete(t).or_else(|| self.file.complete(&self.file.default)) else { return Ok(Value::Nil) };
                file_answer(self, name, new)
            }
            GetKind::Int => match t.trim().parse::<i32>() {
                Ok(i) if (-32768..=32767).contains(&i) => self.check_number(f64::from(i), true),
                Ok(_) => Err("Value must be between -32768 and 32767.".into()),
                Err(_) if any => Ok(Value::str(t)),
                Err(_) => Err("Requires an integer value.".into()),
            },
            GetKind::Real => match t.trim().parse::<f64>() {
                Ok(v) if v.is_finite() => self.check_number(v, false),
                _ if any => Ok(Value::str(t)),
                _ => Err("Requires numeric value.".into()),
            },
            GetKind::Dist => match crate::units::parse_distance(t) {
                Some(v) => self.check_number(v, false),
                None if any => Ok(Value::str(t)),
                None => Err("Requires numeric distance or two points.".into()),
            },
            GetKind::Angle { orient } => match s.angle_settings().direction(t) {
                Some(a) => {
                    let a = if orient { a } else { a - s.lisp_angbase() };
                    self.check_number(a.rem_euclid(std::f64::consts::TAU), false)
                }
                None if any => Ok(Value::str(t)),
                None => Err("Requires valid numeric angle or second point.".into()),
            },
            GetKind::Point | GetKind::Corner | GetKind::EntSel => {
                if any {
                    Ok(Value::str(t))
                } else {
                    Err("Invalid point.".into())
                }
            }
        }
    }

    fn check_number(&self, v: f64, int: bool) -> Result<Value, Again> {
        let bits = self.initget.bits;
        if bits & 2 != 0 && v == 0.0 {
            return Err("Value must be nonzero.".into());
        }
        if bits & 4 != 0 && v < 0.0 {
            return Err("Value must be positive.".into());
        }
        Ok(if int { Value::Int(v as i32) } else { Value::Real(v) })
    }
}

/// What an input to a `get…` prompt gives.
pub enum Answer {
    Value(Value),
    /// Not an answer here: the message says why, and the prompt asks again.
    Again(String),
    /// `getdist`/`getangle` without a base point got its first point: ask for the second.
    Second([f64; 3]),
    /// The prompt goes on as this request (`getfiled`: whether to replace a file, or a name again).
    Ask(Box<GetRequest>),
}

enum Again {
    Msg(String),
    Second([f64; 3]),
    Ask(Box<GetRequest>),
}

impl From<String> for Again {
    fn from(m: String) -> Again {
        Again::Msg(m)
    }
}

impl From<&str> for Again {
    fn from(m: &str) -> Again {
        Again::Msg(m.to_string())
    }
}

fn default_message(kind: GetKind) -> &'static str {
    match kind {
        GetKind::EntSel => "Select object: ",
        _ => "",
    }
}

/// A keyword typed, matched as the command line matches them: the whole word or its capitals.
fn keyword(keywords: &[String], t: &str) -> Option<String> {
    if keywords.is_empty() {
        return None;
    }
    let mut p = Prompt::new("", Accept::TEXT);
    p.keywords = keywords.to_vec();
    p.match_keyword(t).or_else(|| t.strip_prefix('_').and_then(|r| p.match_keyword(r)))
}

impl Session {
    /// ANGBASE (radians).
    fn lisp_angbase(&self) -> f64 {
        self.doc().map(|d| d.header.f64("ANGBASE", 0.0)).unwrap_or(0.0)
    }
}

// ---------------- the builtins ----------------

/// `(initget [bits] [keywords])`.
pub fn initget(ctx: &mut Ctx<'_>, args: Vec<Value>) -> Result<Flow, LispError> {
    let mut ig = Initget::default();
    for a in &args {
        match a {
            Value::Int(b) => ig.bits = *b,
            Value::Str(k) => ig.keywords = k.split_whitespace().map(|w| w.split(',').next().unwrap_or(w).to_string()).collect(),
            Value::Nil => {}
            other => return Err(LispError::bad_arg("stringp", other)),
        }
    }
    ctx.env.initget = Some(ig);
    Ok(Flow::Value(Value::Nil))
}

/// Optional point then optional message, in that order (`getpoint`, `getdist`, `getangle`…).
fn point_and_message(args: &[Value], name: &str) -> Result<(Option<[f64; 3]>, String), LispError> {
    let (mut base, mut msg) = (None, String::new());
    for a in args {
        match a {
            Value::Str(s) => msg = s.to_string(),
            Value::Cons(_) => base = Some(a.as_point().ok_or_else(|| LispError::bad_arg("2D/3D point", a))?),
            Value::Nil => {}
            other => return Err(LispError::bad_arg(if name == "getcorner" { "2D/3D point" } else { "stringp" }, other)),
        }
    }
    if args.len() > 2 {
        return Err(args_err(false));
    }
    Ok((base, msg))
}

fn wait(ctx: &mut Ctx<'_>, kind: GetKind, message: String, base: Option<[f64; 3]>) -> Result<Flow, LispError> {
    wait_with(ctx, kind, message, base, FileAsk::default())
}

fn wait_with(ctx: &mut Ctx<'_>, kind: GetKind, message: String, base: Option<[f64; 3]>, file: FileAsk) -> Result<Flow, LispError> {
    let initget = ctx.env.initget.take().unwrap_or_default();
    // Text printed so far shows before the prompt.
    let printed = std::mem::take(&mut ctx.env.out);
    if !printed.is_empty() {
        for l in printed.split('\n').filter(|l| !l.is_empty()) {
            ctx.s.echo(l.to_string());
            ctx.env.printed_at = Some(ctx.s.log.len());
        }
    }
    Ok(Flow::Wait(Need::Get(GetRequest { kind, message, base, initget, first: None, file: Box::new(file) })))
}

/// `(getfiled title default ext flags)`: a file name, asked at the command line (no dialog opens
/// from LISP, as with FILEDIA 0). A bare name gets `ext`; Enter takes `default`, or gives nil
/// without one. Flag 1 asks for a new file (Save As), and an existing one is replaced only after
/// Yes unless flag 32 is set; otherwise the file must exist. With flag 16, or a default ending in a
/// path delimiter, the default is the folder relative names are in.
pub fn getfiled(ctx: &mut Ctx<'_>, args: Vec<Value>) -> Result<Flow, LispError> {
    let [title, default, ext, flags] = args.as_slice() else { return Err(args_err(args.len() < 4)) };
    let s = |v: &Value| match v {
        Value::Str(s) => Ok(s.to_string()),
        Value::Nil => Ok(String::new()),
        other => Err(LispError::bad_arg("stringp", other)),
    };
    let (title, default, ext) = (s(title)?, s(default)?, s(ext)?);
    let Value::Int(flags) = flags else { return Err(LispError::bad_arg("fixnump", flags)) };
    // The first of "dwg;dxf", without its dot; "*" or "" adds none.
    let ext = ext.split(';').next().unwrap_or("").trim().trim_start_matches('.').to_string();
    let ext = if ext == "*" { String::new() } else { ext };
    let title = if title.is_empty() { "Enter file name".to_string() } else { title.trim_end_matches([' ', ':']).to_string() };
    // Flag 16, or a default ending in a path delimiter: the default is a folder, not a file name.
    let (dir, default) = if flags & 16 != 0 || default.ends_with(['/', '\\']) { (default, String::new()) } else { (String::new(), default) };
    let message = if default.is_empty() { format!("{title}: ") } else { format!("{title} <{default}>: ") };
    // Flag 32: a new file may replace one that is there without asking.
    let file = FileAsk { ext, default, dir, confirm: flags & 32 == 0, replace: None };
    wait_with(ctx, GetKind::File { new: flags & 1 != 0 }, message, None, file)
}

pub fn getpoint(ctx: &mut Ctx<'_>, args: Vec<Value>) -> Result<Flow, LispError> {
    let (base, msg) = point_and_message(&args, "getpoint")?;
    wait(ctx, GetKind::Point, msg, base)
}

pub fn getcorner(ctx: &mut Ctx<'_>, args: Vec<Value>) -> Result<Flow, LispError> {
    let (base, msg) = point_and_message(&args, "getcorner")?;
    if base.is_none() {
        return Err(args_err(true));
    }
    wait(ctx, GetKind::Corner, msg, base)
}

pub fn getdist(ctx: &mut Ctx<'_>, args: Vec<Value>) -> Result<Flow, LispError> {
    let (base, msg) = point_and_message(&args, "getdist")?;
    wait(ctx, GetKind::Dist, msg, base)
}

pub fn getangle(ctx: &mut Ctx<'_>, args: Vec<Value>) -> Result<Flow, LispError> {
    let (base, msg) = point_and_message(&args, "getangle")?;
    wait(ctx, GetKind::Angle { orient: false }, msg, base)
}

pub fn getorient(ctx: &mut Ctx<'_>, args: Vec<Value>) -> Result<Flow, LispError> {
    let (base, msg) = point_and_message(&args, "getorient")?;
    wait(ctx, GetKind::Angle { orient: true }, msg, base)
}

fn message_only(args: &[Value]) -> Result<String, LispError> {
    match args {
        [] | [Value::Nil] => Ok(String::new()),
        [Value::Str(s)] => Ok(s.to_string()),
        [other] => Err(LispError::bad_arg("stringp", other)),
        _ => Err(args_err(false)),
    }
}

pub fn getreal(ctx: &mut Ctx<'_>, args: Vec<Value>) -> Result<Flow, LispError> {
    let m = message_only(&args)?;
    wait(ctx, GetKind::Real, m, None)
}

pub fn getint(ctx: &mut Ctx<'_>, args: Vec<Value>) -> Result<Flow, LispError> {
    let m = message_only(&args)?;
    wait(ctx, GetKind::Int, m, None)
}

pub fn getkword(ctx: &mut Ctx<'_>, args: Vec<Value>) -> Result<Flow, LispError> {
    let m = message_only(&args)?;
    wait(ctx, GetKind::Kword, m, None)
}

/// `(getstring [cr] [msg])`.
pub fn getstring(ctx: &mut Ctx<'_>, args: Vec<Value>) -> Result<Flow, LispError> {
    let (spaces, msg) = match args.as_slice() {
        [] => (false, String::new()),
        [Value::Str(m)] => (false, m.to_string()),
        [cr] => (!cr.is_nil(), String::new()),
        [cr, Value::Str(m)] => (!cr.is_nil(), m.to_string()),
        [_, Value::Nil] => (!args[0].is_nil(), String::new()),
        [_, other] => return Err(LispError::bad_arg("stringp", other)),
        _ => return Err(args_err(false)),
    };
    wait(ctx, GetKind::String { spaces }, msg, None)
}

/// `(entsel [msg])` and `(nentsel [msg])`.
pub fn entsel(ctx: &mut Ctx<'_>, args: Vec<Value>) -> Result<Flow, LispError> {
    let m = message_only(&args)?;
    wait(ctx, GetKind::EntSel, m, None)
}

/// A second point after the first of `getdist`/`getangle` without a base.
pub fn with_first(req: &GetRequest, p: [f64; 3]) -> GetRequest {
    GetRequest { first: Some(p), ..req.clone() }
}
