# The AI Assistant

> **Last reviewed:** 2026-10-29 · **Last updated:** 2026-10-29 · **Change:** minor (the engine refuses the file writes and closes a call wasn't allowed; CLOSE asks first; previously minor: commands given a file ask first; previously minor: every protected command of a script is asked; what the confirmations do and don't cover; the CLI's starting drawing; previously minor: what the agreement covers, confirmations reach command lines and scripts, how `ask` keeps its changes)

**Window ▸ AI Assistant** opens a palette where you ask for a drawing change in your own words
("draw a 60 mm flange with four holes on a 45 mm circle") and the assistant carries it out with
CADCraft's own commands, on the active drawing. `cadcraft-cli ask` does the same from a terminal,
on a drawing of its own (see [From a terminal](#from-a-terminal)). It is the in-app assistant of gap A2 in [gaps.md](gaps.md#ai-features).

## How it works

- **An agent over CADCraft's tools.** `crates/assistant` (L4) runs the loop: the model reads the
  request, calls tools, reads what they return, and calls again until it answers, within
  20 model calls per turn. The tools are the MCP server's (`crates/mcp`): `command_line` (AutoCAD
  command syntax), `script`, `execute` (a command with JSON parameters), `list_commands`,
  `inspect_drawing`, `query_entities`, `new_drawing`, `open`, `save` and `cancel`. Screenshots and
  pointer tools are left out: they don't fit a text conversation.
- **The UI thread runs every tool.** A tool call goes from the worker to the palette and runs
  exactly like a request from the control channel, so the assistant can do nothing a script
  can't. A turn is one undo step: one Undo takes back everything it drew.
- **The drawing's files are the user's.** `new_drawing`, `open` and `save` replace the drawing or
  write a file, so each asks first in the palette ("The assistant wants to save the drawing to
  …"). So does a command line, script or `execute` that names NEW, OPEN, QSAVE, SAVEAS, CLOSE or
  CLOSEALL (or an alias of one), and one that names any other command given a file or folder (EXPORT, EXPORTPDF,
  PLOT, PUBLISH, WBLOCK, TABLEEXPORT, DATAEXTRACTION, APPLOAD, SCRIPT…: "The assistant wants to run
  a command that writes or reads …"). Which commands those are comes from their parameter docs, so
  a new one asks until it is listed as only reading a file into the drawing (IMPORT, INSERT,
  HATCH's pattern file…); an `execute` given a `path` or `file` parameter asks too. Any word
  counts, so a word that is only text asks too, and a script that names several asks each one (a
  yes to a new drawing doesn't approve the save after it). A no stops the whole call and is
  reported to the model, which doesn't retry. `cadcraft-cli ask` asks on a terminal, and declines
  when stdin is not one unless `--yes` is given.
- **The engine holds the line the questions draw.** The questions come from reading the call
  before it runs, so a command it builds at run time (an encoded SCRIPT, a LISP string, a CLOSE
  assembled with `strcat`) asks nothing. So each call runs with only what its questions earned
  (`cadcraft_engine::consent`): writing files after a yes to `save` or to a command given a file,
  closing drawings after a yes to CLOSE, nothing otherwise. Where the engine writes, renames or
  deletes a file (the shared atomic writer behind SAVE, EXPORT, PLOT, PUBLISH and the layout
  saver; WBLOCK, TABLEEXPORT, DATAEXTRACTION, the plot-style and plotter managers, a LISP `open`
  for writing, DRAWINGRECOVERY's discard) and where it closes a drawing, it checks that consent
  and refuses the rest, wherever the call came from. The palette carries the consent to the UI
  thread with the call, and `cadcraft-cli ask` runs the call inside it. A LISP routine a call
  leaves running goes on in later frames with no consent at all, and stops when the turn ends.
  The user's own commands, the control channel and other MCP clients run without a consent, as
  before. What it doesn't cover:
  - The consent is the call's: a command started in one call and finished in the next (SAVEAS
    answered with its file name in a second `command_line`) is refused there. Giving a command and
    its file in one call works.
  - A yes covers the whole call: an approved EXPORT may write another file in the same call.
  - Reading isn't guarded: IMPORT, INSERT, COMPARE, an XREF or a LISP `open` for reading can read
    a file the model names, and what they return goes to the model (APPLOAD and SCRIPT ask first,
    since they run what they read).
  - SAVEFILEPATH is a setting the assistant can change: autosave then writes its own recovery
    files (never an existing file of the user's) in that folder.
- **Plain words in the chat.** While a turn runs, a status line says what the assistant is doing
  ("Running CIRCLE…", "Looking at the drawing…"); the chat keeps what was said, never the
  commands or JSON that ran.

## The model: a FANTASTICO account on the Craft AI layer

The assistant runs on any model a [FANTASTICO](https://fantastico.cc) account offers, paid from the
account's prepaid credit. The desktop app reaches it through the shared Craft AI layer
([storytold/craft-ai-integrations](https://github.com/storytold/craft-ai-integrations)) and the
`craft-ai-fantastico` adapter (`apps/cadcraft/src/assistant_host.rs`):

- **One connection for every Craft app.** The account is a connection in the layer's shared
  store, the one the launcher and the other Craft apps read: connected here, it works there, and
  the other way round.
- **The key stays in the system keychain** (`craft-ai-native`), never in a file of CADCraft's. The
  interface can store a key or replace it, after FANTASTICO has checked it; it can never read one
  back. An older CADCraft kept the key in `<settings dir>/assistant/fantastico.key`: it moves into
  the keychain the first time the assistant needs it, and the file is removed.
- **No request goes out before the user agrees.** Connecting the account (checking the key) and
  reading its models and credit talk to FANTASTICO; the prompt, the drawing and what the commands
  return do not leave the computer before the agreement. The layer refuses a request to a remote service the
  user hasn't agreed to; the palette asks once, naming the service and its address ("To answer,
  the AI Assistant sends your requests, what it reads from the drawing and what its commands
  return to FANTASTICO (…) and to the provider of the chosen model. Allow this for CADCraft?"),
  and a no sends nothing. The agreement is kept by the layer.
- **Each request is a job of the layer**, recorded in its history, with the account's runtime keys
  minted, capped and rotated by FANTASTICO. The palette shows the credit left
  (`total_remaining_usd`, which counts what open keys have set aside but not spent).
- **The model** is the one the account suggests, or one picked in the palette's settings (the
  account's chat models, with their price per million output tokens). The reply language follows
  the user's, or is fixed to English or Italian.

### From a terminal

`cadcraft-cli ask` reads the key from `$FANTASTICO_KEY` and calls the account directly with
`fantastico-sdk`, without the layer: it has no keychain or agreement to keep. It works on a
session of its own, not on the desktop's drawing. It starts from one of: `--open FILE` (that
file), `--sample` (the built-in sample), `--metric` (a new metric drawing), or, with none of the
three, a new imperial drawing; the three can't be combined. The changes are kept only by
`--save` (or `--export` for an image) or a save the assistant was allowed to make; otherwise they
end with the command.

```sh
FANTASTICO_KEY=… cadcraft-cli ask "add four 6 mm holes on a 45 mm circle at 50,30"   --open bracket.dxf --save bracket-holes.dxf
```

Saving onto the `--open` file needs `--in-place`, as in `run`.

## Not done yet

- Replies arrive whole, not streamed as they are written.
- No Stop button: a turn runs until it answers or uses its 20 steps.
- The web build has no assistant (no worker threads there).
- Other providers of the layer (a local model, another account) are not offered in the palette
  yet; the host only looks for a FANTASTICO connection.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-29 | minor | The engine refuses the file writes and closes the user didn't approve for the call (`cadcraft_engine::consent`), also when the call builds them at run time; CLOSE and CLOSEALL ask first; what the guard doesn't cover |
| 2026-10-29 | minor | Commands given a file (EXPORT, WBLOCK, PLOT, PUBLISH…) ask first, by any tool |
| 2026-10-29 | minor | Every protected command of a script is asked; the confirmations are not a sandbox, and the engine-level guard is listed as not done |
| 2026-10-28 | minor | What the agreement covers; confirmations reach command lines, scripts and `execute`; how `ask` keeps its changes, and `--in-place` |
| 2026-10-27 | major | First version: the palette and `cadcraft-cli ask`; the account on the shared Craft AI layer (keychain, agreement, jobs) |
