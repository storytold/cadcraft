# Plug-ins

CADCraft runs **command plug-ins**: WebAssembly modules that act on the open drawing through the
same commands as the menus, the command line, scripts, the CLI, the control channel and MCP. The
sandbox, the manifest (`craft-plugin.json`), the `.craftplugin` bundle, the per-user store,
catalogs and GitHub installs come from the shared `craft-plugin` crate in
[craft-libs](https://github.com/storytold/craft-libs/tree/main/crates/plugin) (RFC-0046); its
README describes the ABI and how to write and publish a plug-in. CADCraft's side is
`crates/engine/src/plugin.rs` and the `plugin.*` commands in `crates/engine/src/cmd/plugins.rs`.

A plug-in calls commands in their JSON form (`{"points": [[0,0],[10,0]]}` for `line`), as
`engine.execute` does: never the interactive prompts and never a dialog. Command ids are
CADCraft's, AutoCAD's command names in lower case; `cadcraft-cli commands` lists them with their
parameters. The selection a plug-in receives is the selected objects' handles (`["2A", "2B"]`).

## Using plug-ins

- **Tools ▸ Plug-ins** lists each installed plug-in's commands, then **Manage Plug-ins...**. A
  command with parameters asks for them in a dialog generated from its manifest.
- **Manage Plug-ins...** lists the installed plug-ins with their commands, and installs from a
  `.craftplugin` file (desktop), from a GitHub repository (`owner/repo` or `owner/repo@tag`: its
  latest release's `.craftplugin`, checked against the release's `SHA256SUMS`) or from a catalog
  (`index.json` on GitHub's hosts, or a `file://` one). **Check for Updates** looks at each
  plug-in's catalog or repository.
- Every install shows a confirmation first: the plug-in's access, the commands it may call, its
  kind, where it comes from and its SHA-256, and says plainly when a repository is in no catalog
  you use. Nothing is written before **Install**.
- The desktop app keeps plug-ins in `Plug-ins/` in its settings directory. The CLI, MCP, the web
  build and runs without a settings directory (an empty `CADCRAFT_CONFIG_DIR`) keep them for the
  session only.
- A plug-in only installs when its manifest names `cadcraft` in `apps`.

## Commands

| Command | Parameters |
|---|---|
| `plugin.list` | `{}` |
| `plugin.install` | `{path}`, `{data: base64, name?}`, `{repo: owner/repo[@tag]}` or `{catalog, id, version?}`; `review: true` checks and describes the bundle without installing; `sha256` installs only that bundle |
| `plugin.update` | `{id?, apply?}`: lists newer releases; `apply` installs those asking for no other permissions |
| `plugin.remove` | `{id}`: the plug-in and its settings |
| `plugin.run` | `{id, command?, params?, fuel?}` (needs an open drawing) |
| `plugin.catalog.list` | `{url?}`: the packages CADCraft can run |
| `plugin.catalog.add` | `{url, remove?}` |

## What a run may do

- A run is **one undo step** named after the plug-in (U takes the whole run back). A run that
  fails (a trap, an exhausted instruction budget, a non-zero status, a refusal it passes on)
  puts the drawing, its selection, views and undo history, and the drafting settings back as they
  were. A run that changes nothing records no step. Plug-ins don't run inside a LISP routine.
- `permissions.access: "read"` plug-ins may call any allowed command, but a call that changes
  the drawing (or records an undo step, or changes the drafting settings such as ORTHO or OSNAP)
  is undone and refused. Selecting objects and changing the view are allowed.
  `permissions.commands` narrows the commands further.
- Settings: each plug-in keeps up to 64 KB of its own settings (keys up to 128 characters), saved
  with the app's preferences.
- **Refused commands**, whatever the manifest asks: every `plugin.*` (and the other names
  `craft-plugin` refuses in every app); the undo history (`u`, `undo`, `redo`, `mredo`, `oops`);
  other drawings and files (`new`, `open`, `close`, `closeall`, `qsave`, `saveas`, `recover`,
  `drawingrecovery`, `document.*`, `import`, `export`, `exportpdf`, `plot`, `preview`,
  `plotwindow`, `publish`, `pdfbook`, `plottermanager`, `-plottermanager`, `stylesmanager`,
  `layout.saveas`, `wblock`, `-wblock`, `datalink`, `datalinkupdate`, `dataextraction`,
  `eattext`, `tableexport`, `compare*`); references to other files (`attach`, `imageattach`,
  `xattach`, `xref`, `-xref`, `xopen`, `xbind`, `refedit`, `refclose`, `refset`); the Block
  Editor (`bedit`, `bclose`, `bsave`, `bsaveas`); code of another kind (`lisp`, `appload`,
  `script`); the user profile and the session's libraries (`options`, `aliasedit`, `reinit`,
  `hatch.patterns`); the clipboard (`copyclip`, `cutclip`, `copybase`, `cutbase`, `pasteclip`,
  `pasteorig`, `pasteblock`); and the clock (`time`).
- Parameters that name files (`path`, `paths`, `file`, `files`, `dir`, `folder`, `template`,
  `patternFile`, at any depth) are refused; the `path` of `array`/`arraypath` names an object and
  is allowed. A plug-in gets no disk, network, clock or other drawing.

Not yet: plug-in panels (`ui_show` runs report that CADCraft doesn't show them), offering files
to save (`saveFile`), refreshing the macOS menu bar's Tools ▸ Plug-ins when plug-ins are installed
or removed while the app runs (the Plug-ins window always lists them), and downloads in the
background (a repository or catalog install waits for the download).
