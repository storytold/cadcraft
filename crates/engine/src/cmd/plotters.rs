//! Plotter configurations: PLOTTERMANAGER / -PLOTTERMANAGER, and the device and paper a page
//! setup or a plot uses.
//!
//! The built-in devices (`CADCraft PDF`, `CADCraft PNG`, `CADCraft SVG`) come first. They offer
//! the standard ISO and ANSI/ARCH paper sizes with 5 mm (ISO) or quarter-inch (inch sizes)
//! margins. User devices are `*.plotter.json` files ([`cadcraft_doc::PlotterConfig`],
//! `docs/plotters.md`) in a folder the host provides through [`set_plotter_store`]. Without
//! one, native builds use the `plotters` folder of the CADCraft settings directory and the web
//! keeps them for the session.

use std::sync::{Mutex, OnceLock};

use cadcraft_doc::{DevicePaper, Orientation, PageSetup, PlotOutput, PlotterConfig};
use cadcraft_geom::{Bounds2, Vec2};
use serde_json::{Value, json};

use super::*;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Session, Step};

/// Where plotter configuration files live. Names are file names ending in
/// [`cadcraft_doc::PLOTTER_FILE_EXT`]; the engine never passes paths.
#[derive(Clone, Copy)]
pub struct PlotterStore {
    /// File names in the folder.
    pub list: fn() -> std::result::Result<Vec<String>, String>,
    pub read: fn(&str) -> std::result::Result<String, String>,
    pub write: fn(&str, &str) -> std::result::Result<(), String>,
    pub delete: fn(&str) -> std::result::Result<(), String>,
}

static STORE: OnceLock<PlotterStore> = OnceLock::new();
static MEMORY: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// Install the host's plotter folder (first call wins).
pub fn set_plotter_store(s: PlotterStore) {
    let _ = STORE.set(s);
}

impl PlotterStore {
    /// A folder kept in memory for the life of the process.
    pub fn memory() -> PlotterStore {
        fn lock() -> std::result::Result<std::sync::MutexGuard<'static, Vec<(String, String)>>, String> {
            MEMORY.lock().map_err(|_| "the plotter folder is unavailable".to_string())
        }
        PlotterStore {
            list: || Ok(lock()?.iter().map(|(n, _)| n.clone()).collect()),
            read: |n| lock()?.iter().find(|(f, _)| f == n).map(|(_, t)| t.clone()).ok_or_else(|| format!("{n}: no such file")),
            write: |n, t| {
                let mut m = lock()?;
                m.retain(|(f, _)| f != n);
                m.push((n.to_string(), t.to_string()));
                Ok(())
            },
            delete: |n| {
                let mut m = lock()?;
                let before = m.len();
                m.retain(|(f, _)| f != n);
                if m.len() == before { Err(format!("{n}: no such file")) } else { Ok(()) }
            },
        }
    }

    /// The `plotters` folder of the CADCraft settings directory (`CADCRAFT_CONFIG_DIR`, else
    /// the platform's settings directory).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn settings_folder() -> PlotterStore {
        fn dir() -> std::result::Result<std::path::PathBuf, String> {
            use std::path::PathBuf;
            let get = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
            let root = match std::env::var_os("CADCRAFT_CONFIG_DIR") {
                Some(r) if !r.is_empty() => Some(PathBuf::from(r)),
                Some(_) => None,
                None => match std::env::consts::OS {
                    "macos" => get("HOME").map(|h| h.join("Library/Application Support/CADCraft")),
                    "windows" => get("APPDATA").map(|r| r.join("CADCraft")),
                    _ => get("XDG_CONFIG_HOME").map(|r| r.join("cadcraft")).or_else(|| get("HOME").map(|h| h.join(".config/cadcraft"))),
                },
            };
            root.map(|r| r.join("plotters")).ok_or_else(|| "there is no settings folder for plotter configurations".to_string())
        }
        /// A plain file name of ours (no separators, no parent references).
        fn file(n: &str) -> std::result::Result<std::path::PathBuf, String> {
            let ok = n.ends_with(cadcraft_doc::PLOTTER_FILE_EXT) && !n.starts_with('.') && !n.contains(['/', '\\', ':']);
            if !ok {
                return Err(format!("`{n}` is not a plotter configuration file name"));
            }
            Ok(dir()?.join(n))
        }
        PlotterStore {
            list: || {
                let rd = match std::fs::read_dir(dir()?) {
                    Ok(rd) => rd,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
                    Err(e) => return Err(e.to_string()),
                };
                Ok(rd
                    .filter_map(|e| e.ok()?.file_name().into_string().ok())
                    .filter(|n| n.ends_with(cadcraft_doc::PLOTTER_FILE_EXT))
                    .take(MAX_DEVICES)
                    .collect())
            },
            read: |n| {
                let p = file(n)?;
                let len = std::fs::metadata(&p).map_err(|e| format!("{n}: {e}"))?.len();
                if len > cadcraft_doc::MAX_PLOTTER_FILE as u64 {
                    return Err(format!("{n}: the plotter configuration file is too large"));
                }
                std::fs::read_to_string(&p).map_err(|e| format!("{n}: {e}"))
            },
            write: |n, t| {
                let p = file(n)?;
                std::fs::create_dir_all(dir()?).map_err(|e| e.to_string())?;
                let tmp = p.with_extension("json.cadcraft-tmp");
                std::fs::write(&tmp, t).map_err(|e| format!("{n}: {e}"))?;
                std::fs::rename(&tmp, &p).map_err(|e| format!("{n}: {e}"))
            },
            delete: |n| std::fs::remove_file(file(n)?).map_err(|e| format!("{n}: {e}")),
        }
    }
}

fn store() -> PlotterStore {
    *STORE.get_or_init(|| {
        #[cfg(not(target_arch = "wasm32"))]
        {
            PlotterStore::settings_folder()
        }
        #[cfg(target_arch = "wasm32")]
        {
            PlotterStore::memory()
        }
    })
}

/// A plot for a system printer: the PDF the PDF plotter wrote, and how to print it.
pub struct PrintJob<'a> {
    pub printer: &'a str,
    /// The paper's name as the printer's device lists it.
    pub paper: &'a str,
    pub copies: u32,
    pub title: &'a str,
    pub pdf: &'a [u8],
}

/// The host's printers: `list` gives them as devices (`system` set), `print` sends a job and
/// returns its id.
#[derive(Clone, Copy)]
pub struct SystemPrinters {
    pub list: fn() -> Vec<PlotterConfig>,
    pub print: fn(&PrintJob) -> std::result::Result<String, String>,
}

static PRINTERS: OnceLock<SystemPrinters> = OnceLock::new();

/// Install the host's printers (first call wins). Without them only files are plotted.
pub fn set_system_printers(p: SystemPrinters) {
    let _ = PRINTERS.set(p);
}

/// Most copies of one print job.
pub const MAX_COPIES: u64 = 999;

/// The system printers as last listed. Listing asks the system (CUPS can be slow, or hang), so
/// it runs on a background thread and this never waits for it: it returns the last list, and
/// starts listing again when there is none yet or it is a few seconds old. Until the first
/// listing finishes there are none; [`system_printers_pending`] says one is under way.
fn system_printers() -> Vec<PlotterConfig> {
    let Some(h) = PRINTERS.get() else { return Vec::new() };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut c = printer_cache();
        if c.listing.is_none() && c.at.is_none_or(|at| at.elapsed() >= PRINTERS_TTL) {
            c.listing = Some(std::time::Instant::now());
            let list = h.list;
            let spawned = std::thread::Builder::new().name("system-printers".into()).spawn(move || {
                let found: Vec<PlotterConfig> = list().into_iter().filter(|p| p.system && !is_builtin(&p.name)).collect();
                let mut c = printer_cache();
                (c.list, c.at, c.listing) = (found, Some(std::time::Instant::now()), None);
            });
            if spawned.is_err() {
                c.listing = None;
            }
        }
        c.list.clone()
    }
    #[cfg(target_arch = "wasm32")]
    {
        (h.list)().into_iter().filter(|p| p.system && !is_builtin(&p.name)).collect()
    }
}

/// How long a printer list is used before the system is asked again.
#[cfg(not(target_arch = "wasm32"))]
const PRINTERS_TTL: std::time::Duration = std::time::Duration::from_secs(5);

/// The last printer list, when it was made, and when a listing under way started.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct PrinterCache {
    list: Vec<PlotterConfig>,
    at: Option<std::time::Instant>,
    listing: Option<std::time::Instant>,
}

#[cfg(not(target_arch = "wasm32"))]
fn printer_cache() -> std::sync::MutexGuard<'static, PrinterCache> {
    static CACHE: OnceLock<Mutex<PrinterCache>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether the system printers are being listed (a dialog showing devices redraws when it ends).
pub fn system_printers_pending() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        PRINTERS.get().is_some() && printer_cache().listing.is_some()
    }
    #[cfg(target_arch = "wasm32")]
    false
}

/// The system printer called `name`, for a command that is about to use it: from the last list,
/// else, while a listing is under way (the first one, or one started because the list is old),
/// waiting for it up to [`PRINTERS_WAIT`], so a printer named before the first listing ended is
/// still found. Only a command naming a device waits, never a dialog drawing its device list.
fn system_printer(name: &str) -> Option<PlotterConfig> {
    let find = |list: Vec<PlotterConfig>| list.into_iter().find(|p| p.name.eq_ignore_ascii_case(name.trim()));
    if let Some(p) = find(system_printers()) {
        return Some(p);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        wait_for_printers();
        find(printer_cache().list.clone())
    }
    #[cfg(target_arch = "wasm32")]
    None
}

/// Wait (up to [`PRINTERS_WAIT`]) for a system printer listing under way, starting one when the
/// list is old: for commands that report the devices, not for dialogs drawing them.
fn wait_for_printers() {
    let _ = system_printers();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let start = std::time::Instant::now();
        while system_printers_pending() && start.elapsed() < PRINTERS_WAIT {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}

/// [`devices`] for a command reporting them: the system printers listed first, if that is
/// under way.
fn listed_devices() -> Vec<PlotterConfig> {
    wait_for_printers();
    devices()
}

/// The device a command names (or a layout's page setup names, for a command about to plot):
/// a built-in device at once; otherwise, once a system printer listing under way has ended (up
/// to [`PRINTERS_WAIT`]), [`find_device`], so a printer is found and wins over a user device of
/// the same name even on the first plot after the app starts. Dialogs never wait.
fn named_device(name: &str) -> Option<PlotterConfig> {
    let n = name.trim();
    if !BUILTIN.iter().any(|(b, _, _)| b.eq_ignore_ascii_case(n.strip_suffix(cadcraft_doc::PLOTTER_FILE_EXT).unwrap_or(n))) {
        wait_for_printers();
    }
    // One wait only: the printers as listed by now (a listing still under way after the wait
    // isn't waited for again here, nor by the unknown-device error).
    find_device(name)
}

/// Longest a plot waits for the system printers to be listed.
#[cfg(not(target_arch = "wasm32"))]
const PRINTERS_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// Print `pdf` on the system printer `printer`; returns the job id.
pub(crate) fn print_pdf(cmd: &str, printer: &str, paper: &str, copies: u64, title: &str, pdf: &[u8]) -> Result<String> {
    let h = PRINTERS.get().ok_or_else(|| bad(cmd, "there are no system printers in this build"))?;
    let Some(dev) = system_printer(printer) else {
        return Err(bad(cmd, format!("unknown printer `{printer}`")));
    };
    let copies = copies.clamp(1, MAX_COPIES) as u32;
    (h.print)(&PrintJob { printer: &dev.name, paper, copies, title, pdf }).map_err(|e| bad(cmd, format!("{}: {e}", dev.name)))
}

/// Most user devices listed.
const MAX_DEVICES: usize = 1000;

/// The built-in devices: name, output, description.
const BUILTIN: [(&str, PlotOutput, &str); 3] = [
    ("CADCraft PDF", PlotOutput::Pdf, "Vector PDF"),
    ("CADCraft PNG", PlotOutput::Png, "Raster PNG image"),
    ("CADCraft SVG", PlotOutput::Svg, "Vector SVG image"),
];

/// A page setup's "no device" name.
const NO_DEVICE: &str = "None";

fn builtin(name: &str, output: PlotOutput, description: &str) -> PlotterConfig {
    let papers = cadcraft_render::PAPER_SIZES
        .iter()
        .map(|p| DevicePaper { name: p.name.into(), width: p.width_mm, height: p.height_mm, margins: p.margins_mm() })
        .collect();
    let default_paper = cadcraft_render::paper_size("A4").map(|p| p.name.to_string()).unwrap_or_default();
    PlotterConfig {
        name: name.into(),
        description: description.into(),
        output,
        resolution: 300,
        orientation: Orientation::Landscape,
        default_paper,
        papers,
        system: false,
    }
}

fn builtins() -> Vec<PlotterConfig> {
    BUILTIN.iter().map(|(n, o, d)| builtin(n, *o, d)).collect()
}

fn is_builtin(name: &str) -> bool {
    BUILTIN.iter().any(|(n, _, _)| n.eq_ignore_ascii_case(name.trim()))
}

/// The user's devices (unreadable files are skipped), sorted by name.
fn user_devices() -> Vec<PlotterConfig> {
    let st = store();
    let mut out: Vec<PlotterConfig> = (st.list)()
        .unwrap_or_default()
        .iter()
        .take(MAX_DEVICES)
        .filter_map(|f| PlotterConfig::from_file_text(&(st.read)(f).ok()?).ok())
        .filter(|c| !is_builtin(&c.name))
        .collect();
    out.sort_by_key(|c| c.name.to_lowercase());
    out.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    out
}

/// Every device: the built-in ones first, then the system printers, then the user's.
pub fn devices() -> Vec<PlotterConfig> {
    let mut v = builtins();
    let printers = system_printers();
    v.extend(user_devices().into_iter().filter(|u| !printers.iter().any(|p| p.name.eq_ignore_ascii_case(&u.name))));
    let at = BUILTIN.len();
    v.splice(at..at, printers);
    v
}

/// Device names, built-in ones first, for a command reporting them (it waits for a system
/// printer listing under way).
pub fn device_names() -> Vec<String> {
    listed_devices().into_iter().map(|c| c.name).collect()
}

/// The device called `name` (any case; a `.plotter.json` suffix is ignored).
pub fn find_device(name: &str) -> Option<PlotterConfig> {
    let n = name.trim();
    let n = n.strip_suffix(cadcraft_doc::PLOTTER_FILE_EXT).unwrap_or(n);
    if n.is_empty() || n.eq_ignore_ascii_case(NO_DEVICE) {
        return None;
    }
    if let Some((b, o, d)) = BUILTIN.iter().find(|(b, _, _)| b.eq_ignore_ascii_case(n)) {
        return Some(builtin(b, *o, d));
    }
    if let Some(p) = system_printers().into_iter().find(|p| p.name.eq_ignore_ascii_case(n)) {
        return Some(p);
    }
    let st = store();
    let direct = cadcraft_doc::plotter_file_name(n).and_then(|f| PlotterConfig::from_file_text(&(st.read)(&f).ok()?).ok());
    direct.filter(|c| c.name.eq_ignore_ascii_case(n)).or_else(|| user_devices().into_iter().find(|c| c.name.eq_ignore_ascii_case(n)))
}

/// The paper of `dev` called `name`, also by a standard size's short name (`A3`, `Letter`…).
fn device_paper<'a>(dev: &'a PlotterConfig, name: &str) -> Option<&'a DevicePaper> {
    dev.paper(name).or_else(|| dev.paper(cadcraft_render::paper_size(name)?.name))
}

fn set_paper(page: &mut PageSetup, p: &DevicePaper) {
    page.paper = p.name.clone();
    page.width_mm = p.width;
    page.height_mm = p.height;
    page.margins_mm = p.margins;
}

/// The error for a device name not found, with the devices as listed now: it never waits for a
/// printer listing (the lookup that failed already did).
fn unknown_device(cmd: &str, name: &str) -> EngineError {
    let names: Vec<String> = devices().into_iter().map(|c| c.name).collect();
    bad(cmd, format!("unknown plotter `{name}` (devices: {})", names.join(", ")))
}

/// PAGESETUP's `device` and the device's papers. Sets `page.device` from `device` (`"None"`:
/// no device) and, from the page's device, the paper: `paper` when the device has it, else on a
/// device change the current paper if the device has it or the device's default paper, with the
/// paper's printable area. A device change also takes the device's default orientation unless
/// `landscape` is given. Returns whether the paper was set here (else `paper`, if any, is a
/// standard size for the caller to look up).
pub fn page_device(cmd: &str, p: &Value, page: &mut PageSetup) -> Result<bool> {
    let given = match p.get("device") {
        Some(v) => Some(v.as_str().ok_or_else(|| bad(cmd, "`device` must be a plotter name"))?.trim()),
        None => None,
    };
    let dev = match given {
        Some(n) if n.is_empty() || n.eq_ignore_ascii_case(NO_DEVICE) => {
            page.device = NO_DEVICE.into();
            return Ok(false);
        }
        Some(n) => Some(named_device(n).ok_or_else(|| unknown_device(cmd, n))?),
        None => find_device(&page.device),
    };
    let Some(dev) = dev else { return Ok(false) };
    let switched = !page.device.eq_ignore_ascii_case(&dev.name);
    page.device = dev.name.clone();
    if switched && p.get("landscape").is_none() {
        page.landscape = dev.orientation == Orientation::Landscape;
    }
    if let Some(name) = p.get("paper") {
        let name = name.as_str().ok_or_else(|| bad(cmd, "`paper` must be a paper name"))?;
        return match device_paper(&dev, name) {
            Some(pp) => {
                set_paper(page, pp);
                Ok(true)
            }
            None if given.is_some() => {
                let known: Vec<&str> = dev.papers.iter().map(|x| x.name.as_str()).take(50).collect();
                Err(bad(cmd, format!("plotter `{}` has no paper `{name}` (papers: {})", dev.name, known.join(", "))))
            }
            None => Ok(false),
        };
    }
    if given.is_some() {
        if let Some(pp) = device_paper(&dev, &page.paper).or_else(|| dev.default_paper()) {
            set_paper(page, pp);
        }
        return Ok(true);
    }
    Ok(false)
}

/// The options PLOT / EXPORTPDF hand the plotter with `device` and the device's papers
/// resolved: `device` (else the layout's page setup device) picks the output (`output`: pdf,
/// png or svg; EXPORTPDF always writes PDF) and `resolution`; a paper of the device becomes
/// `width`/`height`/`margins` (mm) and, when not given, the device's orientation. Plots
/// without a device are unchanged.
pub(crate) fn plot_options(s: &Session, p: &Value, cmd: &str) -> Result<Value> {
    let given = str_param(p, "device").map(str::trim);
    let layout = match str_param(p, "layout") {
        Some(n) if n.eq_ignore_ascii_case("model") => None,
        Some(n) => s.doc()?.layout(n).map(|l| l.page.clone()),
        None => match s.space().layout() {
            Some(n) => s.doc()?.layout(n).map(|l| l.page.clone()),
            None => None,
        },
    };
    let dev = match given {
        Some(n) if n.is_empty() || n.eq_ignore_ascii_case(NO_DEVICE) => None,
        Some(n) => Some(named_device(n).ok_or_else(|| unknown_device(cmd, n))?),
        None => layout.as_ref().and_then(|pg| named_device(&pg.device)),
    };
    let mut out = if p.is_object() { p.clone() } else { json!({}) };
    // Only a system printer chosen as the device prints (below): a `printer` passed with the
    // call is dropped, so EXPORTPDF always writes the PDF and PLOT without one writes its file.
    if let Some(o) = out.as_object_mut() {
        o.remove("printer");
        o.remove("printerPaper");
    }
    let (Some(dev), Some(o)) = (dev, out.as_object_mut()) else { return Ok(out) };
    o.remove("device");
    let output = if cmd == "exportpdf" { PlotOutput::Pdf } else { dev.output };
    o.insert("output".into(), json!(output.as_str()));
    o.insert("resolution".into(), json!(dev.resolution));
    let paper = match str_param(p, "paper") {
        Some(name) => match device_paper(&dev, name) {
            Some(pp) => Some(pp),
            None => {
                let known: Vec<&str> = dev.papers.iter().map(|x| x.name.as_str()).take(50).collect();
                return Err(bad(cmd, format!("plotter `{}` has no paper `{name}` (papers: {})", dev.name, known.join(", "))));
            }
        },
        // A layout keeps its page setup's paper; model space takes the device's default.
        None if layout.is_none() => dev.default_paper(),
        None => None,
    };
    // A system printer gets the plotter's PDF, on the paper by its name on that printer
    // (EXPORTPDF still writes the PDF).
    if dev.system && cmd == "plot" {
        o.insert("printer".into(), json!(dev.name));
        let on = paper.map(|pp| pp.name.clone()).or_else(|| layout.as_ref().map(|pg| pg.paper.clone())).unwrap_or_default();
        o.insert("printerPaper".into(), json!(on));
    }
    if let Some(pp) = paper {
        o.remove("paper");
        o.insert("width".into(), json!(pp.width));
        o.insert("height".into(), json!(pp.height));
        o.insert("margins".into(), json!(pp.margins));
        if p.get("landscape").is_none() && layout.is_none() {
            o.insert("landscape".into(), json!(dev.orientation == Orientation::Landscape));
        }
    }
    Ok(out)
}

/// Whether a plot draws transparency, as PLOTTRANSPARENCYOVERRIDE decides: 0 never, 2 always,
/// otherwise the plot's `plotTransparency` (`None` when it has none: the page setup decides).
pub(crate) fn plot_transparency(s: &Session, p: &Value) -> Option<bool> {
    match s.settings.plot_transparency_override {
        0 => Some(false),
        2 => Some(true),
        _ => p.get("plotTransparency").and_then(Value::as_bool),
    }
}

// ---------- PLOTWINDOW ----------

fn run_plotwindow(s: &mut Session, p: &Value) -> Result<Value> {
    let (a, b) = (point_req("plotwindow", p, "p1")?, point_req("plotwindow", p, "p2")?);
    set_plot_window(s, a, b)
}

fn set_plot_window(s: &mut Session, a: Vec2, b: Vec2) -> Result<Value> {
    let w = Bounds2::new(a, b);
    if !(w.width() > 0.0 && w.height() > 0.0) {
        return Err(bad("plotwindow", "the window needs a width and a height"));
    }
    s.plot_window = Some(w);
    Ok(json!({ "window": [[w.min.x, w.min.y], [w.max.x, w.max.y]] }))
}

/// PLOTWINDOW: two corners picked in the drawing, with the window's rectangle following the cursor.
#[derive(Default)]
struct PlotWindowM {
    first: Option<Vec2>,
}

impl Interactive for PlotWindowM {
    fn name(&self) -> &'static str {
        "PLOTWINDOW"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.first {
            None => Prompt::new("Specify first corner", Accept::POINT),
            Some(a) => Prompt::new("Specify opposite corner", Accept::POINT).base(a),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (i, self.first) {
            (Input::Point(p), None) => {
                self.first = Some(p);
                Ok(Step::Continue)
            }
            (Input::Point(p), Some(a)) => {
                set_plot_window(s, a, p)?;
                Ok(Step::Done)
            }
            (Input::Cancel, _) => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<cadcraft_doc::EntityKind> {
        let Some(a) = self.first else { return Vec::new() };
        let corners = [a, Vec2::new(c.x, a.y), c, Vec2::new(a.x, c.y), a];
        corners
            .windows(2)
            .filter_map(|w| match w {
                [p, q] => Some(super::helpers::line(*p, *q)),
                _ => None,
            })
            .collect()
    }
}

// ---------- PLOTTERMANAGER ----------

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("plotwindow", "Plot Window", run_plotwindow)
            .params("{p1: [x,y], p2: [x,y]} (typed: two corners): the window the Plot dialog's \"Window <\" plots, kept in the session")
            .enabled(has_doc)
            .noundo()
            .interactive(|_| Ok(Box::new(PlotWindowM::default()))),
        CommandSpec::new("plottermanager", "Plotter Manager...", run_plottermanager)
            .menu(&["File", "Plotter Manager..."])
            .params(
                "{action?: list (default)|get|new|set|delete|load|save, name?: device, from?: device to copy (new), output?: pdf|png|svg, description?, resolution?: dpi, orientation?: portrait|landscape, defaultPaper?, paper?: {name, width, height (mm), margins?: [l,b,r,t] mm} (set: add or replace), removePaper?: name (set), text? (load), path? (save: native only; without it returns `text`)} (typed: option prompts)",
            )
            .enabled(always)
            .noundo()
            .interactive(|_| Ok(Box::new(PlotterManagerM::default()))),
        CommandSpec::new("-plottermanager", "Plotter Manager", run_plottermanager)
            .params("as PLOTTERMANAGER (typed: option prompts)")
            .enabled(always)
            .noundo()
            .interactive(|_| Ok(Box::new(PlotterManagerM::default()))),
    ]
}

/// A system printer is the system's: PLOTTERMANAGER doesn't change or delete it (a user file
/// by its name would only be hidden behind it). A copy made with action `new` can be edited.
fn system_read_only(cmd: &str, name: &str) -> EngineError {
    bad(cmd, format!("`{name}` is a system printer: make a copy with action new, from `{name}`"))
}

fn summary(c: &PlotterConfig) -> Value {
    json!({
        "name": c.name,
        "description": c.description,
        "output": c.output.as_str(),
        "builtin": is_builtin(&c.name),
        "system": c.system,
        "papers": c.papers.len(),
        "defaultPaper": c.default_paper,
    })
}

fn device_name(cmd: &str, p: &Value) -> Result<String> {
    let n = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`name` (a plotter name) is required"))?;
    Ok(n.chars().take(cadcraft_doc::MAX_PLOTTER_NAME).collect())
}

/// Write a user device to the store.
fn save_device(cmd: &str, c: &PlotterConfig) -> Result<()> {
    let file = cadcraft_doc::plotter_file_name(&c.name).ok_or_else(|| bad(cmd, "the plotter has no name"))?;
    (store().write)(&file, &c.to_file_text()).map_err(|e| bad(cmd, e))
}

fn mm_param(cmd: &str, v: &Value, key: &str) -> Result<Option<f64>> {
    match v.get(key) {
        None => Ok(None),
        Some(x) => {
            let f = x.as_f64().filter(|f| f.is_finite() && *f > 0.0 && *f <= cadcraft_doc::MAX_PAPER_MM);
            f.map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be a size in mm, up to {}", cadcraft_doc::MAX_PAPER_MM)))
        }
    }
}

/// Apply the editable fields of `p` to `c`.
fn edit_device(cmd: &str, c: &mut PlotterConfig, p: &Value) -> Result<()> {
    if let Some(d) = str_param(p, "description") {
        c.description = d.chars().take(1000).collect();
    }
    if let Some(o) = p.get("output") {
        c.output = o.as_str().and_then(PlotOutput::parse).ok_or_else(|| bad(cmd, "`output` must be pdf, png or svg"))?;
    }
    if let Some(r) = p.get("resolution") {
        let r = r.as_u64().filter(|r| (50..=2400).contains(r)).ok_or_else(|| bad(cmd, "`resolution` must be 50-2400 dpi"))?;
        c.resolution = u32::try_from(r).unwrap_or(300);
    }
    if let Some(o) = p.get("orientation") {
        c.orientation = match o.as_str().map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            Some("portrait") => Orientation::Portrait,
            Some("landscape") => Orientation::Landscape,
            _ => return Err(bad(cmd, "`orientation` must be portrait or landscape")),
        };
    }
    if let Some(rm) = str_param(p, "removePaper") {
        let before = c.papers.len();
        let target = c.paper(rm).map(|x| x.name.clone());
        c.papers.retain(|x| Some(&x.name) != target.as_ref());
        if c.papers.len() == before {
            return Err(bad(cmd, format!("`{}` has no paper `{rm}`", c.name)));
        }
    }
    if let Some(pv) = p.get("paper") {
        let name = pv.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty());
        let name = name.ok_or_else(|| bad(cmd, "`paper.name` is required"))?;
        let existing = c.paper(name).cloned();
        let (w, h) = match (mm_param(cmd, pv, "width")?, mm_param(cmd, pv, "height")?, &existing) {
            (Some(w), Some(h), _) => (w, h),
            (None, None, Some(e)) => (e.width, e.height),
            _ => return Err(bad(cmd, "`paper.width` and `paper.height` (mm) are both required")),
        };
        let margins = match pv.get("margins") {
            None => existing.as_ref().map(|e| e.margins).unwrap_or([0.0; 4]),
            Some(m) => {
                let a: Vec<f64> = m.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
                match a[..] {
                    [l, b, r, t] if a.iter().all(|v| v.is_finite() && (0.0..=1000.0).contains(v)) && l + r < w.min(h) && b + t < w.max(h) => {
                        [l, b, r, t]
                    }
                    _ => return Err(bad(cmd, "`paper.margins` must be [left, bottom, right, top] in mm, leaving a printable area")),
                }
            }
        };
        let new = DevicePaper { name: name.into(), width: w, height: h, margins };
        match existing.and_then(|e| c.papers.iter().position(|x| x.name == e.name)) {
            Some(i) => {
                if let Some(slot) = c.papers.get_mut(i) {
                    *slot = new;
                }
            }
            None if c.papers.len() >= cadcraft_doc::MAX_DEVICE_PAPERS => return Err(bad(cmd, "the plotter has too many papers")),
            None => c.papers.push(new),
        }
    }
    if let Some(d) = str_param(p, "defaultPaper") {
        let n = c.paper(d).map(|x| x.name.clone()).ok_or_else(|| bad(cmd, format!("`{}` has no paper `{d}`", c.name)))?;
        c.default_paper = n;
    }
    Ok(())
}

fn run_plottermanager(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "plottermanager";
    let action = str_param(p, "action").unwrap_or("list").trim().to_ascii_lowercase();
    match action.as_str() {
        "list" => Ok(json!({ "devices": listed_devices().iter().map(summary).collect::<Vec<_>>() })),
        "get" => {
            let n = device_name(cmd, p)?;
            let c = find_device(&n).ok_or_else(|| unknown_device(cmd, &n))?;
            let mut v = serde_json::to_value(&c).unwrap_or(Value::Null);
            if let Some(o) = v.as_object_mut() {
                o.insert("builtin".into(), json!(is_builtin(&c.name)));
                o.insert("system".into(), json!(c.system));
            }
            Ok(v)
        }
        "new" => {
            let name = device_name(cmd, p)?;
            if find_device(&name).is_some() {
                return Err(bad(cmd, format!("plotter `{name}` already exists")));
            }
            let file = cadcraft_doc::plotter_file_name(&name).ok_or_else(|| bad(cmd, "`name` is empty"))?;
            if (store().list)().unwrap_or_default().iter().any(|f| f.eq_ignore_ascii_case(&file)) {
                return Err(bad(cmd, format!("a plotter file `{file}` already exists")));
            }
            let mut c = match str_param(p, "from") {
                Some(f) => find_device(f).ok_or_else(|| unknown_device(cmd, f))?,
                None => {
                    // A copy of the built-in device for the output: the standard papers.
                    let output = match p.get("output") {
                        Some(o) => o.as_str().and_then(PlotOutput::parse).ok_or_else(|| bad(cmd, "`output` must be pdf, png or svg"))?,
                        None => PlotOutput::Pdf,
                    };
                    let (b, o, _) = BUILTIN.iter().find(|(_, o, _)| *o == output).copied().unwrap_or(("CADCraft PDF", PlotOutput::Pdf, ""));
                    builtin(b, o, "")
                }
            };
            c.name = name;
            c.description = String::new();
            edit_device(cmd, &mut c, p)?;
            let c = c.sanitized();
            save_device(cmd, &c)?;
            Ok(summary(&c))
        }
        "set" => {
            let name = device_name(cmd, p)?;
            if is_builtin(&name) {
                return Err(bad(cmd, format!("`{name}` is built in: make a copy with action new, from `{name}`")));
            }
            let mut c = find_device(&name).ok_or_else(|| unknown_device(cmd, &name))?;
            if c.system {
                return Err(system_read_only(cmd, &c.name));
            }
            edit_device(cmd, &mut c, p)?;
            if c.papers.is_empty() {
                return Err(bad(cmd, "a plotter needs at least one paper"));
            }
            let c = c.sanitized();
            save_device(cmd, &c)?;
            Ok(serde_json::to_value(&c).unwrap_or(Value::Null))
        }
        "delete" => {
            let name = device_name(cmd, p)?;
            if is_builtin(&name) {
                return Err(bad(cmd, format!("`{name}` is built in and can't be deleted")));
            }
            let c = find_device(&name).ok_or_else(|| unknown_device(cmd, &name))?;
            if c.system {
                return Err(system_read_only(cmd, &c.name));
            }
            let file = cadcraft_doc::plotter_file_name(&c.name).ok_or_else(|| bad(cmd, "the plotter has no name"))?;
            (store().delete)(&file).map_err(|e| bad(cmd, e))?;
            Ok(json!({ "deleted": c.name }))
        }
        "load" => {
            let text = str_param(p, "text").ok_or_else(|| bad(cmd, "`text` (the file's contents) is required"))?;
            let mut c = PlotterConfig::from_file_text(text).map_err(|e| bad(cmd, e))?;
            if let Some(n) = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()) {
                c.name = n.chars().take(cadcraft_doc::MAX_PLOTTER_NAME).collect();
            }
            if is_builtin(&c.name) {
                return Err(bad(cmd, format!("`{}` is a built-in plotter's name: pass another `name`", c.name)));
            }
            if c.papers.is_empty() {
                return Err(bad(cmd, "the plotter configuration has no papers"));
            }
            save_device(cmd, &c)?;
            Ok(summary(&c))
        }
        "save" => {
            let name = device_name(cmd, p)?;
            let text = find_device(&name).ok_or_else(|| unknown_device(cmd, &name))?.to_file_text();
            match str_param(p, "path") {
                None => Ok(json!({ "name": name, "text": text })),
                Some(_path) => {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        std::fs::write(_path, &text).map_err(|e| bad(cmd, format!("{_path}: {e}")))?;
                        Ok(json!({ "name": name, "path": _path }))
                    }
                    #[cfg(target_arch = "wasm32")]
                    Err(bad(cmd, "paths are not available on the web; omit `path` to get the file as `text`"))
                }
            }
        }
        other => Err(bad(cmd, format!("unknown action `{other}` (list, get, new, set, delete, load, save)"))),
    }
}

// ---------- -PLOTTERMANAGER prompts ----------

const OPTIONS: &[&str] = &["?", "New", "Delete", "Paper"];
const KW: Accept = Accept { point: false, number: false, text: false, select: false, enter: true };

#[derive(Clone, Debug, Default)]
enum Stage {
    #[default]
    Option,
    NewName,
    NewOutput(String),
    Delete,
    PaperDevice,
    PaperName(String),
    PaperSize(String, String),
}

/// PLOTTERMANAGER typed: list the devices (`?`), create one (`New`: name, output), delete one
/// or add a paper size to one (`Paper`: device, paper name, `width,height` in mm).
#[derive(Default)]
pub struct PlotterManagerM {
    stage: Stage,
}

fn typed(i: &Input) -> Option<String> {
    match i {
        Input::Text(t) | Input::Keyword(t) => Some(t.trim().to_string()).filter(|t| !t.is_empty()),
        _ => None,
    }
}

/// A command refusal as a re-prompt message.
fn retry(e: EngineError) -> EngineError {
    match e {
        EngineError::BadParams { msg, .. } => EngineError::Other(msg),
        other => other,
    }
}

impl Interactive for PlotterManagerM {
    fn name(&self) -> &'static str {
        "PLOTTERMANAGER"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match &self.stage {
            Stage::Option => Prompt::new("Enter a plotter option", KW).kw(OPTIONS).default("?"),
            Stage::NewName => Prompt::new("Enter a name for the new plotter", Accept::TEXT),
            Stage::NewOutput(_) => Prompt::new("Enter the output", KW).kw(&["Pdf", "PNg", "Svg"]).default("Pdf"),
            Stage::Delete => Prompt::new("Enter the plotter to delete", Accept::TEXT),
            Stage::PaperDevice => Prompt::new("Enter the plotter to add a paper size to", Accept::TEXT),
            Stage::PaperName(_) => Prompt::new("Enter the paper name", Accept::TEXT),
            Stage::PaperSize(..) => Prompt::new("Enter the paper width,height in mm", Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if i == Input::Cancel {
            return Ok(Step::Cancel);
        }
        let t = typed(&i);
        let need = |t: Option<String>| t.ok_or_else(|| EngineError::Other("Requires a name.".into()));
        match self.stage.clone() {
            Stage::Option => {
                let k = match i {
                    Input::Enter => "?".to_string(),
                    Input::Keyword(k) => k,
                    _ => return Err(EngineError::Other("Invalid option keyword.".into())),
                };
                self.stage = match k.as_str() {
                    "New" => Stage::NewName,
                    "Delete" => Stage::Delete,
                    "Paper" => Stage::PaperDevice,
                    _ => {
                        for c in listed_devices() {
                            let kind = if is_builtin(&c.name) {
                                "built-in"
                            } else if c.system {
                                "system printer"
                            } else {
                                "user"
                            };
                            s.echo(format!("{} ({}, {kind}, {} paper sizes)", c.name, c.output.as_str().to_uppercase(), c.papers.len()));
                        }
                        return Ok(Step::Done);
                    }
                };
                Ok(Step::Continue)
            }
            Stage::NewName => {
                let n = need(t)?;
                if find_device(&n).is_some() {
                    return Err(EngineError::Other(format!("Plotter \"{n}\" already exists.")));
                }
                self.stage = Stage::NewOutput(n);
                Ok(Step::Continue)
            }
            Stage::NewOutput(name) => {
                let output = t.unwrap_or_else(|| "pdf".into());
                run_plottermanager(s, &json!({ "action": "new", "name": name, "output": output })).map_err(retry)?;
                s.echo(format!("Plotter \"{name}\" created."));
                Ok(Step::Done)
            }
            Stage::Delete => {
                let n = need(t)?;
                let r = run_plottermanager(s, &json!({ "action": "delete", "name": n })).map_err(retry)?;
                s.echo(format!("Plotter \"{}\" deleted.", r.get("deleted").and_then(Value::as_str).unwrap_or_default()));
                Ok(Step::Done)
            }
            Stage::PaperDevice => {
                let n = need(t)?;
                let c = find_device(&n).ok_or_else(|| EngineError::Other(format!("Plotter \"{n}\" not found.")))?;
                if is_builtin(&c.name) {
                    return Err(EngineError::Other(format!("\"{}\" is built in; create a new plotter first.", c.name)));
                }
                if c.system {
                    return Err(system_read_only("plottermanager", &c.name));
                }
                self.stage = Stage::PaperName(c.name);
                Ok(Step::Continue)
            }
            Stage::PaperName(dev) => {
                self.stage = Stage::PaperSize(dev, need(t)?);
                Ok(Step::Continue)
            }
            Stage::PaperSize(dev, paper) => {
                let text = need(t)?;
                let nums: Vec<f64> = text.split(',').map(|x| crate::units::parse_distance(x.trim())).collect::<Option<Vec<_>>>().unwrap_or_default();
                let [w, h] = nums[..] else {
                    return Err(EngineError::Other("Enter width,height in mm.".into()));
                };
                let p = json!({ "action": "set", "name": dev, "paper": { "name": paper, "width": w, "height": h } });
                run_plottermanager(s, &p).map_err(retry)?;
                s.echo(format!("Paper \"{paper}\" added to \"{dev}\"."));
                Ok(Step::Done)
            }
        }
    }
}
