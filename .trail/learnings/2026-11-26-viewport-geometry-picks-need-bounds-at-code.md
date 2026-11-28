---
title: Viewport geometry picks need bounds at the same scale
author: code-compliant
date: 2026-11-26
branch: feat/2626-double-click-attribute-editor
id: l-01a0016f
scope: repo
status: active
src: agent
touches:
  - crates/engine/src/select.rs
  - crates/engine/src/spatial.rs
  - crates/ui-egui/src/canvas/pick.rs
---

## Decided

- The attribute editor's double-click checks geometry and candidate bounds at the same viewport annotation scale. The ordinary pick keeps the drawing annotation context.

## Tried and ruled out

- Filtering the ordinary pick after it returns would remove a hidden reference. It could never find a visible reference already excluded by bounds at the drawing annotation scale. A hidden reference could also prevent a text hit on a reference below it in draw order.

## Watch out

- Make the viewport scale and ANNOALLVISIBLE part of the bounds cache key. The ordinary pick and the viewport pick can run on the same unchanged model space.
- Clear the annotation scales on the projected entity before measuring or rendering it. Otherwise the drawing scale can hide or scale it again.

In the code:

- `crates/engine/src/select.rs` shares `entity_for_pick` with `crates/engine/src/spatial.rs`. `crates/ui-egui/src/canvas/pick.rs` asks for the viewport-scale pick only for the attribute editor's double-click in MSPACE.
