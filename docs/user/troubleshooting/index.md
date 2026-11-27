---
title: Troubleshooting
description: Fixes for problems with starting CADCraft, and how to report a bug.
order: 30
verified: 0.6.2
---

## Known problems

- [The app closes right after opening on Windows](app-closes-on-windows.md)
- [A screen magnifier does not follow the crosshair](magnifier-does-not-follow-crosshair.md)

## Report a problem

If your problem is not listed, tell us in [Discord](https://discord.gg/artcraft) or open an issue in the [issue tracker](https://github.com/storytold/cadcraft/issues). Include:

- The CADCraft version (**Help ▸ About CADCraft**) and your operating system.
- What you did, what you expected and what happened.
- The log file. The desktop app writes it to `logs/cadcraft.log` in its settings folder. On Windows that is `%APPDATA%\CADCraft\logs\cadcraft.log`. On macOS it is `~/Library/Application Support/CADCraft/logs/`. On Linux it is `~/.config/cadcraft/logs/`.
- A drawing that shows the problem, if you can share one.

After a crash, the crashed run's log is `cadcraft.1.log` once you start again. Two more starts push it out, so attach it soon.

## After a crash

The desktop app autosaves drawings with unsaved changes every 10 minutes. If it did not close normally, the next start shows **Drawing Recovery** with the autosaved drawings. Click **Open** to get one back, **Discard** to delete it, or **Later** to be asked again next time. A recovered drawing has no file name, so save it to keep it.
