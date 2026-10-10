//! `cadcraft-cli`: headless CADCraft.
//!
//! ```text
//! cadcraft-cli info FILE.dxf                       summary as JSON
//! cadcraft-cli convert IN.dxf OUT.(dxf|svg|png) [--window x0,y0,x1,y1 | --view extents|current]
//! cadcraft-cli run [FILE|--sample|--metric] [--script TEXT|--script-file F.scr] [--cmd 'id {json}']... [--save OUT] [--export OUT.png] [--window ... | --view ...]
//! cadcraft-cli commands [FILTER]                   command catalog (JSON)
//! cadcraft-cli mcp [--connect HOST:PORT]           MCP server on stdio (headless or bridged to the app)
//! cadcraft-cli perf [N]                            timing table on a synthetic N-entity drawing
//! cadcraft-cli sample (bracket|floorplan) OUT       write a built-in sample drawing
//! ```
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::process::ExitCode;

use cadcraft_engine::doc::Space;
use cadcraft_engine::geom::{Bounds2, Vec2};
use cadcraft_engine::{Session, View};
use serde_json::{Value, json};

const USAGE: &str = "usage:
  cadcraft-cli info FILE.dxf
  cadcraft-cli convert IN.dxf OUT.(dxf|dwg|svg|png|pdf) [FRAMING]
  cadcraft-cli run [FILE | --sample | --metric] [--script TEXT] [--script-file F.scr] [--cmd 'id {json}']... [--save OUT.dxf] [--export OUT.(png|svg|pdf)] [FRAMING]
  cadcraft-cli commands [FILTER]
  cadcraft-cli mcp [--connect HOST:PORT]
  cadcraft-cli perf [N]
  cadcraft-cli sample (bracket|floorplan) OUT.(dxf|dwg|svg|png)
  cadcraft-cli --version

image framing (png 2400x1600, svg, pdf; always model space):
  default                  convert: fit the drawing extents.
                           run --export: the current view if the script/commands changed it
                           (zoom, pan, view.set...), otherwise fit the drawing extents.
  --window x0,y0,x1,y1     show this rectangle (drawing units, x0<x1, y0<y1), centred and
                           scaled to fit the image without distortion.
  --view extents           always fit the drawing extents.
  --view current           the current view (as last set by zoom/pan/view.set; a freshly
                           opened drawing's view is its extents plus a 10% margin).";

/// What part of model space an image export shows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Framing {
    /// `run`: the current view if the commands changed it, else the extents. `convert`: extents.
    Auto,
    Extents,
    Current,
    Window(Bounds2),
}

/// Parse `--window x0,y0,x1,y1` (min corner first). Hostile values are errors, never panics.
fn parse_window(text: &str) -> Result<Bounds2, String> {
    let parts: Vec<&str> = text.split(',').map(str::trim).collect();
    let [x0, y0, x1, y1] = parts.as_slice() else {
        return Err(format!("--window needs four numbers x0,y0,x1,y1, got `{text}`"));
    };
    let num = |name: &str, v: &str| -> Result<f64, String> {
        match v.parse::<f64>() {
            Ok(n) if n.is_finite() => Ok(n),
            _ => Err(format!("--window: {name} must be a finite number, got `{v}`")),
        }
    };
    let (x0, y0, x1, y1) = (num("x0", x0)?, num("y0", y0)?, num("x1", x1)?, num("y1", y1)?);
    if !(x0 < x1 && y0 < y1) {
        return Err(format!("--window: need x0 < x1 and y0 < y1 (lower-left corner first), got {x0},{y0},{x1},{y1}"));
    }
    let w = Bounds2 { min: Vec2::new(x0, y0), max: Vec2::new(x1, y1) };
    cadcraft_io::check_window(&w).map_err(|e| format!("--window: {e}"))?;
    Ok(w)
}

/// Parse a framing option (`--window`, `--view`) at `a`, taking its value from `it`. `Ok(false)`
/// means `a` is not a framing option.
fn framing_option<'a>(a: &str, it: &mut impl Iterator<Item = &'a String>, framing: &mut Framing) -> Result<bool, String> {
    let f = match a {
        "--window" => Framing::Window(parse_window(it.next().ok_or("--window needs x0,y0,x1,y1")?)?),
        "--view" => match it.next().map(|v| v.to_ascii_lowercase()).as_deref() {
            Some("extents") => Framing::Extents,
            Some("current") => Framing::Current,
            _ => return Err("--view needs `extents` or `current`".into()),
        },
        _ => return Ok(false),
    };
    if *framing != Framing::Auto && std::mem::discriminant(framing) != std::mem::discriminant(&f) {
        return Err("use only one of --window and --view".into());
    }
    *framing = f;
    Ok(true)
}

/// The model-space view of every open drawing, by drawing id (to tell whether commands changed it).
fn model_views(s: &Session) -> Vec<(u64, Option<View>)> {
    s.docs.iter().map(|st| (st.uid, st.views.iter().find(|(sp, _)| *sp == Space::Model).map(|(_, v)| *v))).collect()
}

/// The active drawing's current model-space view as a rectangle with the viewport's aspect ratio
/// (the CLI viewport is 3:2 like the PNG export, so the image shows exactly this view).
fn current_window(s: &Session) -> Option<Bounds2> {
    let st = s.state().ok()?;
    let v = st.views.iter().find(|(sp, _)| *sp == Space::Model).map(|(_, v)| *v)?;
    let (w, h) = s.viewport_px;
    let half = Vec2::new(v.height * w / h.max(1.0), v.height) * 0.5;
    Some(Bounds2::new(v.center - half, v.center + half))
}

/// The window an image export shows, or `None` to fit the extents. `before` is [`model_views`]
/// taken before the commands ran (`None` for `convert`, where nothing ran).
fn export_window(s: &Session, framing: Framing, before: Option<&[(u64, Option<View>)]>) -> Result<Option<Bounds2>, String> {
    match framing {
        Framing::Window(w) => Ok(Some(w)),
        Framing::Extents => Ok(None),
        Framing::Current => current_window(s).map(Some).ok_or_else(|| "--view current: the drawing has no model-space view".into()),
        Framing::Auto => {
            let Some(before) = before else { return Ok(None) };
            let Ok(st) = s.state() else { return Ok(None) };
            let now = model_views(s);
            let was = before.iter().find(|(uid, _)| *uid == st.uid);
            let is = now.iter().find(|(uid, _)| *uid == st.uid);
            // Only a view the commands changed counts; a drawing opened by the commands keeps
            // the extents framing.
            match (was, is) {
                (Some(a), Some(b)) if a.1 != b.1 => Ok(current_window(s)),
                _ => Ok(None),
            }
        }
    }
}

fn install_io() {
    cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: Some(|d, space, opts| cadcraft_io::plot(d, space, opts).map_err(|e| e.to_string())),
    });
}

fn open(s: &mut Session, path: &str) -> Result<(), String> {
    s.execute("open", &json!({ "path": path })).map(|_| ()).map_err(|e| e.to_string())
}

#[derive(Clone, Copy)]
struct OptionSpec {
    name: &'static str,
    takes_value: bool,
}

fn validate_options(command: &str, args: &[String], specs: &[OptionSpec]) -> Result<(), String> {
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        if let Some(name) = arg.strip_prefix("--") {
            let Some(spec) = specs.iter().find(|spec| spec.name == arg) else {
                return Err(format!("{command}: unknown option --{name}"));
            };
            if spec.takes_value && args.get(i + 1).is_some_and(|value| !value.starts_with("--")) {
                i += 2;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    Ok(())
}

const NO_OPTIONS: &[OptionSpec] = &[];

fn info(path: &str) -> Result<(), String> {
    let mut s = Session::empty();
    open(&mut s, path)?;
    let v = s.execute("drawing.inspect", &json!({ "entities": false })).map_err(|e| e.to_string())?;
    println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    Ok(())
}

fn export(s: &Session, out: &str) -> Result<(), String> {
    export_framed(s, out, None)
}

fn export_framed(s: &Session, out: &str, window: Option<Bounds2>) -> Result<(), String> {
    let d = s.doc().map_err(|e| e.to_string())?;
    let bytes = cadcraft_io::write_framed(d, out, window).map_err(|e| e.to_string())?;
    std::fs::write(out, &bytes).map_err(|e| format!("{out}: {e}"))?;
    eprintln!("wrote {out} ({} bytes)", bytes.len());
    if !cadcraft_io::is_image(out) {
        let lost = cadcraft_engine::cmd::file::not_saved(d);
        if let Some(m) = cadcraft_engine::cmd::file::not_saved_message(&lost) {
            eprintln!("{m}");
        }
    }
    Ok(())
}

fn sample(args: &[String]) -> Result<(), String> {
    let (Some(which), Some(out)) = (args.first(), args.get(1)) else { return Err(USAGE.into()) };
    let d = match which.as_str() {
        "bracket" => cadcraft_engine::sample::bracket(),
        "floorplan" | "floor" => cadcraft_engine::sample::floor_plan(),
        other => return Err(format!("unknown sample `{other}` (bracket, floorplan)")),
    };
    let bytes = cadcraft_io::write(&d, out).map_err(|e| e.to_string())?;
    std::fs::write(out, &bytes).map_err(|e| format!("{out}: {e}"))?;
    eprintln!("wrote {out} ({} bytes)", bytes.len());
    Ok(())
}

/// Reject an explicit framing option without an image output to apply it to.
fn check_framed_output(framing: Framing, out: Option<&str>) -> Result<(), String> {
    match out {
        _ if framing == Framing::Auto => Ok(()),
        Some(o) if !cadcraft_io::is_image(o) => Err(format!("--window/--view only apply to image output (png, svg, pdf), not `{o}`")),
        Some(_) => Ok(()),
        None => Err("--window/--view need an image output (--export OUT.png|svg|pdf)".into()),
    }
}

fn convert(args: &[String]) -> Result<(), String> {
    let mut framing = Framing::Auto;
    let mut files = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if !framing_option(a, &mut it, &mut framing)? {
            files.push(a);
        }
    }
    let [i, o] = files.as_slice() else { return Err(USAGE.into()) };
    check_framed_output(framing, Some(o))?;
    let mut s = Session::empty();
    open(&mut s, i)?;
    let window = export_window(&s, framing, None)?;
    export_framed(&s, o, window)
}

fn run(args: &[String]) -> Result<(), String> {
    let mut s = Session::empty();
    let mut it = args.iter();
    let mut save = None;
    let mut exp: Option<String> = None;
    let mut framing = Framing::Auto;
    let mut steps: Vec<(String, String)> = Vec::new();
    while let Some(a) = it.next() {
        if framing_option(a, &mut it, &mut framing)? {
            continue;
        }
        match a.as_str() {
            "--sample" => {
                s.open_drawing(cadcraft_engine::sample::default_sample(), "Bracket", None);
            }
            "--metric" => {
                s.new_drawing(true);
            }
            "--script" => steps.push(("script".into(), it.next().ok_or("--script needs text")?.replace("\\n", "\n"))),
            "--script-file" => {
                let p = it.next().ok_or("--script-file needs a path")?;
                steps.push(("script".into(), std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?));
            }
            "--cmd" => steps.push(("cmd".into(), it.next().ok_or("--cmd needs `id {json}`")?.clone())),
            "--save" => save = Some(it.next().ok_or("--save needs a path")?.clone()),
            "--export" => exp = Some(it.next().ok_or("--export needs a path")?.clone()),
            f if !f.starts_with("--") => open(&mut s, f)?,
            other => return Err(format!("unknown option {other}")),
        }
    }
    if s.docs.is_empty() {
        s.new_drawing(false);
    }
    check_framed_output(framing, exp.as_deref())?;
    let before = model_views(&s);
    for (kind, body) in steps {
        match kind.as_str() {
            "script" => s.script(&body).map_err(|e| e.to_string())?,
            _ => {
                let (id, json) = body.split_once(' ').unwrap_or((&body, "{}"));
                let params: Value = serde_json::from_str(json).map_err(|e| format!("--cmd {id}: {e}"))?;
                let r = s.execute(id, &params).map_err(|e| e.to_string())?;
                if !r.is_null() {
                    println!("{r}");
                }
            }
        }
    }
    for l in &s.log {
        eprintln!("{l}");
    }
    if let Some(p) = save {
        export(&s, &p)?;
    }
    if let Some(p) = exp {
        let window = if cadcraft_io::is_image(&p) { export_window(&s, framing, Some(&before))? } else { None };
        export_framed(&s, &p, window)?;
    }
    Ok(())
}

/// Small deterministic generator (xorshift64*) so perf runs are repeatable without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }
}

/// A synthetic drawing with `n` mixed entities (lines, circles, arcs, polylines, text) spread
/// over a square whose side grows with sqrt(n), so density stays constant.
fn synthetic(n: usize, side: f64) -> Result<cadcraft_engine::doc::Drawing, String> {
    use cadcraft_engine::doc::{self as d, Common, EntityKind, Space};
    use cadcraft_engine::geom::{PolyVertex, Vec2, Vec3};
    let mut dr = d::Drawing::new_imperial();
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    let v3 = |x: f64, y: f64| Vec3::new(x, y, 0.0);
    for i in 0..n {
        let (x, y) = (r.range(0.0, side), r.range(0.0, side));
        let s = r.range(0.5, 8.0);
        let kind = match i % 5 {
            0 => EntityKind::Line(d::Line { a: v3(x, y), b: v3(x + r.range(-s, s), y + r.range(-s, s)) }),
            1 => EntityKind::Circle(d::Circle { center: v3(x, y), radius: s * 0.5 }),
            2 => EntityKind::Arc(d::Arc { center: v3(x, y), radius: s * 0.5, start: r.range(0.0, 3.0), end: r.range(3.2, 6.2) }),
            3 => EntityKind::LwPolyline(d::LwPolyline {
                vertices: (0..6)
                    .map(|k| PolyVertex {
                        p: Vec2::new(x + f64::from(k) * s * 0.3, y + r.range(-s, s) * 0.3),
                        bulge: if k == 2 { 0.4 } else { 0.0 },
                        start_width: 0.0,
                        end_width: 0.0,
                    })
                    .collect(),
                closed: i % 2 == 0,
                const_width: 0.0,
                elevation: 0.0,
                plinegen: false,
            }),
            _ => EntityKind::Text(d::Text {
                insert: v3(x, y),
                align_pt: None,
                height: s * 0.2,
                value: format!("T{i}"),
                rotation: 0.0,
                width_factor: 1.0,
                oblique: 0.0,
                style: "Standard".into(),
                halign: d::HAlign::Left,
                valign: d::VAlign::Baseline,
            }),
        };
        dr.add(&Space::Model, Common::default(), kind).map_err(|e| e.to_string())?;
    }
    Ok(dr)
}

fn perf(args: &[String]) -> Result<(), String> {
    use cadcraft_engine::doc::Space;
    use cadcraft_engine::geom::{Bounds2, Vec2};
    use cadcraft_engine::{select, snap};
    use std::time::Instant;
    let n: usize = match args.first() {
        Some(a) => a.parse().map_err(|_| format!("perf: bad entity count {a}"))?,
        None => 200_000,
    };
    let n = n.clamp(1, 20_000_000);
    let side = (n as f64).sqrt() * 10.0;
    let mut rows: Vec<(String, f64, String)> = Vec::new();
    let ms = |t: Instant| t.elapsed().as_secs_f64() * 1000.0;

    let t = Instant::now();
    let d = synthetic(n, side)?;
    rows.push(("build synthetic drawing".into(), ms(t), format!("{n} entities")));

    // Display list at a 1600 px wide "zoom extents" view (tolerance = half a pixel).
    let px = side / 1600.0;
    let opts = cadcraft_engine::render::Options { tolerance: px * 0.5, min_dash: px * 2.0, ..Default::default() };
    let t = Instant::now();
    let list = cadcraft_engine::render::build(&d, &Space::Model, &opts);
    rows.push(("display list build".into(), ms(t), format!("{} prims, {} segments", list.prims.len(), list.segment_count())));

    // Hit testing on a zoomed-in view (aperture = 5 px at 0.02 units/px).
    let ap = 0.1;
    let mut r = Rng(42);
    let pts: Vec<Vec2> = (0..1000).map(|_| Vec2::new(r.range(0.0, side), r.range(0.0, side))).collect();
    let t = Instant::now();
    let first = select::pick(&d, &Space::Model, pts.first().copied().unwrap_or(Vec2::ZERO), ap);
    rows.push(("first pick (incl. index build)".into(), ms(t), format!("{first:?}")));
    let t = Instant::now();
    let hits = pts.iter().filter(|p| select::pick(&d, &Space::Model, **p, ap).is_some()).count();
    let total = ms(t);
    rows.push(("pick x1000".into(), total, format!("{hits} hits, {:.3} ms/pick", total / 1000.0)));

    // One edit, then a pick: only the touched chunk is re-indexed.
    let mut edited = d.clone();
    if let Some(h) = edited.model.last().map(|e| e.handle) {
        edited.model.modify(h, |e| e.common.layer = "0".into());
    }
    let t = Instant::now();
    let _ = select::pick(&edited, &Space::Model, pts.first().copied().unwrap_or(Vec2::ZERO), ap);
    rows.push(("pick after one edit (incremental re-index)".into(), ms(t), String::new()));

    let mut sel = 0usize;
    let t = Instant::now();
    for k in 0..20 {
        let c = Vec2::new(r.range(0.0, side), r.range(0.0, side));
        let half = side * 0.05;
        sel += select::select_window(&d, &Space::Model, Bounds2::new(c - Vec2::new(half, half), c + Vec2::new(half, half)), k % 2 == 1).len();
    }
    rows.push(("window/crossing select x20 (10% side)".into(), ms(t), format!("{sel} selected")));

    let t = Instant::now();
    let fence: Vec<Vec2> = (0..4).map(|_| Vec2::new(r.range(0.0, side), r.range(0.0, side))).collect();
    let fsel = select::select_fence(&d, &Space::Model, &fence).len();
    rows.push(("fence select (3 legs)".into(), ms(t), format!("{fsel} selected")));

    let osmode = snap::mode::END | snap::mode::MID | snap::mode::CEN | snap::mode::QUA | snap::mode::INT;
    let t = Instant::now();
    let snaps = pts.iter().filter(|p| snap::osnap(&d, &Space::Model, **p, ap * 2.0, osmode, None, false).is_some()).count();
    let total = ms(t);
    rows.push(("osnap x1000".into(), total, format!("{snaps} hits, {:.3} ms/query", total / 1000.0)));

    let t = Instant::now();
    let bytes = cadcraft_io::write(&d, "perf.dxf").map_err(|e| e.to_string())?;
    rows.push(("DXF write".into(), ms(t), format!("{:.1} MB", bytes.len() as f64 / 1e6)));
    let t = Instant::now();
    let back = cadcraft_io::read(&bytes, "perf.dxf").map_err(|e| e.to_string())?;
    rows.push(("DXF read".into(), ms(t), format!("{} entities", back.entity_count())));

    println!("{:<40} {:>12}  notes", "operation", "ms");
    println!("{}", "-".repeat(80));
    for (name, t, note) in rows {
        println!("{name:<40} {t:>12.2}  {note}");
    }
    Ok(())
}

/// The backend `cadcraft-cli mcp` serves from.
#[derive(Debug, PartialEq, Eq)]
enum McpTarget {
    /// An in-process drawing session (no `--connect`).
    Headless,
    /// The running app's control channel at `HOST:PORT`.
    Connect(String),
}

/// Parses the arguments after `mcp`. `--connect` must be followed by a `HOST:PORT`; a missing,
/// empty or flag-like value is an error, never a silent fall back to a headless session (#103).
fn parse_mcp_args(args: &[String]) -> Result<McpTarget, String> {
    let mut target = McpTarget::Headless;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--connect" => {
                let addr = it
                    .next()
                    .map(|v| v.trim())
                    .filter(|v| !v.is_empty() && !v.starts_with('-'))
                    .ok_or("--connect needs HOST:PORT (the app's --control port, e.g. 127.0.0.1:7979)")?;
                if target != McpTarget::Headless {
                    return Err("--connect given more than once".into());
                }
                target = McpTarget::Connect(addr.to_string());
            }
            other => return Err(format!("mcp: unknown argument {other}\n{USAGE}")),
        }
    }
    Ok(target)
}

fn mcp(args: &[String]) -> Result<(), String> {
    let backend: Box<dyn cadcraft_mcp::Backend> = match parse_mcp_args(args)? {
        McpTarget::Connect(addr) => match cadcraft_mcp::Remote::connect(&addr) {
            Ok(r) => Box::new(r),
            Err(e) => return Err(format!("cadcraft-cli: cannot connect to {addr}: {e}")),
        },
        McpTarget::Headless => Box::new(cadcraft_mcp::Headless::default()),
    };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    cadcraft_mcp::Server::new(backend).serve(stdin.lock(), stdout.lock()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod mcp_args_tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn mcp_without_connect_is_headless() {
        assert_eq!(parse_mcp_args(&args(&[])), Ok(McpTarget::Headless));
    }

    #[test]
    fn mcp_connect_with_address_selects_remote() {
        assert_eq!(parse_mcp_args(&args(&["--connect", "127.0.0.1:7979"])), Ok(McpTarget::Connect("127.0.0.1:7979".into())));
        assert_eq!(parse_mcp_args(&args(&["--connect", "localhost:7979"])), Ok(McpTarget::Connect("localhost:7979".into())));
    }

    #[test]
    fn mcp_connect_without_address_is_an_error_not_headless() {
        for bad in [&["--connect"][..], &["--connect", ""], &["--connect", "  "], &["--connect", "--foo"], &["--connect", "-v"]] {
            let r = parse_mcp_args(&args(bad));
            let Err(e) = r else { panic!("{bad:?} should be rejected, got {r:?}") };
            assert!(e.contains("--connect needs HOST:PORT"), "{bad:?}: {e}");
        }
    }

    #[test]
    fn mcp_rejects_unknown_and_repeated_arguments() {
        assert!(parse_mcp_args(&args(&["--conect", "127.0.0.1:7979"])).is_err());
        assert!(parse_mcp_args(&args(&["127.0.0.1:7979"])).is_err());
        assert!(parse_mcp_args(&args(&["--connect", "127.0.0.1:1", "--connect", "127.0.0.1:2"])).is_err());
    }
}

fn main() -> ExitCode {
    install_io();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest: Vec<String> = args.iter().skip(1).cloned().collect();
    let r = match args.first().map(String::as_str) {
        Some("info") => validate_options("info", &rest, NO_OPTIONS).and_then(|_| rest.first().ok_or_else(|| USAGE.to_string())).and_then(|p| info(p)),
        Some("convert") => convert(&rest),
        Some("run") => run(&rest),
        Some("perf") => validate_options("perf", &rest, NO_OPTIONS).and_then(|_| perf(&rest)),
        Some("sample") => validate_options("sample", &rest, NO_OPTIONS).and_then(|_| sample(&rest)),
        Some("commands") => validate_options("commands", &rest, NO_OPTIONS).map(|_| {
            let s = Session::new();
            let f = rest.first().map(|x| x.to_ascii_lowercase());
            let v: Vec<Value> = cadcraft_engine::command_specs()
                .iter()
                .filter(|c| f.as_ref().is_none_or(|f| c.id.contains(f.as_str()) || c.label.to_ascii_lowercase().contains(f.as_str())))
                .map(|c| serde_json::to_value(c.info(&s)).unwrap_or_default())
                .collect();
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        }),
        Some("mcp") => mcp(&rest),
        Some("--version" | "-V") => {
            println!("cadcraft-cli {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err(USAGE.into()),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn window_parses() {
        assert_eq!(parse_window("0,0,30,20"), Ok(b(0.0, 0.0, 30.0, 20.0)));
        assert_eq!(parse_window(" -5.5, -1e3 ,2e3,4 "), Ok(b(-5.5, -1000.0, 2000.0, 4.0)));
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

    #[test]
    fn framing_options() {
        let parse = |args: &[&str]| -> Result<Framing, String> {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            let mut f = Framing::Auto;
            let mut it = args.iter();
            while let Some(a) = it.next() {
                assert!(framing_option(a, &mut it, &mut f)?, "{a} is a framing option");
            }
            Ok(f)
        };
        assert_eq!(parse(&["--view", "extents"]), Ok(Framing::Extents));
        assert_eq!(parse(&["--view", "CURRENT"]), Ok(Framing::Current));
        assert_eq!(parse(&["--window", "0,0,2,1"]), Ok(Framing::Window(b(0.0, 0.0, 2.0, 1.0))));
        assert!(parse(&["--view"]).is_err());
        assert!(parse(&["--view", "selection"]).is_err());
        assert!(parse(&["--window"]).is_err());
        assert!(parse(&["--window", "0,0,2,1", "--view", "extents"]).is_err());
        assert!(check_framed_output(Framing::Extents, Some("a.dxf")).is_err());
        assert!(check_framed_output(Framing::Extents, None).is_err());
        assert!(check_framed_output(Framing::Extents, Some("a.png")).is_ok());
        assert!(check_framed_output(Framing::Auto, Some("a.dxf")).is_ok());
    }

    #[test]
    fn default_framing_is_extents() {
        // convert: nothing ran, always the extents.
        let s = sample_session();
        assert_eq!(export_window(&s, Framing::Auto, None), Ok(None));
        // run: commands that leave the view alone keep the extents framing.
        let mut s = sample_session();
        let before = model_views(&s);
        s.script("LINE 0,0 10,10\n\n").unwrap();
        assert_eq!(export_window(&s, Framing::Auto, Some(&before)), Ok(None));
        // A fresh drawing too.
        let mut s = Session::empty();
        s.new_drawing(false);
        let before = model_views(&s);
        s.execute("circle", &json!({"center": [0, 0], "radius": 5})).unwrap();
        assert_eq!(export_window(&s, Framing::Auto, Some(&before)), Ok(None));
    }

    #[test]
    fn zoom_window_frames_export() {
        let mut s = sample_session();
        let before = model_views(&s);
        s.execute("zoom", &json!({"mode": "window", "p1": [0, 0], "p2": [30, 20]})).unwrap();
        let w = export_window(&s, Framing::Auto, Some(&before)).unwrap().unwrap();
        // The CLI viewport is 3:2, so a 3:2 window is shown exactly.
        assert_near(w, b(0.0, 0.0, 30.0, 20.0));
        assert_eq!(export_window(&s, Framing::Current, Some(&before)), Ok(Some(w)));
        // --view extents and --window override the zoom.
        assert_eq!(export_window(&s, Framing::Extents, Some(&before)), Ok(None));
        let explicit = b(1.0, 1.0, 2.0, 2.0);
        assert_eq!(export_window(&s, Framing::Window(explicit), Some(&before)), Ok(Some(explicit)));
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
            let mut args: Vec<String> = ["--sample"].iter().chain(extra).map(|a| a.to_string()).collect();
            args.extend(["--export".to_string(), path(out)]);
            run(&args).unwrap();
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
        // Hostile --window: a clear error, nothing written.
        let bad: Vec<String> = ["--sample", "--window", "0,0,NaN,1", "--export", &path("bad.png")].iter().map(|a| a.to_string()).collect();
        assert!(run(&bad).unwrap_err().contains("--window"));
        assert!(!dir.join("bad.png").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).into()).collect()
    }

    #[test]
    fn validators_accept_only_the_documented_options() {
        assert!(validate_options("info", &args(&["file.dxf"]), NO_OPTIONS).is_ok());
        assert!(validate_options("perf", &args(&["100"]), NO_OPTIONS).is_ok());
        assert!(validate_options("sample", &args(&["bracket", "out.dxf"]), NO_OPTIONS).is_ok());
        assert!(validate_options("commands", &args(&["line"]), NO_OPTIONS).is_ok());
        for (command, specs) in [("info", NO_OPTIONS), ("perf", NO_OPTIONS), ("sample", NO_OPTIONS), ("commands", NO_OPTIONS)] {
            assert!(validate_options(command, &args(&["--typo"]), specs).is_err());
        }
    }

    #[test]
    fn unknown_options_are_rejected_before_side_effects() {
        assert_eq!(validate_options("info", &args(&["missing.dxf", "--typo"]), NO_OPTIONS), Err("info: unknown option --typo".into()));
        assert_eq!(validate_options("perf", &args(&["1", "--typo"]), NO_OPTIONS), Err("perf: unknown option --typo".into()));
        assert_eq!(validate_options("sample", &args(&["bracket", "out.dxf", "--typo"]), NO_OPTIONS), Err("sample: unknown option --typo".into()));
    }
}
