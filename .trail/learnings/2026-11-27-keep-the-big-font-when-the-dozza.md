---
title: Keep the Big Font when the normal font answer ends in a comma
author: Dozza
date: 2026-11-27
branch: code-compliant/text-follows-style-1089-noreply
id: l-1c2945c9
scope: repo
status: active
src: agent
touches:
  - crates/engine/src/cmd/textstyle_cli.rs
  - crates/engine/tests/textstyle_cli.rs
---

## Decided
Keep the existing Big Font when a normal font answer ends in a comma. The public command reference distinguishes this from a normal font answer without a comma.

## Tried and ruled out
A review suggestion said to clear the Big Font for a trailing comma. The public reference says that form changes only the normal font, so clearing would lose an existing setting.

## Watch out
A normal font answer without a comma clears the Big Font; Enter keeps both fonts. The command listing also ends the command, rather than asking for another style name.

In the code: crates/engine/src/cmd/textstyle_cli.rs and crates/engine/tests/textstyle_cli.rs. Public behavior references are cited in the command module.
