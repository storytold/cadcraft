//! WBLOCK (its JSON form) and -WBLOCK at the command line: objects, a block or the whole drawing
//! written to a drawing file.

use cadcraft_doc::{Drawing, Entity, Handle};
use cadcraft_geom::{Mat3, Vec2, Vec3};
use serde_json::{Value, json};

use super::super::curves::KW;
use super::super::machines::{SelOutcome, SelectPhase};
use super::super::{CommandSpec, bad, point_param, str_param, targets};
use super::insert_file::stem;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub(super) fn wblock_spec() -> CommandSpec {
    CommandSpec::new("wblock", "Write Block", run_wblock).alias(&["w"]).params("{path, name? | handles?, base?}")
}

/// -WBLOCK erases the objects it writes, whatever its mOde option says: AutoCAD 2024 does, and
/// keeps "Object conversion=Delete" as the setting.
pub(super) fn dash_wblock_spec() -> CommandSpec {
    CommandSpec::new("-wblock", "Write Block", run_dash_wblock)
        .params("{path, name? | handles?, base?} (the objects written are erased)")
        .interactive(|_| Ok(Box::new(DashWblockM::default())))
}

/// The web build has no file system: -WBLOCK would erase objects it can't save.
const NO_FILES: &str = "files can't be written in the web build";

/// What WBLOCK writes.
enum Source<'a> {
    /// These objects, `base` becoming the written drawing's origin.
    Objects(&'a [Handle], Vec2),
    /// A block's objects, from its base point or `base` (in block coordinates).
    Block(&'a str, Option<Vec2>),
    /// The whole drawing.
    Whole,
}

fn run_wblock(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").ok_or_else(|| bad("wblock", "`path` is required"))?.to_string();
    let base = point_param(p, "base");
    let (entities, bytes) = match str_param(p, "name") {
        Some(name) => write(s, "wblock", &path, Source::Block(name, base))?,
        None => write(s, "wblock", &path, Source::Objects(&targets(s, p)?, base.unwrap_or(Vec2::ZERO)))?,
    };
    Ok(json!({ "path": path, "entities": entities, "bytes": bytes }))
}

fn run_dash_wblock(s: &mut Session, p: &Value) -> Result<Value> {
    if cfg!(target_arch = "wasm32") {
        return Err(bad("-wblock", NO_FILES));
    }
    let path = str_param(p, "path").ok_or_else(|| bad("-wblock", "`path` is required"))?.to_string();
    let base = point_param(p, "base");
    let (entities, bytes) = match str_param(p, "name") {
        Some(name) => write(s, "-wblock", &path, Source::Block(name, base))?,
        None => {
            let hs = targets(s, p)?;
            // As at the command line, nothing selected writes no file.
            if !hs.iter().any(|h| s.doc().is_ok_and(|d| d.entity(*h).is_some())) {
                return Err(bad("-wblock", "no objects selected"));
            }
            let written = write(s, "-wblock", &path, Source::Objects(&hs, base.unwrap_or(Vec2::ZERO)))?;
            erase(s, &hs)?;
            written
        }
    };
    Ok(json!({ "path": path, "entities": entities, "bytes": bytes }))
}

/// Write `what` to `path` through the io hooks; the objects written and the file's size. The
/// written drawing keeps the source's settings and styles, as AutoCAD's -WBLOCK does: its units
/// ($INSUNITS, $MEASUREMENT), so a block from a metric drawing is not declared in inches and
/// scaled on insert, and its defaults (text size, current styles, annotation scales).
fn write(s: &Session, cmd: &str, path: &str, what: Source) -> Result<(usize, usize)> {
    let src = s.doc()?;
    // The base point becomes the written drawing's origin: by default 0,0 for objects and the
    // block's base point for a block.
    let d = match what {
        Source::Whole => src.clone(),
        Source::Block(name, base) => {
            let b = src.block(name).ok_or_else(|| bad(cmd, format!("no block `{name}`")))?;
            let m = Mat3::translate(-base.unwrap_or(b.base.xy()));
            with_objects(src, b.entities.iter().map(|e| moved(e, &m)))
        }
        Source::Objects(hs, base) => {
            let m = Mat3::translate(-base);
            with_objects(src, hs.iter().filter_map(|h| src.entity(*h)).map(|e| moved(e, &m)))
        }
    };
    let hooks = super::super::file::io().ok_or_else(|| bad(cmd, "file formats are not available in this build"))?;
    let bytes = (hooks.write)(&d, path).map_err(|e| bad(cmd, e))?;
    // Through a temporary file, as SAVE writes, so a failed write keeps the file it replaces.
    #[cfg(not(target_arch = "wasm32"))]
    crate::atomic_write::write_atomic(std::path::Path::new(path), &bytes).map_err(|e| bad(cmd, format!("{path}: {e}")))?;
    Ok((d.model.len(), bytes.len()))
}

/// A new drawing of `ents`, with `src`'s settings, styles and blocks; its insertion base 0,0.
fn with_objects(src: &Drawing, ents: impl Iterator<Item = Entity>) -> Drawing {
    let mut d = Drawing::new_imperial();
    d.header = src.header.clone();
    d.layers = src.layers.clone();
    d.linetypes = src.linetypes.clone();
    d.text_styles = src.text_styles.clone();
    d.dim_styles = src.dim_styles.clone();
    d.mleader_styles = src.mleader_styles.clone();
    d.table_styles = src.table_styles.clone();
    d.mline_styles = src.mline_styles.clone();
    d.scales = src.scales.clone();
    d.blocks = src.blocks.clone();
    d.header.set("INSBASE", cadcraft_doc::HVal::Point(Vec3::ZERO));
    for mut e in ents {
        e.handle = d.new_handle();
        d.model.push(e);
    }
    d
}

fn moved(e: &Entity, m: &Mat3) -> Entity {
    let mut e = e.clone();
    e.kind.transform(m);
    e
}

fn erase(s: &mut Session, hs: &[Handle]) -> Result<()> {
    let doc = s.doc_mut()?;
    for h in hs {
        doc.remove_entity(*h);
    }
    s.set_selection(Vec::new());
    Ok(())
}

/// The file -WBLOCK writes for a typed name: as named when it ends in .dwg or .dxf, else with
/// .dwg added, as AutoCAD names a drawing.
fn output_path(typed: &str) -> String {
    let t = typed.trim().trim_matches('"');
    let lower = t.to_ascii_lowercase();
    if lower.ends_with(".dwg") || lower.ends_with(".dxf") { t.to_string() } else { format!("{t}.dwg") }
}

/// Whether a file is there to replace; the web has no file system.
fn exists(path: &str) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::path::Path::new(path).exists()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        false
    }
}

/// Where -WBLOCK is in its prompts, as AutoCAD 2024 asks them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Ask {
    #[default]
    File,
    /// The file exists: "… already exists, do you want to replace it? [Yes/No] <N>".
    Replace,
    /// A DXF file's decimal places of accuracy.
    Precision,
    /// An existing block, `=` (the block named as the file), `*` (the whole drawing), or Enter
    /// for objects.
    Block,
    Base,
    /// mOde at the base point prompt: asked, but the objects are erased whatever it says.
    Mode,
    Objects,
}

const SAME_NAME: &str = "= (block=output file)";
const WHOLE: &str = "* (whole drawing)";

#[derive(Default)]
struct DashWblockM {
    ask: Ask,
    path: String,
    base: Vec2,
    sel: SelectPhase,
}

impl Interactive for DashWblockM {
    fn name(&self) -> &'static str {
        "WBLOCK"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        if cfg!(target_arch = "wasm32") {
            s.echo(format!("-WBLOCK: {NO_FILES}."));
            return Ok(Step::Cancel);
        }
        s.echo("Current settings: Object conversion=Delete");
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.ask {
            Ask::File => Prompt::new("Enter name of output file", Accept::TEXT),
            Ask::Replace => Prompt::new(format!("{} already exists, do you want to replace it?", self.path), KW).kw(&["Yes", "No"]).default("N"),
            Ask::Precision => Prompt::new("Enter decimal places of accuracy (0 to 16)", Accept::NUMBER).kw(&["Binary", "Preview"]).default("16"),
            Ask::Block => Prompt::new("Enter name of existing block", Accept::TEXT).kw(&[SAME_NAME, WHOLE]).default("define new drawing"),
            Ask::Base => Prompt::new("Specify insertion base point", Accept::POINT).kw(&["mOde"]),
            Ask::Mode => {
                Prompt::new("Choose mode for selected object conversion", KW).kw(&["Convert to block", "Retain", "Delete"]).default("Delete")
            }
            Ask::Objects => self.sel.prompt(),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.ask {
            Ask::File => {
                if let Input::Text(t) = &i
                    && !t.trim().trim_matches('"').is_empty()
                {
                    self.path = output_path(t);
                    self.ask = if exists(&self.path) { Ask::Replace } else { self.after_file(s) };
                }
                Ok(Step::Continue)
            }
            Ask::Replace => match &i {
                // Yes goes on; No, the default, ends the command; anything else is asked again.
                Input::Keyword(k) if k == "Yes" => {
                    self.ask = self.after_file(s);
                    Ok(Step::Continue)
                }
                Input::Keyword(_) | Input::Enter => Ok(Step::Done),
                _ => {
                    s.echo("Invalid option keyword.");
                    Ok(Step::Continue)
                }
            },
            Ask::Precision => {
                match &i {
                    Input::Enter => self.ask = Ask::Block,
                    // The file is written at full precision whatever is asked.
                    Input::Text(t) if t.trim().parse::<u8>().is_ok_and(|n| n <= 16) => self.ask = Ask::Block,
                    Input::Keyword(k) if k == "Binary" => s.echo("Binary DXF output is not available."),
                    Input::Keyword(_) => {}
                    _ => s.echo("Requires an integer between 0 and 16."),
                }
                Ok(Step::Continue)
            }
            Ask::Block => {
                let name = match &i {
                    Input::Enter => {
                        self.ask = Ask::Base;
                        return Ok(Step::Continue);
                    }
                    Input::Keyword(k) if k == WHOLE => return self.write(s, Source::Whole),
                    Input::Keyword(k) if k == SAME_NAME => stem(&self.path),
                    Input::Text(t) => t.trim().to_string(),
                    // A point (from a LISP list) is a name too: AutoCAD answers Block "0,0" not found.
                    Input::Point(p) => format!("{},{}", p.x, p.y),
                    _ => return Ok(Step::Continue),
                };
                if s.doc()?.block(&name).is_none() {
                    s.echo(format!("Block \"{name}\" not found."));
                    return Ok(Step::Continue);
                }
                self.write(s, Source::Block(&name, None))
            }
            Ask::Base => {
                match i {
                    Input::Keyword(_) => self.ask = Ask::Mode,
                    Input::Enter | Input::Text(_) => s.echo("Point or option keyword required."),
                    Input::Point(p) => {
                        self.base = p;
                        self.sel = SelectPhase::begin(s);
                        if self.sel.done {
                            let picked = self.sel.picked.clone();
                            return self.write_objects(s, &picked);
                        }
                        self.ask = Ask::Objects;
                    }
                    _ => {}
                }
                Ok(Step::Continue)
            }
            Ask::Mode => {
                // Any answer, Enter included, goes back to the base point.
                self.ask = Ask::Base;
                Ok(Step::Continue)
            }
            Ask::Objects => match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                // Nothing selected: nothing is written.
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => self.write_objects(s, &hs),
            },
        }
    }
}

impl DashWblockM {
    /// The prompt after the file name: a DXF file's precision, else the block.
    fn after_file(&self, s: &mut Session) -> Ask {
        if self.path.to_ascii_lowercase().ends_with(".dxf") {
            s.echo("Current DXF settings: Precision= 16 Format= ASCII Preview= No");
            Ask::Precision
        } else {
            Ask::Block
        }
    }

    fn write(&self, s: &mut Session, what: Source) -> Result<Step> {
        if let Err(e) = write(s, "-wblock", &self.path, what) {
            s.echo(e.to_string());
        }
        Ok(Step::Done)
    }

    fn write_objects(&self, s: &mut Session, hs: &[Handle]) -> Result<Step> {
        if hs.is_empty() {
            return Ok(Step::Done);
        }
        match write(s, "-wblock", &self.path, Source::Objects(hs, self.base)) {
            Ok(_) => erase(s, hs)?,
            Err(e) => s.echo(e.to_string()),
        }
        Ok(Step::Done)
    }
}
