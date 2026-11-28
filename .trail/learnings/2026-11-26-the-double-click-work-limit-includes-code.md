---
title: The double-click work limit includes the ordinary pick check
author: code-compliant
date: 2026-11-26
branch: feat/2626-double-click-attribute-editor
id: l-66adde2f
scope: repo
status: active
src: agent
touches:
  - crates/render/src/attrib_pick.rs
---

## Decided

- Use one work budget for the attribute editor hit test. The check for an editable ordinary pick spends from that same budget.

## Tried and ruled out

- An ordinary pick check outside the budget could read every constant attribute in a reference that cannot open the editor. This would remove the work limit from that click path.

## Watch out

- A click can use up the budget before the check finds an editable attribute on the ordinary pick. In that case the editor stays closed. A confirmed editable pick stays available after the later text search uses up the budget.

In the code:

- `crates/render/src/attrib_pick.rs` charges `AttribPick::editable` before it reads the reference or an attribute. `attribute_reference_at` keeps the confirmed pick as a fallback.
