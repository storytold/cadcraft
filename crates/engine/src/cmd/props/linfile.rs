//! Linetypes from the user's own `.lin` files: LINETYPE `{load, file | text}` and the
//! command-line -LINETYPE (`?`, `Load` from the built-in library or a file, `Set`).

use cadcraft_doc::library::files::parse_lin;
use cadcraft_doc::{Drawing, Linetype};
use serde_json::{Value, json};

use super::super::helpers::support_text;
use super::super::qselect::wildcard;
use super::{Result, Session, bad, bool_or, str_param};

use crate::{Accept, Input, Interactive, Prompt, Step};

/// Names no file may define: they are not linetype definitions.
const RESERVED: &[&str] = &["bylayer", "byblock", "continuous"];

/// Whether a LINETYPE call names a file (`file`) or gives its contents (`text`).
pub(super) fn has_file(p: &Value) -> bool {
    p.get("file").is_some_and(|v| !v.is_null()) || p.get("text").is_some_and(|v| !v.is_null())
}

/// What loading from a file did.
#[derive(Default)]
struct Loaded {
    added: Vec<String>,
    reloaded: Vec<String>,
    /// Already in the drawing and not reloaded.
    kept: Vec<String>,
    warnings: Vec<String>,
}

/// Whether `name` is one of the comma-separated names or wildcards in `names`.
fn wanted(names: &str, name: &str) -> bool {
    names.split(',').map(str::trim).filter(|n| !n.is_empty()).any(|n| wildcard(n, name))
}

/// The linetypes of `text` that `names` asks for, without the reserved names.
fn read(text: &str, names: &str, out: &mut Loaded) -> Vec<Linetype> {
    let parsed = parse_lin(text);
    out.warnings.extend(parsed.warnings);
    let mut keep = Vec::new();
    for lt in parsed.items.into_iter().filter(|l| wanted(names, &l.name)) {
        if RESERVED.contains(&lt.name.to_ascii_lowercase().as_str()) {
            out.warnings.push(format!("{} is a reserved name; not loaded", lt.name));
        } else {
            keep.push(lt);
        }
    }
    keep
}

/// Put `lt` into `d`: added, or replacing the definition of the same name when `reload`.
fn put(d: &mut Drawing, lt: Linetype, reload: bool, out: &mut Loaded) {
    for e in &lt.pattern {
        if let Some(st) = e.style.as_deref()
            && d.text_style(st).is_none()
        {
            out.warnings.push(format!("{}: text style {st} is not in this drawing; its text uses the default style", lt.name));
            break;
        }
    }
    match d.linetypes.iter_mut().find(|l| l.name.eq_ignore_ascii_case(&lt.name)) {
        Some(old) if reload => {
            out.reloaded.push(old.name.clone());
            // The drawing keeps the name as it had it (objects refer to it).
            *old = Linetype { name: old.name.clone(), ..lt };
        }
        Some(old) => out.kept.push(old.name.clone()),
        None => {
            out.added.push(lt.name.clone());
            d.linetypes.push(lt);
        }
    }
}

/// LINETYPE `{load?: names | "*", file?: path, text?: contents, reload?, current?}`: load the
/// named linetypes (comma-separated, wildcards allowed; all by default) from a `.lin` file.
pub(super) fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let (text, source) = support_text("linetype", p, "file", "text")?.ok_or_else(|| bad("linetype", "`file` or `text` is required"))?;
    let names = str_param(p, "load").unwrap_or("*");
    let mut out = Loaded::default();
    let found = read(&text, names, &mut out);
    if found.is_empty() {
        return Err(bad("linetype", format!("no linetype matching `{names}` in {source}")));
    }
    let reload = bool_or(p, "reload", false);
    let d = s.doc_mut()?;
    for lt in found {
        put(d, lt, reload, &mut out);
    }
    // Setting the current linetype and the listing are LINETYPE's own.
    let mut rest = p.clone();
    if let Some(o) = rest.as_object_mut() {
        for k in ["file", "text", "load", "reload"] {
            o.remove(k);
        }
    }
    let mut r = super::run_linetype(s, &rest)?;
    if let Some(o) = r.as_object_mut() {
        o.insert("added".into(), json!(out.added));
        o.insert("reloaded".into(), json!(out.reloaded));
        o.insert("alreadyLoaded".into(), json!(out.kept));
        o.insert("warnings".into(), json!(out.warnings));
    }
    Ok(r)
}

// ---------- -LINETYPE ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    Option,
    LoadNames,
    LoadFile,
    Reload,
    Set,
}

/// -LINETYPE at the command line.
pub(super) struct DashLinetype {
    ask: Ask,
    names: String,
    /// Linetypes from the file that are already loaded, asked about one at a time.
    reloads: Vec<Linetype>,
}

impl DashLinetype {
    pub(super) fn new() -> Self {
        DashLinetype { ask: Ask::Option, names: String::new(), reloads: Vec::new() }
    }

    fn list(s: &mut Session) {
        let names: Vec<String> =
            s.doc().map(|d| d.linetypes.iter().map(|l| format!("{:<20} {}", l.name, l.description)).collect()).unwrap_or_default();
        s.echo("Loaded linetypes:");
        for n in names {
            s.echo(n);
        }
    }

    /// Load the asked-for names from the built-in library (`path` empty) or a file.
    fn load(&mut self, s: &mut Session, path: &str) -> Result<()> {
        if path.is_empty() {
            for name in self.names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                let before = s.doc()?.linetypes.len();
                match super::run_linetype(s, &json!({ "load": name })) {
                    Ok(_) if s.doc()?.linetypes.len() > before => s.echo(format!("Linetype(s) {name} loaded.")),
                    Ok(_) => s.echo(format!("Linetype {name} is already loaded.")),
                    Err(e) => s.echo(e.to_string()),
                }
            }
            self.ask = Ask::Option;
            return Ok(());
        }
        let text = match support_text("linetype", &json!({ "file": path }), "file", "text") {
            Ok(Some((text, _))) => text,
            Ok(None) => String::new(),
            Err(e) => {
                s.echo(e.to_string());
                self.ask = Ask::LoadFile;
                return Ok(());
            }
        };
        let mut out = Loaded::default();
        let found = read(&text, &self.names, &mut out);
        for w in out.warnings.drain(..) {
            s.echo(w);
        }
        if found.is_empty() {
            s.echo(format!("No linetype matching {} in {path}.", self.names));
        }
        let d = s.doc_mut()?;
        for lt in found {
            if d.linetype(&lt.name).is_some() {
                self.reloads.push(lt);
            } else {
                put(d, lt, false, &mut out);
            }
        }
        for w in out.warnings.drain(..) {
            s.echo(w);
        }
        for n in &out.added {
            s.echo(format!("Linetype {n} loaded."));
        }
        // `reloads` is asked from its end: reversed, the questions follow the file's order.
        self.reloads.reverse();
        self.ask = if self.reloads.is_empty() { Ask::Option } else { Ask::Reload };
        Ok(())
    }
}

impl Interactive for DashLinetype {
    fn name(&self) -> &'static str {
        "-LINETYPE"
    }

    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let cur = s.doc()?.header.str("CELTYPE", "ByLayer");
        s.echo(format!("Current linetype: \"{cur}\""));
        Ok(Step::Continue)
    }

    fn prompt(&self, s: &Session) -> Prompt {
        match self.ask {
            Ask::Option => Prompt::new("Enter an option", Accept::TEXT).kw(&["?", "Load", "Set"]),
            Ask::LoadNames => Prompt::new("Enter linetype(s) to load", Accept::TEXT),
            Ask::LoadFile => Prompt::new("Enter name of linetype file", Accept::TEXT).default("built-in library"),
            Ask::Reload => {
                let name = self.reloads.last().map(|l| l.name.as_str()).unwrap_or("");
                Prompt::new(format!("Linetype {name} is already loaded. Reload it?"), Accept::TEXT).kw(&["Yes", "No"]).default("Yes")
            }
            Ask::Set => {
                let cur = s.doc().map(|d| d.header.str("CELTYPE", "ByLayer")).unwrap_or_default();
                Prompt::new("Specify linetype name", Accept::TEXT).kw(&["?"]).default(cur)
            }
        }
    }

    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let t = match &i {
            Input::Cancel => return Ok(Step::Cancel),
            Input::Text(t) | Input::Keyword(t) => t.trim().to_string(),
            Input::Enter => String::new(),
            _ => return Ok(Step::Continue),
        };
        match self.ask {
            Ask::Option => match t.to_ascii_lowercase().as_str() {
                "" => return Ok(Step::Done),
                "?" => Self::list(s),
                "load" | "l" => self.ask = Ask::LoadNames,
                "set" | "s" => self.ask = Ask::Set,
                _ => s.echo("Invalid option keyword."),
            },
            Ask::LoadNames if t.is_empty() => self.ask = Ask::Option,
            Ask::LoadNames => {
                self.names = t;
                self.ask = Ask::LoadFile;
            }
            Ask::LoadFile => self.load(s, &t)?,
            Ask::Reload => {
                let yes = !t.to_ascii_lowercase().starts_with('n');
                if let Some(lt) = self.reloads.pop() {
                    let name = lt.name.clone();
                    if yes {
                        let mut out = Loaded::default();
                        put(s.doc_mut()?, lt, true, &mut out);
                        s.echo(format!("Linetype {name} reloaded."));
                    }
                }
                if self.reloads.is_empty() {
                    self.ask = Ask::Option;
                }
            }
            Ask::Set if t == "?" => Self::list(s),
            Ask::Set if t.is_empty() => self.ask = Ask::Option,
            Ask::Set => {
                if let Err(e) = super::run_linetype(s, &json!({ "current": t })) {
                    s.echo(e.to_string());
                }
                self.ask = Ask::Option;
            }
        }
        Ok(Step::Continue)
    }
}
