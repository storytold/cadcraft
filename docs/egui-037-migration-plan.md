# CADCraft — egui 0.37 migration plan

Current reconciliation (2026-10-25): the local tree now incorporates main `e86d895ba1086a7efc2a83048348f89098282d08`. Managed run `egui037-cad-maine86-ci-20261025-0825` passed all seven CI stages: 2,515 passing test executions, zero failures and one ignored across 103 summaries. Independent review verified preservation of both parents and dependency pins; the compare-export regression now verifies document-epoch advancement. Earlier validation counts below describe prior candidates. Native/browser GUI, platform and paired performance checks remain open. Publication requires explicit approval; human review and human merge are required.

**Status (2026-10-25):** reconciled main `b90165666917ec3ac6c308c2ea1c55bb6ea8317d` and passed all 7 mandatory CI stages. Managed job `egui037-cadcraft-main-refresh-ci-20261025-0615` recorded 2,451 passing test executions, zero failures and 1 ignored. The shared-docking adoption uses the reviewed craft-ui revision `650b1c0b058017c4eb3af06d5b39586d6732e07b`; the lock changed only that package and its dependency list. This revision is reachable from the published craft-libs migration branch. Native/platform GUI and IME workflows, final packages and paired app performance remain open. Earlier source identities and counts below are historical.

Current landing status, 2026-10-25: the implementation integrates main `fcfdf33001cf2462bdafb6e1237becd8ef0ea1cb` and selects egui `6d518fc799681571963842f95b55a6b68431b15a`, winit `a517b3846ee8457d84ee4975110bfda6797c44dc`, shared UI `25da1314d443171b4adeaad2cd9454b273f35c6f` and independent PDF/document-path revisions in the lockfile. Full managed CI0300 passed all seven stages, with 2,448 passing tests, no failures and one ignored across 103 summaries. Fresh review0320 verified the tested source, all seven latest-main paths and all 13 new source files.

The review also identified inherited upstream issues in MEASUREINIT reset/web startup and PDF font hardening; these are carried explicitly in the review packet. The egui licence-source URL has been updated to the selected revision; the font bytes are unchanged. Final notice validation is pending. Current-source Windows/macOS/FreeBSD, visible GUI, native IME and performance acceptance are not established by the Linux CI result. Human review and human merge are required.

The original planning and research record follows; its baseline pins and planning-only status are historical.

Status: **planning only**. This document changes no production code, manifest, lockfile,
branch, tag, PR or release. No build, test or GUI run was performed while writing it.
Every future push or new PR requires fresh, explicit human approval after the exact
contents and validation results are shown; nothing in this document grants that approval.
Every future host build/test and owned-VM command goes through
`maintenance/task.py run/status/finish` with its own task, source and evidence.

Scope: the `storytold/cadcraft` checkout at
`/home/codex/work/CraftApps/egui037-migration-20261022/cadcraft` only. The shared
`storytold/egui` fork, `storytold/winit`, `storytold/craft-libs` and the sibling apps
are dependencies and hand-off points, not repositories changed by this plan.

Claims are written to be checked against the named source. Historical compiler output
and measurements are clues, never current acceptance.

Capture acceptance follows the suite contract: a queued-not-issued hidden request and
each settling frame must request `RequestPaintWhileHidden` from logic; only an issued
screenshot command forces paint itself. Callback arrival has no implicit wake, so the
live request owns the viewport waker. Record known issuance UI bounds and a geometry
epoch, reject/retry any intervening resize/DPI/zoom/surface change even if it changes
back, and derive pixel scale from returned dimensions only under an unchanged epoch.
Default limits are 4 requests, 128 MiB/image and 256 MiB aggregate; CADCraft's explicit
64 MiB limits remain stricter. Count reserved in-flight and completed images together.


## 1. Decision in one page

CADCraft's committed dependency state does not currently build as written:
`Cargo.toml:52-57` asks for egui/eframe/egui-wgpu/egui_kittest 0.37 while `craft-ui`
is pinned to `5c874d51…`, whose egui dependency is 0.36.2. The lock therefore resolves
two incompatible egui families, and `DockArea::show` receives a 0.37 `Ui` from a
0.36-typed API. This is a reconciliation task, not a version bump.

Target state:

1. One immutable egui family from the **final reviewed `storytold/egui` revision**.
   That revision is not known or pinned by this plan; it exists only after the approved
   publication of the fork's reviewed patch series. The aiongg candidate
   `a4f9a17f64a6bae61350fb85804c9df0d9865916` and upstream `841a328b` are comparison
   inputs, never production pins.
2. One immutable winit from the **final reviewed `storytold/winit` revision** containing
   the W1-W15 base, W17-W27 language/IME series and the new **W15P** PRIMARY path. The
   aiongg candidate `8b35388bd719e8cc86a95971ba0bc026c9a8f058` contains W17-W27 but no
   PRIMARY and is only a comparison input.
3. `craft-ui` and `craft-docpath` from one **final reviewed `storytold/craft-libs`
   revision** that is current `main` (`49190884…`) plus only the egui-API hunks ported
   from PR #199's `8614e4a…`. The PR head `b68de8c…` and audit tree `ec8840a…` are
   stale unsafe trees and are never merged or checked out as the target.
4. CADCraft source changes limited to: the real compile-error classes
   (`UserData`/`Event::Screenshot`, `MouseWheel.source`, private `FontsView.fonts`), the
   deprecations `clippy -D warnings` rejects, panel-id/`PanelState` migration, the strict
   kittest names/glyphs needed for a green first compatibility PR, and the deliberate
   lifecycle/screenshot/PRIMARY/IME changes below. There is no `update`→`ui` rewrite:
   `eframe::App::ui` and a root `Ui` are already used (`apps/cadcraft/src/main.rs:91`,
   `crates/ui-egui/src/lib.rs:627`).
5. A lifecycle split that is correct under the **actual pinned integration**: both native
   and web call `App::logic` and `App::ui` inside every `Context::run_ui` pass. A
   completed `run_ui` leaves `current_pass_index() == 0`; a logic-only tick also reports
   zero. Visibility is not a phase. Frame-once work uses the zero-pass-index guard;
   input-dependent work moves into `App::ui`; consume-based queues run on every logic
   callback. This is a correction of an existing 0.36/0.37 hazard, not a new 0.37
   lifecycle.
6. Screenshot completion uses the shared craft-ui CL-2 design: one bounded, cancelable
   completion slot per admitted request, a callback holding only a **weak** reference to
   that slot, capture-time viewport/geometry/DPI binding, and aggregate byte bounds. The
   callback never holds a strong `Context` (which would create a Context→command→callback
   cycle) and never writes files or encodes PNGs.
7. One atomic compatibility PR (S2), then behaviour PRs as in §12. S2 must already be
   green under strict kittest accessibility and missing-glyph policy, so it owns the
   accessible-name and `default_fonts`/`monochrome_emoji_fonts` work; S4 then owns the
   broader language, AccessKit-language and Arabic checks.

## 2. Baselines, exact revisions and evidence

### 2.1 Checkouts and revisions

| Role | Revision | Notes |
|---|---|---|
| Assigned CADCraft checkout | `1977a493c7d8340249185126744c5143f096dc86` | `HEAD`; branch `codex/egui-037-plan-20261022`; tracked tree clean; this plan is currently untracked |
| Last coherent 0.36.2 CADCraft | `a08fd7c1d1d7c16f40dc31c25f815370c136a1d4` | Parent of the Dependabot bump that moved direct UI deps to 0.37 |
| craft-libs `main` inspected | `49190884278f16c9a9f5823d7698a8ffb5deb062` | Baseline for the S1 rebase |
| craft-libs PR #199 API commit | `8614e4a04fae516fba16c22b31ff1008cb6be10b` | Source of the port hunks only |
| craft-libs PR #199 head | `b68de8c5f4f1c060904d211a7b1320aa0f7bce78` | Stale tree; never a target |
| craft-libs audit merge | `ec8840af8c5fda695cf195cdd05e5ae078415c07` | Stale audit tree; evidence only |
| Upstream egui 0.37.0 | `841a328ba2dd8f6996beb7189ebb7fc00e668a2a` | Stock control and API source |
| aiongg egui candidate | `a4f9a17f64a6bae61350fb85804c9df0d9865916` | Comparison input; not a production pin |
| winit published base | `ffae5198770d7730e718043f035a772b7dbc3b1b` | `craft/0.30.13`, W1-W15 |
| aiongg winit candidate | `8b35388bd719e8cc86a95971ba0bc026c9a8f058` | W17-W27; no W15P; comparison input |
| Published egui 0.36.2 | `767b1cad294120689a514bb4bd68c077e46679b9` | Rollback patch inventory |

**Final production pins are unresolved until approved publication.** The manifest edits
below use `<storytold/egui final reviewed rev>`, `<storytold/winit final reviewed rev>`
and `<craft-libs final reviewed rev>`. A candidate SHA may not be copied into a
production manifest, and a moving branch is never a pin.

Governing instructions: repository `AGENTS.md`, suite `PLANNING-CONTRACT.md` and the
server instructions. `.agents/` and `.codex/` are empty; there is no nested `AGENTS.md`.
`AGENTS.md` (68 lines) and `CLAUDE.md` (49 lines) are **not** the same text: `CLAUDE.md`
lacks the chrono rule and the contributor-credit section. `AGENTS.md` governs.

### 2.2 Dependency facts in the committed lockfile

`Cargo.lock` currently contains:

* `egui` 0.36.2 **and** 0.37.0; likewise `emath`, `ecolor`, `epaint`.
* `accesskit` 0.24.1 and 0.25.1 (one per egui family).
* `craft-ui` at `git+…?rev=5c874d51…#5c874d51…`, depending on `egui 0.36.2`.
* `craft-docpath` at `git+…?rev=82a24b0…#82a24b0…` (different craft-libs revision,
  no egui dependency).
* `eframe`, `egui-wgpu`, `egui-winit`, `egui_kittest` at 0.37.0, and `winit` 0.30.13
  from crates.io.
* `arboard` 3.7.0 is present in the lockfile, but `cargo tree -i arboard --locked`
  on the native graph prints nothing: the 0.37 `eframe` feature list
  (`apps/cadcraft/Cargo.toml:19`) does not enable `eframe/clipboard`, which forwards
  to `egui-winit/clipboard` (0.36's eframe enabled it unconditionally). **Without
  adding `clipboard`, copy/paste silently stops reaching the OS clipboard**;
  `crates/ui-egui/src/clipboard.rs:29-60` depends on `ctx.copy_text` and
  `Event::Paste`. This is a verified graph fact, not a warning from the old audit.
* No root `[patch.crates-io]`, no `vendor/` and no **external/local path patch** in
  this repository. Internal workspace members are path dependencies by design.
* Read-only `cargo tree -d --locked --offline` shows the 0.36 family reachable only
  through `craft-ui`, and the 0.37 family through CADCraft/eframe/egui-wgpu/kittest.
  This is graph evidence, not a build.
* The candidate fork's own `a4f9a17f/Cargo.toml:178-179` still has
  `[patch.crates-io] winit = { …, branch = "craft/0.30.13-lang" }`. Consumers' patches
  override it, but the fork's own build is not immutable. The final storytold fork/tag
  must replace that branch with the reviewed rev before publication.

### 2.3 Historical compiler clues (not acceptance)

`/home/codex/work/ssd2/task-evidence/egui037-audit-20261022/fork/cadcraft.log` was
produced by a path-patched trial, not by the committed manifests. It reports:

* 3 error classes (16 diagnostics in the `stock` run): `lib.rs:696` (`egui::UserData`),
  `lib.rs:716` (`Event::Screenshot`), `animation.rs:78` (`MouseWheel` pattern lacks
  `source`). The `stock` run additionally reports `Event::MouseWheel` initializers at
  `animation.rs:145,197`, `canvas.rs:2806`, and private `FontsView.fonts` at
  `i18n/tests.rs:407,423,442,557,607,803,912,1017,1125,1168`.
* Warnings include 92 `egui::Id::new` calls (90 in `crates/ui-egui/src`, 2 in tests:
  `tests/docking_workflows.rs:303`, `tests/options_dialog.rs:237`), 51 `ui.id()` calls,
  2 `WidgetType`, 1 deprecated `AsId`, and private-font/dependency warnings.
* The trial patched 11 crates.io crates and both craft-libs crates by path; those path
  patches are evidence of the API delta and must never be committed.

These are clues to re-check on the migration tip, not a fixed work list.

## 3. Current implementation inventory

### 3.1 Manifests and feature targets

| File | Facts that matter here |
|---|---|
| `Cargo.toml:6-11` | workspace version 0.12.0, edition 2024, `rust-version = "1.90"` |
| `Cargo.toml:52-59` | `craft-ui` git rev `5c874d51…`; `egui`, `egui_kittest`, `eframe`, `egui-wgpu` set to `0.37`; unused `arboard` workspace entry |
| `crates/ui-egui/Cargo.toml:7` | UI crate already declares `rust-version = "1.95"` |
| `crates/ui-egui/Cargo.toml:20-32` | `egui`, `egui-wgpu`; dev `egui_kittest` with `wgpu,snapshot`; dev `naga 30` |
| `apps/cadcraft/Cargo.toml:19` | `eframe` features: `default_fonts,wgpu,accesskit,wayland,x11,links,persistence` — **missing `clipboard`** |
| `apps/cadcraft/Cargo.toml:29` | direct `winit 0.30` (x11) for the XWayland event-loop choice |
| `apps/cadcraft-web/Cargo.toml:29` | `eframe` web features: `persistence,default_fonts,wgpu,accesskit` |
| `apps/cadcraft-web/Cargo.toml:26-32` | `web-sys` for file input; `upload.rs` uses `DroppedFile::web_file` |
| `crates/render/Cargo.toml` | rayon only off wasm; not UI |
| `xtask/Cargo.toml` | no egui dependency |

Platform cfgs that must keep compiling:

| Cfgs | Where | What they gate |
|---|---|---|
| `cfg(target_arch = "wasm32")` | `ui-egui/lib.rs` (`now_ms`, `FrameCap`, autosave, dropped files), `ui-egui/theme.rs` (system-font probes), `render`, `dwg`, `apps/cadcraft-web` | no threads/system fonts/files on web; `now_ms` is 0 on wasm; `web_file` is wasm-only |
| `cfg(not(target_arch = "wasm32"))` | `apps/cadcraft` `rfd`, `sys-locale`; `ui-egui` autosave and frame cap | native-only dialogs/locale/autosave |
| `cfg(unix)` / `cfg(all(unix, not(macos)))` | `apps/cadcraft` `winit` x11, `zbus`; `desktop_theme.rs` | XWayland choice, portal theme |
| `cfg(target_os = "macos")` | `muda`, `objc2-app-kit`, `native_menu.rs`, `system_animations.rs`, integrated titlebar | native menu, reduce-motion, traffic-light layout |
| `cfg(target_os = "windows")` | `hidapi` windows-native, `windows` `UI_ViewManagement`, `graphics.rs`, `windows_subsystem` | DX12/FXC startup, animation settings |
| `cfg(target_os = "linux")` | `hidapi` linux-native-basic-udev | spacemouse HID |

`crates/fonts/build.rs` (`CRAFT_FONTS_DIR`) and `crates/ui-egui/build.rs`
(`contributors/*`) are build inputs unrelated to egui; do not change their contracts.

Packages/binaries: `cadcraft` (desktop, eframe), `cadcraft-web` (wasm, eframe web
runner), `cadcraft-cli` (headless, no egui), `cadcraft-ui-egui` (library, kittest),
`xtask` (layers/wasm/parity). There are no examples besides `examples/apartment.dxf`
(48,769 bytes, tracked), no benches, no committed screenshot fixtures.

### 3.2 Entry points

* Native: `apps/cadcraft/src/main.rs:228` `main`; `NativeOptions` at `:262-283`;
  Windows DX12/FXC at `:285` via `graphics::configure`; wgpu buffer-limit workaround
  at `:289-297`; XWayland event loop at `:301-306`; `run_native` at `:307`;
  `set_wgpu(rs, depth_format_from_bits(0, 8))` at `:347`.
* `eframe::App` at `apps/cadcraft/src/main.rs:67-110`: `logic`, `raw_input_hook`,
  `ui`, `on_exit`, `save`, `persist_egui_memory = false`.
* Web: `apps/cadcraft-web/src/main.rs:31-58` `Shell` (same `logic`/`ui` split);
  `WebRunner::start` at `:114`; `?cpu`/`?webgl` at `:108-113`;
  `CadApp::set_wgpu(rs, None)` for web (no stencil).
* Headless: `cadcraft-cli` and `cadcraft-mcp` never construct egui; `ui.render`
  is the CPU raster path in `crates/ui-egui/src/control.rs:470-545`.
* Control channel: `apps/cadcraft/src/control_server.rs`; methods in
  `crates/ui-egui/src/control.rs:195-215` and `docs/control-protocol.md`, enforced
  by `tests/control_methods.rs`.

### 3.3 Actual frame lifecycle and the pinned integration

Verified against native `crates/eframe/src/native/epi_integration.rs:338-365` and
`native/run.rs:235-281`, web `crates/eframe/src/web/app_runner.rs:280-325`, and egui
`context.rs:1099-1158,1190-1218,2124-2129`:

* **Both native and web call `App::logic` and `App::ui` inside the `run_ui` closure,
  once per pass.** Native `update` calls `app.logic(ui.ctx(), frame)` then `app.ui(ui,
  frame)`; web does the same. On a second discard pass the same two calls happen again
  with empty input. This is already true on 0.36.2 (`767b1cad` has the same order); it
  is not a 0.37 introduction.
* A hidden root window runs `ctx.run_logic` → `App::logic` only. Native reaches this
  through `PassMode::LogicOnly` when a redraw is overdue (`native/run.rs:267-281`);
  web through `run_logic` when the tab viewport is not visible (`app_runner.rs:318-328`).
  No egui pass runs, so `current_pass_index()` stays 0 and input/layout remain from the
  last pass.
* `current_pass_index()` is `0` during pass 1 and during a logic-only call, `1+` during
  later passes. After a completed `run_ui`, `end_pass` takes the viewport output
  (`mem::take`), so it is **0** again; do not expect a nonzero index after completion.
* `ViewportInfo::visible()` is derived from `minimized`/`occluded`
  (`egui/src/data/input/viewport_info.rs:98-104`); it is `None` when either flag is
  unknown and `Some(true)` only when both say visible. A compositor can withhold a
  frame without setting either flag, so a logic-only overdue tick can still see
  `None` or `Some(true)`, and a hidden window can still run a full `run_ui`
  when a screenshot or `RequestPaintWhileHidden` asks for pixels
  (`native/wgpu_integration.rs:695-727`). **Visibility cannot distinguish a logic-only
  tick from a forced hidden UI pass.**
* `App::raw_input_hook` is called by `prepare_raw_input` for both `update` and
  `update_logic_only` (`epi_integration.rs:338-344,385-425`) and before the visibility
  branch on web (`app_runner.rs:284`). It must not assume that a UI pass follows.
* `Context::run_ui` re-runs its closure for each pass requested by
  `Context::request_discard` (default `max_passes = 2`). `Context::root_ui` panics while
  the root `Ui` is borrowed (#8535); CADCraft keeps the handed-in `&mut Ui`.
* Screenshots are callback based: `ViewportCommand::Screenshot(ScreenshotCallback)` /
  `Context::request_screenshot` (#8489); the callback receives only
  `Arc<ColorImage>`, runs at most once, and does **not** request a repaint.
* #8748 is a **closed, unmerged** PR (“run the ui of an overdue window without
  painting”) and is not in the pinned 0.37 source. The pinned source has
  `PassMode::LogicOnly`, not `PassMode::Overdue`; its body confirms that such a window
  “only runs `App::logic`”. Do not use #8748 to justify current behavior.

### 3.4 UI state, commands, undo and persistence

* UI state is `UiState` (`lib.rs:70-112`) and is persisted by the desktop
  `apps/cadcraft/src/workspace.rs` as JSON (`docking`, panel visibility, theme, language,
  recent files), version 1, bounded at 64 KiB.
* `persist_egui_memory` is false (`apps/cadcraft/src/main.rs:107`), so eframe does not
  save egui `Memory`. The app's own `Workspace` JSON does not store egui ids or
  `PanelState`.
* All user-visible edits go through `CadApp::run` / `start` / `cmdline`
  (`lib.rs:315-359`) or menus; engine undo is the single transaction owner.
* Docking is `craft_ui::docking::Layout<Panel>` (`docking.rs:1-130`), serialized
  without egui ids; the surface is created with `egui::Id::new("cadcraft-docking")`
  and `cancel_drag` with the same id (`docking.rs:339,375`).
* Top-level panels are `exact_size(...)` and non-resizable: `chrome.rs:69,180,300`,
  `menus.rs:573`, `palettes.rs:330,346`.

### 3.5 GPU, control and screenshots

* `crates/ui-egui/src/gpu.rs:681` `install` registers error/device-lost handlers and
  inserts `Resources` into `egui_wgpu::Renderer::callback_resources`.
* `impl egui_wgpu::CallbackTrait for CanvasCallback` (`gpu.rs:693-780`) uses
  `ScreenDescriptor`/`PaintCallbackInfo` fields unchanged in 0.37.
* `main.rs:188` asks for an 8-bit stencil buffer; clipped paper-space viewports are
  drawn through `Stencil::Mask`/`Stencil::Inside`.
* `ui.screenshot` currently queues `(token, deadline, frames)`, then requests a
  viewport screenshot and matches `Event::Screenshot` by token (`lib.rs:688-740`);
  replies are correlated by `shot_token` only. `save_screenshot`
  (`control.rs:547-559`) writes RGBA PNG.
* `ui.render`, `ui.inspect` and `ui.elements` are CPU/state paths and do not depend
  on a presented frame. `ui.inspect` currently exposes `frameMs`, `buildMs`, `drawMs`,
  `meshMs`, `prims`, `building`, `renderer` (`control.rs:270`) but **not**
  `cumulative_frame_nr`, pass index or `cpu_usage`.
* There is no `view.set` control method. View changes use `cmdline.input` (ZOOM/PAN),
  `engine.execute`, `ui.pointer`, `ui.click`/`ui.move`/`ui.drag`, or `ui.set` for
  `UiState` fields.

### 3.6 Text, fonts, language and accessibility

* `theme::install_fonts` (`theme.rs:314-387`) starts from
  `egui::FontDefinitions::default()`, prepends an installed system UI face, appends
  CJK/Japanese/Arabic/Hangul faces and embedded craft-fonts faces. Tests inspect the
  installed character maps through `ctx.fonts_mut(|view| view.fonts.font(&family))`
  (`i18n/tests.rs:407,423,442,557,607,803,912,1017,1125,1168`). In 0.37
  `FontsView.fonts` is private; use `view.characters(&family)`/`view.has_glyph`.
* The interface catalog has **16** languages (`i18n/mod.rs:173`), including Arabic
  (`ar.tsv` 1235 lines). `theme::can_show` (`theme.rs:640-647`) gates Arabic, Chinese
  and Korean on an installed or embedded face. Bundled epaint fonts cover Latin/Greek/
  Cyrillic but not Arabic, CJK or Hangul.
* Shortcut labels and catalogs contain `⌘` U+2318, `⇧` U+21E7, `↩` U+21A9, `−`
  U+2212, `…` and `°`. In 0.37 `⌘` is only in `emoji-icon-font`, included only with
  `monochrome_emoji_fonts`; `⇧` is only in Hack (Monospace). U+22EE is absent from both
  0.36.2 and 0.37 bundled fonts, so its absence is not a 0.37 change.
* Accessibility metadata is built with `egui::WidgetInfo` in `icons.rs:965-989`,
  `craft_ui` buttons, `egui::accesskit_node_builder` for tab roles, and control-channel
  role strings (`control.rs:113-180`). There is no explicit UI language in AccessKit.
* There are 27 `TextEdit`, 18 `DragValue`, 32 `ComboBox` and 8 `Slider` sites. CADCraft
  currently has no password field and no custom `IMEOutput` producer: its editors use
  the built-in `egui::TextEdit` (27 sites), including the MTEXT editor and dialog/layer
  rename fields.

### 3.7 Tests, gates and CI

* `cargo xtask ci` = fmt, clippy `-D warnings`, workspace tests, assets, layers, wasm
  (`xtask/src/main.rs:151-205`). The wasm step covers L0-L4 plus `ui-egui`, **not**
  `cadcraft-web`.
* `apps/cadcraft` and `apps/cadcraft-web` are app packages; the web runner needs its
  own `cargo check --target wasm32-unknown-unknown` and `trunk build --release`.
* `crates/ui-egui/tests` has **43** `*.rs` files. The affected inventory includes
  `alias_dialog.rs` (an unnamed filter `TextEdit`, §8.3), `animation_effects.rs`
  (two more `MouseWheel` constructors), `chrome_geometry.rs` (status text at **1200×760
  and 900×560**, `tests/chrome_geometry.rs:58`), `docking_workflows.rs`,
  `workspace_focus.rs`, `hidden_command_line.rs`, `pickdrag.rs`,
  `gpu_viewport_stencil.rs`, `gpu_faults.rs`, `shared_controls.rs`,
  `control_methods.rs`, `menubar_hover.rs`, `context_menu.rs`/`context_menus.rs`,
  `style_dialogs.rs`, `dialog_dispatch.rs`, `text_double_click.rs`, `save_version.rs`,
  `rapid_clicks.rs`, `drawing_recovery.rs`, `options_dialog.rs`, `options_followups.rs`,
  and `src/i18n/tests.rs`. There are no golden images; the only PNG output is opt-in
  (`CRAFT_UI_SUITE_SHOTS`, `CRAFT_UI_VISUAL_DIR`).
* There are ~68 direct `CadApp::logic` call sites, many in tests. After the split, a
  direct call on a bare `Context` still reports pass index 0, so it keeps running
  frame work; tests that need multi-pass semantics must use the `run_ui` helper shape
  used by `hidden_command_line.rs:19-30`.

## 4. Upstream 0.37 compile incompatibilities

These remain **after** `craft-ui` is moved to the same egui 0.37 source; before that,
the craft-ui type mismatches dominate. Re-count on the migration tip.

| # | Anchor | Symptom | Replacement and semantics |
|---|---|---|---|
| C1 | `lib.rs:688-706` (`issue_screenshots`), `:707-740` (`collect_screenshots`) | `egui::UserData` gone; `Event::Screenshot` gone | Use `ctx.request_screenshot(move |image: Arc<ColorImage>| …)` (or `ViewportCommand::Screenshot(ScreenshotCallback::new(...))`). Keep the token, path, reply and deadline in CADCraft; the callback hands `(token, image)` to the request's completion slot. Call `ctx.request_repaint()` from the frame tick after completion, not from a callback holding a strong `Context` (§6.5). |
| C2 | `animation.rs:78` pattern; `animation.rs:145,197`, `canvas.rs:2806`, **and tests `animation_effects.rs:102,133`** initializers | `MouseWheel` gained `source: MouseWheelSource` | Add `source: _` to the pattern (or `..`); construct test events with `MouseWheelSource::Unknown` (the platform value on X11/Wayland/Windows/web) or `Wheel`/`Trackpad` where the test is about macOS momentum. Prefer a local test helper so a future field does not break every constructor. |
| C3 | `i18n/tests.rs:407,423,442,557,607,803,912,1017,1125,1168` | `FontsView.fonts` is private | Replace `view.fonts.font(&family).characters()` with the public `view.characters(&family)` or `view.has_glyph(&FontId, c)`. Keep the test intent: the family's actual character map, not a fallback glyph. |
| C4 | `icons.rs:965-989` (`describe_control`), all `WidgetType::Button` | `WidgetType` is a deprecated alias of `accesskit::Role` | Use `egui::Role::Button`; `craft-ui` uses `Role::CheckBox`. Assert the role in a semantic test. |
| C5 | 92 `egui::Id::new(...)` calls (90 src, 2 tests: `docking_workflows.rs:303`, `options_dialog.rs:237`), e.g. `palettes.rs:615,891,957`, `exchange.rs:75-144`, `docking.rs:339,375` | Deprecated; `clippy -D warnings` fails | `egui::Id::unique(...)` (same hash; `Id::new` now calls `Id::unique`), or `ui.make_persistent_id(salt)`/`IdSalt::new` where a scope-local id is wanted. `docking.rs` must keep the exact same id string for `DockArea` and `cancel_drag`. |
| C6 | 51 `ui.id()` calls (e.g. `chrome.rs:83`, `palettes.rs:154`, `cmdline.rs:446`) | Deprecated | `ui.scope_id()`. No value change. |
| C7 | `managers.rs:77` `num_field(ui, id: impl egui::AsId, …)` | `AsId` is a **deprecated blanket trait**, not removed (`egui/src/id.rs:7-12`, exported at `lib.rs:550`); clippy `-D warnings` rejects its use | Take `impl egui::AsIdSalt` and use `ui.make_persistent_id(id)`; the temporary field id becomes scope-local. |
| C8 | `egui::Panel::top/left/right/bottom("…")` (`chrome.rs:69,180,300`, `palettes.rs:330,346`, `menus.rs:573`) | 0.37 panel constructors take `impl AsIdSalt`; `From<&str> for Id` is gone; the panel id becomes `parent_ui.scope_id().with_salt(...)` (`panel.rs:974-976`) instead of 0.36's `Id::new("…")` | Keep the literal names `cc_title_toolbar`, `cc_menubar`, `cc_status`, `cc_file_tabs`, `cc_toolsets_strip`, `cc_palettes`; do not hard-code ids in tests. The first compatibility PR includes the additive `PanelState` key migration in §7. |
| C9 | `UiBuilder::id` inside `craft-ui` | Deprecated | `scope_id`; handled in craft-libs S1, not here. |

No other 0.37 removals appear in CADCraft source: no `AsId` removal, no
`WidgetType::TextEdit`, no `global_theme_preference_switch`, no `show_inside`,
no `ImeEvent` matching, no `ScrollArea::id_source`.

## 5. Runtime and semantic changes CADCraft must decide

| Area | 0.37 change (primary source) | CADCraft decision |
|---|---|---|
| Layout/frame | `Context::run_ui` multi-pass; **native and web call `App::logic` per pass**; hidden root runs `run_logic` only; completed `run_ui` reports pass index 0 | Use `current_pass_index() == 0` for frame-once work; move input to `App::ui`; never use `visible()` as a phase (§6). |
| Screenshot | callback-based (#8489) | Bounded, cancelable per-request completion slot; weak callback; capture-time binding; deadline; one file/reply (§6.5). |
| Clipboard | `eframe` gains optional `clipboard` (#8392) | Add `"clipboard"` to the desktop `eframe` features. On web, use the browser path; verify paste/copy in a real browser. |
| Fonts | `default_fonts` no longer default (#8583); eframe `system_fonts` default (#8512) | Enable `default_fonts` and `monochrome_emoji_fonts` in **S2**; keep `system_fonts` off and set `NativeOptions::system_font_fallback = false`. |
| Glyphs | kittest panics on a missing glyph by default (#8601); U+22EE absent from both 0.36.2 and 0.37 bundled fonts | Enable the monochrome emoji face; run a strict glyph sweep in **S2**; replace uncovered UI symbols with code-drawn art/text; never set a global `MissingGlyphPolicy::Tofu` for the suite. |
| TextEdit | clicking another widget collapses the selection before the button runs (#8763; unmerged #8764) | Assert the **exact selected range** is formatted by the MTEXT Bold/Italic/Underline/Overline toolbar and any dialog action; pointer and keyboard paths. CADCraft's MTEXT toolbar is drawn before the `TextEdit` and uses its own `st.sel`, which may make it immune; prove it, do not assume it. If a regression is real, preserve the range in CadCraft or adopt the fork fix; do not patch by disabling selection. |
| Menus | arrow-key navigation within open menus (#8548), popup sizing passes | Re-run `menubar_hover`/`context_menu(s)`; add keyboard traversal. |
| Accessibility | `WidgetType`→`Role`, unnamed inputs fail kittest (#8555/#8589/#8611), `label_inputs_by`/`name_inputs`/`accessible_language` | Name every input reached by tests in S2; add the systematic unnamed-widget audit; put the UI language on the AccessKit root via fork E17 in S4. |
| Clipboard/PRIMARY | 0.37 supports X11/Wayland PRIMARY (#8428); Wayland clipboard #8657/E4; **W15P is new** | Require winit W15P + egui E4/P2b in the final pins; test the native matrix; do not default to a PRIMARY-only fallback (§9). |
| Web | `DroppedFile::web_file` opt-in (#8669) | eframe enables it on wasm; assert the feature is in the **wasm** graph and add `cargo check -p cadcraft-web`. |
| Scroll/animations | kinetic scrolling (#8662/#8663), repaint-burst fix (#8595), style sizing (#8617/#8445) | Re-run animation on/off tests; apply wheel/zoom once and reuse the resulting view for later passes (§6); measure idle frames; keep explicit spacing overrides. |
| Hidden window | pinned eframe runs `PassMode::LogicOnly` for an overdue root; **#8748 is closed and unmerged**; no deferred viewports in CADCraft | Keep control/LISP/autosave/screenshot deadlines alive on logic ticks; handle input only in a known UI pass. Do not claim #8748 behavior. A fork that later adopts an overdue full pass must pin that exact revision and re-run the hidden tests. |

## 6. Root `Ui`, multi-pass and logic-only semantics

### 6.1 Rules

1. `CadApp::ui(&mut self, ui: &mut egui::Ui)` remains the only root-`Ui` entry.
   Panels are added in the existing order; dialogs/windows/areas continue to use
   `ui.ctx()`. No code calls `Context::root_ui` inside `ui` (it would panic, #8535).
2. `frame_tick(ctx) := ctx.current_pass_index() == 0`. It is true on pass 1 and on
   every logic-only tick, and false on later passes. It is the only frame-once guard.
   Do **not** use `cumulative_frame_nr` for this: `run_logic` does not increment it, so
   a hidden tick and the next full frame share a number and would duplicate work.
3. Input-dependent work (shortcuts, clipboard mirror, dropped files, canvas zoom
   from wheel events) runs in `App::ui` on the first pass, never in `App::logic`.
   `run_logic` leaves events/time stale; reading `dropped_files` or `Event::Paste`
   there replays a user action on every hidden tick.
4. Consume-based background work runs on **every** `App::logic` call: the control
   mpsc drain, opening-job receiver, screenshot completion receiver, GPU-fault
   `take`, and the Plot/Hatch resume flags. These cannot duplicate because each item
   is consumed once. They must not be hidden behind the pass-index guard, or a fresh
   control reply, opening result or GPU fault arriving during a later pass would
   starve until the next frame.
5. Deadline/time-based work (LISP slice, autosave, queued-screenshot issue, workspace
   persistence, spacemouse poll, native-menu poll) runs on `frame_tick`. Autosave and
   the screenshot queue are additionally idempotent by their own deadlines; the guard
   keeps pacing deterministic rather than relying on that.
6. UI is never shown from `logic`. The opening spinner currently drawn at
   `lib.rs:580` moves into `draw()` as a foreground `Area`; `opening::show` keeps its
   rendering code but is called from `ui`.
7. Per-pass handlers that do run twice must be idempotent by a saved value, a
   `shown_*` field or a consumed queue. `control::begin_frame` still clears the
   observed-widget list at the start of each UI pass.
8. Input events are removed only after the app handled them: `raw.dropped_files`
   always; `Copy`/`Cut`/`Paste`/`MiddleClickPaste` only when
   `!ctx.egui_wants_keyboard_input()` (the guard `menus::shortcuts` uses). A focused
   `TextEdit` still receives them. Do not clear `Text`/`Key` before `CadApp::ui`
   because the command line reads them there.

### 6.2 Split (design, not code)

```rust
fn frame_tick(ctx: &egui::Context) -> bool {
    ctx.current_pass_index() == 0   // pass 1 or a logic-only tick; 0 after completion
}

pub fn logic(&mut self, ctx: &egui::Context) {
    // Consume-based: every call, never behind the guard.
    self.drain_control(ctx);
    self.finish_opening();
    self.collect_screenshots(ctx);
    self.gpu_fault_tick(ctx);
    managers::plot_window_resume(self, ctx);
    hatchdlg::resume(self);

    if frame_tick(ctx) {
        // Once per layout frame or hidden tick.
        self.session.lisp_tick();
        if self.session.lisp_busy() { ctx.request_repaint(); }
        self.close_request_frame(ctx);
        self.apply_theme_fonts_zoom(ctx);
        self.issue_screenshots(ctx);
        self.autosave_tick(ctx);
        if !self.synthetic.is_empty() { ctx.request_repaint(); }
    }
}

pub fn ui(&mut self, ui: &mut egui::Ui) {
    let ctx = ui.ctx();
    if frame_tick(ctx) {
        self.ui_input(ctx);       // shortcuts, clipboard mirror, dropped files
        self.frame_wheel(ctx);    // apply fresh wheel/zoom exactly once
        self.frame_cap_wait();    // pacing belongs to a painted pass
    } // later passes draw the already-updated state; never re-inject consumed wheel input
    i18n::with_language(self.language(), || self.draw(ui));
}
```

`ui_input` clears `raw.dropped_files` after opening them, and calls
`menus::shortcuts` (which consumes shortcuts and clipboard events). `frame_wheel`
runs `animation::unsmooth_scroll` when animations are off and applies the frame's
wheel scroll/zoom once. Later passes render the resulting view; they neither re-inject
events nor reapply a cached delta in `canvas.rs`. `frame_cap_wait` is moved out of `raw_input_hook` because the hook also
runs on logic-only ticks where no paint will happen.

`raw_input_hook` keeps only work that is safe without a pass: the desktop theme
override, `cmdline::capture_tab`, and synthetic-event injection. Injection drains
`self.synthetic` once into `RawInput`; eframe preserves that input in
`pending_raw_input` across logic-only ticks, so repeated ticks do not duplicate the
same event. Add a test that a queued synthetic event is applied once after a hidden
tick followed by a shown frame.

### 6.3 Side-effect placement

| Work | Anchor | Class | Why / test |
|---|---|---|---|
| LISP slice and repaint while busy | `lib.rs:538-541` | frame tick | One slice per layout frame; a hidden window keeps it alive. Test a LISP routine and a two-pass frame plus a minimized tick. |
| Close-request / CancelClose | `lib.rs:542-545` | frame tick | `run_logic` updates viewport close/focus; cancel must work while hidden. |
| Font install, theme/animation/zoom | `lib.rs:546-577` | frame tick (guarded `shown_*`) | Applied before pass 1; repeated calls are no-ops. Tests call `logic` repeatedly and expect a theme change. |
| Control queue | `lib.rs:664-687` | every call (consume) | Requests/replies must not duplicate; each mpsc item is handled once. Test one command in a two-pass frame mutates once, and a request queued between passes is answered without waiting for the next frame. |
| Opening jobs + spinner | `lib.rs:579-580`, `opening.rs:77-97` | receiver every call; spinner in `ui` | Worker results are consumed once; drawing is UI. Test the spinner shape appears in a `run_ui` output and no `Area` is created from `logic`. |
| Synthetic repaint | `lib.rs:581-583` | frame tick | Synthetic events are drained in the hook; request a repaint while they remain. |
| Screenshot collection/issue | `lib.rs:584-585`, `:688-740` | collection every call; issue frame tick | Correlation/deadline survive multi-pass; one request → one callback → one reply. |
| GPU faults | `lib.rs:586`, `autosave.rs:94-103` | every call (`take`) | Device errors arrive between frames; `save_now` runs once per fault. |
| Plot/Hatch resumes | `lib.rs:587-588` | every call (flags/take) | State-machine continuation; a second pass must not re-open a dialog twice. |
| Autosave | `lib.rs:589`, `autosave.rs:85-90` | frame tick | `Autosaver::tick` is time-based; after scheduling a write it records `written`, so it is idempotent, but one tick per frame keeps pacing deterministic. (SAVETIME 0 disables autosave; it does not write twice.) |
| Shortcuts + clipboard mirror | `lib.rs:590`, `menus.rs:629-700`, `clipboard.rs:29-60` | `ui` first pass | Input-derived; a hidden `run_logic` must not replay stale `Event::Paste` or copy twice. |
| Dropped files | `lib.rs:591-597` | `ui` first pass, clear after | **Current hazard**: stale `dropped_files` would re-open on every hidden tick and twice in a two-pass frame. Test one drop, then two hidden ticks. |
| Wheel/scroll/zoom input | `lib.rs:568-570`, `animation.rs:104-130`, `canvas.rs:1334,1396` | `ui` first pass only | Apply the input once; later passes draw the updated view/ScrollArea state without re-injection. Assert exactly one notch equals one view/scroll step under forced two-pass layout, animations on/off. |
| Spacemouse poll (host) | `main.rs:80`, `spacemouse.rs:61-80` | frame tick | `poll` copies `shared.motion` and re-applies it per call; one poll per frame. Test with a counter. |
| Native menu poll (macOS) | `main.rs:74-76` | frame tick | Rebuilds enablement a few times per second; not per pass. |
| Workspace persistence (host) | `main.rs:81,93` | frame tick (logic only) | `save_if_changed` is idempotent but must not run per pass; `save_on_exit` stays in `on_exit`. Remove the duplicate call from `App::ui`. |
| Web inbox drain / dirty bridge | `apps/cadcraft-web/src/main.rs:33-44` | drain every call; dropped files in `ui`; dirty report frame tick | DOM reporting is idempotent; dropped files are input and must be read once. |
| `raw_input_hook` frame cap/synthetics | `lib.rs:601-625` | per logic callback | Runs in `update` and `update_logic_only`; move the frame-cap wait to `ui` pass 1; synthetic injection is drain-based and preserved in pending input. |

### 6.3.1 Host and web frame token

The host `eframe::App::logic` implementations are outside `CadApp`, so they use the
same public token rather than a new scheduler:

* native `apps/cadcraft/src/main.rs:68-84`: call `self.cad.logic(ctx)` on every
  `App::logic` invocation; wrap `native_menu.poll`, `spacemouse.poll`,
  `persist_workspace(false)` and the `quit_requested` check in
  `if ctx.current_pass_index() == 0`. Remove `persist_workspace` from `App::ui`
  (`main.rs:91-94`), which currently runs it a second time. `spacemouse.poll` copies
  `shared.motion` rather than taking it (`spacemouse.rs:61-80`), so without the guard a
  second pass applies the same motion twice.
* web `apps/cadcraft-web/src/main.rs:31-44`: `Inbox::drain` (background results) runs
  on every logic call; `Bridge::report_dirty` runs on `frame_tick`; `take_dropped`
  moves to `Shell::ui` and clears `raw.dropped_files` after reading them. Remove the
  duplicate `report_dirty` from `Shell::ui`. `Inbox::take_dropped` currently clones
  `raw.dropped_files` without consuming them (`upload.rs:71-77`), so it would read the
  same files twice in a two-pass frame and on every hidden tab tick.
* `raw_input_hook` remains per logic callback on both hosts (see §3.3); only the frame
  cap moves to the first painted pass.

Add host/web counters in S3 tests: one spacemouse poll per frame, one native-menu
poll per frame, one dirty report per frame, and one dropped-file read per event.

### 6.4 Screenshot callback correlation and bounds

State per admitted request (owned by `CadApp`, with paths/replies/deadlines kept in
the app):

```
ShotRequest {
  token: u64, path: Option<String>, reply: Sender<Value>, deadline: f64,
  viewport: ViewportId, session_generation: u64,
  expected_px: (u32, u32), expected_ppp: f32,
  slot: Arc<ShotSlot>,            // one completion slot per request
}
ShotSlot { state: Mutex<ShotState>, wake: egui::Context }
ShotState { canceled: bool, result: Option<Result<Arc<ColorImage>, String>>, bytes: usize }
```

`issue_screenshots` waits for `canvas.building()` and at least three accepted UI frames (an intentional settling unit, never three discarded passes), then
calls `ctx.request_screenshot` with a closure holding **`Weak<ShotSlot>` only**:

```rust
let slot = Arc::downgrade(&request.slot);
ctx.request_screenshot(move |image: Arc<ColorImage>| {
    if let Some(slot) = slot.upgrade() {
        let wake = slot.complete(image); // stores image/result, releases the lock
        if let Some(ctx) = wake { ctx.request_repaint(); } // Context lives in the slot
    }
});
```

The callback must not capture a `Context` strongly: the callback is itself stored in
that context's viewport commands, so a strong `Context` would form a cycle. It must
not encode PNGs, write files, run commands, or hold a lock while waking.

`collect_screenshots` drains the slot results on every logic call, checks the token
and request/session binding, and replies with `save_screenshot(&image, path)`. Binding:

* CADCraft has ROOT only: bind to an explicit CadApp session/document generation.
  The callback supplies pixels only; viewport identity is request-owned, not callback
  metadata. Reject generation changes/close and never reuse an old result for a new document;
* At issuance refresh expected geometry from the target UI content rect and ppp,
  after the settling wait. If no fresh UI geometry exists, request a paint before issue.
  Revalidate a monotonic resize/DPI/zoom/surface epoch before saving; derive scale from
  returned pixels and unchanged logical bounds, including E2 fitting. A mismatch is:
  reply `{"ok":false,"error":"window geometry changed during capture"}` and release
  the image rather than silently pairing it with a newer request;
* a late callback whose slot is canceled or dropped releases the image without a
  file or reply; a timed-out request leaves no file and cannot later write one;
* admission limits: at most 4 pending/queued requests and at most 64 MiB aggregate
  reserved in-flight plus completed image bytes (64 MiB per-image maximum as well).
  Reserve checked expected width×height×4 bytes at admission, adjust before issue after
  a geometry change only if capacity permits, otherwise reject. Requests over the
  limit are rejected immediately with an error reply. `pending_bytes` is decremented
  on drain, timeout, cancellation and dropped slots;
* cancel all slots on close/viewport loss and in `on_exit`; cancellation is a flag,
  not a process kill. `now_ms` is 0 on wasm, so the deadline uses
  `web_time::Instant` on web, never stale InputState::time; do not leave an infinite
  web deadline.

CADCraft adopts the shared craft-ui CL-2 completion-slot API (the eleven existing craft-ui creative consumers'
approach; DeckCraft, FigCraft and ArtCraft Native keep local adapters).
Request records, paths, PNG serialization, timeouts and the reply schema stay in
CADCraft. If the provisional CL-2 API is unavailable, S3 is blocked rather than
growing a second divergent helper.

Regression assertions:

* two requests in one frame, distinct paths, both files have the harness's pixel size
  and each reply names its own path;
* a request while a popup forces a second pass produces exactly one file;
* a timed-out request leaves no file and cannot later write one;
* a minimized/hidden window reports the documented timeout, or succeeds through a
  forced hidden paint (`RequestPaintWhileHidden`/screenshot), never hangs;
* admission overflow returns an error and does not grow memory;
* a completion after resize/DPI change returns the geometry-mismatch error;
* a late callback after close writes nothing.

kittest fulfils `ViewportCommand::Screenshot` in `Harness::handle_screenshots`
(0.37 `egui_kittest/src/lib.rs:909-947`), so routing/bounds are testable without a
real window.

### 6.5 Multi-pass and logic-only tests

* `two_pass_frame_runs_frame_logic_once`: force a discard pass (open a ComboBox or
  menu); assert one non-idempotent counter/LISP slice and one control execution.
  Mutation: move the LISP slice into an unguarded call; the test must fail.
* `logic_tick_with_unknown_visibility`: call `run_logic` with
  `minimized=None, occluded=None`; queue a control `ping`, a screenshot deadline and
  one LISP slice; assert all progress and no UI pass.
* `logic_tick_with_visible_true`: same with `minimized=Some(false), occluded=None`;
  this is the case the old `visible()` predicate got wrong.
* `hidden_painted_two_pass_frame`: with `minimized=Some(true)`, run a full `run_ui`
  with a forced second pass; assert frame work once, input once, and the final pass
  renders the once-updated view without applying wheel movement again.
* `hidden_logic_ticks_do_not_reopen_dropped_files`: one shown frame with a dropped
  file, then two `run_logic` ticks; the file opens once. Mutation: move drop handling
  into `logic`; the test must fail.
* `hidden_logic_ticks_do_not_paste_twice`: one `Event::Paste`, a shown frame, then
  two `run_logic` ticks; the paste command runs once.
* `fresh_control_between_passes`: request a discard in pass 1, enqueue a control
  request before pass 2; it is answered in pass 2, not starved.
* `control_queue_answers_while_minimized`: `ping`/`ui.inspect` replies arrive on
  `run_logic` ticks and no UI pass occurs.
* `opening_spinner_is_drawn_in_ui`: the spinner `Shape` appears in a `run_ui` output;
  no `Area` is created from `logic`.
* `spacemouse_one_poll_per_frame` and `web_drop_once`: counters across a multi-pass
  frame and a hidden tick.
* `hidden_child_only_if_adopted`: CADCraft has no deferred viewports, so assert that
  (no `show_viewport_deferred` call) and record #8748 as not applicable. If a later
  fork pin adopts an overdue full pass, add a hidden-parent/deferred-child
  open/update/close test with overdue redraw and unknown visibility before claiming it.
* `undo_and_save_reopen_still_single_step`: command → undo → redo → save → reopen via
  control methods; assert entity counts and DXF bytes, not just `ok`.
* `direct_logic_tests_do_not_silently_skip_frame_work`: audit the ~68 direct
  `CadApp::logic` call sites; tests whose intent is a frame use the `run_ui` helper.

## 7. Panels, docking, IDs and persistence

* Keep the panel order from §3.3. The root `Ui` is full-window; panels consume space
  before `CentralPanel`, so a panel id or separator/clip change moves the canvas rect.
  Assert `canvasRect` equals the window minus the top/bottom/left chrome at
  **1600×1000** (new test) and keep the existing status-bar geometry at
  **1200×760 and 900×560** (`chrome_geometry.rs:58`).
* Keep the panel id strings `cc_title_toolbar`, `cc_menubar`, `cc_status`,
  `cc_file_tabs`, `cc_toolsets_strip`, `cc_palettes`. Do not rename them during the
  `AsIdSalt` move; tests and `ui.elements` refer to behaviour around them.
* **Panel `PanelState` migration (first compatibility PR).** 0.36 keyed a panel's
  `PanelState` by `Id::new("cc_x")`; 0.37 keys it by
  `parent_ui.scope_id().with_salt(IdSalt::new("cc_x"))` (`panel.rs:974-976`). CADCraft
  currently sets `persist_egui_memory=false` and its top-level panels are
  `exact_size(...)`/non-resizable, so no `PanelState` is currently file-persisted. The
  first compatibility PR nevertheless owns the saved-key migration and performs it
  additively before the first UI pass: if the new key has no `egui::PanelState` and
  the old `Id::unique("cc_x")` key does,
  copy the old value to the new key; keep the old key for rollback. Seed a
  `Context` with an old-key `PanelState` in a test and assert the new key sees it.
  This prevents a one-time width reset if eframe memory persistence is enabled or a
  panel becomes resizable, and covers saved test fixtures/other consumers.
* `DockArea::new(egui::Id::new("cadcraft-docking"))` and
  `cancel_drag::<Panel>(ctx, egui::Id::new("cadcraft-docking"))` must migrate together
  to `Id::unique` with the same string. A mismatch leaves a drag active after a
  workspace replacement; `docking_workflows.rs` already tests host cancel.
* `DockArea::direction` is an **explicit opt-in visual policy** from craft-libs
  §7.7/CL-3. CADCraft passes `DockDirection::LeftToRight` by default and
  `RightToLeft` only in S5b if chrome mirroring is implemented. Never reverse saved
  tab order to simulate RTL. Preserve serialized split first/second, axes, ratios,
  floating rectangles and logical tab order; painting, hit tests, drag insertion,
  accessibility order and trailing resize grips must use the same computed
  rectangles; physical splitter direction is unchanged. Test LTR/RTL click, drag,
  reorder, resize, keyboard focus, protected floating groups and an unchanged
  serialized workspace; deliberately mirror paint without hit coordinates to prove
  detection.
* Docking layout persistence is `Workspace` JSON (`docking.rs:74-130`), not egui ids.
  Assert `ui.dock`, `ui.set`, save/reload with floating groups, stacks and hidden
  panels after the craft-ui update; changing direction must not rewrite the JSON.
* Narrow-window behavior remains: toolbar overflow (`chrome.rs:96-120`) keeps every
  command reachable; `chrome_geometry.rs` must keep passing at 1×/2×.
* There are no secondary native viewports (`show_viewport_deferred`/immediate viewport
  use), so fork E1 (`ViewportHooks`) is unused. “Viewport” in CADCraft code
  (`canvas.rs`, `vpscale.rs`, `render/lib.rs`) means a paper-space layout viewport.

## 8. Fonts, glyphs, language, RTL and accessibility

### 8.1 Fonts and glyph coverage

* S2 enables `default_fonts` and `monochrome_emoji_fonts` on the workspace `egui`
  dependency. Without the latter, `⌘` (U+2318) has no bundled face (it was in
  `emoji-icon-font`, previously unconditional). This is a visible glyph change and
  therefore part of the compatibility PR, not a later “no behaviour change” stage.
* `⇧` (U+21E7) is only in Hack (Monospace), while `Button::shortcut_text` uses the
  Button text style (Proportional body in CADCraft). Decide in S2: draw the macOS
  modifier symbols in code (the WCS chevron already is, `canvas.rs:2558`), keep `⇧`
  only in monospaced contexts, or use text on macOS. Do not silently enable tofu.
* Split the coverage test by source, because bundled fonts cannot cover every catalog:
  * bundled-font sweep: every non-ASCII character in `i18n/*.tsv` that is not in a
    `can_show`-gated script, plus `shortcut_label` outputs, must have a glyph in the
    Proportional or Monospace family under `default_fonts` + `monochrome_emoji_fonts`,
    without the system-font prepend (deterministic on CI/web);
  * pinned-fixture sweep: Arabic/Thai/CJK/Japanese/Hebrew/color-emoji fixtures from
    §8.4 loaded into a test `Context` cover the gated languages’ catalog characters.
  * `theme::can_show` (`theme.rs:640-647`) remains the gate for `ar`/`zh*`/`ko`;
    record which languages are gated and which fixtures were installed.
* Keep `theme::install_fonts`' caps (`MAX_UI_FONT_BYTES`) and file reads unchanged.
  Do not adopt the 0.37 `GlyphRasterizer`/`FontProvider` APIs; set
  `NativeOptions::system_font_fallback = false` explicitly and do not enable eframe's
  `system_fonts` feature. This keeps CADCraft's capped app-controlled chain
  authoritative and avoids the #8512 system-font discovery cost.
* Keep `FontsView` access test-only through `view.characters(...)`/`view.has_glyph`.

### 8.2 Language and RTL

* In `CadApp` (frame tick or first UI pass, before the AccessKit tree is built), call
  `ctx.set_language(Some(language.to_owned()))` when the resolved language changes
  (fork E17). This places the BCP 47 tag on the AccessKit root; the web runner's
  speech/hidden-input language uses it through E18.
* Do not adopt `craft-i18n`; CADCraft owns its TSV catalog and `with_language`.
* Arabic: keep Arabic **text** rendering (bidi + shaping, E5/E12/E13) and verify
  `ar.tsv` screens. `Style::right_to_left` (E9) mirrors egui menus, scroll bars,
  checkboxes, combo boxes and popups, but not CADCraft's custom chrome or docking.
  S4 keeps `right_to_left = false` (Arabic UI stays LTR with correctly shaped text,
  documented in `docs/localization-parity.md`). S5b may enable it only with app-owned
  chrome mirroring and craft-ui `DockArea::direction`; until then do not claim an RTL UI.
* Logical text is authoritative. `crates/fonts/src/bidi.rs`/`mtext.rs` compute visual
  runs for rendering from logical text (`visual_runs`, `char_levels`); clipboard,
  DXF/DWG save, accessibility and search continue to use logical strings. Never feed
  display-bidi (visual-order) text into shaping, the clipboard or `IMEOutput`.
  Test an Arabic MTEXT: copy, save/reopen and compare the logical string; no doubled
  bidi and no reordered clipboard.
* AccessKit language test: after switching to `pt-BR`/`ar`, assert the root node's
  language and `Options::language`; text-run direction assertions belong to fork E17.

### 8.3 Accessible names and roles (S2, not S4)

* kittest 0.37 defaults `check_accessibility = true` and
  `missing_glyph_policy = MissingGlyphPolicy::Panic`
  (`egui_kittest/src/builder.rs:50-51`); `check_accessibility()` runs after every
  `run`/`run_steps` (`lib.rs:519-520`). Existing `Harness` tests therefore cannot be
  green if an input is unnamed or a glyph is missing.
* Known failure: `alias_dialog.rs:35-37` renders the Alias dialog; its filter is
  `ui.text_edit_singleline(&mut st.filter)` (`aliasdlg.rs:164-165`) after a separate
  `ui.label("Filter")` with no `labelled_by`/placeholder. Name it in S2.
* Use `Response::labelled_by(label_response.id)` or
  `ui.label_inputs_by(label.id, |ui| …)` for rows whose label CADCraft draws;
  `hint_text` is acceptable for placeholder-labelled search fields, not numeric entry.
* Keep `icons::describe_control` labels/tab roles; migrate `WidgetType` to
  `Role::Button` and assert `get_by_role`/`get_by_label` for a toolbar button, a toolset
  tab and a dialog input.
* Add one test per dialog group that opens the dialog and asserts
  `harness.unnamed_widgets()` is empty. Do not disable `check_accessibility` globally.
  If a third-party widget cannot be named, name it at the call site and record why; a
  per-test `with_accessibility_check(false)` requires an explicit enumerated exception,
  a linked issue and a removal condition, not a suite-wide switch.
* S2 runs the full `cargo xtask ci` plus `cargo test -p cadcraft-ui-egui --all-targets`
  and keeps `MissingGlyphPolicy::Panic`; S4 adds broader language/AccessKit-language
  coverage, not the first green gate.

### 8.4 Pinned licensed font fixtures

Use the fixtures pinned by the egui plan §10.6; fetch them into the task evidence
directory, never commit them into CADCraft, and retain adjacent licences. CADCraft’s
own policy remains: optional central `craft-fonts` via `CRAFT_FONTS_DIR`, no font
binaries in this repository, no new `ATTRIBUTION.md` row for test-only external files.

| Fixture | Source/path | SHA256 | License |
|---|---|---|---|
| Arabic | craft-fonts `2481a8a324b986a45954e90045ed1f8f2efaaf3c` `fonts/noto-sans-arabic/NotoSansArabic.ttf` | `63111b5b2e074dd48cc67692e0a2726d86ee94c1c37fe8598257b7b4e87e869e` | OFL-1.1 |
| Thai | craft-fonts `fonts/noto-sans-thai/NotoSansThai.ttf` | `5a1c559bb539583c8a1fd99d1c5b9491e5e14478c9cd2bd0970d5c3096cc9ef8` | OFL-1.1 |
| CJK SC/Latin | craft-fonts `fonts/noto-sans-cjk-sc/NotoSansCJKsc-Regular.otf` | `2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b` | OFL-1.1 |
| Japanese | craft-fonts `fonts/biz-ud-pgothic/BIZUDPGothic-Regular.ttf` | `258d7156c165f2ff774b6efee637c22c3b950de0d8a10e501137061bc8085d01` | OFL-1.1 |
| Hebrew | google/fonts `8b0a1d0f5983c89bc2b93f1b5fb55f9e252744b5` `ofl/notosanshebrew/NotoSansHebrew[wdth,wght].ttf` | `7ef36a2c3593758cdb622e1bdef4f84523e92fbc3ccc667438dd80ff54c2de88` | OFL-1.1 |
| Color emoji | googlefonts/noto-emoji `22e564626297b4df0a40570ad81d6c05cc7c38bd` `fonts/NotoColorEmoji.ttf` | `39ee3c587e10e89669b9ff32703261d10d5f9c4dd5ad147b6b5a1c5200591817` | OFL-1.1 |

Bundled Latin/emoji comparisons use the exact `epaint_default_fonts` bytes from the
pinned baseline/candidate: `Ubuntu-Light.ttf`
`80307b8da7649aa4ee4d484b232140e3ce1ec0ca093073d3c53c8f5a5ced7a70` and
`NotoEmoji-Regular.ttf`
`415dc6290378574135b64c808dc640c1df7531973290c4970c51fdeb849cb0c5` (record licence
files). Fix the Hebrew variable axes to weight 400/width 100. All fixture hashes and
licences enter the evidence manifest; no script may silently skip for a missing fixture.

## 9. Clipboard, PRIMARY, IME and keyboard

* Add `"clipboard"` to the desktop `eframe` features and assert
  `cargo tree -i arboard` reaches `egui-winit`. Regression: Copy in CADCraft, paste
  into another app; copy text elsewhere, paste into CADCraft (the DXF-text mirror
  remains in `clipboard.rs`). The feature-graph assertion is the detector; a kittest
  copy/paste assertion cannot prove the OS clipboard because kittest does not run
  eframe's OS clipboard.
* **Required PRIMARY design.** The final winit pin must contain W15P (PRIMARY on the
  existing connection/event queue, per-seat lifecycle, SCTK 0.19.2
  `PrimarySelectionManagerState`/device/source/offer, no second regular
  `wl_data_device`), and the final egui pin must contain E4/P2b routing
  `get_primary_text`/`set_primary_text` through it. The current candidate winit
  `8b35388b` has no PRIMARY path, and the current candidate egui `a4f9a17f` lacks the
  later P2b integration; neither is acceptable as the final pin.
* PRIMARY acceptance (native, on wlroots/Hyprland, KWin, Mutter, plus X11):
  pointer- and keyboard-triggered ownership (including keyboard-only focus), serial freshness, owner/seat changes, empty/absent offers,
  self-paste (marker MIME, no deadlock), canceled/slow transfers, unchanged selection
  not reclaimed, password selection never offered. CADCraft defaults to XWayland when
  `DISPLAY` is set (`main.rs:301-306`); `CADCRAFT_WAYLAND=1` is the native Wayland
  exposure. A PRIMARY-only fallback is allowed only as a **measured** fallback with
  explicit owner acceptance, not as the default limitation.
* Middle-click: `Event::MiddleClickPaste { pos, text }` exists in 0.37. CADCraft's
  command line is custom-painted, not a `TextEdit`, so decide in S5a: handle it only
  when the pointer is in the command-line bar rect (stored state), insert at the buffer
  end, and never start a command or duplicate a `Paste`; otherwise document the gap.
  `Event::PasteImage` is ignored (no image paste).
* **E21b/P7b is a new prerequisite, not existing IME behavior.** Winit W27 exposes
  `set_ime_surrounding_text`, but candidate egui-winit never calls it and
  `IMEOutput` has no logical text. The final egui pin must contain E21b/P7b: an
  optional transient bounded payload (≤4000 UTF-8 bytes, UTF-8-boundary safe,
  preedit excluded, byte cursor/anchor remapped), a built-in `TextEdit` producer, and
  a Wayland adapter sending it on text/selection/IME-owner change; clear on password,
  focus loss, no IME, unavailable context and reset. Keep the final IME descriptor and
  payload together across passes. It is transient and excluded from serialized debug/
  persistence output. CADCraft has no custom `IMEOutput` producer (all editors use
  built-in `TextEdit`; the MTEXT editor and rename fields inherit it), so no app
  model adapter is required unless one is added later. The suite inventory's custom
  producers (DesignCraft/FigCraft/WordCraft/VectorCraft/EffectCraft/FilmCraft/
  DeckCraft/PhotoCraft) do not apply here: CADCraft's custom-painted command line
  receives IME commits as `Event::Text` and produces no `IMEOutput`. If a custom
  producer is added,
  it must supply logical model text and convert its own cursor coordinates to UTF-8
  bytes; `None` placeholders are not a port.
* IME tests: `Event::Ime(ImeEvent::Commit)` into a layer-rename field and the MTEXT
  editor; outgoing surrounding text updates immediately after delete+commit and
  deletion-only batches; scalar/grapheme vs byte indices; supplementary emoji,
  combining marks and RTL logical order; preedit exclusion and caret movement; a
  4000-byte/long-selection bound; same-window field switch and two windows; focus
  loss/reset/late serial; password/no-context clear; multi-pass final-output selection.
  Native Wayland Vietnamese/predictive IME verifies the protocol trace and editor
  content/undo. macOS NSRange/W26 remains a separate unqualified row.
* Keep `cmdline::capture_tab` (`cmdline.rs:56-74`) and synthetic injection. Tab capture
  only when `egui_wants_keyboard_input()` is false; Escape/Enter still reach the
  command line (`hidden_command_line.rs`).
* Non-Latin layouts: fork E7/E19 and winit W25 report Latin letters for shortcuts; test
  Cmd/Ctrl shortcuts with a Russian/Arabic layout sequence and the winit unit tests.
* Clipboard `ctx.copy_text` is an output side effect: keep the serial-guarded mirror in
  the first UI pass so a two-pass frame cannot copy twice.

## 10. GPU, wgpu, stencil and Windows

* CADCraft uses `egui-wgpu` paint callbacks, not a custom renderer; the callback trait
  is unchanged. Keep `gpu::install` error/device-lost handlers and the stencil check.
* The canvas draws at `info.screen_size_px` and relies on egui's scissor for clipping
  (`gpu.rs:731-780`). Fork E2+E3 fits an oversized surface to
  `max_texture_dimension_2d` and returns configure/render errors instead of panicking;
  CADCraft consumes it through eframe. Add a fork `surface_fit` unit test; a real
  oversized window cannot be produced in the available environment, so record it as
  an unresolved hardware gate.
* Windows: `graphics::configure` forces DX12 + FXC and excludes GL. Upstream #8736
  reports FXC compile cost for 0.37's predictable-filter shader (~57 ms vs 14 ms,
  about +40 ms startup). Measure CADCraft start-to-first-`ping` on the Windows VM
  (fresh processes) and compare 0.36/stock 0.37/fork; report renderer construction
  separately from first frame. Do not hide a regression by changing CADCraft's FXC
  choice, and do not report compile duration as UI performance.
* Keep `NativeOptions.stencil_buffer = 8` (`main.rs:188,277`). The fork's surface fit
  must preserve the matching stencil texture size. `gpu_viewport_stencil.rs` stays.
* Keep `WgpuConfiguration::default().with_surface_config(surface_config(...))`
  (`gpu.rs:53-65`) and the `max_buffer_size` adapter workaround (`main.rs:289-297`).
* Web: `?cpu` bypasses the GPU canvas; `?webgl` forces GL; the web runner passes
  `None` for stencil (`apps/cadcraft-web/src/main.rs:133-137`). Do not change that
  assumption without a WebGL2/browser test.
* Environment wording: this server has **no inherited display session and no running
  compositor**. The parent can access an i915 PCI `8086:3E96` render node
  (`/dev/dri/renderD128` readable/writable, `graphics-device-inspection.json`), but a
  worker sandbox may hide `/dev/dri`, and access alone is not a qualified Vulkan/
  OpenGL/real-GPU result. Windows remains the Microsoft Basic Render Driver
  (software D3D12). Report software and real-GPU tables separately.

## 11. Dependencies, fork patches and vendors

### 11.1 Target pins (final revisions unresolved)

Before pinning, the fork and winit revisions must be reachable from the organisation
repositories; Cargo fetches a repository by `rev`, so a commit that exists only in
`aiongg/*` is not publishable as `storytold/*`. Publication is a separate human
approval. The final revisions are placeholders until that approval:

```toml
craft-ui     = { version = "0.1.0", git = "https://github.com/storytold/craft-libs", rev = "<craft-libs final reviewed rev>", features = ["serde"] }
craft-docpath= { version = "0.1.0", git = "https://github.com/storytold/craft-libs", rev = "<same final craft-libs rev>" }
egui         = { version = "0.37.0", features = ["default_fonts", "monochrome_emoji_fonts"] }
egui_kittest = "0.37.0"
eframe       = { version = "0.37.0", default-features = false }
egui-wgpu    = "0.37.0"
```

and one root redirect for exactly the crates in the graph:

```toml
[patch.crates-io]
egui = { git = "https://github.com/storytold/egui", rev = "<storytold/egui final reviewed rev>" }
epaint = { git = "https://github.com/storytold/egui", rev = "<same egui rev>" }
emath = { git = "https://github.com/storytold/egui", rev = "<same egui rev>" }
ecolor = { git = "https://github.com/storytold/egui", rev = "<same egui rev>" }
epaint_default_fonts = { git = "https://github.com/storytold/egui", rev = "<same egui rev>" }
egui-wgpu = { git = "https://github.com/storytold/egui", rev = "<same egui rev>" }
egui-winit = { git = "https://github.com/storytold/egui", rev = "<same egui rev>" }
eframe = { git = "https://github.com/storytold/egui", rev = "<same egui rev>" }
egui_kittest = { git = "https://github.com/storytold/egui", rev = "<same egui rev>" }
winit = { git = "https://github.com/storytold/winit", rev = "<storytold/winit final reviewed rev>" }
```

Trim any crate not in the graph; an unused `[patch]` entry is a Cargo **warning**, not
a build error, but it is still removed. `egui_extras` is only a dev-dependency of
`craft-ui` and is not in CADCraft's graph. No `branch` and no `path` may appear for a
published pin; the audit's local path patches are evidence only. The final fork/tag
must also replace the fork's own root `[patch] winit = { branch = ... }` with the
reviewed winit rev, or document an immutable equivalent.

`rust-version`: keep the workspace at `1.90` for engine crates and add
`rust-version = "1.95"` to `apps/cadcraft` and `apps/cadcraft-web`, matching
`crates/ui-egui/Cargo.toml:7` and `README.md:126-127`.

### 11.2 craft-libs PR #199: port hunks, never the tree

`git diff --numstat 49190884 b68de8c -- crates/ui` is **78 insertions, 222 deletions**
(14 files). The stale trees `b68de8c`/`ec8840a` are missing current `craft-platform`
and revert docking/printing/security fixes (`c67b1e1` floating-group permission,
`4f85efb` bounded `shown_names`, and related settings/printer hardening). Never merge
or check them out as the target.

S1 creates a fresh branch from current `main` `49190884` and applies only the egui-API
hunks from `8614e4a` (plus the relevant README change from `1764bca`):

* `WidgetType`→`Role`;
* `UiBuilder::id`/`Ui::id`→`scope_id`;
* `Id::new`→`Id::unique`;
* dev-deps on the final egui 0.37 revision (also bump workspace egui and UI egui_extras/egui_kittest dev-dependencies together);
* the docking fixture's U+22EE→`…` workaround;
* the CL-2 capture-slot prototype and CL-3 opt-in `DockArea::direction` API.

Preserve the current docking-permission and print-name hardening. Run
`cargo test -p craft-ui --all-features`, the workspace tests, strict clippy and the
all-feature wasm check; merge; publish an immutable commit; then pin CADCraft to it.
`craft-docpath` is unchanged between `82a24b0` and `49190884`, so both CADCraft
craft-libs dependencies can use the same new revision.

### 11.3 Language work #198

Consume #198 through the final fork/winit pins (E5/E12-E21, W17-W27, plus E21b/W27);
do not add a `craft-i18n` dependency or adopt its UI model. CADCraft keeps its TSV
catalog and `with_language`, and uses only `Context::set_language` and
`Style::right_to_left` from the fork. Any app-side #198 adoption is separate.

### 11.4 Fork patch disposition for CADCraft

Provenance: `egui/PATCHES.md` on the candidate and commits `0.37.0..a4f9a17f`. The
final pin must carry the whole reviewed series; the table records what CADCraft
exercises and what would catch removal.

| Patch | What it does | CADCraft use | Keep / remove condition | Test |
|---|---|---|---|---|
| E1 | eframe `ViewportHooks` | Not used (no secondary windows) | Keep for craft-tablet; remove if upstream adds it | fork unit test |
| E2+E3 | fit oversized wgpu surface; setup errors | via eframe wgpu | Keep while emilk/egui#8361 open | fork `surface_fit`; CADCraft GPU gate unresolved |
| E4 | Wayland clipboard via winit | `CADCRAFT_WAYLAND=1` | Keep while #8657 draft; **requires W15 and W15P PRIMARY** | regular + PRIMARY native matrix |
| E5/E12/E13 | bidi shaping/order/marks/caret, graphemes | Arabic UI/text | Keep while #8577 open | fork text tests; Arabic UI label/MTEXT |
| E6 | IME focus not interrupted | Text fields | Keep | kittest IME commit |
| E7 | Windows VK letter keys | Non-Latin shortcuts | Keep; needs W25 | Windows VM |
| E8 | TextEdit glyph overshoot | Thai/Vietnamese/Arabic input | Keep | tall-glyph TextEdit |
| E9 | `Style::right_to_left` chrome | §8.2 decision | Keep; activation staged | fork RTL chrome; CADCraft opt-in only |
| E10 | web key/lang | Browser keys | Keep | browser/manual |
| E11 | vello_cpu single-thread raster | Not used | Keep | fork |
| E14/E15/E16 | fallback baseline, line breaker, cursor affinity | CJK/Indic editing | Keep | fork; CJK screens |
| E17 | `Context::set_language`, AccessKit language, bidi runs | Used in S4 | Keep until upstream | root-language test |
| E18 | eframe `Frame::set_ui_language`/web fallback | Web speech/lang | Keep | web/manual + language test |
| E19 | Latin layout chars for shortcuts | Non-Latin layouts | Keep; needs W25 | Windows/Linux manual |
| E20 | widget text in Ui language | All translated widgets | Keep | language render test |
| E21 | IME delete-surrounding bytes | macOS/Wayland IME | Keep; needs W26/W27 | fork + native IME |
| E21b | bounded outgoing IME surrounding text + TextEdit producer + Wayland adapter | All built-in `TextEdit` editors | **Required new prerequisite**; keep with W27 state/serial | outgoing-state/focus/password/UTF-8/4000-byte tests |
| P2b | E4 PRIMARY integration | Wayland PRIMARY | Required with W15P | native PRIMARY matrix |

Each “remove when” is a review condition: remove a patch only after the replacement is
verified in the same consumer graph and the table is updated.

### 11.5 winit

CADCraft needs the winit fork because E4/E19/E21/E21b call APIs in W15/W15P/W25/W26/
W27 not in crates.io 0.30.13. Patch winit to the **final reviewed rev**, not a branch;
keep `winit` 0.30.13-compatible `dpi`. `apps/cadcraft/Cargo.toml:29` keeps its direct
winit dependency for `EventLoopBuilderExtX11`; the patch makes it and eframe's winit one
package.

Do not delete W1-W15 or W17-W27 tests (`tests/ime_patches.rs`,
`tests/pointer_patches.rs`). W15P needs pure seat/offer/serial/owned-marker tests, a
`WAYLAND_DEBUG=1`/test-compositor protocol trace, and the native Hyprland/KWin/Mutter
matrix. W16 (pinch/rotation) remains outside this cut.

### 11.6 Vendors and lockfile checks (native and wasm)

* CADCraft has no vendor directory and no external path patch. Nothing is deleted.
* craft-libs' `vendor/krilla`, `vendor/hayro*` are PDF-only and unrelated; leave them.
* pdfcraft's vendored `egui-wgpu`/winit are another repository's responsibility; do not
  remove them here or copy their patches into CADCraft.
* After regeneration, assert on both target feature sets:
  * native: `cargo tree -d`, `cargo tree -i egui`, `cargo tree -i winit`,
    `cargo tree -i arboard`, `cargo metadata --format-version 1`;
  * wasm: `cargo tree --target wasm32-unknown-unknown -d`,
    `cargo tree -i egui --target wasm32-unknown-unknown`,
    `cargo tree -e features --target wasm32-unknown-unknown -p cadcraft-web`,
    and `cargo metadata` filtered/interpreted for `apps/cadcraft-web`;
  * no duplicate egui/emath/ecolor/epaint/accesskit 0.36/0.37 pairs;
  * every egui-family source is `git+https://github.com/storytold/egui?rev=<final>`;
    winit is the storytold fork; craft-libs crates share one final rev;
  * `harfrust` may legitimately appear at 0.12 (egui) and 0.14 (`cadcraft-fonts`);
  * `arboard` is present through `egui-winit` because `clipboard` is on;
  * the wasm graph has `egui/web_file`, eframe wgpu/accesskit and no native-only
    `arboard`, X11 or Wayland packages; assert these, and test a negative fixture with
    a missing family patch or a moved branch (it must fail the graph gate);
  * no manifest contains `path = "/home/…"` or a `branch =` for a published pin;
  * `cargo check --locked` succeeds on the final native and wasm locks.
* Keep the old coherent lock (`a08fd7c`) in evidence for rollback; do not delete the old
  dependency set from review notes.

Shared handoff requirement: consume complete egui P8 including P7c's selected #8763/#8748
fixes and P7 key-release pairing. MTEXT selection-to-format assertions and ordinary held-key
layout/focus changes are S5/S6 gates. Built-in TextEdit's E21b producer inherits P7b's owner
identity even when unavailable, Available-empty distinction, committed W27 clear, latest-pass
None, stale-epoch invalidation and current-epoch serial-mismatch behavior. Add a native test
contrasting stale epoch with current-epoch mismatch; do not implement fork internals locally.

## 12. Staged implementation PRs

Publication order assumes each repository has its own approval. No stage is pushed,
tagged, merged or posted by this plan.

```
storytold/winit final rev (bounded W15/W15P, W17-W27 lifecycle)
        -> storytold/egui final P8 (P2b/P7/P7b/P7c included)
        -> craft-libs S1: current main + PR199 API hunks + CL-2F/CL-3
        -> published craft-libs final rev
        -> cadcraft S2 (atomic compatibility; strict names/glyphs; clipboard/fonts;
                         PanelState migration; native+wasm graph)
             -> S3 lifecycle/screenshot/host
             -> S4 language/AccessKit/RTL text
             -> S5a PRIMARY/clipboard matrix (required for parity)
             -> S5b RTL activation (optional, only with chrome mirroring)
             -> S6 docs + acceptance evidence
```

* **S1 craft-libs** — fresh branch from `49190884`; apply only the `8614e4a` API
  hunks; preserve docking-permission/printing hardening; add CL-2 capture slot and
  CL-3 opt-in direction API; tests/clippy/wasm; merge; publish a final rev.
* **S2 CADCraft compatibility** — final pins; `clipboard`, `default_fonts`,
  `monochrome_emoji_fonts`; app `rust-version`; C1-C8 mechanical migrations; panel
  `PanelState` migration; accessible names and glyph sweep; lockfile regeneration;
  shared CL-2F callback migration including basic drain/timeout/reply handling and
  native **and wasm** graph assertions. It is atomic and must compile and pass
  `cargo xtask ci` (and `cargo test -p cadcraft-ui-egui --all-targets`). It is **not**
  a “no behaviour change” stage: the emoji font, accessible names and panel keys change.
* **S3 CADCraft lifecycle** — `frame_tick` split, opening spinner move, host/web frame
  token, once-per-frame wheel application, shared capture lifecycle stress, dropped-file fix,
  bounded counters, tests and mutation probes.
* **S4 CADCraft text/accessibility/language** — broader glyph coverage via pinned
  fixtures, `Context::set_language`, AccessKit language, Arabic shaping checks, docs
  row for RTL status.
* **S5a CADCraft clipboard/PRIMARY (required for parity)** — middle-click decision and
  the native Wayland/X11 PRIMARY matrix against W15P/E4/P2b. A measured fallback is
  recorded only if native evidence proves the design cannot meet a required platform.
* **S5b CADCraft RTL activation (optional)** — `DockArea::direction`,
  `Style::right_to_left` and app-owned chrome mirroring; otherwise document the gap.
* **S6 evidence and docs** — performance report, screenshots, before/after matrices,
  docs revision history, remaining gates.

Rollback: keep the `a08fd7c` source/lock pair as the last coherent 0.36.2 state. S2 is
the compatibility boundary; after it, revert the stack (source + lock) rather than the
lock alone. S3, S4, S5a and S5b are independent commits and can be reverted without touching the pins.
No partial rollback to 0.36 is promised after S2 until the stack is reverted as a unit.

## 13. Verification plan

### 13.1 Compilation and repository gates (through `task.py run/status/finish`)

* `cargo xtask ci` (fmt, clippy -D warnings, workspace tests, assets, layers, wasm).
* `cargo check --workspace --all-targets --locked`.
* `cargo test -p cadcraft-ui-egui --all-targets` **alone** (UI crate dev/test font
  features self-sufficient).
* `cargo check -p cadcraft-web --target wasm32-unknown-unknown` and
  `trunk build --release` from `apps/cadcraft-web`; the wasm32 target is installed,
  but `trunk`/`wasm-bindgen` and a browser are not, so the bundle/browser run remain
  blocking unknowns.
* Native **and** wasm graph checks from §11.6.
* Optional features: `cargo test -p cadcraft-ui-egui --features …` where the crate has
  features (currently none); craft-libs all-features/craft-ui all-features in S1.
* Windows: a task-specific craft-windows clone, separate target/evidence, native
  `cargo test --workspace --locked`, `cargo build --release -p cadcraft`, then the GUI
  workflow in §13.5. Do not reuse the baseline executable as new-source evidence.

### 13.2 Behavioural regressions

| Behaviour | Assertion | Anchor |
|---|---|---|
| Multi-pass frame side effects | one non-idempotent counter/LISP slice/control execution per frame with a popup open | §6.5 |
| Hidden logic | control replies, autosave deadlines and screenshot timeouts while minimized/unknown visibility; dropped file opens once | §6.5 |
| Fresh queues | a control request queued between passes is answered without waiting for the next frame | §6.5 |
| Hidden child (conditional) | if a later fork pin adopts an overdue full pass, parent occluded/overdue plus child open/update/close; CADCraft currently has no child viewports, so record N/A rather than claim #8748 | §6.5, §14.5 |
| Screenshot correlation | two tokens/two paths; timeout/cancel/admission/geometry mismatch | §6.4 |
| Command/undo | create → undo → redo → entity count | engine + UI control test |
| Save/reopen | `examples/apartment.dxf` opened, edited, saved to temp, reopened; entity count and file bytes | `apps/cadcraft`/CLI |
| Theme/language | light/dark/System, `en`/`de`/`zh`/`ar`, no text loss; AccessKit language | `lib.rs`/`i18n` tests |
| Panels/docking | canvas rect, tab order, floating group move, workspace reload, unchanged direction JSON | `docking_workflows`, `chrome_geometry` |
| Keyboard/focus | Tab/Shift-Tab, Enter/Space activation, Escape/Enter command line, IME commit | `workspace_focus`, `hidden_command_line`, new IME |
| Clipboard | Copy/Cut/Paste across apps, DXF mirror, X11/Wayland PRIMARY matrix | `clipboard.rs`, native |
| Selection | MTEXT toolbar formats the exact selected range; pointer and keyboard paths | `#8763`/`#8764` test |
| GPU/stencil | clipped viewport pixels, surface/stencil sizes, device-loss notice | `gpu_viewport_stencil`, `gpu_faults` |
| Fonts/glyphs | bundled sweep + pinned fixtures; no tofu/clipped combining marks | §8.1/§8.4 |
| Web | `?cpu`, `?webgl`, open/download/iframe host | browser matrix |

### 13.3 Screenshots, geometry and semantic accessibility

* Capture before/after PNGs for 1600×1000 and 900×560, 1.0/1.25/2.0 scale, dark/light,
  Start tab and a drawing with Layers/Properties/Tool Sets open, an in-window menu, the
  command line with keywords, MTEXT editor, Layer Properties Manager and Options.
* Compare geometry semantically first (panel rects, canvas rect, text bounding boxes,
  no overlap), then pixels with stated tolerances: exact for flat fills/sampled
  backgrounds; changed-pixel ratio ≤0.5% per window and no text box moving more than
  2 px unless the change is the intended 0.37 style adjustment. Record both images and
  diff counts; never update a golden without a stated intent.
* Accessibility: `unnamed_widgets()` empty for every opened dialog, expected
  role/label/value for representative inputs, tab traversal order, focus visibility and
  AccessKit root language after a switch.
* kittest `with_context_setup` is the supported way to install deterministic fonts/
  options before the first pass (#8686); do not use `Context::run_logic` to initialise
  fonts or read input twice.

### 13.4 Mutation probes

Break the implementation once per feature and record the failure:
1. move only the deliberately non-idempotent LISP slice/counter into a second-pass
   call; the two-pass test must fail (the other queue work is consume-based and would
   still run once, so it is not a valid probe);
2. map two screenshot tokens to one path; the correlation test must fail;
3. remove the `clipboard` feature; the **`cargo tree -i arboard`** assertion must fail,
   and a manual OS-clipboard check must fail (a kittest copy/paste assertion is not a
   detector);
4. put dropped-file handling back in `logic`; the hidden-tick/reopen test must fail;
5. remove an accessible name; `unnamed_widgets` must fail;
6. change `Role::Button` to `Role::Unknown`; the role test must fail;
7. remove `monochrome_emoji_fonts`; the `⌘` glyph test must fail;
8. leave `DockArea` and `cancel_drag` ids different; the drag-cancel test must fail;
9. skip the panel `PanelState` migration; the seeded old-key test must fail;
10. reapply the wheel delta in pass two; the two-pass view-position assertion must fail;
11. send a screenshot completion after resize/DPI change; the mismatch test must fail;
12. remove the 4000-byte IME bound/preedit exclusion; the IME tests must fail.

A probe is not a deliverable; record the failure output in evidence.

### 13.5 Platform matrix

| Platform | What can be qualified here | Limits / required preparation |
|---|---|---|
| Linux host, headless | compile, clippy, tests, kittest, `cadcraft-cli perf`, wasm compile | no inherited `DISPLAY`/`WAYLAND_DISPLAY`, no compositor/Xvfb; not native focus/IME/picker/GPU evidence |
| Linux X11 | functional GUI and focus/clipboard after provisioning Xvfb + a window manager; record software renderer/versions | Xvfb/manager absent at inspection; must be provisioned as an isolated managed job; results labelled virtual-display/software |
| Linux Wayland | W15/W15P/PRIMARY, W27/E21b IME after provisioning Weston/Sway/wlroots/KWin/Mutter | compositors absent; XWayland default path is separate; native capability evidence required |
| Linux GPU | parent-visible i915 `8086:3E96` renderD128; qualify Vulkan/OpenGL/wgpu via a managed probe | worker sandbox may hide `/dev/dri`; access alone is not GPU acceptance; software and real-GPU tables separate |
| Windows craft-windows VM | native compile/tests, GUI launch, screenshots, control channel, Basic Render Driver | software D3D12; IME/keyboard layouts only if the guest has them; no real-GPU claim |
| Browser | `cargo check`/`trunk build`; interactive run needs a pinned browser and local server | no browser/trunk installed; WebGPU/WebGL backend must be recorded |
| macOS | release CI only, no local hardware | native focus/IME/picker/traffic-light layout unresolved |
| FreeBSD | CI workflow/VM only | no local evidence |

Native focus/IME/file-picker behavior and real-GPU performance are not established by
headless rendering or the software renderer. macOS/browser rows remain unresolved unless
real evidence is produced.

## 14. Performance plan

### 14.1 Environment and identities

Record for every run: exact CADCraft SHA, `Cargo.lock` SHA256, feature set,
`CRAFT_FONTS_DIR` (or absent), fixture (`examples/apartment.dxf`, synthetic counts),
OS/kernel, Rust version, GPU or software renderer/driver, display scale, viewport size,
build profile (`release`), and whether the canvas is GPU or `?cpu`. On Linux record
`lscpu`, `uname -a`, DRM device identities and `vulkaninfo --summary` when available,
plus the Xvfb/compositor/window-manager versions. On Windows record the guest adapter
and `dxdiag` Basic Render Driver. Never mix software and real-GPU tables.

### 14.2 Baselines and attribution

* **B0**: `a08fd7c` with the coherent 0.36.2 egui family and craft-ui `5c874d51`.
* **B0u**: B0 in a measurement-only worktree with `frame_nr`/pass/CPU counters and
  idle-frame instrumentation backported (never committed).
* **B0e**: `1977a493` engine/format dependency state measured with `cadcraft-cli perf`
  (engine-only, no UI) to attribute tiny-skia/skrifa/harfrust/quick-xml changes.
* **C0**: the S2 compatibility commit. It includes the clipboard/font/accessibility/
  panel-key changes, so it is not a pure dependency baseline.
* **C0u**: C0 with the same measurement-only counters.
* **Cs**: C0's source compiled against **stock egui 0.37 `841a328b`** (and stock
  craft-ui API port) where it compiles; if a fork-only API is already used, use the
  same operation in a pinned minimal adapter/harness and explain the semantic
  difference. Record B0/Cs/C0 source and lock identities.
* **C1..Cn**: each later stage, compared with C0 for that stage.

Do not compare B0 directly with S4 as if only the UI changed. Use B0 vs B0e for the
non-UI delta, C0 vs Cs for fork-vs-stock, and B0 vs C0 for the total compatibility
delta. `ui.inspect` does not expose `cumulative_frame_nr`; C0u/B0u add the counter in
the measurement worktree, and S3 may add bounded production fields. `cpu_usage` is
available only from the host `eframe::Frame`, so the host records it; `CadApp::logic`
does not receive a `Frame`.

### 14.3 Matched operations and sample counts

Use the suite minimums: **≥10 warmups**, **≥30 steady samples** for discrete
operations, **≥20 fresh startup processes**, and **≥1,000 frames** before reporting a
frame p99. Interleave baseline/candidate (ABAB) on the same machine and lock; report raw
samples, median, p95, p99 (only with ≥1,000 frames), peak RSS, allocations/texture
uploads and idle CPU/repaint count. Exclude the first run after a cold shader/font cache;
record cold vs warm separately.

1. **Engine control**: `cadcraft-cli perf 200000` and
   `cadcraft-cli perf examples/apartment.dxf`; repeat the single-shot command in a
   driver script (the CLI has no repeat flag), ≥10 warmups + ≥30 samples, parse the
   timing table.
2. **Idle**: open the sample, no input, 60 s; frames from the measurement counter,
   process CPU (`/proc/<pid>/stat`, `Get-Process` CPU), peak working set; animations on
   and off; ≥1,000 frames.
3. **Pan/zoom**: 200 view steps across five zoom bands via `cmdline.input` ZOOM/PAN or
   `ui.drag` (there is no `view.set`), a redraw each; `frameMs`, `buildMs`, `drawMs`,
   `meshMs`, `prims`; ≥1,000 frames.
4. **Selection/properties**: 10,000 synthetic entities, open Properties, pan, re-select;
   `drawMs`/`frameMs`; include the `#8763`/`#8764` selection action.
5. **Command line/menus**: 200 commands and 50 menu opens; frame time and idle repaint.
6. **Dialogs**: open/close Layers, Options, Plot, MTEXT 20 times; frame time and
   allocation/upload counters; ≥1,000 frames for the p99 claim.
7. **Save/reopen**: open `examples/apartment.dxf`, edit, save to temp, reopen; wall
   time and `drawMs`.
8. **#8731 text layout/tessellation**: 120 ASCII labels + 120 rounded-rect controls
   adapted to CADCraft's chrome; measure tessellation CPU/instruction cost per frame on
   B0/Cs/C0; test mixed/color rows and keep color-emoji/wrap semantics.
9. **#8736 Windows startup**: fresh process, poll the control port until `ping`
   succeeds; renderer construction separate from first frame; ≥20 fresh processes on
   B0/Cs/C0. Record the absolute +ms for FXC.
10. **#8512 fonts**: measure CADCraft startup with `system_font_fallback=false` and
    record that no system-font discovery is performed; compare a stock-eframe default
    only as a separate row. Do not enable `system_fonts` to “fix” a number.

### 14.4 Thresholds and noise policy

Predefine and freeze before candidate measurement:

* investigate a repeatable **>5% median** or **>10% p95** latency regression;
* investigate **>5% peak-memory** increase and any new idle busy-loop/audio underrun;
* for sub-millisecond measurements require the relative increase to exceed **0.05 ms**
  or corroborate with stable instruction/allocation counts;
* app-specific gates may be stricter, never looser. CADCraft engine operations
  ≤ +5% median and UI `frameMs`/`cpuMs` p95 ≤ +10% are the floor. Startup uses the same
  median/p95 rule; **no “+100 ms” allowance** hides the ~+40 ms FXC cost. If +40 ms is
  accepted, it is a documented tradeoff with owner review, not a threshold change.
* Same Rust/toolchain, `release`, features/fonts/fixture, GPU/driver/DPI/window size;
  no concurrent builds/noisy workload; rerun noisy comparisons; never expand a
  tolerance after seeing a failure; never report compile duration as UI performance.
* Screenshots use the §13.3 pixel policy; a 0.37 style change (#8617/#8445) is documented
  with a before/after crop before acceptance.

### 14.5 Upstream-reported issues (reported, not measured here)

* #8731: plain-text tessellation ≈+60% in 0.37, color-glyph branch in
  `Tessellator::tessellate_text`; measure operation 8 on B0/Cs/C0; a row-level fast path
  is a fork decision, not a CADCraft workaround.
* #8736: `egui-wgpu` 0.37 `wrap_texel_coord` makes the DX12/FXC pipeline ~4× slower to
  compile, ~+40 ms startup; CADCraft defaults to FXC. Measure operation 9; do not
  change the FXC choice to hide it. Clamp-only is not a valid Repeat/MirroredRepeat fix.
* #8512: system-font startup cost; CADCraft disables the provider, so record absence of
  that cost rather than adding it.
* #8595: repaint-burst fix may reduce idle CPU; measure operation 2.
* #8528: cross-viewport galley cache; CADCraft has one viewport, not applicable.
* #8763/unmerged #8764: selection loss before a clicked widget. Test the MTEXT toolbar
  and dialog actions with pointer and keyboard; the fork owns any selected fix, and an
  app workaround must preserve the range and undo semantics with a removal condition.
* #8748: **closed and unmerged**; not behavior in the pin. Test root hidden/logic-only
  behavior; a deferred-child overdue full pass exists only if a later fork pin adopts
  it, with that exact revision and native evidence. Do not infer phase from `visible()`.

## 15. Documentation updates

* `docs/ui-parity.md`: Accessibility row — AccessKit roles/names/language tested in
  kittest; native screen-reader coverage still not established.
* `docs/localization-parity.md`: Arabic row — shaping/RTL status, whether the UI is
  mirrored, glyph coverage; add a revision-history line.
* `docs/control-protocol.md`: new `ui.inspect.perf` fields (frame/pass counters) and
  unchanged screenshot client semantics; note screenshot admission/timeout errors.
* `README.md`: UI builds require Rust 1.95; keep the core 1.90 minimum.
* No `ATTRIBUTION.md`, contributor or asset changes are part of this migration. Keep
  `@skylartaylor` username-only credit.

## 16. Risks, blocking unknowns and acceptance

Blocking before calling the migration complete:

1. **Final pins/publication reachability**: only the approved final `storytold/egui`,
   `storytold/winit` and `storytold/craft-libs` revisions are production pins. The
   aiongg candidates are comparison inputs. The final fork/tag must replace the fork's
   own moving winit branch patch.
2. **W15P/E4 PRIMARY**: the final winit rev must include W15P and the final egui rev
   E4/P2b, with native wlroots/KWin/Mutter/X11 evidence. A PRIMARY-only fallback is a
   measured, explicitly accepted exception.
3. **E21b/P7b IME**: the final egui rev must include the bounded outgoing surrounding
   text producer/adapter; otherwise IME is not complete and the limitation is explicit.
4. **craft-libs S1**: port only `8614e4a` hunks onto `49190884`; never merge
   `b68de8c`/`ec8840a`; preserve `c67b1e1`/`4f85efb`/`craft-platform` and README.
5. **Toolchain**: egui 0.37 requires Rust 1.95; apps must state it. The server's
   default `rustc` is 1.99.0 and 1.95.0/1.96.1/1.97.1 are installed; `trunk`, a browser
   and a compositor/Xvfb are absent.
6. **Display/renderer**: no inherited display; parent-visible i915 renderD128 is
   unqualified and may be hidden from a worker; Windows is software D3D12. Native focus,
   IME, picker and real-GPU performance remain unresolved until managed evidence.
7. **Web**: `cargo check -p cadcraft-web` and a wasm graph gate are required; `trunk`
   build and a real browser run are blocking unknowns.
8. **Windows/macOS native**: Basic Render Driver only; focus/IME/picker/traffic-light
   layout and real-GPU behavior unqualified; macOS hardware unavailable.
9. **Fonts**: monochrome emoji changes `⌘`; the `⇧` decision may change macOS labels;
   pinned fixtures are external and must keep their licences. `can_show` gating remains.
10. **RTL**: `Style::right_to_left` alone does not mirror CADCraft chrome; use craft-ui
    `DockArea::direction` only with app-owned mirroring; do not claim an RTL UI earlier.
11. **Screenshot on web**: `now_ms` is 0 on wasm; the new slot deadline must use a web
    clock, and a browser screenshot remains a separate gate.
12. **Oversized surface**: no environment can produce a window larger than
    `max_texture_dimension_2d`; E2+E3 is covered by the fork unit test only.
13. **Review/evidence**: a fresh independent adversarial review of this exact document
    SHA256 is required before implementation; each implementation stage needs a fresh
    adversarial review at its exact candidate revision, with regressions and measured
    before/after performance. The baseline is captured before production edits.

Acceptance criteria (all evidenced, not inferred): S2 compiles with one egui family and
one winit; `cargo xtask ci` passes with strict kittest names/glyphs; native and wasm
graph assertions pass; S3 lifecycle/screenshot/bounds tests and mutation probes pass;
S4 language/AccessKit/Arabic tests pass; S5a PRIMARY is evidenced or recorded
as an explicit fallback; before/after screenshots and performance samples meet §14.4 or
have a reviewed explanation; remaining platform gates are listed with the exact
unverified behaviors.

## References (primary sources inspected)

* egui 0.37.0 changelog (`CHANGELOG.md` at `841a328b`) and crate sources:
  `Context::run_ui`/`run_logic`/`end_pass`/`current_pass_index`, `PanelState`/`Panel::id`,
  `ScreenshotCallback`, `MouseWheelSource`, `Role`, `Id`/`AsId`/`AsIdSalt`,
  `FontDefinitions`, `MissingGlyphPolicy`, kittest `builder.rs`/`accessibility.rs`.
* Native/web eframe integrations: `native/epi_integration.rs`, `native/run.rs`,
  `native/wgpu_integration.rs`, `web/app_runner.rs` at `a4f9a17f` and `767b1cad`.
* Candidate fork `PATCHES.md` and commits `0.37.0..a4f9a17f`; winit plan W15P and
  `craft/0.30.13-lang`; craft-libs `49190884`, `8614e4a`, `b68de8c`, `ec8840a`.
* `/home/codex/work/ssd2/task-evidence/egui037-migration-20261022/upstream-discussions.json`
  (#8731, #8736, #8763, #8748, #8535, #8489, #8555, #8583, #8512, #8428, #8657,
  #8577, #8641), `lifecycle-source-audit.md`, `ime-surrounding-gap.md`,
  `graphics-device-inspection.json`, and `craft-libs/docs/egui-037-suite-rollout.md`.
  These are reported findings/history, not fresh measurements.

## Appendix A — source anchors

Manifests: `Cargo.toml:6-11,52-59`; `crates/ui-egui/Cargo.toml:7,20-32`;
`apps/cadcraft/Cargo.toml:19,29`; `apps/cadcraft-web/Cargo.toml:20,26-32`.

App shell: `apps/cadcraft/src/main.rs:67-110` (App impl), `:185,188` (app id/stencil),
`:228-307` (options/run), `:343-348` (set_wgpu), `:301-306` (XWayland).
`apps/cadcraft-web/src/main.rs:31-58` (Shell), `:108-130` (web renderer/stencil),
`apps/cadcraft-web/src/upload.rs:70-77` (`take_dropped`).
`apps/cadcraft/src/workspace.rs:149-170` (save_if_changed/save_on_exit).

UI lifecycle: `crates/ui-egui/src/lib.rs:195-267` (CadApp fields), `:444-455`
(finish_opening), `:536-598` (logic), `:601-625` (raw_input_hook), `:627-662`
(ui/draw), `:664-687` (drain_control), `:688-740` (screenshots), `:745-755` (now_ms).
`crates/ui-egui/src/opening.rs:77-97` (spinner Area).

UI structure: `chrome.rs:69,180,300` (panels), `menus.rs:569-589` (menu bar),
`docking.rs:74-130,318-399` (Workspace/DockArea), `palettes.rs:330,346` (panels),
`theme.rs:314-387` (fonts), `:640-647` (can_show), `:697-713` (theme apply),
`animation.rs:75-130` (wheel/unsmooth), `icons.rs:948-989` (buttons/roles),
`cmdline.rs:56-74` (Tab), `:251-330` (keyboard), `clipboard.rs:29-60` (mirror/paste),
`mtext_editor.rs:146-164,217-224,420-455` (formatting toolbar/selection).

GPU/control: `gpu.rs:53-65` (surface config), `:681-780` (install/callback),
`control.rs:195-215` (methods), `:255-272` (inspect), `:547-559` (save_screenshot),
`tests/gpu_viewport_stencil.rs`, `tests/gpu_faults.rs`.

Tests: 43 files under `crates/ui-egui/tests`; see §3.7; `tests/chrome_geometry.rs:58`
uses 1200×760/900×560; `tests/hidden_command_line.rs:19-30` is the `run_ui` helper shape.

## Appendix B — commands proposed for the parent to run (not run here)

All build/test commands must go through `maintenance/task.py run/status/finish` with one
unique task and evidence path, e.g.:

```sh
python3 /home/codex/work/maintenance/task.py run \
  --job-id egui037-cadcraft-ci-01 --kind build --task egui037-cadcraft \
  --source /home/codex/work/CraftApps/egui037-migration-20261022/cadcraft \
  --evidence /home/codex/work/ssd2/task-evidence/egui037-migration-20261022/cadcraft \
  -- bash -c 'cargo xtask ci'
```

and equivalents for:

```sh
cargo check --workspace --all-targets --locked
cargo test -p cadcraft-ui-egui --all-targets
cargo check -p cadcraft-web --target wasm32-unknown-unknown
(cd apps/cadcraft-web && trunk build --release)
cargo tree -d
cargo tree -i egui
cargo tree --target wasm32-unknown-unknown -d
cargo tree -i egui --target wasm32-unknown-unknown
cargo tree -e features --target wasm32-unknown-unknown -p cadcraft-web
cargo metadata --format-version 1
cargo run -p cadcraft-cli -- perf 200000
cargo run -p cadcraft-cli -- perf examples/apartment.dxf
```

Use the craft-windows clone helper for the Windows stage, a fresh target/evidence
directory, `guest-exec.py`/build equivalents appropriate to CADCraft, and the existing
`task.py`/`vm.py` sequence. Do not reuse the baseline's built binary as new-source
evidence, and do not run these commands in this planning task.


## Local implementation update — 2026-10-23

The user clarified that reviewed plans remain local and implementation proceeds before publication. The earlier requirement to publish/land this plan first is superseded; pushes and PR publication still require explicit approval.

Canonical `storytold/cadcraft` `main` now resolves to `f043540d13dac03ce895a96c78a8349f3e40bd5b`. The owned checkout was fast-forwarded to this revision. Earlier baseline SHAs and line anchors above are retained as historical review evidence; use this new revision and its exact lockfile for current-main integration and paired measurements. The original refs remain reachable.

Preserve browser page zoom; test canvas pinch routing because this page does not add DesignCraft touch-action:none.

The complete static delta review and exact old/new object identities are retained in the task evidence: `canonical-diffs-refresh-20261023.json` and `overnight-consumer-review-20261023.md`. This amendment claims no new build, native GUI or performance acceptance.

## Local implementation update — 2026-10-24 (upstream review v9 resolutions)

The v9 source review found no high-severity migration regression in the native UI
library/integration gate and listed four conservative-behaviour notes plus three
strict-glyph/accessibility gaps. This update records how the concrete findings are
resolved without widening the migration contract:

* **Conservative capture identity stays.** `SurfaceGeometry` equality keeps
  `window_focused`, `minimized` and `occluded` alongside viewport, pixels-per-point
  and texture limits. Any observed focus/visibility transition can change the
  presented frame, so an admitted capture is rejected rather than re-used; the
  struct's documentation now states this intent instead of narrowing the epoch.
* **No-op `document.switch` stays strict.** `run_switch` bumps the document epoch
  even when the requested index is already active, because the command cancels the
  active command and an external client cannot prove the switch was unobservable.
  The source records that the conservative bump is deliberate.
* **Invalid pass geometry invalidates the retained surface.** `note_pass_geometry`
  now mirrors `note_raw_geometry`: a pass that cannot report a finite positive
  pixels-per-point clears `capture_geometry` and bumps both epochs. Previously the
  previous surface survived and a later admission could bind to stale geometry.
* **MTEXT Diameter is painted, not shaped.** U+2300 is absent from egui's bundled
  Proportional/Monospace/emoji faces and from the web font set. The toolbar button
  now paints CADCraft's own circle-and-slash glyph and carries the semantic name
  "Diameter"; the document text still uses U+2300, which the drafting font covers.
* **Affected controls are named.** The MTEXT height `DragValue` gets the AccessKit
  name "Text height" (its tooltip alone is not a name), and QuickCalc's conversion
  `Value` field is associated with its visible label and a meaningful placeholder
  instead of the unexplained "1".
* **Strict bundled-font regressions.** Focused kittest regressions render the
  Diameter button, MTEXT height field and QuickCalc value row with bundled
  `FontDefinitions` only (no `theme::install_fonts` system prepend), the default
  `MissingGlyphPolicy::Panic` and accessibility checks. No global tofu policy or
  accessibility opt-out is introduced.

The first macOS native-menu poll now refreshes enablement immediately by
initializing that timer to `f64::NEG_INFINITY`; `now_ms` starts near zero, so the
former `0.0` sentinel suppressed the first ~300 ms of enablement.

This amendment is source-only: it does not claim a cargo, fmt, clippy, test, wasm,
Windows/macOS or GUI result. Publication still requires fresh explicit approval
after the parent's own validation.

### Full MTEXT toolbar font regression

The bundled-font check also covers the entire formatting toolbar: alignment, list and tab
marks are drawn with original geometry, and the color button relies on egui’s menu affordance
instead of an uncovered arrow character. The font and color controls publish semantic names.
The whole-toolbar harness retains strict missing-glyph and accessible-name checks, so a system
font cannot hide an unsupported toolbar character. Native visual review remains required.


## Native MTEXT insertion follow-up (2026-10-24)

Native Wayland job29 exercised real keyboard circle creation and MTEXT entry, the
Diameter button, callback screenshot, save, normal quit, restart and reopen. The
new entities and exact `Migration 0.37 ⌀` text survive DXF reopening. The editor
showed tofu for U+2300 even though its toolbar icon was already painted correctly.
A bounded original circle/slash `GlyphRasterizer` now supplies only U+2300 when no
installed font covers it; it keeps the stored Unicode character and cursor indexes
unchanged and works without optional font files on native/web. Font setup installs
it idempotently. A real toolbar-click/TextEdit test uses the strict bundled-font
harness and checks inserted contents and caret. Job30 validates this follow-up;
fresh native visual confirmation and review remain pending.
