# Customizing and automating CADCraft

What you can use today to automate CADCraft or bring over add-ons written for other CAD programs,
and what is not there yet.

| You have | In CADCraft | More |
|---|---|---|
| LISP routines (`.lsp`) | Run with APPLOAD or `(load "name")`. The core language, `command`, entity lists, selection sets, `get…` input and `c:` commands work. | [autolisp.md](autolisp.md) |
| Startup LISP (`acad.lsp`, `acaddoc.lsp`) | Loaded from the support folder at startup and for each drawing. | [autolisp.md](autolisp.md#at-startup) |
| Scripts (`.scr`) | SCRIPT (Tools > Run Script...), `cadcraft-cli run --script-file`, and script text over MCP and the control channel. | below |
| Command aliases (`.pgp`) | ALIASEDIT `{file}` loads an alias file for the session; REINIT reads it again. | below |
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

## Command aliases

ALIASEDIT reads a `.pgp` alias file (`ALIAS, *COMMAND` lines) and adds its aliases for the rest of
the session. Lines that would start external programs are skipped. From a script or the control
channel: `aliasedit {"file": "/path/to/aliases.pgp"}`.

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
