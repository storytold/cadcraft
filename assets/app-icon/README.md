# App icons

The stable icon is `cadcraft-1024.png` (the padded master). The packaging scripts derive the other
sizes from it:

| File | Where it goes |
|---|---|
| `cadcraft.ico` | `cadcraft.exe` resources and the MSI shortcuts (Windows) |
| `cadcraft.icns` | `CADCraft.app/Contents/Resources` (macOS) |
| `cadcraft-256.png` | the window and About icon on Linux, and the AppImage/menu entry |
| `hicolor/<size>/apps/ai.storyteller.cadcraft.png` | the freedesktop icon theme, so docks and menus find the app by its id |

## Nightly

The Nightly channel has its own icon, so a nightly window, taskbar button and bundle are not
mistaken for the stable app. It is the same mark — foreground unchanged, no badge — on a deeper
background, derived from the stable 1024 master (never an upscale):

| File | Where it goes |
|---|---|
| `cadcraft-nightly-1024.png` | the master the `.icns` is generated from |
| `cadcraft-nightly.png` | the Linux nightly tar (`cadcraft-nightly.png` beside the executable) |
| `cadcraft-nightly.ico` | `cadcraft.exe` of a nightly build (`build.rs`) |
| `cadcraft-nightly.icns` | `CADCraft Nightly.app/Contents/Resources`, `CFBundleIconFile cadcraft-nightly` |

`nightly-provenance.json` records the exact transform (which pixels changed, and from which
colour), so the nightly icon can be regenerated or reviewed against the stable one. Regenerating
either set is a deliberate change: check the result at 16 px before replacing a file.
