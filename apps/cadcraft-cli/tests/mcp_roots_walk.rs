//! Walks every command in the registry through the real MCP server with paths outside the
//! automation roots (#1472). Reads show as a changed access time and writes as a changed size,
//! modification time or folder, whatever the command does with the path. The same walk without
//! roots has to find escapes, or finding none under roots would prove nothing.
#![cfg(unix)]
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Never crash).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, FileTimes};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

/// Longest a call may take before the server counts as hung.
const CALL_TIMEOUT: Duration = Duration::from_secs(60);
/// The extensions commands choose their behaviour by.
const EXTENSIONS: &[&str] = &["dxf", "dwg", "dwt", "scr", "lsp", "lin", "pat", "pgp", "csv", "txt", "png", "pdf", "svg", "ctb"];
/// The parameter names a command might take a path under.
const PATH_KEYS: &[&str] =
    &["path", "file", "template", "patternFile", "from", "to", "source", "target", "output", "folder", "dir", "filename", "document"];
/// A valid 1x1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1F, 0x15, 0xC4,
    0x89, 0, 0, 0, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0, 1, 0, 0, 5, 0, 1, 0x0D, 0x0A, 0x2D, 0xB4, 0, 0, 0, 0, 0x49, 0x45, 0x4E, 0x44,
    0xAE, 0x42, 0x60, 0x82,
];

/// `p` as the text a tool call passes.
fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// A running `cadcraft-cli mcp`.
struct Server {
    child: Child,
    stdin: ChildStdin,
    replies: Receiver<Value>,
    next: u64,
}

impl Server {
    /// Start the real binary as an MCP server and initialize it.
    fn start(args: &[String], cwd: &Path, config: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_cadcraft-cli"))
            .arg("mcp")
            .args(args)
            .current_dir(cwd)
            .env("CADCRAFT_CONFIG_DIR", config)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn cadcraft-cli");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, replies) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<Value>(&line)
                    && tx.send(v).is_err()
                {
                    break;
                }
            }
        });
        let mut server = Self { child, stdin, replies, next: 1 };
        server.request("initialize", json!({"protocolVersion": "2025-06-18"})).expect("initialize");
        server
    }

    /// Send one JSON-RPC request; `None` if the server stopped answering.
    fn request(&mut self, method: &str, params: Value) -> Option<Value> {
        let id = self.next;
        self.next += 1;
        let req = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{req}").ok()?;
        loop {
            let reply = self.replies.recv_timeout(CALL_TIMEOUT).ok()?;
            if reply["id"] == id {
                return Some(reply);
            }
        }
    }

    /// A tool's first text block, or `None` when the server hung or stopped.
    fn tool(&mut self, name: &str, arguments: Value) -> Option<String> {
        let reply = self.request("tools/call", json!({"name": name, "arguments": arguments}))?;
        Some(reply["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string())
    }

    /// Run an engine command through the `execute` tool.
    fn execute(&mut self, command: &str, params: Value) -> Option<String> {
        self.tool("execute", json!({"command": command, "params": params}))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The folders of one walk: `in` and `out` are the roots, nothing under `outside` may be touched.
/// A new folder of this test's own: made here, so nothing else put it there first.
fn fresh_dir(tag: &str) -> PathBuf {
    let mut n = 0u32;
    loop {
        let dir = std::env::temp_dir().join(format!("cc-walk-{tag}-{}-{n}", std::process::id()));
        match std::fs::create_dir(&dir) {
            Ok(()) => return dir,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => panic!("{}: {e}", dir.display()),
        }
    }
}

/// Remove the file or folder at `p`.
fn remove(p: &Path) {
    let _ = if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
}

/// Remove what `dir` holds, if it exists.
fn clear(dir: &Path) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        remove(&entry.path());
    }
}

struct Fixture {
    base: PathBuf,
    outside: PathBuf,
    /// Each path under `outside`, with its size, modification time and access time.
    baseline: BTreeMap<PathBuf, (u64, SystemTime, SystemTime)>,
    /// Whether this file system records reads at all (`noatime` doesn't).
    sees_reads: bool,
}

impl Fixture {
    /// A new fixture, finding out whether this file system records reads.
    fn new(tag: &str) -> Self {
        let base = fresh_dir(tag);
        for d in ["in", "out"] {
            std::fs::create_dir(base.join(d)).unwrap();
        }
        let probe = base.join("probe");
        std::fs::write(&probe, "x").unwrap();
        age_access_time(&probe);
        let before = std::fs::metadata(&probe).unwrap().accessed().unwrap();
        std::fs::read(&probe).unwrap();
        let sees_reads = std::fs::metadata(&probe).unwrap().accessed().unwrap() != before;
        let outside = base.join("outside");
        let mut f = Self { base, outside, baseline: BTreeMap::new(), sees_reads };
        f.reset();
        f
    }

    /// Lay out `outside` afresh and start watching it.
    fn reset(&mut self) {
        let o = self.outside.clone();
        // The running server's working directory stays: without it a relative path can't be resolved.
        clear(&o.join("cwd"));
        for entry in std::fs::read_dir(&o).into_iter().flatten().flatten() {
            if entry.file_name() != "cwd" {
                remove(&entry.path());
            }
        }
        for d in ["cwd", "config/plotters", "recover"] {
            std::fs::create_dir_all(o.join(d)).unwrap();
        }
        let dxf = cadcraft_io::write(&cadcraft_engine::doc::Drawing::new_imperial(), "x.dxf").unwrap();
        for ext in EXTENSIONS {
            let body: &[u8] = match *ext {
                "dxf" | "dwg" | "dwt" => &dxf,
                "scr" => b"CIRCLE 0,0 5\n",
                "lsp" => b"(princ 1)\n",
                "lin" => b"*WALK,walk\nA,1,-1\n",
                "pat" => b"*WALK,walk\n0,0,0,0,1\n",
                "pgp" => b"W,*LINE\n",
                "csv" | "txt" => b"a,b\n1,2\n",
                "png" => PNG,
                _ => b"canary",
            };
            std::fs::write(o.join(format!("canary.{ext}")), body).unwrap();
        }
        // Two images, so that a drawing naming both loads them on threads of their own.
        std::fs::write(o.join("second.png"), PNG).unwrap();
        let device = cadcraft_engine::doc::PlotterConfig { name: "Office".into(), ..Default::default() };
        std::fs::write(o.join("config/plotters/Office.plotter.json"), device.to_file_text()).unwrap();
        self.watch();
    }

    /// Start watching what is under `outside` now.
    fn watch(&mut self) {
        let o = self.outside.clone();
        self.baseline.clear();
        for p in tree(&o) {
            age_access_time(&p);
            let m = std::fs::metadata(&p).unwrap();
            self.baseline.insert(p, (m.len(), m.modified().unwrap(), m.accessed().unwrap()));
        }
    }

    /// What happened under `outside` since [`Self::reset`]. Only metadata is read: this leaves
    /// the access times as they are.
    fn touched(&self) -> Vec<String> {
        let mut seen = Vec::new();
        for (p, (len, modified, accessed)) in &self.baseline {
            let name = p.strip_prefix(&self.outside).unwrap().display();
            match std::fs::metadata(p) {
                Err(_) => seen.push(format!("removed {name}")),
                Ok(m) if m.modified().unwrap() != *modified || (!m.is_dir() && m.len() != *len) => seen.push(format!("wrote {name}")),
                Ok(m) if self.sees_reads && m.accessed().unwrap() != *accessed => seen.push(format!("read {name}")),
                Ok(_) => {}
            }
        }
        seen
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// Every file and folder under `dir`.
fn tree(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![dir.to_path_buf()];
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        if e.path().is_dir() {
            out.extend(tree(&e.path()));
        } else {
            out.push(e.path());
        }
    }
    out
}

/// Put `p`'s access time in the past, so that the next read updates it even under `relatime`.
fn age_access_time(p: &Path) {
    let past = SystemTime::now() - Duration::from_secs(3 * 24 * 3600);
    File::open(p).unwrap().set_times(FileTimes::new().set_accessed(past)).unwrap();
}

/// The `action` or `option` values a parameter doc lists: `action?: list|open|discard (…)`.
fn actions(doc: &str) -> Vec<String> {
    let mut out = Vec::new();
    for key in ["action", "option"] {
        let Some(at) = doc.find(key) else { continue };
        let rest = doc[at + key.len()..].trim_start_matches(['?', ':', ' ']);
        let end = rest.find([',', '}', '(']).unwrap_or(rest.len());
        out.extend(rest[..end].split('|').map(|w| w.trim().trim_matches('"').to_string()).filter(|w| !w.is_empty()));
    }
    out
}

/// The calls to try for command `id`: every path-like key set to `target`, with each action.
fn variants(doc: &str, target: &str, handle: &str, table: &str) -> Vec<Value> {
    let mut base = json!({"handle": table, "handles": [handle], "at": [0, 0], "name": "walk"});
    for key in PATH_KEYS {
        base[*key] = json!(target);
    }
    let mut named = base.clone();
    named["name"] = json!(target);
    let mut device = base.clone();
    device["name"] = json!("Office");
    // Commands refuse parameters that don't go together, so each key the doc names goes on its own too.
    let lower = doc.to_ascii_lowercase();
    let documented = PATH_KEYS.iter().filter(|key| **key == "path" || lower.contains(&key.to_ascii_lowercase()));
    let alone = documented.flat_map(|key| {
        [
            json!({*key: target}),
            json!({*key: target, "handles": [handle]}),
            json!({*key: target, "handle": table}),
            json!({*key: target, "name": "Office"}),
        ]
    });
    let mut out = Vec::new();
    for v in [base, named, device].into_iter().chain(alone) {
        out.push(v.clone());
        for a in actions(doc) {
            let mut with = v.clone();
            with["action"] = json!(a);
            with["option"] = json!(a);
            out.push(with);
        }
    }
    out
}

/// Whether a command's doc says it takes a file. These get every extension; the rest, one.
fn takes_a_file(doc: &str) -> bool {
    let d = doc.to_ascii_lowercase();
    ["path", "file", "template", "folder", "action", "option"].iter().any(|w| d.contains(w))
}

/// A fresh drawing with a circle and a table, selected; the circle's and table's handles.
fn seed(server: &mut Server) -> Option<(String, String)> {
    // Closed first: the walk starts tens of thousands of drawings, and they would all stay open.
    server.execute("closeall", json!({}))?;
    server.execute("new", json!({}))?;
    let handle_of = |text: &str| serde_json::from_str::<Value>(text).ok().and_then(|v| v["handle"].as_str().map(str::to_string)).unwrap_or_default();
    let circle = server.execute("circle", json!({"center": [0, 0], "radius": 1}))?;
    let table = server.execute("table", json!({"at": [5, 5], "rows": 2, "columns": 2}))?;
    server.execute("selectall", json!({}))?;
    Some((handle_of(&circle), handle_of(&table)))
}

/// What one walk saw.
struct Walk {
    /// The command and the call that touched each file outside.
    escapes: BTreeMap<String, Vec<String>>,
    calls: usize,
    commands: usize,
    /// Calls that hung or stopped the server.
    lost: Vec<String>,
    sees_reads: bool,
}

/// Run the walk, under `in` and `out` as roots or with no roots at all.
fn walk(tag: &str, roots: bool) -> Walk {
    let mut f = Fixture::new(tag);
    let args = if roots {
        vec!["--automation-read-root".into(), s(&f.base.join("in")), "--automation-write-root".into(), s(&f.base.join("out"))]
    } else {
        vec![]
    };
    let cwd = f.outside.join("cwd");
    let config = f.outside.join("config");
    let start = |f: &mut Fixture| {
        let server = Server::start(&args, &cwd, &config);
        f.reset();
        server
    };
    let mut server = start(&mut f);
    let listed = server.tool("list_commands", json!({})).expect("list_commands");
    let specs: Vec<Value> = serde_json::from_str(&listed).expect("the command list");
    assert!(specs.len() > 400, "the walk has to see the registry: {} commands", specs.len());
    let mut result = Walk { escapes: BTreeMap::new(), calls: 0, commands: specs.len(), lost: Vec::new(), sees_reads: f.sees_reads };

    let mut steps: Vec<(String, Value)> = Vec::new();
    let o = f.outside.clone();
    let canary = |ext: &str| s(&o.join(format!("canary.{ext}")));
    let (circle, table) = seed(&mut server).expect("seed");
    assert!(!table.is_empty(), "the seed table has no handle");
    for spec in &specs {
        let id = spec["id"].as_str().unwrap().to_string();
        let doc = spec["params"].as_str().unwrap_or_default();
        let mut targets: Vec<String> = if takes_a_file(doc) { EXTENSIONS.iter().map(|e| canary(e)).collect() } else { vec![canary("dxf")] };
        if takes_a_file(doc) {
            // A file that isn't there yet, in a folder that isn't there yet, and a relative path.
            targets.extend(["dxf", "png", "pdf", "csv"].iter().map(|e| s(&o.join(format!("fresh/x.{e}")))));
            targets.push("../canary.dxf".to_string());
        }
        for target in targets {
            for v in variants(doc, &target, &circle, &table) {
                steps.push((id.clone(), v));
            }
        }
    }

    let run_step =
        |server: &mut Server, f: &mut Fixture, result: &mut Walk, id: &str, label: String, call: &mut dyn FnMut(&mut Server) -> Option<String>| {
            result.calls += 1;
            let done = call(server);
            let touched = f.touched();
            if !touched.is_empty() {
                for what in touched {
                    result.escapes.entry(id.to_string()).or_default().push(format!("{what}: {label}"));
                }
            }
            if done.is_none() {
                result.lost.push(format!("{id}: {label}"));
            }
            done.is_some()
        };

    for (id, params) in &steps {
        let label = params.to_string();
        let params = params.clone();
        let alive = run_step(&mut server, &mut f, &mut result, id, label, &mut |sv| sv.execute(id, params.clone()));
        let dirty = !f.touched().is_empty();
        if !alive {
            server = start(&mut f);
        } else if dirty {
            f.reset();
        }
        // A command can stop the server without its own reply being lost.
        if seed(&mut server).is_none() {
            result.lost.push(format!("after {id}: {params}"));
            server = start(&mut f);
            seed(&mut server);
        }
    }

    // The ways in that aren't one command with a path parameter.
    let dxf = canary("dxf");
    let png = canary("png");
    let scr = canary("scr");
    let lsp = canary("lsp");
    let tools: Vec<(&str, &str, Value)> = vec![
        ("tool:open", "open", json!({"path": dxf})),
        ("tool:save", "save", json!({"path": dxf})),
        ("tool:save-new", "save", json!({"path": s(&o.join("fresh/y.dxf"))})),
        ("tool:render", "render", json!({"path": png})),
        ("tool:command_line", "command_line", json!({"text": format!("SCRIPT {scr}")})),
        ("tool:command_line", "command_line", json!({"text": format!("OPEN {dxf}")})),
        ("tool:command_line", "command_line", json!({"text": format!("SAVEAS {dxf}")})),
        ("tool:script", "script", json!({"text": format!("OPEN {dxf}\n")})),
    ];
    for (label, name, a) in tools {
        let text = a.to_string();
        let a = a.clone();
        let alive = run_step(&mut server, &mut f, &mut result, label, text, &mut |sv| sv.tool(name, a.clone()));
        if alive {
            f.reset();
        } else {
            server = start(&mut f);
        }
        seed(&mut server);
    }
    let lisp = [
        format!("(setq f (open {dxf:?} \"r\")) (read-line f) (close f)"),
        format!("(setq f (open {dxf:?} \"w\")) (write-line \"x\" f) (close f)"),
        format!("(setq f (open {:?} \"w\")) (write-line \"x\" f) (close f)", s(&o.join("new.txt"))),
        format!("(load {lsp:?})"),
    ];
    for code in lisp {
        let label = code.clone();
        run_step(&mut server, &mut f, &mut result, "lisp:code", label, &mut |sv| sv.execute("lisp", json!({"code": code})));
        f.reset();
        seed(&mut server);
    }
    // Settings that name a file or a folder, then the commands that use them.
    let chained: Vec<(&str, Vec<(&str, Value)>)> = vec![
        (
            "stylesmanager",
            vec![
                ("stylesmanager", json!({"action": "new", "name": "walk.ctb"})),
                ("stylesmanager", json!({"action": "save", "name": "walk.ctb", "path": canary("ctb")})),
                ("stylesmanager", json!({"action": "load", "path": canary("ctb")})),
            ],
        ),
        (
            "pdfbook",
            vec![("pline", json!({"points": [[0, 0], [210, 0], [210, 297], [0, 297]], "closed": true})), ("pdfbook", json!({"path": canary("pdf")}))],
        ),
        ("setvar:SAVEFILEPATH", vec![("setvar", json!({"name": "SAVEFILEPATH", "value": s(&o.join("recover"))})), ("drawingrecovery", json!({}))]),
        ("setvar:QNEWTEMPLATE", vec![("setvar", json!({"name": "QNEWTEMPLATE", "value": dxf})), ("new", json!({}))]),
    ];
    for (label, calls) in chained {
        for (id, params) in calls {
            run_step(&mut server, &mut f, &mut result, label, format!("{id} {params}"), &mut |sv| sv.execute(id, params.clone()));
        }
        f.reset();
        seed(&mut server);
    }
    result
}

/// One line saying what a walk saw.
fn summary(w: &Walk) -> String {
    format!(
        "{} commands, {} calls, reads seen: {}, {} commands touched files outside, {} calls lost the server",
        w.commands,
        w.calls,
        w.sees_reads,
        w.escapes.len(),
        w.lost.len()
    )
}

/// Under roots, nothing in the registry reaches a file outside them.
#[test]
fn no_command_escapes_the_roots() {
    let w = walk("roots", true);
    println!("under roots: {}", summary(&w));
    let found: Vec<String> = w.escapes.iter().flat_map(|(id, what)| what.iter().take(3).map(move |l| format!("{id}: {l}"))).collect();
    assert!(found.is_empty(), "escapes under the roots:\n{}", found.join("\n"));
    assert!(w.lost.is_empty(), "calls that lost the server: {:?}", w.lost);
}

/// The control: without roots the same walk finds these commands reaching the same files. If this
/// fails, the walk has stopped reaching them and [`no_command_escapes_the_roots`] proves nothing.
#[test]
fn the_walk_finds_escapes_without_roots() {
    let w = walk("control", false);
    println!("without roots: {}", summary(&w));
    let found: BTreeSet<&str> = w.escapes.keys().map(String::as_str).collect();
    println!("touched files outside: {}", found.iter().copied().collect::<Vec<_>>().join(" "));
    let writers = [
        "saveas",
        "export",
        "plot",
        "exportpdf",
        "publish",
        "pdfbook",
        "wblock",
        "-wblock",
        "layout.saveas",
        "layout",
        "stylesmanager",
        "plottermanager",
        "tableexport",
        "dataextraction",
        "tool:save",
        "tool:save-new",
        "tool:render",
        "lisp:code",
    ];
    for id in writers {
        assert!(found.contains(id), "the walk no longer finds `{id}` writing outside: {found:?}");
    }
    if w.sees_reads {
        for id in [
            "open",
            "new",
            "recover",
            "script",
            "import",
            "insert",
            "minsert",
            "xattach",
            "-xref",
            "compare",
            "imageattach",
            "attach",
            "aliasedit",
            "linetype",
            "hatch.patterns",
            "appload",
            "datalink",
            "tool:open",
            "setvar:QNEWTEMPLATE",
            "setvar:SAVEFILEPATH",
        ] {
            assert!(found.contains(id), "the walk no longer finds `{id}` reading outside: {found:?}");
        }
    }
}

/// A drawing inside the roots can't bring in what it names outside them: an image or an xref saved
/// with a path outside stays unloaded, and nothing outside is read.
#[test]
fn a_drawing_cannot_reach_files_it_names_outside_the_roots() {
    let mut f = Fixture::new("named");
    let host = f.base.join("in/host.dxf");
    {
        // Made without roots: a drawing someone hands the agent.
        let mut free = Server::start(&[], &f.outside.join("cwd"), &f.outside.join("config"));
        free.execute("new", json!({})).unwrap();
        let image = free.execute("imageattach", json!({"path": s(&f.outside.join("canary.png"))})).unwrap();
        assert!(image.contains("handle"), "the image attaches: {image}");
        let second = free.execute("imageattach", json!({"path": s(&f.outside.join("second.png"))})).unwrap();
        assert!(second.contains("handle"), "the second image attaches: {second}");
        let xref = free.execute("xattach", json!({"path": s(&f.outside.join("canary.dxf")), "name": "walk"})).unwrap();
        assert!(xref.contains("handle"), "the xref attaches: {xref}");
        free.execute("saveas", json!({"path": s(&host)})).unwrap();
    }
    let roots = vec!["--automation-read-root".into(), s(&f.base.join("in")), "--automation-write-root".into(), s(&f.base.join("out"))];
    let mut server = Server::start(&roots, &f.outside.join("cwd"), &f.outside.join("config"));
    f.reset();
    let opened = server.tool("open", json!({"path": s(&host)})).unwrap();
    let images = server.execute("image", json!({})).unwrap();
    let xrefs = server.execute("xref", json!({})).unwrap();
    server.tool("render", json!({})).unwrap();
    let touched = f.touched();
    assert!(touched.is_empty(), "{touched:?}\nopen: {opened}\nimage: {images}\nxref: {xrefs}");
    assert!(images.contains("canary.png") && !images.contains("\"Loaded\""), "the image stays unloaded: {images}");
    assert!(images.contains("\"foundAt\": null") || images.contains("\"foundAt\":null"), "and it isn't said to exist: {images}");
    assert!(!xrefs.contains("\"Loaded\""), "the xref stays unloaded: {xrefs}");
}

/// A font linked into the drawing's folder isn't read from where the link leads.
#[test]
fn a_font_linked_beside_a_drawing_is_not_followed_out() {
    let mut f = Fixture::new("font");
    let host = f.base.join("in/host.dxf");
    {
        let mut free = Server::start(&[], &f.outside.join("cwd"), &f.outside.join("config"));
        free.execute("new", json!({})).unwrap();
        free.execute("style", json!({"name": "Walk", "font": "walkfont.shx"})).unwrap();
        free.execute("style.current", json!({"name": "Walk"})).unwrap();
        let text = free.execute("text", json!({"at": [0, 0], "height": 5, "text": "walk"})).unwrap();
        assert!(text.contains("handle"), "the text is drawn: {text}");
        free.execute("saveas", json!({"path": s(&host)})).unwrap();
    }
    let saved = std::fs::read_to_string(&host).unwrap();
    assert!(saved.to_ascii_lowercase().contains("walkfont.shx"), "the drawing names the font");
    std::fs::write(f.outside.join("walkfont.shx"), b"AutoCAD-86 shapes 1.0").unwrap();
    std::os::unix::fs::symlink(f.outside.join("walkfont.shx"), f.base.join("in/walkfont.shx")).unwrap();
    let roots = vec!["--automation-read-root".into(), s(&f.base.join("in")), "--automation-write-root".into(), s(&f.base.join("out"))];
    let mut server = Server::start(&roots, &f.outside.join("cwd"), &f.outside.join("config"));
    f.reset();
    std::fs::write(f.outside.join("walkfont.shx"), b"AutoCAD-86 shapes 1.0").unwrap();
    f.watch();
    server.tool("open", json!({"path": s(&host)})).unwrap();
    server.tool("render", json!({})).unwrap();
    let touched = f.touched();
    assert!(touched.is_empty(), "{touched:?}");
}

/// Saving through a link that stands outside the roots and points in doesn't leave a backup
/// beside the link.
#[test]
fn a_backup_stays_inside_the_roots() {
    let f = Fixture::new("backup");
    let target = f.base.join("out/t.dxf");
    std::fs::write(&target, cadcraft_io::write(&cadcraft_engine::doc::Drawing::new_imperial(), "x.dxf").unwrap()).unwrap();
    let link = f.outside.join("link.dxf");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let roots = vec!["--automation-read-root".into(), s(&f.base.join("in")), "--automation-write-root".into(), s(&f.base.join("out"))];
    let mut server = Server::start(&roots, &f.outside.join("cwd"), &f.outside.join("config"));
    server.tool("new_drawing", json!({})).unwrap();
    let saved = server.tool("save", json!({"path": s(&link)})).unwrap();
    assert!(saved.contains("outside the write root"), "{saved}");
    assert!(!f.outside.join("link.dxf.bak").exists() && !f.outside.join("link.bak").exists(), "{saved}");
}

/// Deleting a plotter acts on the entry in the settings folder, whatever it points at.
#[test]
fn a_plotter_link_outside_the_roots_is_not_removed() {
    let f = Fixture::new("plotter");
    let inside = f.base.join("out/kept.plotter.json");
    std::fs::write(&inside, cadcraft_engine::doc::PlotterConfig { name: "Kept".into(), ..Default::default() }.to_file_text()).unwrap();
    let entry = f.outside.join("config/plotters/Kept.plotter.json");
    std::os::unix::fs::symlink(&inside, &entry).unwrap();
    let roots = vec!["--automation-read-root".into(), s(&f.base.join("in")), "--automation-write-root".into(), s(&f.base.join("out"))];
    let mut server = Server::start(&roots, &f.outside.join("cwd"), &f.outside.join("config"));
    let reply = server.execute("plottermanager", json!({"action": "delete", "name": "Kept"})).unwrap();
    assert!(std::fs::symlink_metadata(&entry).is_ok(), "the link is still there: {reply}");
}

/// Without roots, a settings folder that can't be read leaves the built-in devices listed.
#[test]
fn without_roots_an_unreadable_settings_folder_still_lists_devices() {
    let f = Fixture::new("devices");
    let not_a_folder = f.outside.join("canary.txt");
    let mut server = Server::start(&[], &f.outside.join("cwd"), &not_a_folder);
    let list = server.execute("plottermanager", json!({"action": "list"})).unwrap();
    assert!(list.contains("devices"), "{list}");
}

/// A font beside a drawing inside the roots is still used.
#[test]
fn a_font_beside_a_drawing_inside_the_roots_is_used() {
    let f = Fixture::new("font-inside");
    let host = f.base.join("in/host.dxf");
    {
        let mut free = Server::start(&[], &f.outside.join("cwd"), &f.outside.join("config"));
        free.execute("new", json!({})).unwrap();
        free.execute("style", json!({"name": "Walk", "font": "walkinside.shx"})).unwrap();
        free.execute("style.current", json!({"name": "Walk"})).unwrap();
        free.execute("text", json!({"at": [0, 0], "height": 5, "text": "walk"})).unwrap();
        free.execute("saveas", json!({"path": s(&host)})).unwrap();
    }
    let font = f.base.join("in/walkinside.shx");
    std::fs::write(&font, b"AutoCAD-86 shapes 1.0").unwrap();
    age_access_time(&font);
    let before = std::fs::metadata(&font).unwrap().accessed().unwrap();
    let roots = vec!["--automation-read-root".into(), s(&f.base.join("in")), "--automation-write-root".into(), s(&f.base.join("out"))];
    let mut server = Server::start(&roots, &f.outside.join("cwd"), &f.outside.join("config"));
    server.tool("open", json!({"path": s(&host)})).unwrap();
    server.tool("render", json!({})).unwrap();
    if f.sees_reads {
        assert_ne!(std::fs::metadata(&font).unwrap().accessed().unwrap(), before, "the font was read");
    }
}

/// The fixture leaves the server's working directory in place, so a relative path meets the roots
/// and not a folder that has gone.
#[test]
fn a_relative_path_is_refused_by_the_roots() {
    let mut f = Fixture::new("relative");
    let roots = vec!["--automation-read-root".into(), s(&f.base.join("in")), "--automation-write-root".into(), s(&f.base.join("out"))];
    let mut server = Server::start(&roots, &f.outside.join("cwd"), &f.outside.join("config"));
    f.reset();
    let reply = server.tool("open", json!({"path": "../canary.dxf"})).unwrap();
    assert!(reply.contains("outside the read root"), "{reply}");
    let touched = f.touched();
    assert!(touched.is_empty(), "{touched:?}");
}
