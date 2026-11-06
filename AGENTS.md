<!-- craft-house-rules:start core source=dde09ea31e2e457f5b4653a6e76f7cdf55944bdf sha256=540e6cea75d12d0710c3dca89658328e81e508ecef102446ca5bb95b7c0b92bb -->
## House rules

These apply in every Craft repository. A later rule may be stricter, not weaker. The source is `standards/house-rules-core.md` in `storytold/craft-contrib`. Propose changes there, not in a copy.

1. **A person is accountable.** Whoever runs an agent answers for what it opens and for its review comments.
2. **Check, then claim.** Before writing code, read the issue comments, search pull requests for `#N` and the keywords, and check recent commits on `main`. Comment on the issue first. Release a claim you will not finish.
3. **Verify, don't guess.** Open a definition before using it. Confirm the cause in the code, even when someone names a location.
4. **Keep fixes narrow and tested.** Fix what the issue describes, where the bug is. Add a test that fails without the fix.
5. **Review the diff before opening a pull request.** Identifiers exist as used, the test fails without the fix, nothing unrelated changed, and the description is true.
6. **Look for a better pull request.** If another fixes the same thing, close yours when it is better or equal, and say why.
7. **Report only what ran.** Say what you tested and what you could not run. Without a local build or test, say so and rely on CI before marking ready.
8. **`main` changes through a pull request.** Do not push to it. Branch protection enforces this. There is no exception.
9. **Stop and ask only about** licensing outside the allowlist, publishing, releases, a new remote, secrets, spending money, destructive git, and anything else hard to undo. Otherwise keep working. `plan/` is a gitignored notebook, not rules. Without it, do not ask for it and do not stop.
10. **Keep secrets and personal details out** of the repository, logs and requests.
11. **CI stays shared.** Workflows call `storytold/craft-common-actions`. Do not change CI in a feature or fix pull request. If CI cannot cover a change, open an issue there.
12. **Guards say why.** A check that looks removable says what it prevents. Read that before changing it.
13. **Instructions must be readable by everyone.** No notes about one machine, and no links to files a reader cannot open. `craftrules` is retired. Do not link to it.
14. **Keep pull requests moving.** Concurrent pull requests are fine; unanswered actionable feedback is not. Respond in 7 days, a reminder at 14, closure eligible at 30, from when an actionable request waits on the author. Suggestions and unconfirmed bot findings do not start the clock. A fix, explanation or agreed plan resets it; unrelated activity does not. Waiting on a reviewer or maintainer pauses it and does not count against the author. A maintainer decides closure; it is never automatic. Reuse search results, do not poll, and stop at a rate limit.
15. **No agent attribution.** A pull request does not say an agent wrote it. A repository rule may not require that.
<!-- craft-house-rules:end -->
<!-- craft-house-rules:start rust source=dde09ea31e2e457f5b4653a6e76f7cdf55944bdf sha256=387e0ccb4b49e567f60951d7d6fff7305054c89f43ad812b9993dc7c38528013 -->
## Rust Craft repositories

These apply to native Craft apps and to shared Rust libraries. Select them by repository type. A repository that merely contains some Rust does not take this block. Rules later in a repository's file may be stricter, but not weaker. The source is `standards/house-rules-rust.md` in [storytold/craft-contrib](https://github.com/storytold/craft-contrib).

1. **Never crash.** Non-test code does not panic and has no `unsafe`, except where the repository file names an exception. Return errors, treat input as hostile, and bound recursion and allocations. New production crate roots copy the `deny` attribute from an existing crate: `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`, `clippy::unimplemented`, `clippy::todo`, `clippy::unreachable`. Every crash fix comes with a test that panicked before the fix.
2. **Clean room.** Learn from observed behaviour and public specifications. Do not copy another product's code, assets, strings or artwork. Check a dependency's licence before adding it.
3. **Assets and fonts.** Every asset is original or suitably licensed, and is recorded in `ATTRIBUTION.md` in the same change. Font files live in [storytold/craft-fonts](https://github.com/storytold/craft-fonts), not in the app repository.
4. **Rust only for native Craft apps.** No handwritten JavaScript or TypeScript, and no Tauri, Electron or webview shell, in those apps. Shared tooling follows its documented technical scope and maintainer-approved exceptions. Craft PR Review's existing Tauri and TypeScript UI is one such exception. It does not relax this rule for native apps. Preserve any existing web build.
5. **Time goes through `chrono`.** Do not hand-roll calendar conversion. For dates beyond what `chrono` can represent, follow [standards/time.md](https://github.com/storytold/craft-contrib/blob/main/standards/time.md).
6. **Contributor credits.** Add or edit only your own person's entry in `contributors/people.toml`, and only after asking them. Never guess a name or hand-edit `contributors.json`.

## Licence allowlist

Dependencies and assets use SPDX identifiers. An expression is allowed when every identifier is in this list. A repository may name a shorter list. It may not add an identifier here by editing its own copy of these rules.

- `ISC`
- `MIT`
- `BSD-2-Clause`
- `BSD-3-Clause`
- `Apache-2.0`
- `OFL-1.1`
- `Ubuntu-font-1.0`
- `Bitstream-Vera`
- `CC0-1.0`
- `CC-BY-4.0`
- `CC-BY-SA-4.0`
- `LicenseRef-Public-Domain`

`LicenseRef-Storyteller-Trademark` is not in this list. It covers Storyteller brand marks only, where a repository already says so.
<!-- craft-house-rules:end -->

# CADCraft — agent instructions

Clean-room, open-source, pure-Rust CAD app targeting AutoCAD parity and superiority (speed, openness, agent control). Native on macOS/Windows/Linux/FreeBSD; web via WASM. Siblings with same conventions: `../photocraft`, `../vectorcraft`, `../filmcraft`, `../lightcraft`, `../pdfcraft`, `../effectcraft`, `../designcraft`.

## Session start
Read [`ROADMAP.md`](ROADMAP.md) and [`docs/gaps.md`](docs/gaps.md). `plan/` = optional gitignored notebook (status, execution notes, `plan/autocad/*`); don't ask for it, don't stop when absent. Licence choices stay inside house-rules allowlist.

## Never crash
Crash loses user drawings. **Outranks feature work.**
- No panics in non-test code: no `unwrap()` `expect()` `panic!` `unreachable!` `todo!` `unimplemented!`; no `unsafe` (`unsafe_code = "forbid"`).
- Errors = `Result<T, E>` via crate error type and `?`. Unfinished feature returns error / "not available yet", never panics.
- Input-derived numbers are hostile (DXF/DWG, command-line text, MCP/control params): `get()` not `[i]`, checked arithmetic, no NaN casts, cap input-sized allocations and loop counts.
- Bound recursion (nested/cyclic block refs: `MAX_BLOCK_DEPTH`).
- Last-resort guard: `Session::execute` and every interactive-command step (factory, `begin`, input, `prompt`, `preview` and passes after it; `crates/engine/src/guard.rs`) run under `catch_unwind`; escaped panic restores drawing and reports error. No-op on wasm (panic aborts).
- Every crash fix lands with a regression test (see `crates/engine/tests/panic_hunt.rs`, `hostile_dxf_does_not_panic` in `crates/io/src/tests.rs`).
- Every workspace crate manifest has `[lints] workspace = true`; every production crate root carries `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`.

## Non-negotiables
- **Clean-room.** AutoCAD on dev machine may be observed black-box only: run it, use UI with synthetic drawings, screenshots (by window id) stored only under `plan/autocad/screenshots/` (never committed). Never read/disassemble/copy anything inside the AutoCAD bundle (names/listings only); never copy Autodesk fonts (SHX), hatch patterns (`.pat`), linetypes (`.lin`), templates, CUI/PGP, icons, artwork, help text; never commit files produced by AutoCAD. Formats from public specs (Autodesk DXF Reference, ODA public DWG spec) and own synthetic tests. Never copy/link GPL/LGPL/AGPL code (LibreDWG, LibreCAD, QCAD, FreeCAD planegcs, SolveSpace, OpenCASCADE); don't read their sources.
- **Assets (most serious rule).** No Autodesk/Adobe/Avid iconography, images, fonts, patterns, artwork, ever. Icons drawn in code (`crates/ui-egui/src/icons.rs`); hatch patterns and linetypes own definitions (`crates/doc/src/library.rs`); drafting font own (`crates/fonts/src/stroke.rs`). File assets must be original, public domain/CC0, OSI-licensed, or redistributable Creative Commons (or open-source licensed by the contributor who made it), and **need a row in `ATTRIBUTION.md`** (author, source, licence; `cargo xtask assets` enforces). Only exception: ArtCraft brand in `docs/brand/` (trademarks, `docs/brand/LICENSE-brand.txt`). Never commit screenshots of Autodesk software.
- **Fonts** live in [`storytold/craft-fonts`](https://github.com/storytold/craft-fonts), never here. Optional build input (`CRAFT_FONTS_DIR`, embedded by `crates/fonts/build.rs`), never a `Cargo.toml` dependency. Code and tests must work without it.
- **Shared test corpora** in separate repos (`storytold/<app>-corpus`); no large binary fixtures here.
- **Everything is a command.** User-visible behavior = command in `crates/engine/src/cmd/*` (`CommandSpec`: id = AutoCAD command name lowercase, label, menu path, shortcut, aliases, params doc, `enabled`, JSON `run`, optional `interactive` prompt machine) + tests. UI-only commands: `crates/ui-egui/src/menus.rs` (`UI_COMMANDS`). Command line, menus, toolbar, Tool Sets, scripts, CLI, control channel, MCP all reach the same commands.
- **Programmatic calls never open dialogs.** `engine.execute` runs JSON form with defaults; only menu/toolbar invocation starts the interactive prompt sequence.
- **Layering** (`cargo xtask layers`): L0 `geom`,`dxf` → L1 `color`,`doc` → L2 `fonts`,`render`,`constraints` → L3 `io` → L4 `engine` → L5 `ui-egui`,`mcp` → apps. Nothing below L5 depends on egui/eframe/winit/wgpu/rfd. UI crate is swappable.
- **UI is thin:** panels read engine state, act via `app.run(id, params)` / `app.start(id)` / `app.cmdline(text)`. Colors from `theme::Tokens`.
- **Rust only** (no handwritten JS/TS). **Never break wasm** (`cargo xtask wasm`).
- **Quality gate** before every commit: `cargo xtask ci` (fmt, clippy -D warnings, tests, assets, layers, wasm). Commit after each building feature arc.

## Running and inspecting
- `cargo run --release -p cadcraft -- --sample --control PORT` (sample drawing + control channel). Pick a free port; other crafting apps use control ports too.
- JSON lines on `127.0.0.1:PORT` (each request carries the session `token`; see `docs/control-protocol.md`):
  - `{"id":1,"method":"cmdline.input","params":{"text":"circle 0,0 5"}}`: types at command line like a user (prompts, keywords, `@dx,dy`, `@d<a`, direct distance entry).
  - `{"id":2,"method":"engine.execute","params":{"command":"line","params":{"points":[[0,0],[10,0]]}}}`: JSON form.
  - `{"id":3,"method":"ui.screenshot","params":{"path":"/tmp/shot.png"}}` then read the PNG.
  - Methods: `crates/ui-egui/src/control.rs`; protocol: `docs/control-protocol.md`.
- UI work: look at result (screenshot, read PNG), compare with `plan/autocad/screenshots/`. No frame presented (screen locked) → `ui.render` or `cadcraft-cli`.
- Headless: `cadcraft-cli run --sample --script 'LINE 0,0 10,10\n' --export out.png`; `cadcraft-cli info file.dxf`; `cadcraft-cli mcp`.
- Shell: `mv`/`cp` may be aliased interactive; use `/bin/mv -f` / `/bin/cp -f`.
- Parallel agents: separate `CARGO_TARGET_DIR` each; edit only owned crates; delete your target dir when done.

## Roadmap
`ROADMAP.md` (committed) = one-page summary (stage, parity numbers, estimates): assessment `docs/target-app-parity.md`; work list `docs/gaps.md` (pick from top rows); milestones `docs/roadmap.md`; checklists `docs/geometry-parity.md`, `docs/ui-parity.md`, `docs/file-format-parity.md`, `docs/hardware-parity.md`, `docs/localization-parity.md`. PRs don't edit these docs: name affected rows in PR description; maintainers batch-update rows, status lines, revision history (avoids PR conflicts). `cargo xtask parity` regenerates menu-breadth checklist in `docs/parity-checklist.md`.

## Contributor credits (About window)
- About ▸ Contributors/Models compiled from `contributors/contributors.json` (commit stats; never hand-edit) and `contributors/people.toml` (names people chose). See `docs/contributors.md`.
- Agent preparing a PR for a contributor: check whether their GitHub username has `[people.<username>]` in `contributors/people.toml`. If not, ask once whether to credit more than username (real name, display name, public GitHub profile name via `sync_github_name = true`). Yes → add only their own entry (copy template at top of file), include in their PR, committed as them. No → change nothing (credited as `@username`).
- Never add/edit/guess/copy anyone else's entry or name (not from git config, commit authors, GitHub profiles). Never hand-edit `contributors.json`.

## CI workflows
- `.github/workflows/` call shared workflows in [`storytold/craft-common-actions`](https://github.com/storytold/craft-common-actions) at `@main`, with their inputs and secrets. No other jobs: if no shared workflow does it, it doesn't go here.
- No CI changes in a feature/fix PR. If existing workflows can't build/test a change, link an existing craft-common-actions issue/PR, open an issue, or a PR only for a small fix safe for every app that doesn't increase CI time.
- Some workflow files still run their own jobs. Don't add to them. Moving a job into craft-common-actions is its own task, never part of another PR; the job stays here until a shared workflow runs it.
