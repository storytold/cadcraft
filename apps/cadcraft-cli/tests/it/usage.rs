//! The command line's behaviour: exit codes, streams and messages for help requests, unknown and
//! malformed options, value flags in both spellings, repeats and arity. Every case goes through
//! `invoke`, so the table rows are the record of what each input does. Usage errors are clap's
//! (exit 2, stderr); a subcommand's own failures are `error: …` with exit 1.

use std::path::{Path, PathBuf};

struct Invoke {
    code: i32,
    stdout: String,
    stderr: String,
}

fn invoke(args: &[&str]) -> Invoke {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = cadcraft_cli::run(args, &mut out, &mut err);
    Invoke { code, stdout: String::from_utf8_lossy(&out).into_owned(), stderr: String::from_utf8_lossy(&err).into_owned() }
}

/// What a stream holds.
#[derive(Clone, Copy)]
enum Expect {
    Empty,
    /// The first line equals this.
    Line(&'static str),
    /// The first line starts with this.
    Starts(&'static str),
}

use Expect::{Empty, Line, Starts};

struct Case {
    name: &'static str,
    args: &'static [&'static str],
    code: i32,
    stdout: Expect,
    stderr: Expect,
}

const fn case(name: &'static str, args: &'static [&'static str], code: i32, stdout: Expect, stderr: Expect) -> Case {
    Case { name, args, code, stdout, stderr }
}

/// A scratch directory holding `b.dxf` (the bracket sample) and `s.scr` (a one-line script).
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("cc-cli-usage-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let b = dir.join("b.dxf");
        let r = invoke(&["sample", "bracket", b.to_str().expect("utf-8 path")]);
        assert_eq!(r.code, 0, "{}", r.stderr);
        std::fs::write(dir.join("s.scr"), "LINE 0,0 1,1\n").expect("write script");
        Scratch(dir)
    }

    /// Replaces `{dir}` with the scratch directory and `{b}` with the sample drawing.
    fn expand(&self, text: &str) -> String {
        let dir = self.0.to_str().expect("utf-8 path");
        text.replace("{b}", &Path::new(dir).join("b.dxf").to_string_lossy()).replace("{dir}", dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn check(what: &str, stream: &str, got: &str, expect: Expect, scratch: &Scratch) {
    let first = got.lines().next().unwrap_or("");
    match expect {
        Empty => assert!(got.is_empty(), "{what}: {stream} should be empty, got {got:?}"),
        Line(l) => assert_eq!(first, scratch.expand(l), "{what}: {stream}"),
        Starts(p) => assert!(first.starts_with(&scratch.expand(p)), "{what}: {stream} should start with {p:?}, got {first:?}"),
    }
}

fn run_cases(tag: &str, cases: &[Case]) {
    let scratch = Scratch::new(tag);
    for c in cases {
        let args: Vec<String> = c.args.iter().map(|a| scratch.expand(a)).collect();
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let what = format!("{}: {args:?}", c.name);
        let r = invoke(&args);
        assert_eq!(r.code, c.code, "{what}: exit code (stderr {:?})", r.stderr);
        check(&what, "stdout", &r.stdout, c.stdout, &scratch);
        check(&what, "stderr", &r.stderr, c.stderr, &scratch);
    }
}

#[test]
fn top_level_requests() {
    run_cases(
        "top",
        &[
            case("--help", &["--help"], 0, Line("cadcraft-cli: headless CADCraft"), Empty),
            case("-h", &["-h"], 0, Line("cadcraft-cli: headless CADCraft"), Empty),
            case("help", &["help"], 0, Line("cadcraft-cli: headless CADCraft"), Empty),
            case("--version", &["--version"], 0, Starts("cadcraft-cli "), Empty),
            case("-V", &["-V"], 0, Starts("cadcraft-cli "), Empty),
            case("version is not a subcommand", &["version"], 2, Empty, Line("error: unrecognized subcommand 'version'")),
            case("bare", &[], 2, Empty, Line("cadcraft-cli: headless CADCraft")),
            case("unknown subcommand", &["foo"], 2, Empty, Line("error: unrecognized subcommand 'foo'")),
            case("unknown top-level flag", &["--bogus"], 2, Empty, Line("error: unexpected argument '--bogus' found")),
            case("empty subcommand", &[""], 2, Empty, Line("error: unrecognized subcommand ''")),
        ],
    );
}

/// Each subcommand's help, from `--help`, `-h` or `help <subcommand>`: its first line is the
/// subcommand's summary.
#[test]
fn subcommand_help_requests() {
    run_cases(
        "subhelp",
        &[
            case("info --help", &["info", "--help"], 0, Line("Print a drawing's summary as JSON"), Empty),
            case("info -h", &["info", "-h"], 0, Line("Print a drawing's summary as JSON"), Empty),
            case(
                "convert --help",
                &["convert", "--help"],
                0,
                Line("Convert a drawing; OUT's extension picks the format (dxf, dwg, dwt, svg, png, pdf)"),
                Empty,
            ),
            case(
                "convert -h",
                &["convert", "-h"],
                0,
                Line("Convert a drawing; OUT's extension picks the format (dxf, dwg, dwt, svg, png, pdf)"),
                Empty,
            ),
            case(
                "run --help",
                &["run", "--help"],
                0,
                Line("Open drawings (or start a new one), run scripts and commands in order, save or export the result"),
                Empty,
            ),
            case(
                "run -h",
                &["run", "-h"],
                0,
                Line("Open drawings (or start a new one), run scripts and commands in order, save or export the result"),
                Empty,
            ),
            case("commands --help", &["commands", "--help"], 0, Line("Print the command catalog as JSON"), Empty),
            case(
                "mcp --help",
                &["mcp", "--help"],
                0,
                Line("Run the MCP server on stdio: a headless drawing session, or a bridge to the running app"),
                Empty,
            ),
            case(
                "perf --help",
                &["perf", "--help"],
                0,
                Line(
                    "Time the engine on a synthetic N-entity drawing (default 200000), or the canvas display-list build of a drawing, zoomed in step by step",
                ),
                Empty,
            ),
            case("sample --help", &["sample", "--help"], 0, Line("Write a built-in sample drawing"), Empty),
            case(
                "store-write --help",
                &["store-write", "--help"],
                0,
                Line("Upload a local file to storage at PATH, as JSON (path, size, ETag)"),
                Empty,
            ),
            case("store-read --help", &["store-read", "--help"], 0, Line("Download storage PATH to a local file"), Empty),
            case("store-stat --help", &["store-stat", "--help"], 0, Line("Print a stored file's metadata (size, ETag, modified) as JSON"), Empty),
            case("store-list --help", &["store-list", "--help"], 0, Line("Print a storage folder's direct children as JSON"), Empty),
            case("store-remove --help", &["store-remove", "--help"], 0, Line("Delete a stored file (a missing one is success)"), Empty),
            case(
                "store-probe --help",
                &["store-probe", "--help"],
                0,
                Line("Print a backend's capabilities, and what the server reports, as JSON"),
                Empty,
            ),
            case(
                "store-recover --help",
                &["store-recover", "--help"],
                0,
                Line("Finish or drop writes a crash interrupted, as JSON (safe at every startup)"),
                Empty,
            ),
            case(
                "help run",
                &["help", "run"],
                0,
                Line("Open drawings (or start a new one), run scripts and commands in order, save or export the result"),
                Empty,
            ),
            case("help info", &["help", "info"], 0, Line("Print a drawing's summary as JSON"), Empty),
            case(
                "help after a flag missing its value",
                &["run", "--sample", "--script", "--help"],
                0,
                Line("Open drawings (or start a new one), run scripts and commands in order, save or export the result"),
                Empty,
            ),
            case(
                "help after an unknown flag",
                &["run", "--bogus", "--help"],
                0,
                Line("Open drawings (or start a new one), run scripts and commands in order, save or export the result"),
                Empty,
            ),
            case("help after an extra argument", &["info", "{b}", "extra", "-h"], 0, Line("Print a drawing's summary as JSON"), Empty),
            case("--help after -- is a file name", &["run", "--", "--help"], 1, Empty, Starts("error: invalid parameters for `open`: --help:")),
            case("run --version", &["run", "--version"], 2, Empty, Line("error: unexpected argument '--version' found")),
            case("info --version", &["info", "--version"], 2, Empty, Line("error: unexpected argument '--version' found")),
        ],
    );
}

#[test]
fn unknown_flags_on_every_subcommand() {
    run_cases(
        "unknown",
        &[
            case("info", &["info", "{b}", "--bogus"], 2, Empty, Line("error: unexpected argument '--bogus' found")),
            case("info, flag first", &["info", "--bogus", "{b}"], 2, Empty, Line("error: unexpected argument '--bogus' found")),
            case("info, short flag", &["info", "-x"], 2, Empty, Line("error: unexpected argument '-x' found")),
            case("info, a file name starting with - after --", &["info", "--", "-x"], 1, Empty, Starts("error: invalid parameters for `open`: -x:")),
            case("convert", &["convert", "{b}", "{dir}/o.svg", "--bogus"], 2, Empty, Line("error: unexpected argument '--bogus' found")),
            case("run", &["run", "--sample", "--bogus"], 2, Empty, Line("error: unexpected argument '--bogus' found")),
            case("run, flag first", &["run", "--bogus"], 2, Empty, Line("error: unexpected argument '--bogus' found")),
            case("commands", &["commands", "--bogus"], 2, Empty, Line("error: unexpected argument '--bogus' found")),
            case("commands -- ends the options", &["commands", "--"], 0, Line("["), Empty),
            case("mcp", &["mcp", "--bogus"], 2, Empty, Line("error: unexpected argument '--bogus' found")),
            case("mcp positional", &["mcp", "extra"], 2, Empty, Line("error: unexpected argument 'extra' found")),
            case("perf", &["perf", "--bogus"], 2, Empty, Line("error: unexpected argument '--bogus' found")),
            case("sample", &["sample", "--bogus"], 2, Empty, Line("error: unexpected argument '--bogus' found")),
            case("sample after name", &["sample", "bracket", "--bogus"], 2, Empty, Line("error: unexpected argument '--bogus' found")),
            case(
                "store-write",
                &["store-write", "a.dxf", "{b}", "--store-root", "{dir}", "--bogus"],
                2,
                Empty,
                Line("error: unexpected argument '--bogus' found"),
            ),
            case(
                "store-read",
                &["store-read", "a.dxf", "{dir}/o.dxf", "--store-root", "{dir}", "--bogus"],
                2,
                Empty,
                Line("error: unexpected argument '--bogus' found"),
            ),
            case(
                "store-stat",
                &["store-stat", "a.dxf", "--store-root", "{dir}", "--bogus"],
                2,
                Empty,
                Line("error: unexpected argument '--bogus' found"),
            ),
            case("store-list", &["store-list", "--store-root", "{dir}", "--bogus"], 2, Empty, Line("error: unexpected argument '--bogus' found")),
            case(
                "store-remove",
                &["store-remove", "a.dxf", "--store-root", "{dir}", "--bogus"],
                2,
                Empty,
                Line("error: unexpected argument '--bogus' found"),
            ),
            case("store-probe", &["store-probe", "--store-root", "{dir}", "--bogus"], 2, Empty, Line("error: unexpected argument '--bogus' found")),
            case(
                "store-recover",
                &["store-recover", "--store-root", "{dir}", "--bogus"],
                2,
                Empty,
                Line("error: unexpected argument '--bogus' found"),
            ),
        ],
    );
}

/// `--x=v` is a spelling of `--x v`, and the one a value starting with `-` needs.
#[test]
fn value_flags_in_both_forms() {
    run_cases(
        "forms",
        &[
            case("--window v", &["convert", "{b}", "{dir}/w1.svg", "--window", "0,0,100,100"], 0, Empty, Starts("wrote {dir}/w1.svg (")),
            case("--window=v", &["convert", "{b}", "{dir}/w2.svg", "--window=0,0,100,100"], 0, Empty, Starts("wrote {dir}/w2.svg (")),
            case(
                "run --window v",
                &["run", "--sample", "--window", "0,0,100,100", "--export", "{dir}/w3.svg"],
                0,
                Empty,
                Starts("wrote {dir}/w3.svg ("),
            ),
            case(
                "run --window=v",
                &["run", "--sample", "--window=0,0,100,100", "--export", "{dir}/w4.svg"],
                0,
                Empty,
                Starts("wrote {dir}/w4.svg ("),
            ),
            case("--view v", &["convert", "{b}", "{dir}/v1.svg", "--view", "extents"], 0, Empty, Starts("wrote {dir}/v1.svg (")),
            case("--view=v", &["convert", "{b}", "{dir}/v2.svg", "--view=extents"], 0, Empty, Starts("wrote {dir}/v2.svg (")),
            case("run --view v", &["run", "--sample", "--view", "current", "--export", "{dir}/v3.svg"], 0, Empty, Starts("wrote {dir}/v3.svg (")),
            case("run --view=v", &["run", "--sample", "--view=extents", "--export={dir}/v4.svg"], 0, Empty, Starts("wrote {dir}/v4.svg (")),
            case("--script v", &["run", "--sample", "--script", "LINE 0,0 1,1"], 0, Empty, Line("Command: LINE")),
            case("--script=v", &["run", "--sample", "--script=LINE 0,0 1,1"], 0, Empty, Line("Command: LINE")),
            case("--script-file v", &["run", "--sample", "--script-file", "{dir}/s.scr"], 0, Empty, Starts("Command: LINE")),
            case("--script-file=v", &["run", "--sample", "--script-file={dir}/s.scr"], 0, Empty, Starts("Command: LINE")),
            case("--cmd v", &["run", "--sample", "--cmd", "nosuch"], 1, Empty, Line("error: unknown command `nosuch`")),
            case("--cmd=v", &["run", "--sample", "--cmd=nosuch"], 1, Empty, Line("error: unknown command `nosuch`")),
            case("--cmd=-v", &["run", "--sample", "--cmd=-x"], 1, Empty, Line("error: unknown command `-x`")),
            case("--save v", &["run", "--sample", "--save", "{dir}/s1.dxf"], 0, Empty, Starts("wrote {dir}/s1.dxf (")),
            case("--save=v", &["run", "--sample", "--save={dir}/s2.dxf"], 0, Empty, Starts("wrote {dir}/s2.dxf (")),
            case("--export v", &["run", "--sample", "--export", "{dir}/e1.svg"], 0, Empty, Starts("wrote {dir}/e1.svg (")),
            case("--export=v", &["run", "--sample", "--export={dir}/e2.svg"], 0, Empty, Starts("wrote {dir}/e2.svg (")),
            case(
                "--window=-v: negative coordinates need =",
                &["convert", "{b}", "{dir}/w5.svg", "--window=-1000,-1000,1000,1000"],
                0,
                Empty,
                Starts("wrote {dir}/w5.svg ("),
            ),
            case(
                "--window -v: a value starting with - is a flag",
                &["convert", "{b}", "{dir}/w6.svg", "--window", "-1000,-1000,1000,1000"],
                2,
                Empty,
                Line("error: unexpected argument '-1' found"),
            ),
            case("--connect v", &["mcp", "--connect", "127.0.0.1:1"], 1, Empty, Starts("error: cannot connect to 127.0.0.1:1:")),
            case("--connect=v", &["mcp", "--connect=127.0.0.1:1"], 1, Empty, Starts("error: cannot connect to 127.0.0.1:1:")),
        ],
    );
}

/// A value flag at the end of the arguments, or followed by another flag, is missing its value:
/// a flag is never taken as a value.
#[test]
fn flags_missing_their_value() {
    run_cases(
        "missing",
        &[
            case(
                "convert --window last",
                &["convert", "{b}", "{dir}/o.svg", "--window"],
                2,
                Empty,
                Line("error: a value is required for '--window <x0,y0,x1,y1>' but none was supplied"),
            ),
            case(
                "convert --view last",
                &["convert", "{b}", "{dir}/o.svg", "--view"],
                2,
                Empty,
                Line("error: a value is required for '--view <extents|current>' but none was supplied"),
            ),
            case(
                "convert --window then flag",
                &["convert", "{b}", "{dir}/o.svg", "--window", "--view", "extents"],
                2,
                Empty,
                Line("error: a value is required for '--window <x0,y0,x1,y1>' but none was supplied"),
            ),
            case(
                "convert --view then flag",
                &["convert", "{b}", "{dir}/o.svg", "--view", "--window", "0,0,1,1"],
                2,
                Empty,
                Line("error: a value is required for '--view <extents|current>' but none was supplied"),
            ),
            case(
                "run --window last",
                &["run", "--sample", "--window"],
                2,
                Empty,
                Line("error: a value is required for '--window <x0,y0,x1,y1>' but none was supplied"),
            ),
            case(
                "run --view last",
                &["run", "--sample", "--view"],
                2,
                Empty,
                Line("error: a value is required for '--view <extents|current>' but none was supplied"),
            ),
            case(
                "run --script last",
                &["run", "--sample", "--script"],
                2,
                Empty,
                Line("error: a value is required for '--script <TEXT>' but none was supplied"),
            ),
            case(
                "run --script then flag",
                &["run", "--sample", "--script", "--sample"],
                2,
                Empty,
                Line("error: a value is required for '--script <TEXT>' but none was supplied"),
            ),
            case(
                "run --script-file last",
                &["run", "--script-file"],
                2,
                Empty,
                Line("error: a value is required for '--script-file <F.scr>' but none was supplied"),
            ),
            case(
                "run --script-file then flag",
                &["run", "--sample", "--script-file", "--sample"],
                2,
                Empty,
                Line("error: a value is required for '--script-file <F.scr>' but none was supplied"),
            ),
            case(
                "run --cmd last",
                &["run", "--sample", "--cmd"],
                2,
                Empty,
                Line("error: a value is required for '--cmd <id {json}>' but none was supplied"),
            ),
            case(
                "run --cmd then flag",
                &["run", "--sample", "--cmd", "--sample"],
                2,
                Empty,
                Line("error: a value is required for '--cmd <id {json}>' but none was supplied"),
            ),
            case(
                "run --save last",
                &["run", "--sample", "--save"],
                2,
                Empty,
                Line("error: a value is required for '--save <OUT.dxf>' but none was supplied"),
            ),
            case(
                "run --export last",
                &["run", "--sample", "--export"],
                2,
                Empty,
                Line("error: a value is required for '--export <OUT.png>' but none was supplied"),
            ),
            case(
                "mcp --connect last",
                &["mcp", "--connect"],
                2,
                Empty,
                Line("error: a value is required for '--connect <HOST:PORT>' but none was supplied"),
            ),
            case(
                "mcp --connect then flag",
                &["mcp", "--connect", "--lazy"],
                2,
                Empty,
                Line("error: a value is required for '--connect <HOST:PORT>' but none was supplied"),
            ),
            case(
                "mcp --connect empty",
                &["mcp", "--connect", " "],
                2,
                Empty,
                Line("error: invalid value ' ' for '--connect <HOST:PORT>': expected HOST:PORT (the app's --control port, e.g. 127.0.0.1:7979)"),
            ),
            case(
                "mcp --connect without a port",
                &["mcp", "--connect", "example.com"],
                2,
                Empty,
                Line(
                    "error: invalid value 'example.com' for '--connect <HOST:PORT>': expected HOST:PORT (the app's --control port, e.g. 127.0.0.1:7979)",
                ),
            ),
            case(
                "mcp --connect port out of range",
                &["mcp", "--connect", "host:99999"],
                2,
                Empty,
                Line(
                    "error: invalid value 'host:99999' for '--connect <HOST:PORT>': expected HOST:PORT (the app's --control port, e.g. 127.0.0.1:7979)",
                ),
            ),
            case(
                "mcp --connect=-v",
                &["mcp", "--connect=-v"],
                2,
                Empty,
                Line("error: invalid value '-v' for '--connect <HOST:PORT>': expected HOST:PORT (the app's --control port, e.g. 127.0.0.1:7979)"),
            ),
        ],
    );
}

#[test]
fn repeated_and_combined_flags() {
    run_cases(
        "repeat",
        &[
            case(
                "--save twice",
                &["run", "--sample", "--save", "{dir}/a.dxf", "--save", "{dir}/b2.dxf"],
                2,
                Empty,
                Line("error: the argument '--save <OUT.dxf>' cannot be used multiple times"),
            ),
            case(
                "--export twice",
                &["run", "--sample", "--export", "{dir}/x1.svg", "--export", "{dir}/x2.svg"],
                2,
                Empty,
                Line("error: the argument '--export <OUT.png>' cannot be used multiple times"),
            ),
            case(
                "--window twice",
                &["run", "--sample", "--window", "0,0,1,1", "--window", "0,0,2,2", "--export", "{dir}/ww.svg"],
                2,
                Empty,
                Line("error: the argument '--window <x0,y0,x1,y1>' cannot be used multiple times"),
            ),
            case(
                "convert --window twice",
                &["convert", "{b}", "{dir}/cw.svg", "--window", "0,0,1,1", "--window", "0,0,2,2"],
                2,
                Empty,
                Line("error: the argument '--window <x0,y0,x1,y1>' cannot be used multiple times"),
            ),
            case(
                "--view twice, different",
                &["run", "--sample", "--view", "extents", "--view", "current", "--export", "{dir}/vv.svg"],
                2,
                Empty,
                Line("error: the argument '--view <extents|current>' cannot be used multiple times"),
            ),
            case(
                "--view twice, same",
                &["run", "--sample", "--view", "extents", "--view", "extents", "--export", "{dir}/vs.svg"],
                2,
                Empty,
                Line("error: the argument '--view <extents|current>' cannot be used multiple times"),
            ),
            case("--sample twice", &["run", "--sample", "--sample"], 2, Empty, Line("error: the argument '--sample' cannot be used multiple times")),
            case("--metric twice", &["run", "--metric", "--metric"], 2, Empty, Line("error: the argument '--metric' cannot be used multiple times")),
            case(
                "--lazy twice",
                &["mcp", "--connect", "127.0.0.1:1", "--lazy", "--lazy"],
                2,
                Empty,
                Line("error: the argument '--lazy' cannot be used multiple times"),
            ),
            case(
                "--launch twice",
                &["mcp", "--connect", "127.0.0.1:1", "--launch", "--launch"],
                2,
                Empty,
                Line("error: the argument '--launch' cannot be used multiple times"),
            ),
            case(
                "--script, --script-file and --cmd repeat",
                &[
                    "run",
                    "--sample",
                    "--script",
                    "LINE 0,0 1,1",
                    "--script",
                    "LINE 0,0 2,2",
                    "--script-file",
                    "{dir}/s.scr",
                    "--script-file",
                    "{dir}/s.scr",
                ],
                0,
                Empty,
                Starts("Command: LINE"),
            ),
            case(
                "--window and --view",
                &["run", "--sample", "--window", "0,0,1,1", "--view", "extents", "--export", "{dir}/wv.svg"],
                2,
                Empty,
                Line("error: the argument '--window <x0,y0,x1,y1>' cannot be used with '--view <extents|current>'"),
            ),
            case(
                "convert --window and --view",
                &["convert", "{b}", "{dir}/cwv.svg", "--window", "0,0,1,1", "--view", "extents"],
                2,
                Empty,
                Line("error: the argument '--window <x0,y0,x1,y1>' cannot be used with '--view <extents|current>'"),
            ),
            case(
                "--view is case-insensitive",
                &["run", "--sample", "--view", "EXTENTS", "--export", "{dir}/vu.svg"],
                0,
                Empty,
                Starts("wrote {dir}/vu.svg ("),
            ),
            case(
                "--connect twice",
                &["mcp", "--connect", "127.0.0.1:1", "--connect", "127.0.0.1:2"],
                2,
                Empty,
                Line("error: the argument '--connect <HOST:PORT>' cannot be used multiple times"),
            ),
            case("--lazy without --connect", &["mcp", "--lazy"], 2, Empty, Line("error: the following required arguments were not provided:")),
            case("--launch without --connect", &["mcp", "--launch"], 2, Empty, Line("error: the following required arguments were not provided:")),
            case("--sample --metric: both accepted", &["run", "--sample", "--metric"], 0, Empty, Empty),
            case("--sample and a file", &["run", "--sample", "{b}"], 0, Empty, Empty),
            case("two files", &["run", "{b}", "{b}"], 0, Empty, Empty),
            case(
                "--window without export",
                &["run", "--sample", "--window", "0,0,1,1"],
                2,
                Empty,
                Line("error: the following required arguments were not provided:"),
            ),
            case(
                "--view without export",
                &["run", "--sample", "--view", "current"],
                2,
                Empty,
                Line("error: the following required arguments were not provided:"),
            ),
            case(
                "--window with a dxf output",
                &["convert", "{b}", "{dir}/o.dxf", "--window", "0,0,1,1"],
                2,
                Empty,
                Line("error: --window only applies to image output (png, svg, pdf), not `{dir}/o.dxf`"),
            ),
            case(
                "--view with a dxf output",
                &["convert", "{b}", "{dir}/o.dxf", "--view", "extents"],
                2,
                Empty,
                Line("error: --view only applies to image output (png, svg, pdf), not `{dir}/o.dxf`"),
            ),
            case(
                "run --window with a dxf export",
                &["run", "--sample", "--window", "0,0,1,1", "--export", "{dir}/o.dxf"],
                2,
                Empty,
                Line("error: --window only applies to image output (png, svg, pdf), not `{dir}/o.dxf`"),
            ),
            case(
                "run --view with a dxf export",
                &["run", "--sample", "--view", "current", "--export", "{dir}/o.dxf"],
                2,
                Empty,
                Line("error: --view only applies to image output (png, svg, pdf), not `{dir}/o.dxf`"),
            ),
        ],
    );
}

#[test]
fn arity_and_positional_arguments() {
    run_cases(
        "arity",
        &[
            case("info without a file", &["info"], 2, Empty, Line("error: the following required arguments were not provided:")),
            case("info with two files", &["info", "{b}", "extra"], 2, Empty, Line("error: unexpected argument 'extra' found")),
            case("info missing file", &["info", "missing.dxf"], 1, Empty, Starts("error: invalid parameters for `open`: missing.dxf:")),
            case("convert with no files", &["convert"], 2, Empty, Line("error: the following required arguments were not provided:")),
            case("convert with one file", &["convert", "{b}"], 2, Empty, Line("error: the following required arguments were not provided:")),
            case(
                "convert with three files",
                &["convert", "{b}", "{dir}/o.svg", "{dir}/x.svg"],
                2,
                Empty,
                Line("error: unexpected argument '{dir}/x.svg' found"),
            ),
            case("run missing file", &["run", "missing.dxf"], 1, Empty, Starts("error: invalid parameters for `open`: missing.dxf:")),
            case("commands, no match", &["commands", "zzzznomatch"], 0, Line("[]"), Empty),
            case(
                "commands with an extra argument",
                &["commands", "zzzznomatch", "extra"],
                2,
                Empty,
                Line("error: unexpected argument 'extra' found"),
            ),
            case("sample without arguments", &["sample"], 2, Empty, Line("error: the following required arguments were not provided:")),
            case("sample without an output", &["sample", "bracket"], 2, Empty, Line("error: the following required arguments were not provided:")),
            case(
                "sample with a bad name",
                &["sample", "nosuch", "{dir}/o.dxf"],
                2,
                Empty,
                Line("error: invalid value 'nosuch' for '<bracket|floorplan>'"),
            ),
            case(
                "sample names are case-sensitive",
                &["sample", "Bracket", "{dir}/o.dxf"],
                2,
                Empty,
                Line("error: invalid value 'Bracket' for '<bracket|floorplan>'"),
            ),
            case(
                "sample with an empty output",
                &["sample", "bracket", ""],
                2,
                Empty,
                Line("error: a value is required for '<OUT>' but none was supplied"),
            ),
            case("info with an empty file name", &["info", ""], 2, Empty, Line("error: a value is required for '<FILE>' but none was supplied")),
            case("perf with an empty file name", &["perf", ""], 2, Empty, Line("error: a value is required for '[N|FILE]' but none was supplied")),
            case(
                "sample with an extra argument",
                &["sample", "bracket", "{dir}/sx.dxf", "extra"],
                2,
                Empty,
                Line("error: unexpected argument 'extra' found"),
            ),
            case("sample floor alias", &["sample", "floor", "{dir}/sf.dxf"], 0, Empty, Starts("wrote {dir}/sf.dxf (")),
            case("sample with a bad extension", &["sample", "bracket", "{dir}/o.xyz"], 1, Empty, Line("error: unsupported file type `xyz`")),
            case("perf with a word", &["perf", "abc"], 1, Empty, Starts("error: invalid parameters for `open`: abc:")),
            case("perf with a negative number", &["perf", "-5"], 2, Empty, Line("error: unexpected argument '-5' found")),
            case("perf with a missing file", &["perf", "missing.dxf"], 1, Empty, Starts("error: invalid parameters for `open`: missing.dxf:")),
            case("perf 1", &["perf", "1"], 0, Starts("operation"), Empty),
            case("perf 0 is raised to 1", &["perf", "0"], 0, Starts("operation"), Empty),
            case("perf with two counts", &["perf", "1", "2"], 2, Empty, Line("error: unexpected argument '2' found")),
        ],
    );
}

#[test]
fn bad_values() {
    run_cases(
        "values",
        &[
            case(
                "--view sideways",
                &["convert", "{b}", "{dir}/o.svg", "--view", "sideways"],
                2,
                Empty,
                Line("error: invalid value 'sideways' for '--view <extents|current>'"),
            ),
            case(
                "run --view sideways",
                &["run", "--sample", "--view", "sideways"],
                2,
                Empty,
                Line("error: invalid value 'sideways' for '--view <extents|current>'"),
            ),
            case(
                "--view sideways before the output check",
                &["convert", "{b}", "{dir}/o.dxf", "--view", "sideways"],
                2,
                Empty,
                Line("error: invalid value 'sideways' for '--view <extents|current>'"),
            ),
            case(
                "--window 3 numbers",
                &["convert", "{b}", "{dir}/o.svg", "--window", "1,2,3"],
                2,
                Empty,
                Line("error: invalid value '1,2,3' for '--window <x0,y0,x1,y1>': expected four numbers x0,y0,x1,y1"),
            ),
            case(
                "--window 5 numbers",
                &["convert", "{b}", "{dir}/o.svg", "--window", "1,2,3,4,5"],
                2,
                Empty,
                Line("error: invalid value '1,2,3,4,5' for '--window <x0,y0,x1,y1>': expected four numbers x0,y0,x1,y1"),
            ),
            case(
                "--window reversed",
                &["convert", "{b}", "{dir}/o.svg", "--window", "3,3,1,1"],
                2,
                Empty,
                Line("error: invalid value '3,3,1,1' for '--window <x0,y0,x1,y1>': need x0 < x1 and y0 < y1 (lower-left corner first)"),
            ),
            case(
                "--window not numbers",
                &["convert", "{b}", "{dir}/o.svg", "--window", "a,b,c,d"],
                2,
                Empty,
                Line("error: invalid value 'a,b,c,d' for '--window <x0,y0,x1,y1>': x0 must be a finite number, got `a`"),
            ),
            case(
                "--window NaN",
                &["convert", "{b}", "{dir}/o.svg", "--window", "NaN,0,1,1"],
                2,
                Empty,
                Line("error: invalid value 'NaN,0,1,1' for '--window <x0,y0,x1,y1>': x0 must be a finite number, got `NaN`"),
            ),
            case(
                "--window too large",
                &["convert", "{b}", "{dir}/o.svg", "--window", "0,0,1e13,1"],
                2,
                Empty,
                Line("error: invalid value '0,0,1e13,1' for '--window <x0,y0,x1,y1>': export window: coordinates must lie within ±1e12"),
            ),
            case(
                "run --window 3 numbers",
                &["run", "--sample", "--window", "1,2,3", "--export", "{dir}/o.svg"],
                2,
                Empty,
                Line("error: invalid value '1,2,3' for '--window <x0,y0,x1,y1>': expected four numbers x0,y0,x1,y1"),
            ),
            case(
                "run --window reversed",
                &["run", "--sample", "--window", "3,3,1,1", "--export", "{dir}/o.svg"],
                2,
                Empty,
                Line("error: invalid value '3,3,1,1' for '--window <x0,y0,x1,y1>': need x0 < x1 and y0 < y1 (lower-left corner first)"),
            ),
            case(
                "--cmd bad json",
                &["run", "--sample", "--cmd", "line {bad json"],
                2,
                Empty,
                Line("error: invalid value 'line {bad json' for '--cmd <id {json}>': key must be a string at line 1 column 2"),
            ),
            case(
                "--cmd bad json after a good step: nothing runs",
                &["run", "--sample", "--cmd", "line {\"points\":[[0,0],[1,1]]}", "--cmd", "line {"],
                2,
                Empty,
                Starts("error: invalid value 'line {' for '--cmd <id {json}>': "),
            ),
            case(
                "--cmd without an id",
                &["run", "--sample", "--cmd", " {}"],
                2,
                Empty,
                Line("error: invalid value ' {}' for '--cmd <id {json}>': expected a command id, then optionally its JSON parameters"),
            ),
            case(
                "--cmd empty",
                &["run", "--sample", "--cmd="],
                2,
                Empty,
                Line("error: invalid value '' for '--cmd <id {json}>': expected a command id, then optionally its JSON parameters"),
            ),
            case("--cmd unknown id", &["run", "--sample", "--cmd", "nosuch"], 1, Empty, Line("error: unknown command `nosuch`")),
            case(
                "--cmd {last} with no earlier handle",
                &["run", "--sample", "--cmd", "erase {\"handles\":[\"{last}\"]}"],
                1,
                Empty,
                Line("error: --cmd erase: `{last}` needs an earlier --cmd in this run to have returned a handle"),
            ),
            case("--script-file missing", &["run", "--sample", "--script-file", "missing.scr"], 1, Empty, Starts("error: missing.scr:")),
        ],
    );
}

/// `mcp` serves on the process's real stdin and stdout, so the headless server runs through the
/// binary, with a closed stdin.
#[test]
fn mcp_headless_with_a_closed_stdin() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_cadcraft-cli"))
        .arg("mcp")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run cadcraft-cli");
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
}

/// A drawing to open whose name isn't valid Unicode reaches the handler as a path; the engine opens
/// files by text name, so it is refused there (exit 1), naming the file. Text that isn't Unicode is
/// a usage error.
#[cfg(unix)]
#[test]
fn non_unicode_arguments() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let invoke_os = |args: Vec<OsString>| {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = cadcraft_cli::run(args, &mut out, &mut err);
        assert!(out.is_empty());
        (code, String::from_utf8_lossy(&err).into_owned())
    };
    let latin1 = || OsString::from_vec(b"caf\xe9.dxf".to_vec());
    let (code, err) = invoke_os(vec![OsString::from("info"), latin1()]);
    assert_eq!(code, 1);
    assert_eq!(err.lines().next(), Some("error: caf\u{FFFD}.dxf: the file name is not valid Unicode"));
    let (code, err) = invoke_os(vec![OsString::from("run"), OsString::from("--sample"), OsString::from("--cmd"), latin1()]);
    assert_eq!(code, 2);
    assert!(err.starts_with("error: invalid UTF-8 was detected"), "{err}");
}

/// An output name that isn't valid Unicode is written as given: only its extension picks the format.
#[cfg(unix)]
#[test]
fn non_unicode_output_names() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let s = Scratch::new("nonunicode-out");
    let latin1 = |ext: &str| {
        let mut n = b"caf\xe9.".to_vec();
        n.extend_from_slice(ext.as_bytes());
        s.0.join(OsString::from_vec(n))
    };
    let run = |args: Vec<OsString>| {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = cadcraft_cli::run(args, &mut out, &mut err);
        (code, String::from_utf8_lossy(&err).into_owned())
    };
    let b = s.0.join("b.dxf");
    let cases: [(Vec<OsString>, std::path::PathBuf); 3] = [
        (vec!["sample".into(), "bracket".into(), latin1("dxf").into()], latin1("dxf")),
        (vec!["convert".into(), b.clone().into(), latin1("svg").into()], latin1("svg")),
        (vec!["run".into(), "--sample".into(), "--save".into(), latin1("dwg").into()], latin1("dwg")),
    ];
    for (args, path) in cases {
        let (code, err) = run(args.clone());
        assert_eq!(code, 0, "{args:?}: {err}");
        assert!(path.exists(), "{path:?}");
        assert!(std::fs::metadata(&path).expect("stat output").len() > 0, "{path:?}");
    }
    // An input with such a name is still refused.
    let (code, err) = run(vec!["info".into(), latin1("dxf").into()]);
    assert_eq!(code, 1);
    assert!(err.contains("not valid Unicode"), "{err}");
}

/// An output that is an input file is refused before any work (exit 2, the input untouched) unless
/// `--in-place` allows it; the same file under another spelling counts, another existing file doesn't.
#[test]
fn output_that_replaces_an_input() {
    let s = Scratch::new("inplace");
    let dir = s.0.to_str().expect("utf-8 path").to_string();
    let b = format!("{dir}/b.dxf");
    let original = std::fs::read(&b).expect("read input");
    std::fs::create_dir_all(format!("{dir}/sub")).expect("create sub");
    let dotted = format!("{dir}/sub/../b.dxf");
    #[allow(unused_mut)]
    let mut spellings = vec![b.clone(), dotted];
    #[cfg(unix)]
    {
        let link = format!("{dir}/link.dxf");
        std::os::unix::fs::symlink(&b, &link).expect("symlink");
        spellings.push(link);
    }
    for out in &spellings {
        for args in [vec!["convert", &b, out], vec!["run", &b, "--save", out], vec!["run", &b, "--export", out]] {
            let r = invoke(&args);
            assert_eq!(r.code, 2, "{args:?}: {}", r.stderr);
            assert!(r.stdout.is_empty(), "{args:?}");
            assert!(r.stderr.starts_with(&format!("error: {out} is an input file")), "{args:?}: {}", r.stderr);
            assert!(r.stderr.contains("--in-place"), "{args:?}: {}", r.stderr);
            assert_eq!(std::fs::read(&b).expect("read input"), original, "{args:?}: input changed");
        }
    }

    // Another existing file is replaced as before.
    let other = format!("{dir}/other.dxf");
    std::fs::write(&other, "x").expect("write other");
    let r = invoke(&["convert", &b, &other]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_ne!(std::fs::read(&other).expect("read other"), b"x");

    // --in-place allows it, for each spelling and flag: the input itself is rewritten, and a
    // symlink stays a link to it.
    let line = "line {\"points\":[[0,0],[1,1]]}";
    let lines = |p: &str| std::fs::read_to_string(p).expect("read input").lines().filter(|l| l.trim_end() == "LINE").count();
    let before = lines(&b);
    for out in &spellings {
        std::fs::write(&b, &original).expect("restore input");
        let r = invoke(&["convert", &b, out, "--in-place"]);
        assert_eq!(r.code, 0, "convert {out}: {}", r.stderr);
        assert_eq!(lines(&b), before, "convert {out} keeps the drawing");
        for flag in ["--save", "--export"] {
            std::fs::write(&b, &original).expect("restore input");
            let r = invoke(&["run", &b, "--cmd", line, "--in-place", flag, out]);
            assert_eq!(r.code, 0, "{flag} {out}: {}", r.stderr);
            assert_eq!(lines(&b), before + 1, "{flag} {out}: the input gains the line");
        }
    }
    #[cfg(unix)]
    {
        let link = format!("{dir}/link.dxf");
        assert!(std::fs::symlink_metadata(&link).expect("stat link").file_type().is_symlink(), "{link} is still a symlink");
    }

    // An output that doesn't exist yet is never an input.
    let new = format!("{dir}/new.dxf");
    let r = invoke(&["run", &b, "--save", &new]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(std::path::Path::new(&new).is_file(), "{new} written");
}

/// JSON results are pretty-printed; `--compact` puts each on one line (RFC-0012 rule 1.3). `run`
/// prints one line per `--cmd` result either way.
#[test]
fn json_output_is_pretty_unless_compact() {
    let s = Scratch::new("compact");
    let b = format!("{}/b.dxf", s.0.to_str().expect("utf-8 path"));
    for args in [&["info", b.as_str()][..], &["commands", "ellipse"]] {
        let help = invoke(&[args[0], "--help"]);
        assert!(
            help.stdout.contains("--compact") && help.stdout.contains("One line of JSON instead of pretty-printed"),
            "{} --help: {}",
            args[0],
            help.stdout
        );
        let pretty = invoke(args);
        assert_eq!(pretty.code, 0, "{args:?}: {}", pretty.stderr);
        assert!(pretty.stdout.lines().count() > 1, "{args:?} is pretty-printed: {}", pretty.stdout);
        let compact = invoke(&[args, &["--compact"]].concat());
        assert_eq!(compact.code, 0, "{args:?} --compact: {}", compact.stderr);
        assert_eq!(compact.stdout.lines().count(), 1, "{args:?} --compact is one line: {}", compact.stdout);
        let a: serde_json::Value = serde_json::from_str(&pretty.stdout).expect("pretty JSON");
        let c: serde_json::Value = serde_json::from_str(&compact.stdout).expect("compact JSON");
        assert_eq!(a, c, "{args:?}: same JSON either way");
    }
    let r = invoke(&["run", "--sample", "--cmd", "drawing.inspect", "--cmd", "drawing.inspect"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.stdout.lines().count(), 2, "one line per --cmd result: {}", r.stdout);
    assert_eq!(invoke(&["run", "--sample", "--compact"]).code, 2, "run takes no --compact");
}
