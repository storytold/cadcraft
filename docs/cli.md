# cadcraft-cli

`cadcraft-cli` is headless CADCraft: it reads, converts and exports drawings, runs scripts and
commands, prints the command catalog and serves MCP. It follows
[RFC-0012: Command-line conventions](https://github.com/storytold/craft-contrib/pull/18).

`cadcraft-cli --help` and `cadcraft-cli help <subcommand>` (or `<subcommand> --help`) are the
reference for every subcommand, flag and value; this page doesn't repeat them. Examples are in the
[README](../README.md#drive-it-from-agents-mcp-and-the-cli), MCP setup in [mcp.md](mcp.md),
scripts and the JSON command forms in [customization.md](customization.md).

## Conventions

| | |
|---|---|
| Exit codes | `0` success, `1` the command failed, `2` usage error. |
| Streams | Results on stdout. `info` and `commands` print pretty JSON, or one line with `--compact`; `run` prints one JSON line per `--cmd` result (JSON lines); a command with no result (JSON `null`) prints no line. Errors (`error: …`), warnings and `wrote …` lines on stderr. `mcp` speaks MCP on stdout and writes nothing else there. |
| Usage errors | An unknown or repeated flag, a missing or malformed value (`--window`, `--view`, `--cmd` JSON, a sample name), flags that conflict (`--window` with `--view`) or that need another (`run --window` without `--export`) stop before any work, with exit 2. `--script`, `--script-file`, `--cmd` and `run`'s files repeat and run in command-line order. |
| Outputs that replace an input | `convert IN OUT` and `run FILE… --save OUT` / `--export OUT` refuse an OUT that is the same file as an input (any spelling or symlink of it): usage error, exit 2, nothing opened or written. `--in-place` allows it. A save a script or `--cmd` runs is the engine's. |
| Values starting with `-` | Written with `=`: `--window=-10,-10,10,10`, `--export=-plan.png`. |
| Paths | Read as given, including names that aren't valid Unicode. The engine opens files by text name, so such a name is reported as an error (exit 1) where a drawing is opened; an output takes any name whose extension is ASCII. |
| Completions and man pages | The Linux packages install them. To generate them yourself: `cargo run -p cadcraft-cli --example man-and-completions -- <out-dir>`. |

## Running steps in the open app

`run --connect HOST:PORT` sends `run`'s drawings and steps to a desktop app started with
`--control PORT` instead of running them headless, so each call edits the drawing on screen and
the next call finds it as the last one left it:

```sh
cadcraft-cli run --connect 127.0.0.1:7979 --script "circle 0,0 25"
cadcraft-cli run --connect 127.0.0.1:7979 --cmd 'line {"points":[[0,0],[100,50]]}' \
  --cmd 'properties.set {"handles":["{last}"],"color":1}'
cadcraft-cli run --connect 127.0.0.1:7979 --export view.png
```

Each step is one [control protocol](control-protocol.md) request, sent with the session token
as `mcp --connect` sends it: a file is `app.open`, `--metric` is `new {"metric":true}`, a
`--script` or `--script-file` is `cmdline.script`, a `--cmd` is `engine.execute`, and `--save` or
`--export` is `app.save` (the app's Save As; an image exports and the drawing keeps its name).
Without a file or `--metric` the steps act on the drawing the app has open. Paths are made
absolute first, script files are read and every value checked before the first request, and the
first refused step stops the run (exit 1, naming the step). A script's command-line output goes
to stderr, as headless.

The control channel doesn't say which drawing is current, so `{last}` is forgotten after a
`--script` or a `--cmd` that can change it (`new`, `open`, `close`, `closeall`,
`document.switch`): the run fails rather than apply a handle to another drawing. `--sample`,
`--window` and `--view` are headless only. As for `mcp --connect`, `CADCRAFT_CONTROL_PORT` is
not read: a variable must not turn a headless run into edits of the open drawing.

From WSL, call the Windows `cadcraft-cli.exe` (WSL runs Windows programs), not a Linux build: the
app listens on Windows' loopback, which WSL's default networking doesn't reach, and writes its token
under `%LOCALAPPDATA%`. Relative paths and Linux paths work from a folder inside WSL, since the
`.exe` then runs in `\\wsl.localhost\<distro>\…`. From a folder on a drive (`/mnt/c/…`), a Linux path
such as `/home/me/plan.dxf` has no drive and would land on `C:`, so it is refused: give a relative
path or convert it with `wslpath -w`.

## Environment variables

Variables are fallbacks for flags; `--help` shows each next to its flag. An empty value counts as
unset and the flag wins over the variable, except for `CADCRAFT_CONFIG_DIR` (see below).

| Variable | Flag | Subcommands |
|---|---|---|
| `CADCRAFT_CONFIG_DIR` | `--config-dir` (the settings folder; plotter configurations are in its `plotters` folder; empty means no settings folder) | `convert`, `run`, `mcp`, `sample` |
| `CADCRAFT_FONTALT` | `--fontalt` (FONTALT) | `convert`, `run`, `mcp`, `sample` |
| `CADCRAFT_FONTFALLBACK` | `--fontfallback` (FONTFALLBACK) | `convert`, `run`, `mcp`, `sample` |

The desktop app's are listed by `cadcraft --help`; its one flag fallback is
`CADCRAFT_CONTROL_PORT` for `--control`, where a bad value is a usage error that names the
variable.

One variable is not a flag fallback: **`CADCRAFT_CAP_HEIGHT`** pins the cap height a
metric-compatible substitute font is scaled by, so text on a substitute keeps the size of the
font it stands in for (`LiberationSans=715.8;LiberationSans-Bold=715.8`; each entry names the
substitute's PostScript name and the cap height to give it, in font units at 1000 units per em —
the cap height of the font it replaces, not the substitute's own; entries without a name or with
a height below 1 or not finite are dropped). It is a packaging hook for installations whose
substitutes lack the original's OS/2 cap height, and applies to every subcommand.

## Not followed yet

- **`CADCRAFT_CONFIG_DIR` is the one flag fallback where an empty variable doesn't count as
  unset** (RFC-0012 rule 1.7): empty (or `--config-dir ""`) means no settings folder, as it did
  before the port.
- **No variable for `mcp --connect`.** Photocraft's `mcp --bridge` falls back to its
  `*_CONTROL_PORT` variable. Here an exported `CADCRAFT_CONTROL_PORT` would silently switch `mcp`
  from a headless session to the running app, so `--connect` stays a flag only.
- **The desktop app on Windows** is built without a console, so a usage error in its launch flags
  isn't visible there; the app exits with code 2.
- **The `string-error` ast-grep rule** covers the CLI code only. The engine's file hooks
  (`cadcraft_engine::cmd::file::IoHooks`) still return `Result<_, String>`, so `install_io` maps
  errors to strings at that boundary.
- **No AGENTS.md line for the ast-grep scan.** RFC-0012 asks an app's AGENTS.md to have coding
  agents run the scan before pushing (`ast-grep scan -c lint/ast-grep/sgconfig.yml`); CI runs it
  on every PR that touches the CLI code.
- **`perf` prints a text table**, with no `--json` (whether text listings get one is an open
  question in RFC-0012).
- **Saves a command or script runs** (`--cmd 'saveas {…}'`, a script's `SAVEAS`) are the engine's
  and are not checked against the opened drawings; only `--save` and `--export` are.
