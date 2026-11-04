//! The corpus harness: every DXF and DWG file of a folder is opened, saved as DXF and as DWG,
//! reopened and compared with the drawing first opened; the findings go to a Markdown and a JSON
//! report. A file that fails, or even makes CADCraft panic, is a reported failure: each stage runs
//! under `catch_unwind` here, as a last resort of the harness only.

use std::collections::BTreeMap;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::time::Instant;

use cadcraft_doc::Drawing;
use serde::Serialize;

use super::{Category, Census, Change, Comparison, Options, Tally, census, census_of, clip, compare, compare_census};

/// A drawing file format the harness checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Dxf,
    Dwg,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Dxf => "dxf",
            Format::Dwg => "dwg",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Format::Dxf => "DXF",
            Format::Dwg => "DWG",
        }
    }
    /// The format of a file's bytes: DWG by its magic, otherwise DXF.
    pub fn of(bytes: &[u8]) -> Format {
        if cadcraft_dwg::is_dwg(bytes) { Format::Dwg } else { Format::Dxf }
    }
    fn other(self) -> Format {
        match self {
            Format::Dxf => Format::Dwg,
            Format::Dwg => Format::Dxf,
        }
    }
}

/// How the harness runs.
#[derive(Clone, Debug, PartialEq)]
pub struct CorpusOptions {
    pub compare: Options,
    /// Also save in the other format (DWG → DXF, DXF → DWG).
    pub cross_format: bool,
    /// Count what each file holds, to report what opening it leaves out.
    pub census: bool,
    /// Keep the saved files in this folder (and reopen them from there).
    pub keep_saved: Option<PathBuf>,
}

impl Default for CorpusOptions {
    fn default() -> Self {
        CorpusOptions { compare: Options::default(), cross_format: true, census: true, keep_saved: None }
    }
}

/// Largest file the harness opens.
pub const MAX_FILE_BYTES: u64 = 1 << 30;
/// Deepest folder nesting walked.
const MAX_DEPTH: usize = 32;
/// Most files taken from one corpus.
const MAX_FILES: usize = 100_000;

/// One save → reopen of a drawing.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteReport {
    /// `dwg → dxf`.
    pub route: String,
    pub saved_bytes: usize,
    /// Where the saved file was kept, when it was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_path: Option<String>,
    pub save_error: Option<String>,
    pub reopen_error: Option<String>,
    /// What reopening reported (records the reader couldn't place or decode).
    pub reopen_notes: Vec<String>,
    /// What the writer itself left out of the saved file: records per DXF type (upper case) it
    /// can't write in that format (for DWG: other programs' custom entities and objects). Each
    /// is lost; the reopened drawing's entity counts show the same entities missing, and a strict
    /// corpus run fails on any.
    pub not_written: BTreeMap<String, usize>,
    /// The reopened drawing against the drawing first opened.
    pub comparison: Comparison,
    pub seconds: f64,
}

impl RouteReport {
    pub fn ok(&self) -> bool {
        self.save_error.is_none() && self.reopen_error.is_none()
    }
    /// Saved and reopened with no difference at all.
    pub fn lossless(&self) -> bool {
        self.ok() && self.not_written.is_empty() && self.comparison.is_empty()
    }
    /// How many records the writer left out ([`Self::not_written`]).
    pub fn not_written_total(&self) -> usize {
        self.not_written.values().fold(0usize, |a, n| a.saturating_add(*n))
    }
}

/// `2 ACME_WIDGET, 1 OLE2FRAME`.
fn per_type(m: &BTreeMap<String, usize>) -> String {
    m.iter().map(|(t, n)| format!("{n} {t}")).collect::<Vec<_>>().join(", ")
}

/// Everything found for one file.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileReport {
    /// The path relative to the corpus folder.
    pub path: String,
    pub format: Option<Format>,
    /// `AC1032` for a DWG, the `$ACADVER` of a DXF.
    pub version: String,
    pub bytes: u64,
    pub open_error: Option<String>,
    /// What opening reported (a DWG's records the reader skipped, damaged records).
    pub open_notes: Vec<String>,
    pub open_seconds: f64,
    /// What the file holds (for a DWG: its DXF rendition), when counted.
    pub file_census: Option<Census>,
    /// What the drawing read from it holds.
    pub drawing_census: Option<Census>,
    /// What the drawing holds less or more than the file.
    pub read: Comparison,
    pub routes: Vec<RouteReport>,
}

impl FileReport {
    pub fn opened(&self) -> bool {
        self.open_error.is_none()
    }
}

impl CorpusReport {
    /// What makes a strict corpus run fail, one line each: a file that didn't open, a round trip
    /// that didn't save or reopen, and records a writer left out ([`RouteReport::not_written`]).
    /// Other differences are reported, not failed on.
    pub fn strict_failures(&self) -> Vec<String> {
        let mut out = Vec::new();
        for f in &self.files {
            if !f.opened() {
                out.push(format!("{}: failed to open", f.path));
            }
            for r in &f.routes {
                if !r.ok() {
                    out.push(format!("{} ({}): failed to save or reopen", f.path, r.route));
                }
                if !r.not_written.is_empty() {
                    out.push(format!("{} ({}): lost on save, not written: {}", f.path, r.route, per_type(&r.not_written)));
                }
            }
        }
        out
    }
}

/// The whole corpus run.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorpusReport {
    pub corpus: String,
    /// UTC, `2026-10-21 09:30`.
    pub generated: String,
    pub cadcraft_version: String,
    pub abs_tol: f64,
    pub rel_tol: f64,
    pub files: Vec<FileReport>,
}

/// Run `f`, turning a panic into an error message.
fn guard<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    std::panic::catch_unwind(AssertUnwindSafe(f)).map_err(|p| {
        let msg =
            p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "no message".into());
        format!("CRASH (panic): {msg}")
    })
}

/// A drawing's notes on what reading it left out.
fn notes(d: &Drawing) -> Vec<String> {
    let mut v = d.read_notes.clone();
    for (section, n) in &d.skipped_records {
        v.push(format!("{n} record(s) of the {section} section couldn't be placed and were skipped"));
    }
    v
}

/// Save `d` as `to`, reopen it and compare. `keep` is the file to keep the saved bytes in (and
/// reopen them from).
pub fn round_trip(d: &Drawing, from: Format, to: Format, opts: &Options, keep: Option<&Path>) -> RouteReport {
    let start = Instant::now();
    let mut r = RouteReport { route: format!("{} → {}", from.ext(), to.ext()), ..RouteReport::default() };
    let name = format!("roundtrip.{}", to.ext());
    let saved = match guard(|| crate::write_noted(d, &name, &crate::SaveOptions::default())) {
        Ok(Ok((b, not_written))) => {
            r.not_written = not_written;
            b
        }
        Ok(Err(e)) => {
            r.save_error = Some(e.to_string());
            r.seconds = start.elapsed().as_secs_f64();
            return r;
        }
        Err(e) => {
            r.save_error = Some(e);
            r.seconds = start.elapsed().as_secs_f64();
            return r;
        }
    };
    r.saved_bytes = saved.len();
    let mut bytes = saved;
    if let Some(path) = keep {
        let kept = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(path, &bytes)).and_then(|_| std::fs::read(path));
        match kept {
            Ok(b) => {
                bytes = b;
                r.saved_path = Some(path.display().to_string());
            }
            Err(e) => r.reopen_notes.push(format!("couldn't keep the saved file at {}: {e}", path.display())),
        }
    }
    let reopened = match guard(|| crate::read(&bytes, &name)) {
        Ok(Ok(d2)) => d2,
        Ok(Err(e)) => {
            r.reopen_error = Some(e.to_string());
            r.seconds = start.elapsed().as_secs_f64();
            return r;
        }
        Err(e) => {
            r.reopen_error = Some(e);
            r.seconds = start.elapsed().as_secs_f64();
            return r;
        }
    };
    r.reopen_notes.extend(notes(&reopened));
    match guard(|| compare(d, &reopened, opts)) {
        Ok(c) => r.comparison = c,
        Err(e) => r.reopen_error = Some(format!("comparing: {e}")),
    }
    attribute_unwritten(&mut r, to);
    r.seconds = start.elapsed().as_secs_f64();
    r
}

/// Say on the lost entity rows of a type the writer couldn't write that this is why they are
/// lost, so a writer's gap reads as one and isn't taken for a reader's or a comparison's.
fn attribute_unwritten(r: &mut RouteReport, to: Format) {
    if r.not_written.is_empty() {
        return;
    }
    for d in r.comparison.differences.iter_mut().filter(|d| d.category == Category::Count && d.change == Change::Lost) {
        if let Some(t) = r.not_written.keys().find(|t| d.detail.starts_with(&format!("{t}: "))) {
            d.detail.push_str(&format!(" (not written: the {} writer can't write {t})", to.label()));
        }
    }
}

/// The kept saved file of `rel` saved as `to`, in `dir`.
fn kept_path(dir: &Path, rel: &str, to: Format) -> PathBuf {
    let flat: String = rel.chars().map(|c| if matches!(c, '/' | '\\' | ':') { '_' } else { c }).collect();
    dir.join(format!("{flat}.roundtrip.{}", to.ext()))
}

/// Check one file's bytes: open, count, save in each format, reopen, compare. `rel` names it in
/// the report.
pub fn check_bytes(bytes: &[u8], rel: &str, opts: &CorpusOptions) -> FileReport {
    let format = Format::of(bytes);
    let mut f = FileReport { path: rel.to_string(), format: Some(format), bytes: bytes.len() as u64, ..FileReport::default() };
    if format == Format::Dwg {
        f.version = cadcraft_dwg::version(bytes).unwrap_or_default();
    }
    let start = Instant::now();
    let name = format!("source.{}", format.ext());
    let opened = guard(|| crate::read_source(bytes, std::path::Path::new(&name)));
    f.open_seconds = start.elapsed().as_secs_f64();
    let (d, dxf) = match opened {
        Ok(Ok(x)) => x,
        Ok(Err(e)) => {
            f.open_error = Some(e.to_string());
            return f;
        }
        Err(e) => {
            f.open_error = Some(e);
            return f;
        }
    };
    if format == Format::Dxf {
        f.version = d.header.str("ACADVER", "");
    }
    f.open_notes = notes(&d);
    f.drawing_census = guard(|| census_of(&d)).ok();
    if opts.census {
        let source = dxf.as_deref().unwrap_or(bytes);
        match guard(|| census(source)) {
            Ok(Some(c)) => {
                if let Ok(read) = guard(|| compare_census(&c, &d, &opts.compare)) {
                    f.read = read;
                }
                f.file_census = Some(c);
            }
            Ok(None) => f.open_notes.push("the file's groups couldn't be counted".into()),
            Err(e) => f.open_notes.push(format!("counting the file's groups: {e}")),
        }
    }
    let mut targets = vec![format];
    if opts.cross_format {
        targets.push(format.other());
    }
    for to in targets {
        let keep = opts.keep_saved.as_deref().map(|dir| kept_path(dir, rel, to));
        f.routes.push(round_trip(&d, format, to, &opts.compare, keep.as_deref()));
    }
    f
}

/// Check one file on disk; `rel` names it in the report.
pub fn check_file(path: &Path, rel: &str, opts: &CorpusOptions) -> FileReport {
    let fail = |e: String| FileReport { path: rel.to_string(), open_error: Some(e), ..FileReport::default() };
    match std::fs::metadata(path) {
        Ok(m) if m.len() > MAX_FILE_BYTES => return fail(format!("the file is {} bytes, more than the {MAX_FILE_BYTES} the harness opens", m.len())),
        Ok(_) => {}
        Err(e) => return fail(format!("can't read the file: {e}")),
    }
    match std::fs::read(path) {
        Ok(bytes) => check_bytes(&bytes, rel, opts),
        Err(e) => fail(format!("can't read the file: {e}")),
    }
}

/// Whether a path names a DXF or DWG file (by extension).
pub fn is_drawing_file(path: &Path) -> bool {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).is_some_and(|e| e == "dxf" || e == "dwg")
}

/// Every DXF and DWG file under `dir`, sorted, leaving out hidden folders and the folders in
/// `skip` (the report folder). Symbolic links are not followed.
pub fn corpus_files(dir: &Path, skip: &[PathBuf]) -> Vec<PathBuf> {
    walk(dir, skip).files
}

/// What [`walk`] found, and what it couldn't look at, so a report never passes for a whole one.
#[derive(Debug, Default)]
pub struct Walk {
    pub files: Vec<PathBuf>,
    /// Folders and entries that couldn't be read, with why.
    pub unreadable: Vec<String>,
    /// Files past [`MAX_FILES`] or folders past the depth limit were left out.
    pub truncated: bool,
}

/// [`corpus_files`], also saying what it had to leave out.
pub fn walk(dir: &Path, skip: &[PathBuf]) -> Walk {
    let skip: Vec<PathBuf> = skip.iter().map(|p| p.canonicalize().unwrap_or_else(|_| p.clone())).collect();
    let mut w = Walk::default();
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        let entries = match std::fs::read_dir(&d) {
            Ok(entries) => entries,
            Err(e) => {
                w.unreadable.push(format!("{}: {e}", d.display()));
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    w.unreadable.push(format!("{}: {e}", d.display()));
                    continue;
                }
            };
            let path = entry.path();
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(e) => {
                    w.unreadable.push(format!("{}: {e}", path.display()));
                    continue;
                }
            };
            if ft.is_dir() {
                let hidden = entry.file_name().to_string_lossy().starts_with('.');
                let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
                if hidden || skip.contains(&canon) {
                    continue;
                }
                if depth < MAX_DEPTH {
                    stack.push((path, depth + 1));
                } else {
                    w.truncated = true;
                }
            } else if ft.is_file() && is_drawing_file(&path) {
                if w.files.len() < MAX_FILES {
                    w.files.push(path);
                } else {
                    w.truncated = true;
                }
            }
        }
    }
    w.files.sort();
    w.unreadable.sort();
    w
}

/// `path` relative to `root`, with forward slashes.
pub fn relative(path: &Path, root: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

/// Check `files` (paths under `root`, which names the corpus in the report), calling `progress`
/// with (index, count, relative path) before each.
pub fn run(root: &Path, files: &[PathBuf], opts: &CorpusOptions, progress: &mut dyn FnMut(usize, usize, &str)) -> CorpusReport {
    let mut report = CorpusReport {
        corpus: root.display().to_string(),
        generated: now_utc(),
        cadcraft_version: env!("CARGO_PKG_VERSION").to_string(),
        abs_tol: opts.compare.abs_tol,
        rel_tol: opts.compare.rel_tol,
        files: Vec::new(),
    };
    for (i, path) in files.iter().enumerate() {
        let rel = relative(path, root);
        progress(i, files.len(), &rel);
        let f = match guard(|| check_file(path, &rel, opts)) {
            Ok(f) => f,
            Err(e) => FileReport { path: rel, open_error: Some(e), ..FileReport::default() },
        };
        report.files.push(f);
    }
    report
}

/// The current time, UTC, as `2026-10-21 09:30`.
fn now_utc() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let (h, m) = ((secs % 86_400) / 3600, (secs % 3600) / 60);
    // Days to a civil date (proleptic Gregorian), after Howard Hinnant's public-domain algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {h:02}:{m:02} UTC")
}

// ---------------------------------------------------------------------------------------------
// Rendering

/// Most differences listed per round trip in the Markdown report (all are in the JSON).
const MD_ROWS: usize = 300;

/// Text for a Markdown table cell.
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\r', '\n'], " ")
}

fn size(n: u64) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / f64::from(1u32 << 20))
    } else if n >= 1 << 10 {
        format!("{:.0} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

fn status(err: &Option<String>) -> String {
    match err {
        None => "ok".into(),
        Some(e) if e.starts_with("CRASH") => "**CRASH**".into(),
        Some(_) => "**failed**".into(),
    }
}

fn tally_cells(t: Tally) -> String {
    format!("{} | {} | {}", t.lost, t.added, t.changed)
}

impl CorpusReport {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
    }

    /// Totals per category over every round trip.
    pub fn totals(&self) -> std::collections::BTreeMap<Category, Tally> {
        let mut all: std::collections::BTreeMap<Category, Tally> = std::collections::BTreeMap::new();
        for r in self.files.iter().flat_map(|f| f.routes.iter()) {
            for (c, t) in &r.comparison.totals {
                let slot = all.entry(*c).or_default();
                slot.add(Change::Lost, t.lost);
                slot.add(Change::Added, t.added);
                slot.add(Change::Changed, t.changed);
            }
        }
        all
    }

    pub fn to_markdown(&self) -> String {
        let mut s = String::new();
        let opened = self.files.iter().filter(|f| f.opened()).count();
        let routes: Vec<&RouteReport> = self.files.iter().flat_map(|f| f.routes.iter()).collect();
        let save_failed = routes.iter().filter(|r| r.save_error.is_some()).count();
        let reopen_failed = routes.iter().filter(|r| r.reopen_error.is_some()).count();
        let lossless = routes.iter().filter(|r| r.lossless()).count();
        let crashes = self.files.iter().filter(|f| f.open_error.as_deref().is_some_and(|e| e.starts_with("CRASH"))).count()
            + routes.iter().filter(|r| [&r.save_error, &r.reopen_error].iter().any(|e| e.as_deref().is_some_and(|e| e.contains("CRASH")))).count();
        s.push_str("# CADCraft round-trip report\n\n");
        s.push_str(&format!("- Corpus: `{}`\n", self.corpus));
        s.push_str(&format!("- Generated: {} by CADCraft {}\n", self.generated, self.cadcraft_version));
        s.push_str(&format!("- Tolerance for numbers: {:e} absolute + {:e} relative\n", self.abs_tol, self.rel_tol));
        s.push_str(&format!(
            "- Files: {} ({opened} opened, {} failed to open); round trips: {} ({save_failed} failed to save, {reopen_failed} failed to reopen, {lossless} without any difference); crashes: {crashes}\n",
            self.files.len(),
            self.files.len() - opened,
            routes.len()
        ));
        let mut unwritten: BTreeMap<String, usize> = BTreeMap::new();
        for r in &routes {
            for (t, n) in &r.not_written {
                let slot = unwritten.entry(format!("{t} ({})", r.route)).or_insert(0);
                *slot = slot.saturating_add(*n);
            }
        }
        if unwritten.is_empty() {
            s.push_str("- Lost on save (records a writer can't write): none\n\n");
        } else {
            s.push_str(&format!("- **Lost on save** (records a writer can't write): {}\n\n", per_type(&unwritten)));
        }
        s.push_str(
            "Each file is opened, saved in its own format and in the other one, and the saved file is reopened and compared with \
             the drawing first opened. *Read* counts what the file holds that the drawing read from it doesn't (for a DWG: its \
             DXF rendition). Lost/added/changed count items (entities, records, values).\n\n",
        );
        s.push_str("## Summary\n\n");
        s.push_str("| File | Format | Size | Open | Read lost | Round trip | Save | Reopen | Not written | Lost | Added | Changed |\n");
        s.push_str("|---|---|---|---|---|---|---|---|---|---|---|---|\n");
        for f in &self.files {
            let fmt = f.format.map_or("?".to_string(), |x| format!("{} {}", x.label(), f.version).trim().to_string());
            let open = if f.opened() { format!("ok ({:.1} s)", f.open_seconds) } else { status(&f.open_error) };
            let lead = format!("| `{}` | {} | {} | {} | {} ", cell(&f.path), cell(&fmt), size(f.bytes), open, f.read.tally(Category::Read).lost);
            if f.routes.is_empty() {
                s.push_str(&format!("{lead}| — | — | — | — | — | — | — |\n"));
            }
            for (i, r) in f.routes.iter().enumerate() {
                let head = if i == 0 { lead.clone() } else { "| | | | | ".to_string() };
                let t = r.comparison.total();
                let reopen = if r.save_error.is_some() { "—".to_string() } else { status(&r.reopen_error) };
                let unwritten = if r.not_written.is_empty() { "0".to_string() } else { format!("**{}**", r.not_written_total()) };
                s.push_str(&format!("{head}| {} | {} | {} | {unwritten} | {} |\n", r.route, status(&r.save_error), reopen, tally_cells(t)));
            }
        }
        s.push_str("\n## By category (all round trips)\n\n| Category | Lost | Added | Changed |\n|---|---|---|---|\n");
        let totals = self.totals();
        for c in Category::ALL {
            if let Some(t) = totals.get(&c).filter(|t| t.total() > 0) {
                s.push_str(&format!("| {} | {} |\n", c.label(), tally_cells(*t)));
            }
        }
        if totals.values().all(|t| t.total() == 0) {
            s.push_str("| (no differences) | 0 | 0 | 0 |\n");
        }
        s.push_str("\n## Files\n");
        for f in &self.files {
            self.file_markdown(f, &mut s);
        }
        s
    }

    fn file_markdown(&self, f: &FileReport, s: &mut String) {
        s.push_str(&format!("\n### `{}`\n\n", f.path));
        let fmt = f.format.map_or("unknown format".to_string(), |x| format!("{} {}", x.label(), f.version).trim().to_string());
        s.push_str(&format!("- {fmt}, {}\n", size(f.bytes)));
        match &f.open_error {
            Some(e) => {
                s.push_str(&format!("- Open: {}: {}\n", status(&f.open_error), cell(e)));
                return;
            }
            None => {
                let what = f.drawing_census.as_ref().map_or(String::new(), |c| {
                    let n: usize = c.entities.values().sum();
                    let nb: usize = c.block_entities.values().sum();
                    format!(
                        ": {n} entities in model and paper space, {} blocks holding {nb} entities, {} layers, {} unmodelled objects kept",
                        c.blocks,
                        c.tables.get("LAYER").copied().unwrap_or(0),
                        c.objects.values().sum::<usize>()
                    )
                });
                s.push_str(&format!("- Open: ok in {:.1} s{what}\n", f.open_seconds));
            }
        }
        for n in &f.open_notes {
            s.push_str(&format!("- Note on opening: {}\n", cell(n)));
        }
        if let Some(c) = &f.file_census
            && !c.objects.is_empty()
        {
            let kept = f.drawing_census.as_ref().map(|d| &d.objects);
            let list: Vec<String> = c
                .objects
                .iter()
                .map(|(k, n)| match kept.and_then(|m| m.get(k)) {
                    Some(m) => format!("{k} {n} ({m} kept as unmodelled)"),
                    None => format!("{k} {n}"),
                })
                .collect();
            s.push_str(&format!("- Objects in the file: {}\n", cell(&list.join(", "))));
        }
        if !f.read.is_empty() {
            s.push_str("\n#### Read (file → drawing)\n\n");
            diff_table(&f.read, s);
        }
        for r in &f.routes {
            s.push_str(&format!("\n#### {}\n\n", r.route));
            match (&r.save_error, &r.reopen_error) {
                (Some(e), _) => s.push_str(&format!("- Save: {}: {}\n", status(&r.save_error), cell(e))),
                (None, Some(e)) => {
                    s.push_str(&format!("- Save: ok ({})\n", size(r.saved_bytes as u64)));
                    s.push_str(&format!("- Reopen: {}: {}\n", status(&r.reopen_error), cell(e)));
                }
                (None, None) => {
                    let t = r.comparison.total();
                    s.push_str(&format!(
                        "- Saved {} and reopened in {:.1} s: {} lost, {} added, {} changed\n",
                        size(r.saved_bytes as u64),
                        r.seconds,
                        t.lost,
                        t.added,
                        t.changed
                    ));
                }
            }
            if !r.not_written.is_empty() {
                s.push_str(&format!("- **Lost on save**, not written (the writer can't write these types): {}\n", per_type(&r.not_written)));
            }
            if let Some(p) = &r.saved_path {
                s.push_str(&format!("- Saved file: `{p}`\n"));
            }
            for n in &r.reopen_notes {
                s.push_str(&format!("- Note on reopening: {}\n", cell(n)));
            }
            if !r.comparison.is_empty() {
                s.push('\n');
                diff_table(&r.comparison, s);
            }
        }
    }

    /// Write `report.md` and `report.json` into `dir` (created if needed); returns their paths.
    pub fn write(&self, dir: &Path) -> std::io::Result<(PathBuf, PathBuf)> {
        std::fs::create_dir_all(dir)?;
        let (md, json) = (dir.join("report.md"), dir.join("report.json"));
        std::fs::write(&md, self.to_markdown())?;
        std::fs::write(&json, self.to_json())?;
        Ok((md, json))
    }
}

/// A comparison's differences as a Markdown table (the first [`MD_ROWS`]).
fn diff_table(c: &Comparison, s: &mut String) {
    let tallies: Vec<String> = Category::ALL
        .iter()
        .filter_map(|cat| {
            let t = c.tally(*cat);
            (t.total() > 0).then(|| format!("{} {}/{}/{}", cat.label(), t.lost, t.added, t.changed))
        })
        .collect();
    s.push_str(&format!("By category (lost/added/changed): {}\n\n", tallies.join(" · ")));
    s.push_str("| Category | Change | Where | Detail |\n|---|---|---|---|\n");
    for d in c.differences.iter().take(MD_ROWS) {
        s.push_str(&format!("| {} | {} | {} | {} |\n", d.category.label(), d.change.label(), cell(&clip(&d.place, 80)), cell(&clip(&d.detail, 400))));
    }
    let more = c.differences.len().saturating_sub(MD_ROWS).saturating_add(c.omitted);
    if more > 0 {
        s.push_str(&format!("\n… and {more} more differences (see report.json; differences beyond the listing limit are only counted).\n"));
    }
}
