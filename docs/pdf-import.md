# Import a PDF as editable CAD geometry

Choose **File > Import PDF...**, select the PDF, then choose a page and import options.
The selected page becomes editable objects in the current drawing. Use **File > Save As...**
to save the result as DXF or DWG. The source PDF is not changed.

The command is `PDFIMPORT` (alias `PDFI`). It is also available to the CLI and automation:

```text
pdfimport {"path":"plan.pdf","page":2,"scale":50}
```

Page numbers start at 1. The scale multiplies the printed page size. For a drawing printed
at 1:50, use 50 to restore model size, then check a known dimension. The import converts
PDF points to the current drawing's units. A page containing viewports at different scales
needs separate calibration for each viewport. An `at` point sets the insertion offset.

Automation can pass base64 `data` with `name: "plan.pdf"` instead of a file path. The JSON
options include `page`, `password`, `at`, `scale`, `text`, `fills`, and `lineweights`.
File access still follows the CLI/MCP automation roots.

To convert the first page into a new drawing from a terminal:

```sh
cadcraft-cli convert plan.pdf plan.dxf
cadcraft-cli convert plan.pdf plan.dwg
```

For another page, create a drawing, run `pdfimport` with its page number, then export it.
The same reader handles PDFs opened as new drawings. Save the drawing under a DXF or DWG
name to keep the editable objects.

## What is converted

- Stroked paths become lines and polylines. Recognized circular curves retain arc segments;
  other curves become short line segments.
- Solid fills become hatches. PDF fill rules determine holes and overlapping contours.
  A nonzero-rule fill whose contours overlap or nest in the same direction is split into
  triangles so the filled area matches the PDF. That hatch has one boundary loop per
  triangle and its curved edges become short straight segments.
- Recoverable text becomes editable text with the drawing's Standard text style. Its font
  appearance can differ from the PDF. Text already converted to outlines remains geometry.
- Colours and lineweights are mapped to CAD properties. Objects are grouped into
  `PDF_Geometry`, `PDF_Solid Fills`, and `PDF_Text`, rather than original CAD layers.
- Page rotation, crop origin and PDF user units are applied. Rectangular clipping is applied.

The import reports omissions and approximations in its result and command history.
Scanned images are not traced and OCR is not provided. Raster images, unsupported fonts,
gradient and pattern paints, and white background fills can be omitted. Nonrectangular
clips, transparency, blend modes, dash patterns and some curves crossing clips can differ
from the PDF. Check the imported drawing against the source before using its dimensions.

A PDF is a plotted page. It does not normally contain the original blocks, constraints,
associative dimensions, unprinted entities or 3D model. Those cannot be restored by import.

## Implementation and provenance

The application maps the shared `craft-pdf` interpreter output into `cadcraft-doc` entities.
The engine command is independent of the GUI and imports a page as one undoable action.
Input bytes, pages, entities, vertices and text are bounded. A truncated interpreter result
fails the import rather than claiming a complete conversion.

This adapts the existing CADCraft work in [PR #2787](https://github.com/storytold/cadcraft/pull/2787),
commit `ec17f63f189d1dccf91357701f9bba243ca88dd5`. Attribution and dependency asset notices
are recorded in `ATTRIBUTION.md` and `NOTICE`.
