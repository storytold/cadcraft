---
title: Open and save DXF and DWG drawings
description: Open DXF and DWG files in CADCraft, save them in either format and version, and pick the default format for new drawings.
order: 20
verified: 0.6.2
---

CADCraft opens DXF and DWG drawings and saves both, in versions 2000 through 2018. New drawings are saved as DXF 2018 unless you change the default. On macOS, use Cmd in place of Ctrl.

## Open a drawing

1. Choose **File ▸ Open...**. The shortcut is Ctrl+O.
2. In the file dialog, pick a `.dwg` or `.dxf` file. The first file type in the list is **Drawings (DWG, DXF)**. It also takes `.dwt` templates.
3. Click Open. The drawing opens.

CADCraft reads ASCII DXF from R12 through 2018, binary DXF from R13 through 2018, and DWG from R13 through 2018. DXF files older than 2007 that use another code page, such as Chinese, Japanese, Cyrillic or Central European text, open with their text intact.

The same dialog opens SVG and SVGZ files (**SVG drawings (SVG, SVGZ)**). CADCraft turns their shapes, paths, groups and Inkscape layers into drawing objects. Fills, text and images in an SVG are not read.

## Save a drawing

1. Choose **File ▸ Save** to save over the file you opened. The shortcut is Ctrl+S. A new drawing has no file yet, so this asks where to save.
2. To save under a new name or in another format, choose **File ▸ Save As...**. The shortcut is Ctrl+Shift+S.
3. In the dialog, pick the file type: **Drawing (DXF)**, **Drawing (DWG)** or **Drawing Template (DWT)**. The dialog also offers **SVG**, **PNG** and **PDF**, which export an image or PDF instead: see [Export a drawing to PDF, SVG or PNG](export-pdf-svg-png.md). A name ending in `.dwg` or `.dxf` selects that type first.

A drawing you opened from a file keeps that file's format and version when you save it. Files older than 2000 are saved as 2000. A binary DXF stays binary. DWG files are saved as 2004 or later, even when the file you opened was a 2000 DWG.

## Choose the default format and version for new drawings

1. Choose **File ▸ Default Save Format**.
2. Pick **DXF** or **DWG**.
3. To save new drawings for older CAD programs, choose **File ▸ Default Save Format** again and pick a version, from **Version 2018** down to **Version 2000**.

The same settings are in **Tools ▸ Options...** on the **Open and Save** tab, as **Save new drawings as** and **Default save version**.

## What to know before you trade files with other CAD programs

CADCraft does not model every kind of object yet. When you save as DXF, objects it does not model, such as 3D solids, are written back as they were read. When you save as DWG, some of them are still lost. The [file format notes](https://github.com/storytold/cadcraft/blob/v0.6.2/docs/file-format-parity.md) list what is kept and what is lost. Keep a copy of the original file until you have checked the saved one.

## Related

- [Export a drawing to PDF, SVG or PNG](export-pdf-svg-png.md)
- [FAQ: Which file formats does CADCraft support?](../faq/file-formats.md)
- [After a crash](../troubleshooting/index.md#after-a-crash)
