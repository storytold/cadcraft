---
title: Draw your first shapes at the command line
description: Type LINE, CIRCLE, OFFSET and ZOOM at the CADCraft command line to draw and move around a drawing.
order: 10
verified: 0.6.2
---

You draw in CADCraft by typing a command at the command line and answering its prompts with typed coordinates or clicks. The command line is at the bottom of the window. If you do not see it, choose **Window ▸ Command Line** (Ctrl+9). On macOS, use Cmd in place of Ctrl.

## Steps

1. Open a drawing, for example with **Help ▸ Open Sample Drawing**.
2. Click in the command line and type `line`, then press Enter. `L` also starts the command.
3. Type `0,0` and press Enter for the first point.
4. Type `@10,0` and press Enter. The `@` means "from the last point": this goes 10 units to the right.
5. Type `@0,5` and press Enter, then type `c` and press Enter to close the shape. You now have a closed triangle.
6. Type `circle 5,2 1` and press Enter. This draws a circle with its center at 5,2 and a radius of 1.
7. Type `offset 0.25` and press Enter, then pick the circle and click on the side where the copy should go.
8. Type `zoom e` and press Enter to zoom to the extents of the drawing.

## Good to know

- To enter a distance and an angle, type `@5<45`: 5 units at 45 degrees from the last point.
- To repeat the last command, press Enter or Space. You can also right-click in the drawing and choose **Repeat** followed by the command name, such as **Repeat LINE**.
- While a command runs, a quick right-click works like Enter. Hold the right button a moment longer to get a menu with **Enter**, **Cancel** and the prompt's options.
- Prompts show options in square brackets, such as `[Keywords]`. You can click a keyword or type it.
- Typing at the command line, using a menu item and clicking a tool all run the same command.

## Related

- [Manage layers](manage-layers.md)
- [Export a drawing to PDF, SVG or PNG](export-pdf-svg-png.md)
