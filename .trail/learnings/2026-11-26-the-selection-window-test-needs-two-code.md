---
title: The selection window test needs two pointer events in one frame
author: code-compliant
date: 2026-11-26
branch: feat/2626-double-click-attribute-editor
id: l-c26f87d2
scope: repo
status: active
src: agent
touches:
  - crates/ui-egui/tests/it/block_attribute_double_click.rs
  - crates/ui-egui/src/canvas.rs
---

## Decided

- The test sends the second press and release in one `egui::RawInput` frame. The canvas checks for a double-click before it handles pointer presses.

## Tried and ruled out

- `egui_kittest::Harness::event` queues events, but `Harness::step` runs one frame per event. The second press then clears the selection window before the editor opens. The test passed with the bug present.

## Watch out

- Clear `FullOutput::textures_delta` when the headless test discards the frame output. Egui checks that the test handles or clears the texture changes.

In the code:

- `crates/ui-egui/tests/it/block_attribute_double_click.rs` sends both events to `egui::Context::run_ui`.
- `crates/ui-egui/src/canvas.rs` returns early when a double-click opens the editor.
