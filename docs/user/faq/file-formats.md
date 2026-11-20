---
title: Which file formats does CADCraft support?
description: CADCraft opens and saves DXF and DWG drawings, opens SVG files and exports PDF, SVG and PNG.
order: 10
verified: 0.6.2
---

CADCraft opens and saves DXF and DWG. It also opens SVG files and DWT templates, and it exports PDF, SVG and PNG.

| Format | Open                                            | Save                                          |
|--------|-------------------------------------------------|-----------------------------------------------|
| DXF    | R12 through 2018 ASCII, R13 through 2018 binary | 2000 through 2018 (a binary DXF stays binary) |
| DWG    | R13 through 2018                                | 2000 through 2018                             |
| DWT    | yes                                             | yes                                           |
| SVG    | SVG and SVGZ, shapes and paths only             | export only                                   |

New drawings are saved as 2018 unless you pick another default version. See [Open and save DXF and DWG drawings](../guides/open-and-save-drawings.md).

PDF, SVG and PNG are export formats. See [Export a drawing to PDF, SVG or PNG](../guides/export-pdf-svg-png.md).

Not everything in a drawing survives a DWG save yet. DXF saves keep objects CADCraft does not model, such as 3D solids, but DWG saves lose some of them. Keep the original file until you have checked the saved one.
