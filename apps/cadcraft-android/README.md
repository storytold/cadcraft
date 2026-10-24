# Cadcraft on Android

Cadcraft on Android: a small `apps/` crate that hosts the existing `cadcraft_ui_egui` interface in an
`android-activity` (GameActivity) window, on top of `cadcraft_engine`. The desktop entry crate is not
compiled, so `rfd`, `arboard` and the macOS/Windows/Linux platform code never enter the dependency
graph.

## What the shell does

- **Entry point** — builds as `libmain.so` (`[lib] name = "main"`, `crate-type = ["cdylib"]`), loaded
  with `System.loadLibrary`; `android_main` is the usual `android-activity` entry.
- **Storage** — `AndroidApp::internal_data_path()` is the data directory (`dirs` answers nothing on Android). Cadcraft's `ui.json`/`prefs.json` pair is *not* loaded or saved yet, so UI state starts from the defaults each launch.
- **File pickers** — Cadcraft's `Services` hooks are filled from the Storage Access Framework. `pick_open`
  starts `ACTION_OPEN_DOCUMENT` through the Activity (six JNI calls in `src/saf.rs`), the
  picked drawing is copied into `<filesDir>/import/`, and its real path is sent back over
  the app's own control channel, so the *desktop* `app.open` command does the opening.
  `pick_save` answers with a scratch path in `<filesDir>/export/`, then a worker shows
  `ACTION_CREATE_DOCUMENT` and copies the finished file out once its size has been stable
  for 1.5 s.
- **Fonts** — Android does not expose the Unix font directories, so the CJK face from `/system/fonts`
  is appended to every family *after* Cadcraft installs its theme (any earlier, and the theme's named
  families would be dropped).
- **Touch** — hit targets are raised to 40 pt, sliders widened and hover tooltips suppressed, applied
  after the theme's style exists. The layout is not zoomed: it is designed for a desktop window and a
  phone gives ~411 pt, so the next step is a touch rework, not a scale factor.
  `crates/ui-egui/src/menus.rs` also grows its menu bar with the style's `interact_size`:
  the desktop keeps the 22 pt row, but a touch style's 40 pt row would clip the labels.

## Known gaps

- Printing has no backend, so the Print dialog is unreachable rather than broken.
- Documents open from the app's own import copy; "save back to the original" is not wired.
- There is no folder picker (`ACTION_OPEN_DOCUMENT_TREE` returns a tree the engine cannot
  write to by path).

- The system clipboard is not wired (`arboard` has no Android backend); Cadcraft's own clipboard is
  still used.
- The launcher icons are the Android Studio template's placeholder art.

## Build

```bash
cargo ndk -t arm64-v8a -o apps/cadcraft-android/jniLibs build --release -p cadcraft-android
cd apps/cadcraft-android/android && ./gradlew assembleRelease
```

`jniLibs/` and the Gradle output are gitignored; the APK is not part of this change.
