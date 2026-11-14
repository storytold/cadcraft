# Customizing and automating CADCraft

What you can use today to automate CADCraft or bring over add-ons written for other CAD programs,
and what is not there yet.

| You have | In CADCraft | More |
|---|---|---|
| LISP routines (`.lsp`) | Run with APPLOAD or `(load "name")`. The core language, `command`, entity lists, selection sets, `get…` input and `c:` commands work. | [autolisp.md](autolisp.md) |
| Startup LISP (`acad.lsp`, `acaddoc.lsp`) | Loaded from the support folder at startup and for each drawing. | [autolisp.md](autolisp.md#at-startup) |
| Scripts (`.scr`) | SCRIPT (Tools > Run Script...), `cadcraft-cli run --script-file`, and script text over MCP and the control channel. | below |
| Command aliases (`.pgp`) | ALIASEDIT `{file}` loads an alias file and keeps it; the aliases and the file survive restarts. REINIT reads it again. | below |
| Compiled plug-ins (C++, .NET) | Not supported. A plug-in API is planned, for plug-ins written in Rust and compiled to WebAssembly. | [gaps.md](gaps.md) (E4) |
| Programs that drive the CAD program from outside | The JSON control channel, the MCP server and `cadcraft-cli`. Any language that can open a TCP socket or start a process can use them. | below |

## LISP routines

Most routines that only draw, edit and query objects run as they are. Not yet supported: Visual
LISP ActiveX functions (`vlax-…`, `vla-…`), DCL dialogs and reactors ([gaps.md](gaps.md) E1). A
routine that uses them needs those parts rewritten, for example with `command` calls and `get…`
prompts instead of a dialog.

Put routines you want in every session in the support folder (`support` in the settings folder;
paths in [autolisp.md](autolisp.md#at-startup)) and load them from `acad.lsp` or `acaddoc.lsp`, or
with `(autoload "file" '("CMD"))` so they load the first time their command is typed.

## Scripts

A `.scr` file is what you would type at the command line, one line at a time. Run it with SCRIPT
in the app, or headless:

```sh
cadcraft-cli run drawing.dxf --script-file setup.scr --save out.dxf
```

As in AutoCAD, a line end ends an input, and so does a space. Three things keep their spaces: a
command's free-text prompt (TEXT or MTEXT contents) takes the rest of the line, a
`command {json}` call takes the rest of its line, and a LISP expression keeps the spaces inside
its parentheses. A LISP routine's text prompt still splits at spaces. An empty input (an empty
line, two spaces in a row, a space at the end of a line) is Enter. A command that keeps prompting
after its last input, such as LINE or PLINE, needs that Enter once it's done, or the next line
answers its prompt:

```
LINE 0,0 @30,0

CIRCLE 15,15 6
```

Without the empty line, `CIRCLE` would be refused as a point and `15,15` and `6` would become more
segments of the line. Script text sent as a string (`--script`, the MCP `script` tool) needs the
empty line's own line end too: `"LINE 0,0 @30,0\n\n"` ends LINE, but a single trailing `\n` adds no
empty line, and empty text runs nothing.

If anything still waits for input at the end of a `cadcraft-cli run` (a command, or a LISP routine
in a `get…` function), the CLI notes it with its prompt. It checks once, after the last step, so
input an earlier `--script` left waiting that a later `--script` answers gets no note. The MCP
`script` tool's result shows what waits after each call in `state.prompt`.

## Command aliases

ALIASEDIT reads a `.pgp` alias file (`ALIAS, *COMMAND` lines); its aliases replace the ones you
had. Lines that would start external programs are skipped. From a script or the control channel:
`aliasedit {"file": "/path/to/aliases.pgp"}`. The Command Aliases dialog's **Import…** button picks
a `.pgp` file, loads it the same way, shows its path and warnings, and remembers the folder it came
from; on the web (no native file dialog) type `ALIASEDIT` with the file's contents in its `text`
parameter instead.

Aliases are user settings, never part of a drawing. The desktop and web apps keep them, and the
file REINIT reads, with your other preferences, so aliases added, edited, imported or removed in
the Command Aliases dialog (or with ALIASEDIT's `set`, `remove` and `clear`) are there after a
restart, and REINIT reads the same file again. The headless CLI keeps no preferences: there,
aliases last for that run.

## Driving CADCraft from another program

Every command has a JSON form that runs without dialogs. `cadcraft-cli commands [FILTER]` prints
the catalog: each command's id and its parameters.

- **Headless batch jobs:** `cadcraft-cli run FILE --cmd 'id {json}' --save OUT --export OUT.png`.
  A `--cmd` parameter value of exactly `{last}` is replaced with the last object handle an earlier
  `--cmd` in the same run returned, so `viewport.set {"handle":"{last}","scale":1}` can follow
  `mview`. Every other value is passed unchanged.
- **The running app:** start it with `--control PORT` and send JSON lines to `127.0.0.1:PORT`.
  See [control-protocol.md](control-protocol.md).

  ```sh
  printf '%s\n' '{"id":1,"method":"cmdline.input","params":{"text":"circle 0,0 5"}}' | nc -w 2 127.0.0.1 7979
  ```

- **AI agents:** `cadcraft-cli mcp`, headless or connected to the app. See [mcp.md](mcp.md).
