# Drawing sheets A4, A3, A2

Scripts (`.scr`) that draw an ISO 5457-style sheet in millimetres: the sheet edge, a frame
with a 20 mm binding margin on the left and 10 mm elsewhere, and a 180x40 mm title block
(Tytuł rysunku, Wykonał, Format, Skala, Data) in the bottom-right corner.

| File | Sheet |
|---|---|
| `arkusz-A4.scr` | A4 portrait, 210x297 |
| `arkusz-A3.scr` | A3 landscape, 420x297 |
| `arkusz-A2.scr` | A2 landscape, 594x420 |

The `.dxf` files are the scripts' output, ready to open.

Run in the app with `SCRIPT`, or headless:

    cadcraft-cli run --metric --script-file examples/sheets/arkusz-A3.scr --save drawing.dxf

Each `TEXT` line is followed by an empty line, which ends the text.
