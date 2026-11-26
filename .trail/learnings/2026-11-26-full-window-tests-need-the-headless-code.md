---
title: Full-window tests need the headless UI runtime
author: code-compliant
date: 2026-11-26
branch: feat/2626-double-click-attribute-editor
id: l-60836e0f
scope: repo
status: active
src: agent
touches:
  - crates/ui-egui/tests/attedit_dialog.rs
  - crates/ui-egui/tests/it/block_attribute_double_click.rs
  - crates/ui-egui/src/framework.rs
---

## Decided

- Install the headless craft-ui runtime once before a test context renders the full CADCraft window. The dock header needs app command state from the runtime.

## Tried and ruled out

- A standalone branch test run passed with the old runtime. CI built the merge into main with the new runtime, where an uninitialised context panicked at `dock.float_panel`. Repeating CI with the same merge would use the same context setup.

## Watch out

- Check direct `egui::Context` calls as well as the harness builder. CI can stop after the first failures, before it reaches another file with the same setup problem.
- The runtime holds events after a pointer press for the next frame. The selection-window test draws only the canvas so its press and release arrive together.

In the code:

- `crates/ui-egui/tests/attedit_dialog.rs` and `crates/ui-egui/tests/it/block_attribute_double_click.rs` call `framework::install_headless`. The helper is in `crates/ui-egui/src/framework.rs`.
