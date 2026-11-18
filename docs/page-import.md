# Importing a title-block page

On native builds, drop a `.pdf` or `.docx` into the drawing window, or run
`import` with `{"path":"page.pdf"}`. Only the first page is imported.

The page becomes one block containing filled CAD geometry, including outlined
lettering. Saving to DXF or DWG carries that geometry in the drawing; the source
page and converter executables are not needed to display the saved block.
This is a static rendition, not an editable Word object or PDF underlay.

Select the page and drag a blue corner handle to resize it. The opposite corner
stays fixed and the aspect ratio is preserved. Release commits one undoable edit;
Escape cancels the drag. MOVE fades the original page while positioning its preview.

## Native converters

PDF import needs `mutool` (MuPDF) on PATH. DOCX import additionally needs `soffice`
(LibreOffice). `CADCRAFT_MUTOOL` and `CADCRAFT_SOFFICE` can select executable paths.
The converters are separate programs, not bundled or linked into CADCraft.

DOCX layout is produced by LibreOffice and may differ from Microsoft Word,
particularly when fonts are missing. To preserve Word's own layout, export a PDF
from Word and import that PDF instead.

The converter processes currently run synchronously, with a 45-second timeout per
process. Import can block the interface while they run. Source input and rendered
SVG are limited to 32 MiB; converter disk and memory use is not sandboxed or capped.
Use trusted documents while this prototype is reviewed.

Only supported solid-color paths, rectangular clips and embedded bitmaps can be
converted. Unsupported compositing, gradients and patterns may be refused. Raster
content retains its original pixel detail; converting it to rectangles does not
increase its resolution. Outlined text can produce many entities and large files.

The page dimensions are converted from millimetres for drawings using inches,
feet, millimetres, centimetres or metres; other drawing units use one unit per mm.
Explicit `at` and `scale` import parameters override placement and scale.

`oleconvert` can also replace a selected OLE frame containing a supported cached
Word metafile presentation. It reads the cached presentation without running an
OLE server. The original Word document cannot be edited through the converted block.
