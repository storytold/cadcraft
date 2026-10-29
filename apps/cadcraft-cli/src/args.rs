//! The command line, as clap types. Values are parsed into their types here and the relations
//! between flags are checked, so a bad value or combination is a usage error (exit 2) naming the
//! flag, before any work starts; the handlers receive typed values only. A flag a subcommand
//! doesn't take is a usage error too, so a typo can't be silently ignored. A value flag never takes
//! the next flag as its value: a value starting with `-` is written `--save=-out.dxf` (or
//! `./-out.dxf`).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use cadcraft_engine::geom::{Bounds2, Vec2};
use cadcraft_io::Format;
use cadcraft_mcp::args::{blank_env_is_unset, name_env_vars};
use clap::builder::{OsStringValueParser, PathBufValueParser, TypedValueParser};
use clap::error::ErrorKind;
use clap::parser::ValueSource;
use clap::{ArgAction, ArgGroup, ArgMatches, Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use serde_json::Value;

/// Help text that names the formats of `list`, built once from [`Format::READ`] or [`Format::WRITE`]
/// so it can't drift from what the code accepts.
fn with_formats(cell: &'static std::sync::OnceLock<String>, before: &str, list: &[Format], after: &str) -> &'static str {
    cell.get_or_init(|| format!("{before}({}){after}", Format::list(list)))
}

/// The drawing an input argument names.
fn read_help() -> &'static str {
    static C: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    with_formats(&C, "The drawing to read ", Format::READ, "")
}

fn open_help() -> &'static str {
    static C: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    with_formats(&C, "Drawings to open ", Format::READ, "")
}

fn write_help() -> &'static str {
    static C: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    with_formats(&C, "The file to write; its extension picks the format ", Format::WRITE, "")
}

fn save_help() -> &'static str {
    static C: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    with_formats(&C, "Save the drawing to this file; its extension picks the format ", Format::WRITE, "")
}

fn export_help() -> &'static str {
    static C: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    with_formats(&C, "Export the drawing to this file; its extension picks the format ", Format::WRITE, "")
}

fn convert_about() -> &'static str {
    static C: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    with_formats(&C, "Convert a drawing; OUT's extension picks the format ", Format::WRITE, "")
}

fn convert_long_about() -> &'static str {
    static C: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    C.get_or_init(|| {
        format!(
            "{}\n\nImage output (png 2400x1600, svg, pdf) shows model space, fitted to the drawing extents unless\n--window or --view says otherwise.",
            convert_about()
        )
    })
}

/// cadcraft-cli: headless CADCraft
#[derive(Parser)]
#[command(name = "cadcraft-cli", version = cadcraft_engine::build_version(), arg_required_else_help = true)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    Info(InfoArgs),
    Convert(ConvertArgs),
    Run(RunArgs),
    Commands(CommandsArgs),
    Mcp(McpArgs),
    Perf(PerfArgs),
    Sample(SampleArgs),
}

/// Print a drawing's summary as JSON
#[derive(Args)]
pub(crate) struct InfoArgs {
    #[arg(help = read_help())]
    pub(crate) file: PathBuf,
    /// One line of JSON instead of pretty-printed
    #[arg(long)]
    pub(crate) compact: bool,
}

/// Convert a drawing; OUT's extension picks the format
#[derive(Args)]
#[command(about = convert_about(), long_about = convert_long_about())]
pub(crate) struct ConvertArgs {
    #[arg(value_name = "IN", help = read_help())]
    pub(crate) input: PathBuf,
    #[arg(value_name = "OUT", help = write_help())]
    pub(crate) output: PathBuf,
    #[command(flatten)]
    pub(crate) framing: FramingArgs,
    /// Allow an output that is an input file, replacing the original
    #[arg(long)]
    pub(crate) in_place: bool,
    #[command(flatten)]
    pub(crate) settings: Settings,
}

/// The settings the desktop app reads from its environment, for the subcommands that draw or run
/// commands. A variable is the flag's fallback; an empty one counts as unset, except
/// `CADCRAFT_CONFIG_DIR`.
#[derive(Args)]
#[command(next_help_heading = "Settings")]
pub(crate) struct Settings {
    /// The settings folder; plotter configurations are in its `plotters` folder (default: the
    /// platform's settings folder). An empty value means no settings folder
    // Not RFC-0012 rule 1.7's "an empty variable counts as unset": an empty value is the
    // documented way to run with no settings folder (isolated runs, tests), and it predates the
    // CLI port. Keep it out of any blanket treatment of empty variables.
    #[arg(long, value_name = "DIR", env = "CADCRAFT_CONFIG_DIR", value_parser = OsStringValueParser::new().map(PathBuf::from))]
    pub(crate) config_dir: Option<PathBuf>,
    /// The font that replaces a missing one (FONTALT)
    #[arg(long, value_name = "FONT", env = "CADCRAFT_FONTALT")]
    pub(crate) fontalt: Option<String>,
    /// The fonts tried, in order, for characters a font lacks, comma-separated (FONTFALLBACK)
    #[arg(long, value_name = "FONTS", env = "CADCRAFT_FONTFALLBACK")]
    pub(crate) fontfallback: Option<String>,
}

impl Settings {
    /// Install the settings for this run; an unset one is the default. An empty or blank
    /// `--config-dir` is no settings folder.
    pub(crate) fn apply(&self) {
        cadcraft_engine::settings_dir::set(self.settings_dir());
        cadcraft_engine::fonts::ttf::set_font_alt(self.fontalt.as_deref().unwrap_or(""));
        cadcraft_engine::fonts::ttf::set_fallback_fonts(self.fontfallback.as_deref().unwrap_or(""));
    }

    /// The settings folder: `--config-dir`, else the platform's; `None` when the flag is empty or
    /// blank.
    fn settings_dir(&self) -> Option<PathBuf> {
        match &self.config_dir {
            Some(dir) if dir.to_str().is_some_and(|d| d.trim().is_empty()) => None,
            Some(dir) => Some(dir.clone()),
            None => cadcraft_engine::settings_dir::platform(|k| std::env::var_os(k)),
        }
    }
}

// What part of model space an image shows: shared by `convert` and `run`.
#[derive(Args)]
pub(crate) struct FramingArgs {
    /// Show this rectangle (drawing units, x0<x1, y0<y1), centred and scaled to fit the image
    /// without distortion. A negative x0 needs the `=` form: --window=-10,0,10,5
    #[arg(long, value_name = "x0,y0,x1,y1", value_parser = parse_window, conflicts_with = "view")]
    pub(crate) window: Option<Bounds2>,
    /// What part of model space the image shows (case-insensitive)
    #[arg(long, value_name = "extents|current", value_enum, ignore_case = true)]
    pub(crate) view: Option<ViewMode>,
}

/// `--view`.
#[derive(Clone, Copy, Debug, PartialEq, ValueEnum)]
pub(crate) enum ViewMode {
    /// Always fit the drawing extents
    Extents,
    /// The current view, as last set by zoom, pan or view.set (a freshly opened drawing's view is
    /// its extents plus a 10% margin)
    Current,
}

/// Open drawings (or start a new one), run scripts and commands in order, save or export the result
///
/// Drawings open in the order given, and the last one is current; without any, a new imperial
/// drawing is used. --script, --script-file and --cmd steps run in the order given, and each
/// --cmd prints its JSON result on one line (JSON lines); a command with no result prints nothing.
/// An image --export (png 2400x1600, svg, pdf) shows model space:
/// the current view if the steps changed it (zoom, pan, view.set…), otherwise the drawing
/// extents, unless --window or --view says otherwise.
#[derive(Args)]
#[command(group(ArgGroup::new("framing").args(["window", "view"]).multiple(true).requires("export")))]
#[command(after_long_help = r#"--cmd chaining:
  A --cmd parameter value of exactly "{last}" is replaced with the last object handle an
  earlier --cmd in the same run returned, so a command can act on what the previous one made:
    --cmd 'mview {"p1":[20,20],"p2":[200,150]}'        prints {"viewports":["100"]}
    --cmd 'viewport.set {"handle":"{last}","scale":1}' changes viewport 100
  It works in any parameter, at any depth (e.g. "handles":["{last}"]); every other string is
  passed unchanged. With no earlier returned handle the run fails and says so."#)]
pub(crate) struct RunArgs {
    #[arg(value_name = "FILE", help = open_help())]
    pub(crate) files: Vec<PathBuf>,
    /// Open the built-in sample drawing
    #[arg(long)]
    pub(crate) sample: bool,
    /// Start a new metric drawing
    #[arg(long)]
    pub(crate) metric: bool,
    /// Command-line script text to run (`\n` separates lines)
    #[arg(long, value_name = "TEXT", action = ArgAction::Append)]
    pub(crate) script: Vec<String>,
    /// A script file (.scr) to run
    #[arg(long, value_name = "F.scr", action = ArgAction::Append)]
    pub(crate) script_file: Vec<PathBuf>,
    /// An engine command and its JSON parameters (default {}); see "--cmd chaining" in --help
    #[arg(long, value_name = "id {json}", action = ArgAction::Append, value_parser = cmd_step)]
    pub(crate) cmd: Vec<CmdStep>,
    #[arg(long, value_name = "OUT.dxf", help = save_help())]
    pub(crate) save: Option<PathBuf>,
    #[arg(long, value_name = "OUT.png", help = export_help())]
    pub(crate) export: Option<PathBuf>,
    #[command(flatten)]
    pub(crate) framing: FramingArgs,
    /// Allow an output that is an input file, replacing the original
    #[arg(long)]
    pub(crate) in_place: bool,
    #[command(flatten)]
    pub(crate) settings: Settings,
    /// The drawings, in command-line order, set by [`parse`].
    #[arg(skip)]
    pub(crate) drawings: Vec<Drawing>,
    /// The steps, in command-line order, set by [`parse`].
    #[arg(skip)]
    pub(crate) steps: Vec<Step>,
}

/// A drawing `run` opens.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Drawing {
    File(PathBuf),
    Sample,
    Metric,
}

/// A `--script`, `--script-file` or `--cmd` of `run`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Step {
    Script(String),
    ScriptFile(PathBuf),
    Cmd(CmdStep),
}

/// `--cmd 'id {json}'`: an engine command and its parameters. `{last}` placeholders are filled in
/// when it runs.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CmdStep {
    pub(crate) id: String,
    pub(crate) params: Value,
}

/// Print the command catalog as JSON
#[derive(Args)]
pub(crate) struct CommandsArgs {
    /// Only the commands whose id or label contains this text (case-insensitive)
    pub(crate) filter: Option<String>,
    /// One line of JSON instead of pretty-printed
    #[arg(long)]
    pub(crate) compact: bool,
}

/// Run the MCP server on stdio: a headless drawing session, or a bridge to the running app
#[derive(Args)]
pub(crate) struct McpArgs {
    /// Bridge to the app's control channel (its --control port, e.g. 127.0.0.1:7979)
    #[arg(long, value_name = "HOST:PORT", value_parser = connect_addr)]
    pub(crate) connect: Option<String>,
    /// Reach the app at the first call instead of at startup, so the server can start before the
    /// app is open
    #[arg(long, requires = "connect")]
    pub(crate) lazy: bool,
    /// Like --lazy, and a call that finds nothing at HOST:PORT opens the app with that control
    /// port first (macOS)
    #[arg(long, requires = "connect")]
    pub(crate) launch: bool,
    #[command(flatten)]
    pub(crate) settings: Settings,
}

/// Time the engine on a synthetic N-entity drawing (default 200000), or the canvas display-list
/// build of a drawing, zoomed in step by step
#[derive(Args)]
pub(crate) struct PerfArgs {
    /// An entity count (clamped to 1-20000000), or a drawing to time
    #[arg(value_name = "N|FILE", value_parser = PathBufValueParser::new().map(perf_target))]
    pub(crate) target: Option<PerfTarget>,
}

/// What `perf` times.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PerfTarget {
    /// A synthetic drawing with this many entities, within [`MAX_PERF_COUNT`].
    Count(usize),
    File(PathBuf),
}

/// The largest synthetic drawing `perf` builds.
const MAX_PERF_COUNT: usize = 20_000_000;

/// `perf N|FILE`: a value that reads as a count is one, clamped to 1..=[`MAX_PERF_COUNT`] so `perf
/// 0` still times something; anything else is a drawing.
fn perf_target(p: PathBuf) -> PerfTarget {
    match p.to_str().and_then(|t| t.parse::<usize>().ok()) {
        Some(n) => PerfTarget::Count(n.clamp(1, MAX_PERF_COUNT)),
        None => PerfTarget::File(p),
    }
}

/// Write a built-in sample drawing
#[derive(Args)]
pub(crate) struct SampleArgs {
    /// Which sample
    #[arg(value_name = "bracket|floorplan", value_enum)]
    pub(crate) name: SampleName,
    #[arg(value_name = "OUT", help = write_help())]
    pub(crate) output: PathBuf,
    #[command(flatten)]
    pub(crate) settings: Settings,
}

/// `sample`'s drawings.
#[derive(Clone, Copy, Debug, PartialEq, ValueEnum)]
pub(crate) enum SampleName {
    Bracket,
    #[value(alias = "floor")]
    Floorplan,
}

/// What part of model space an image export shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Framing {
    /// `run`: the current view if the commands changed it, else the extents. `convert`: extents.
    Auto,
    Extents,
    Current,
    Window(Bounds2),
}

/// Why a `--window` value is refused: clap prints it after the flag and its value (exit 2).
#[derive(Debug, thiserror::Error)]
pub(crate) enum WindowError {
    #[error("expected four numbers x0,y0,x1,y1")]
    NeedsFour,
    #[error("{name} must be a finite number, got `{value}`")]
    NotFinite { name: &'static str, value: String },
    #[error("need x0 < x1 and y0 < y1 (lower-left corner first)")]
    Order,
    #[error(transparent)]
    Limits(#[from] cadcraft_io::IoError),
}

/// Why a value is refused: clap prints it after the flag and its value (exit 2).
#[derive(Debug, thiserror::Error)]
pub(crate) enum ValueError {
    #[error("expected HOST:PORT (the app's --control port, e.g. 127.0.0.1:7979)")]
    Connect,
    #[error("expected a command id, then optionally its JSON parameters")]
    CmdId,
    #[error(transparent)]
    CmdJson(#[from] serde_json::Error),
}

/// Why `parse` refuses flags clap accepted: clap prints it as a usage error (exit 2).
#[derive(Debug, thiserror::Error)]
enum UsageError {
    /// The framing flags choose what an image shows; a drawing file has nothing to frame.
    #[error("{flag} only applies to image output (png, svg, pdf), not `{}`", .out.display())]
    FramingNeedsImage { flag: &'static str, out: PathBuf },
    /// The output is one of the input files, so writing it would replace the original.
    #[error("{} is an input file, so the result would replace the original: choose another output, or pass --in-place to overwrite it", .0.display())]
    OutIsInput(PathBuf),
}

impl UsageError {
    fn into_clap(self, cmd: &mut clap::Command) -> clap::Error {
        let kind = match self {
            Self::FramingNeedsImage { .. } | Self::OutIsInput(_) => ErrorKind::ArgumentConflict,
        };
        cmd.error(kind, self)
    }
}

/// Parse `--window x0,y0,x1,y1` (min corner first). Hostile values are errors, never panics.
pub(crate) fn parse_window(text: &str) -> Result<Bounds2, WindowError> {
    let parts: Vec<&str> = text.split(',').map(str::trim).collect();
    let [x0, y0, x1, y1] = parts.as_slice() else {
        return Err(WindowError::NeedsFour);
    };
    let num = |name: &'static str, v: &str| -> Result<f64, WindowError> {
        match v.parse::<f64>() {
            Ok(n) if n.is_finite() => Ok(n),
            _ => Err(WindowError::NotFinite { name, value: v.to_string() }),
        }
    };
    let (x0, y0, x1, y1) = (num("x0", x0)?, num("y0", y0)?, num("x1", x1)?, num("y1", y1)?);
    if !(x0 < x1 && y0 < y1) {
        return Err(WindowError::Order);
    }
    let w = Bounds2 { min: Vec2::new(x0, y0), max: Vec2::new(x1, y1) };
    cadcraft_io::check_window(&w)?;
    Ok(w)
}

/// `run --cmd 'id {json}'`: the id up to the first space, then JSON parameters (default `{}`).
/// Whether they suit the command is the command's to say.
fn cmd_step(s: &str) -> Result<CmdStep, ValueError> {
    let (id, json) = s.split_once(' ').unwrap_or((s, "{}"));
    if id.is_empty() {
        return Err(ValueError::CmdId);
    }
    Ok(CmdStep { id: id.to_string(), params: serde_json::from_str(json)? })
}

impl FramingArgs {
    /// The framing `--window` or `--view` asks for (clap refuses both together).
    pub(crate) fn framing(&self) -> Framing {
        match (self.window, self.view) {
            (Some(w), _) => Framing::Window(w),
            (None, Some(ViewMode::Extents)) => Framing::Extents,
            (None, Some(ViewMode::Current)) => Framing::Current,
            (None, None) => Framing::Auto,
        }
    }

    /// The framing flag given, if any.
    fn flag(&self) -> Option<&'static str> {
        if self.window.is_some() {
            Some("--window")
        } else if self.view.is_some() {
            Some("--view")
        } else {
            None
        }
    }
}

/// A framing flag given with an output that isn't an image (the extension decides). `run` without
/// `--export` is clap's `requires` error.
fn framing_error(cli: &Cli) -> Option<UsageError> {
    let (framing, out): (&FramingArgs, Option<&Path>) = match &cli.command {
        Command::Convert(c) => (&c.framing, Some(&c.output)),
        Command::Run(r) => (&r.framing, r.export.as_deref()),
        _ => return None,
    };
    let (flag, out) = (framing.flag()?, out?);
    (!cadcraft_io::is_image(out)).then(|| UsageError::FramingNeedsImage { flag, out: out.to_path_buf() })
}

/// An output that is the same file as an input, without `--in-place`. An output that doesn't exist
/// can't be an input, and an I/O error in the comparison is left to the open or write that follows.
fn out_is_input(cli: &Cli) -> Option<UsageError> {
    let same = |input: &Path, out: &Path| out.exists() && same_file::is_same_file(input, out).unwrap_or(false);
    match &cli.command {
        Command::Convert(c) if !c.in_place && same(&c.input, &c.output) => Some(UsageError::OutIsInput(c.output.clone())),
        Command::Run(r) if !r.in_place => {
            let inputs = r.drawings.iter().filter_map(|d| if let Drawing::File(f) = d { Some(f) } else { None });
            let inputs: Vec<&PathBuf> = inputs.collect();
            [&r.save, &r.export].into_iter().flatten().find(|out| inputs.iter().any(|i| same(i, out))).map(|out| UsageError::OutIsInput(out.clone()))
        }
        _ => None,
    }
}

/// The backend `cadcraft-cli mcp` serves from.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum McpTarget {
    /// An in-process drawing session (no `--connect`).
    Headless,
    /// The running app's control channel at `HOST:PORT`; `lazy`: reached at the first call;
    /// `launch`: opened then if it isn't running.
    Connect { addr: String, lazy: bool, launch: bool },
}

impl McpArgs {
    pub(crate) fn target(&self) -> McpTarget {
        match &self.connect {
            // `--launch` connects at the first call too (the app opens then).
            Some(addr) => McpTarget::Connect { addr: addr.clone(), lazy: self.lazy || self.launch, launch: self.launch },
            None => McpTarget::Headless,
        }
    }
}

/// `mcp --connect`: an empty, flag-like or port-less address is an error, never a silent fall back
/// to a headless session (#103). The host is not resolved here.
fn connect_addr(s: &str) -> Result<String, ValueError> {
    let s = s.trim();
    let valid = !s.starts_with('-')
        && s.rsplit_once(':').is_some_and(|(host, port)| {
            let host = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
            !host.is_empty() && port.parse::<u16>().is_ok()
        });
    if valid { Ok(s.to_string()) } else { Err(ValueError::Connect) }
}

/// The values of `id` with their command-line indices.
fn indexed<'a, T: Clone + Send + Sync + 'static>(m: &'a ArgMatches, id: &str) -> impl Iterator<Item = (usize, &'a T)> {
    let at = m.indices_of(id).into_iter().flatten();
    let values = m.try_get_many::<T>(id).ok().flatten().into_iter().flatten();
    at.zip(values)
}

/// The command-line index of flag `id`, when it was given.
fn flag_index(m: &ArgMatches, id: &str) -> Option<usize> {
    if m.value_source(id) == Some(ValueSource::CommandLine) { m.indices_of(id).and_then(Iterator::last) } else { None }
}

/// `run`'s drawings and steps back in command-line order: clap keeps each flag's values apart,
/// their indices say how they interleave.
fn run_order(m: &ArgMatches) -> (Vec<Drawing>, Vec<Step>) {
    let mut drawings: Vec<(usize, Drawing)> = indexed(m, "files").map(|(i, f): (_, &PathBuf)| (i, Drawing::File(f.clone()))).collect();
    drawings.extend(flag_index(m, "sample").map(|i| (i, Drawing::Sample)));
    drawings.extend(flag_index(m, "metric").map(|i| (i, Drawing::Metric)));
    drawings.sort_by_key(|d| d.0);
    let mut steps: Vec<(usize, Step)> = indexed(m, "script").map(|(i, s): (_, &String)| (i, Step::Script(s.clone()))).collect();
    steps.extend(indexed(m, "script_file").map(|(i, s): (_, &PathBuf)| (i, Step::ScriptFile(s.clone()))));
    steps.extend(indexed(m, "cmd").map(|(i, c): (_, &CmdStep)| (i, Step::Cmd(c.clone()))));
    steps.sort_by_key(|s| s.0);
    (drawings.into_iter().map(|d| d.1).collect(), steps.into_iter().map(|s| s.1).collect())
}

/// Parse the arguments (without the binary name). clap reads them as `OsString`s: a path need not
/// be valid Unicode, text must. A bare `--help` or `-h` is help even after a mistake on the same
/// line (`run --script --help`): clap reports the mistake first.
pub(crate) fn parse(args: &[OsString]) -> Result<Cli, clap::Error> {
    parse_with(command(|k| std::env::var_os(k)), args)
}

/// The command line. An empty or blank variable counts as unset, except `CADCRAFT_CONFIG_DIR`:
/// empty means no settings folder. `var` looks a variable up.
fn command(var: impl Fn(&OsStr) -> Option<OsString> + Copy) -> clap::Command {
    let blank = move |a: clap::Arg| if a.get_id() == "config_dir" { a } else { blank_env_is_unset(a, var) };
    Cli::command().mut_args(blank).mut_subcommands(|s| s.mut_args(blank))
}

/// [`parse`] with `cmd` from [`command`]; a bad value from a variable is a usage error naming the
/// variable.
fn parse_with(mut cmd: clap::Command, args: &[OsString]) -> Result<Cli, clap::Error> {
    let argv = std::iter::once(OsString::from("cadcraft-cli")).chain(args.iter().cloned());
    let matches = match cmd.try_get_matches_from_mut(argv) {
        // `use_stderr`: a real error, not the help `-h` asked for (a retry would make it `--help`).
        Err(e) if e.use_stderr() && args.iter().take_while(|a| *a != "--").any(|a| *a == "--help" || *a == "-h") => {
            let help = [OsString::from("cadcraft-cli"), args.first().cloned().unwrap_or_default(), OsString::from("--help")];
            return Err(cmd.try_get_matches_from_mut(help).err().unwrap_or(e));
        }
        matches => matches.map_err(|e| name_env_vars(e, &cmd, args))?,
    };
    let mut cli = Cli::from_arg_matches(&matches)?;
    if let (Command::Run(r), Some((_, m))) = (&mut cli.command, matches.subcommand()) {
        (r.drawings, r.steps) = run_order(m);
    }
    if let Some(e) = framing_error(&cli).or_else(|| out_is_input(&cli)) {
        if let Some(sub) = matches.subcommand_name().and_then(|n| cmd.find_subcommand_mut(n)) {
            return Err(e.into_clap(sub));
        }
        return Err(e.into_clap(&mut cmd));
    }
    Ok(cli)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse_args(a: &[&str]) -> Result<Cli, clap::Error> {
        parse(&a.iter().map(OsString::from).collect::<Vec<_>>())
    }

    fn mcp(a: &[&str]) -> Result<McpTarget, clap::Error> {
        let args: Vec<&str> = std::iter::once("mcp").chain(a.iter().copied()).collect();
        match parse_args(&args)?.command {
            Command::Mcp(m) => Ok(m.target()),
            _ => panic!("{a:?} is not mcp"),
        }
    }

    /// Every file argument's help names each format of the list it uses.
    #[test]
    fn help_names_every_format() {
        let cli = Cli::command();
        let help = |sub: &str, arg: &str| {
            let a = cli.find_subcommand(sub).and_then(|c| c.get_arguments().find(|a| a.get_id() == arg)).expect("argument exists");
            a.get_help().map(|h| h.to_string()).unwrap_or_default()
        };
        let cases: [(&str, &str, &[Format]); 8] = [
            ("info", "file", Format::READ),
            ("convert", "input", Format::READ),
            ("convert", "output", Format::WRITE),
            ("run", "files", Format::READ),
            ("run", "save", Format::WRITE),
            ("run", "export", Format::WRITE),
            ("sample", "output", Format::WRITE),
            ("convert", "", Format::WRITE),
        ];
        for (sub, arg, list) in cases {
            let text = if arg.is_empty() {
                cli.find_subcommand(sub).and_then(|c| c.get_about()).map(|a| a.to_string()).unwrap_or_default()
            } else {
                help(sub, arg)
            };
            for f in list {
                assert!(text.contains(f.extension()), "`{sub} {arg}` help `{text}` lacks {}", f.extension());
            }
        }
    }

    #[test]
    fn mcp_without_connect_is_headless() {
        assert_eq!(mcp(&[]).unwrap(), McpTarget::Headless);
    }

    #[test]
    fn mcp_connect_with_address_selects_remote() {
        let to = |addr: &str, lazy, launch| McpTarget::Connect { addr: addr.into(), lazy, launch };
        assert_eq!(mcp(&["--connect", "127.0.0.1:7979"]).unwrap(), to("127.0.0.1:7979", false, false));
        assert_eq!(mcp(&["--connect", " localhost:7979 "]).unwrap(), to("localhost:7979", false, false));
        assert_eq!(mcp(&["--lazy", "--connect", "localhost:7979"]).unwrap(), to("localhost:7979", true, false));
        assert_eq!(mcp(&["--connect", "localhost:7979", "--launch"]).unwrap(), to("localhost:7979", true, true));
        assert_eq!(mcp(&["--connect", "[::1]:7979"]).unwrap(), to("[::1]:7979", false, false));
        // Without an app to reach they mean nothing: never a headless session.
        for flag in ["--lazy", "--launch"] {
            assert_eq!(mcp(&[flag]).unwrap_err().kind(), clap::error::ErrorKind::MissingRequiredArgument, "{flag}");
        }
    }

    #[test]
    fn mcp_connect_without_address_is_an_error_not_headless() {
        for bad in [
            &["--connect"][..],
            &["--connect", ""],
            &["--connect", "  "],
            &["--connect=-v"],
            &["--connect", "example.com"],
            &["--connect", ":7979"],
            &["--connect", "host:99999"],
            &["--connect", "[]:1"],
        ] {
            let r = mcp(bad);
            let Err(e) = r else { panic!("{bad:?} should be rejected, got {r:?}") };
            assert_eq!(e.exit_code(), 2, "{bad:?}");
            assert!(e.to_string().contains("--connect <HOST:PORT>"), "{bad:?}: {e}");
        }
    }

    #[test]
    fn mcp_rejects_unknown_and_repeated_arguments() {
        assert!(mcp(&["--conect", "127.0.0.1:7979"]).is_err());
        assert!(mcp(&["127.0.0.1:7979"]).is_err());
        // clap names the unknown flag, not the value it left missing.
        for flag in ["--foo", "-v"] {
            assert_eq!(mcp(&["--connect", flag]).unwrap_err().kind(), clap::error::ErrorKind::UnknownArgument, "{flag}");
        }
        assert!(mcp(&["--connect", "127.0.0.1:1", "--connect", "127.0.0.1:2"]).is_err());
    }

    #[test]
    fn run_keeps_command_line_order() {
        let args =
            ["run", "a.dxf", "--script", "S1", "--metric", "--cmd", "c1", "b.dxf", "--script-file", "f.scr", "--sample", "--cmd", "c2 {\"n\":1}"];
        let Command::Run(r) = parse_args(&args).unwrap().command else { panic!("not run") };
        let file = |f: &str| Drawing::File(f.into());
        let cmd = |id: &str, params| Step::Cmd(CmdStep { id: id.into(), params });
        assert_eq!(r.drawings, [file("a.dxf"), Drawing::Metric, file("b.dxf"), Drawing::Sample]);
        assert_eq!(r.steps, [Step::Script("S1".into()), cmd("c1", json!({})), Step::ScriptFile("f.scr".into()), cmd("c2", json!({"n": 1}))]);
    }

    #[test]
    fn cmd_values() {
        assert_eq!(cmd_step("line").unwrap(), CmdStep { id: "line".into(), params: json!({}) });
        assert_eq!(cmd_step("line [1]").unwrap(), CmdStep { id: "line".into(), params: json!([1]) });
        for bad in ["", " {}", "line {bad json", "line "] {
            assert!(cmd_step(bad).is_err(), "`{bad}` should be rejected");
        }
    }

    #[test]
    fn perf_targets() {
        let target = |a: &[&str]| -> Option<PerfTarget> {
            let args: Vec<&str> = std::iter::once("perf").chain(a.iter().copied()).collect();
            let Command::Perf(p) = parse_args(&args).unwrap().command else { panic!("not perf") };
            p.target
        };
        assert_eq!(target(&[]), None);
        assert_eq!(target(&["5"]), Some(PerfTarget::Count(5)));
        assert_eq!(target(&["0"]), Some(PerfTarget::Count(1)));
        assert_eq!(target(&["999999999"]), Some(PerfTarget::Count(MAX_PERF_COUNT)));
        assert_eq!(target(&["a.dxf"]), Some(PerfTarget::File("a.dxf".into())));
        assert_eq!(target(&["1e3"]), Some(PerfTarget::File("1e3".into())));
    }

    #[test]
    fn framing_options() {
        let framing = |out: &str, a: &[&str]| -> Result<Framing, clap::Error> {
            let args: Vec<&str> = ["convert", "a.dxf", out].iter().chain(a).copied().collect();
            let Command::Convert(c) = parse_args(&args)?.command else { panic!("not convert") };
            Ok(c.framing.framing())
        };
        let b = |x0, y0, x1, y1| Bounds2::new(Vec2::new(x0, y0), Vec2::new(x1, y1));
        assert_eq!(framing("b.png", &[]).unwrap(), Framing::Auto);
        assert_eq!(framing("b.png", &["--view", "extents"]).unwrap(), Framing::Extents);
        assert_eq!(framing("b.png", &["--view", "CURRENT"]).unwrap(), Framing::Current);
        assert_eq!(framing("b.png", &["--window", "0,0,2,1"]).unwrap(), Framing::Window(b(0.0, 0.0, 2.0, 1.0)));
        assert_eq!(framing("b.dxf", &[]).unwrap(), Framing::Auto);
        let kind = |out: &str, a: &[&str]| framing(out, a).map(|_| ()).unwrap_err().kind();
        assert_eq!(kind("b.png", &["--view", "selection"]), ErrorKind::InvalidValue);
        assert_eq!(kind("b.png", &["--window", "0,0,2,1", "--view", "extents"]), ErrorKind::ArgumentConflict);
        assert_eq!(kind("b.dxf", &["--view", "extents"]), ErrorKind::ArgumentConflict);
        assert_eq!(kind("b.dxf", &["--window", "0,0,2,1"]), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn window_parses() {
        let b = |x0, y0, x1, y1| Bounds2::new(Vec2::new(x0, y0), Vec2::new(x1, y1));
        assert_eq!(parse_window("0,0,30,20").unwrap(), b(0.0, 0.0, 30.0, 20.0));
        assert_eq!(parse_window(" -5.5, -1e3 ,2e3,4 ").unwrap(), b(-5.5, -1000.0, 2000.0, 4.0));
    }

    #[test]
    fn hostile_window_is_an_error() {
        for w in [
            "",
            "1,2,3",
            "1,2,3,4,5",
            "a,0,1,1",
            "NaN,0,1,1",
            "0,0,inf,1",
            "0,0,1e400,1",
            "0,0,0,1",
            "0,0,1,0",
            "5,0,1,1",
            "0,5,1,1",
            "-1e300,-1e300,1e300,1e300",
            "0,0,1e-12,1e-12",
        ] {
            assert!(parse_window(w).is_err(), "`{w}` should be rejected");
        }
    }

    /// The subcommands that draw or run commands take the settings flags; the others refuse them.
    #[test]
    fn settings_flags() {
        let Command::Convert(c) =
            parse_args(&["convert", "a.dxf", "b.png", "--config-dir", "/p", "--fontalt", "Noto Sans", "--fontfallback="]).unwrap().command
        else {
            panic!("not convert")
        };
        assert_eq!(c.settings.config_dir, Some(PathBuf::from("/p")));
        assert_eq!((c.settings.fontalt.as_deref(), c.settings.fontfallback.as_deref()), (Some("Noto Sans"), Some("")));
        for empty in ["--config-dir=", "--config-dir= "] {
            let Command::Run(r) = parse_args(&["run", "--sample", empty]).unwrap().command else { panic!("not run") };
            assert_eq!(r.settings.settings_dir(), None, "`{empty}` is no settings folder");
        }
        let Command::Convert(c) = parse_args(&["convert", "a.dxf", "b.png", "--config-dir", "/p"]).unwrap().command else { panic!("not convert") };
        assert_eq!(c.settings.settings_dir(), Some(PathBuf::from("/p")));
        assert!(parse_args(&["sample", "bracket", "b.dxf", "--fontalt", "x"]).is_ok());
        assert!(parse_args(&["mcp", "--config-dir", "/p"]).is_ok());
        assert!(parse_args(&["info", "a.dxf", "--config-dir", "/p"]).is_err());
    }

    #[test]
    fn blank_variables_count_as_unset_except_the_config_dir() {
        let cmd = command(|_| Some("".into()));
        for sub in cmd.get_subcommands().filter(|s| s.get_arguments().any(|a| a.get_id() == "fontalt")) {
            let env = |id: &str| sub.get_arguments().find(|a| a.get_id() == id).and_then(|a| a.get_env());
            assert_eq!((env("fontalt"), env("fontfallback")), (None, None), "{}", sub.get_name());
            assert_eq!(env("config_dir"), Some(OsStr::new("CADCRAFT_CONFIG_DIR")), "{}", sub.get_name());
        }
    }

    /// `--fontalt` falls back to `PATH` and rejects every value here, as a bad value from a
    /// variable would; tests can't set a variable (`set_var` is unsafe).
    #[test]
    fn a_bad_value_from_a_variable_names_the_variable() {
        // Cargo sets CARGO_PKG_NAME for every test binary, so the variable always has a value.
        let reject = clap::builder::PossibleValuesParser::new(["never"]);
        let cmd = || Cli::command().mut_subcommand("sample", |s| s.mut_arg("fontalt", |a| a.env("CARGO_PKG_NAME").value_parser(reject.clone())));
        let args = |a: &[&str]| a.iter().map(OsString::from).collect::<Vec<_>>();
        let text = parse_with(cmd(), &args(&["sample"])).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(text.contains("for 'CARGO_PKG_NAME'") && !text.contains("--fontalt"), "{text}");
        let text = parse_with(cmd(), &args(&["sample", "--fontalt", "x"])).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(text.contains("--fontalt"), "{text}");
    }
}
