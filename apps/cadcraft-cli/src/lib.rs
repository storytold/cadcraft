//! `cadcraft-cli`: headless CADCraft. The subcommands and their options are the clap types in
//! `args.rs`; `cadcraft-cli --help` lists them.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::ffi::OsString;
use std::io::Write;

use cadcraft_engine::Session;
use serde_json::json;

mod args;
mod commands;
mod convert;
mod error;
mod info;
mod mcp;
mod output;
mod perf;
mod run;
mod sample;
mod store;

#[cfg(test)]
use args::Framing;
use args::{Command, parse};
use clap::CommandFactory;
use commands::commands;
use convert::convert;
use error::Result;
use info::info;
use mcp::mcp;
use output::Streams;
#[cfg(test)]
use output::{export_window, model_views, write_atomic};
use perf::perf;
use run::run_cmds;
#[cfg(test)]
use run::{fill_last, returned_handle};
use sample::sample;
use store::{store_list, store_probe, store_read, store_recover, store_remove, store_stat, store_write};

fn install_io() {
    cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: Some(|d, space, opts| cadcraft_io::plotter::plot(d, space, opts).map_err(|e| e.to_string())),
    });
    cadcraft_engine::cmd::file::set_write_as(|d, name, o| {
        cadcraft_io::write_with(d, name, &cadcraft_io::SaveOptions { version: o.version, binary: o.binary }).map_err(|e| e.to_string())
    });
    // AutoLISP's entget, entmake and entmod see entities as DXF groups.
    cadcraft_engine::cmd::file::set_entity_codec(cadcraft_engine::cmd::file::EntityCodec {
        groups: cadcraft_io::entity_groups,
        entity: cadcraft_io::entity_from_groups,
    });
}

/// `path` as text, for the engine and io calls that take a file name as a string.
fn unicode(path: &std::path::Path) -> Result<&str> {
    path.to_str().ok_or_else(|| error::CliError::NotUnicode(path.to_path_buf()))
}

fn open(s: &mut Session, path: &std::path::Path) -> Result {
    s.execute("open", &json!({ "path": unicode(path)? }))?;
    Ok(())
}

/// The command line's clap definition, for generating shell completions and man pages.
pub fn command() -> clap::Command {
    args::Cli::command()
}

/// Run the CLI on its arguments (without the program name). Returns the process exit code (0 ok,
/// 1 failure, 2 usage). clap reads the arguments as they are: a value that is not valid Unicode
/// is a usage error, never a panic as with `std::env::args()`.
pub fn run<I, T>(args: I, out: &mut dyn Write, err: &mut dyn Write) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    // Help and `--version` (`help`, `-h`, `<subcommand> --help`, `-V`) come back as errors too,
    // for `out`, exit 0: the subcommand never runs.
    let cli = match parse(&args) {
        Ok(cli) => cli,
        Err(e) => {
            let to: &mut dyn Write = if e.use_stderr() { err } else { out };
            return match write!(to, "{}", e.render()) {
                Err(w) if w.kind() != std::io::ErrorKind::BrokenPipe => {
                    let _ = writeln!(err, "error: can't write the output: {w}");
                    1
                }
                _ => e.exit_code(),
            };
        }
    };
    // The engine's hooks are set-once, so installing them on every call is harmless.
    install_io();
    match &cli.command {
        Command::Convert(a) => a.settings.apply(),
        Command::Run(a) => a.settings.apply(),
        Command::Mcp(a) => a.settings.apply(),
        Command::Sample(a) => a.settings.apply(),
        Command::Info(_)
        | Command::Commands(_)
        | Command::Perf(_)
        | Command::StoreWrite(_)
        | Command::StoreRead(_)
        | Command::StoreStat(_)
        | Command::StoreList(_)
        | Command::StoreRemove(_)
        | Command::StoreProbe(_)
        | Command::StoreRecover(_) => {}
    }
    let mut io = Streams::new(out, err);
    let result = match &cli.command {
        Command::Info(a) => info(a, &mut io),
        Command::Convert(a) => convert(a, &mut io),
        Command::Run(a) => run_cmds(a, &mut io),
        Command::Commands(a) => commands(a, &mut io),
        Command::Mcp(a) => mcp(a),
        Command::Perf(a) => perf(a.target.as_ref(), &mut io),
        Command::Sample(a) => sample(a, &mut io),
        Command::StoreWrite(a) => store_write(a, &mut io),
        Command::StoreRead(a) => store_read(a, &mut io),
        Command::StoreStat(a) => store_stat(a, &mut io),
        Command::StoreList(a) => store_list(a, &mut io),
        Command::StoreRemove(a) => store_remove(a, &mut io),
        Command::StoreProbe(a) => store_probe(a, &mut io),
        Command::StoreRecover(a) => store_recover(a, &mut io),
    };
    let mut code = 0;
    if let Err(e) = result {
        io.errln(format_args!("error: {e}"));
        code = 1;
    }
    io.finish();
    if io.stdout_failed() { 1 } else { code }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_engine::doc::Space;
    use cadcraft_engine::geom::{Bounds2, Vec2};

    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cadcraft-cli-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    struct FailingWriter(std::io::ErrorKind);

    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(self.0.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_failed_write_of_help_exits_1_unless_the_pipe_is_closed() {
        let mut err = Vec::new();
        assert_eq!(run(["--help"], &mut FailingWriter(std::io::ErrorKind::StorageFull), &mut err), 1);
        assert!(String::from_utf8_lossy(&err).starts_with("error: "));
        assert_eq!(run(["--help"], &mut FailingWriter(std::io::ErrorKind::BrokenPipe), &mut Vec::new()), 0);
    }

    #[test]
    fn write_atomic_replaces_file_and_leaves_no_staging_files() {
        let dir = scratch_dir("replace");
        let out = dir.join("a.dxf");
        write_atomic(&out, b"old").expect("create");
        write_atomic(&out, b"new").expect("overwrite");
        assert_eq!(std::fs::read(&out).expect("read"), b"new");
        assert_eq!(std::fs::read_dir(&dir).expect("list").count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_atomic_failure_keeps_destination_and_cleans_up() {
        let dir = scratch_dir("fail");
        let out = dir.join("target");
        std::fs::create_dir(&out).expect("dir as destination");
        std::fs::write(out.join("keep"), b"x").expect("seed");
        assert!(write_atomic(&out, b"new").is_err());
        assert_eq!(std::fs::read(out.join("keep")).expect("kept"), b"x");
        assert_eq!(std::fs::read_dir(&dir).expect("list").count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn b(x0: f64, y0: f64, x1: f64, y1: f64) -> Bounds2 {
        Bounds2::new(Vec2::new(x0, y0), Vec2::new(x1, y1))
    }

    fn sample_session() -> Session {
        let mut s = Session::empty();
        s.open_drawing(cadcraft_engine::sample::default_sample(), "Bracket", None);
        s
    }

    fn view_box(svg: &str) -> String {
        let i = svg.find("viewBox=\"").map(|i| i + 9).unwrap_or(0);
        svg[i..].split('"').next().unwrap_or("").to_string()
    }

    fn assert_near(a: Bounds2, e: Bounds2) {
        let d = (a.min - e.min).len().max((a.max - e.max).len());
        assert!(d < 1e-9, "{a:?} != {e:?}");
    }

    #[test]
    fn last_placeholder_takes_the_previous_handle() {
        let mut s = sample_session();
        let r = s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
        let h = returned_handle(&s, &r).expect("LINE returns its handle");
        assert_eq!(fill_last(&json!({"handle": "{last}"}), Some(h)).unwrap(), json!({"handle": h.hex()}));
        // Nested, as in a selection list.
        assert_eq!(fill_last(&json!({"handles": ["{last}", "1"]}), Some(h)).unwrap(), json!({"handles": [h.hex(), "1"]}));
        // Only the exact token: a name or a sentence that merely contains it stays as written.
        for p in [json!("last"), json!("{last} "), json!({"name": "last"})] {
            assert_eq!(fill_last(&p, Some(h)).unwrap(), p);
        }
        // Nothing returned yet: an error, not a token passed to the command as a bad handle.
        assert!(fill_last(&json!({"handle": "{last}"}), None).unwrap_err().to_string().contains("{last}"));
    }

    /// Handles are read from a result by looking the strings up in the drawing: counts and names
    /// are not mistaken for handles, and `mview`'s `viewports` works like `handle`/`handles`.
    #[test]
    fn returned_handle_reads_only_real_handles() {
        let mut s = Session::empty();
        s.new_drawing(false);
        s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
        let r = s.execute("mview", &json!({"p1": [20, 20], "p2": [200, 150]})).unwrap();
        let h = returned_handle(&s, &r).expect("mview returns its viewport handle");
        assert!(r["viewports"].as_array().is_some_and(|a| a.iter().any(|v| v.as_str() == Some(h.hex().as_str()))), "{r}");
        assert_eq!(returned_handle(&s, &json!({"changed": 3, "name": "A1"})), None);
        // The most recent of several.
        let a = s.execute("line", &json!({"points": [[0, 0], [1, 0]]})).unwrap();
        let b = s.execute("line", &json!({"points": [[0, 0], [2, 0]]})).unwrap();
        let (ha, hb) = (returned_handle(&s, &a).unwrap(), returned_handle(&s, &b).unwrap());
        assert_eq!(returned_handle(&s, &json!({"handles": [ha.hex(), hb.hex()]})), Some(hb));
    }

    #[test]
    fn default_framing_is_extents() {
        // convert: nothing ran, always the extents.
        let s = sample_session();
        assert_eq!(export_window(&s, Framing::Auto, None).unwrap(), None);
        // run: commands that leave the view alone keep the extents framing.
        let mut s = sample_session();
        let before = model_views(&s);
        s.script("LINE 0,0 10,10\n\n").unwrap();
        assert_eq!(export_window(&s, Framing::Auto, Some(&before)).unwrap(), None);
        // A fresh drawing too.
        let mut s = Session::empty();
        s.new_drawing(false);
        let before = model_views(&s);
        s.execute("circle", &json!({"center": [0, 0], "radius": 5})).unwrap();
        assert_eq!(export_window(&s, Framing::Auto, Some(&before)).unwrap(), None);
    }

    #[test]
    fn zoom_window_frames_export() {
        let mut s = sample_session();
        let before = model_views(&s);
        s.execute("zoom", &json!({"mode": "window", "p1": [0, 0], "p2": [30, 20]})).unwrap();
        let w = export_window(&s, Framing::Auto, Some(&before)).unwrap().unwrap();
        // The CLI viewport is 3:2, so a 3:2 window is shown exactly.
        assert_near(w, b(0.0, 0.0, 30.0, 20.0));
        assert_eq!(export_window(&s, Framing::Current, Some(&before)).unwrap(), Some(w));
        // --view extents and --window override the zoom.
        assert_eq!(export_window(&s, Framing::Extents, Some(&before)).unwrap(), None);
        let explicit = b(1.0, 1.0, 2.0, 2.0);
        assert_eq!(export_window(&s, Framing::Window(explicit), Some(&before)).unwrap(), Some(explicit));
        // A taller window keeps its height; the 3:2 image adds room left and right.
        s.execute("zoom", &json!({"mode": "center", "center": [5, 5], "height": 4})).unwrap();
        assert_near(export_window(&s, Framing::Auto, Some(&before)).unwrap().unwrap(), b(2.0, 3.0, 8.0, 7.0));
    }

    #[test]
    fn zoom_object_frames_the_selection() {
        let mut s = sample_session();
        let before = model_views(&s);
        // The smallest entity with an area, selected then framed with ZOOM Object.
        let d = s.doc().unwrap();
        let bounds = |e: &cadcraft_engine::doc::Entity| cadcraft_engine::doc::entity_bounds(d, e, 0);
        let e = d
            .model
            .iter()
            .filter(|e| bounds(e).width() > 0.0 && bounds(e).height() > 0.0)
            .min_by(|a, b| bounds(a).width().total_cmp(&bounds(b).width()))
            .unwrap();
        let (h, eb, ext) = (e.handle, bounds(e), d.extents(&Space::Model));
        s.set_selection(vec![h]);
        s.execute("zoom", &json!({"mode": "object"})).unwrap();
        let w = export_window(&s, Framing::Auto, Some(&before)).unwrap().unwrap();
        assert!(w.contains(eb.min) && w.contains(eb.max));
        assert!(w.width() < ext.width() && w.height() < ext.height(), "{w:?} vs extents {ext:?}");
    }

    #[test]
    fn run_export_honours_zoom() {
        let dir = std::env::temp_dir().join(format!("cadcraft-cli-165-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = |n: &str| dir.join(n).to_string_lossy().to_string();
        let run_svg = |extra: &[&str], out: &str| -> String {
            let mut args: Vec<String> = ["run", "--sample"].iter().chain(extra).map(|a| a.to_string()).collect();
            args.extend(["--export".to_string(), path(out)]);
            assert_eq!(run(args, &mut Vec::new(), &mut Vec::new()), 0);
            std::fs::read_to_string(path(out)).unwrap()
        };
        let base = run_svg(&[], "base.svg");
        let zoomed = run_svg(&["--cmd", r#"zoom {"mode":"window","p1":[0,0],"p2":[30,20]}"#], "zoom.svg");
        let window = run_svg(&["--window", "0,0,30,20"], "window.svg");
        let forced = run_svg(&["--cmd", r#"zoom {"mode":"window","p1":[0,0],"p2":[30,20]}"#, "--view", "extents"], "forced.svg");
        assert_ne!(view_box(&zoomed), view_box(&base));
        assert_eq!(view_box(&zoomed), "0 0 30.000000 20.000000");
        assert_eq!(zoomed, window);
        assert_eq!(forced, base);
        // Hostile --window: a usage error, nothing written.
        let mut err = Vec::new();
        assert_eq!(run(["run", "--sample", "--window", "0,0,NaN,1", "--export", &path("bad.png")], &mut Vec::new(), &mut err), 2);
        assert!(String::from_utf8_lossy(&err).contains("--window"));
        assert!(!dir.join("bad.png").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
