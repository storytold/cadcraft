# Hardware parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-15 · **Change:** minor (compact entity representation reduces retained drawing memory) · **Target:** Autodesk AutoCAD 2027 (Mac and Windows)

The hardware AutoCAD uses and how CADCraft compares, per platform. Work items:
[gaps.md](gaps.md#hardware).

**Headline (estimated): hardware ≈ 45%.** 2D GPU drawing, HiDPI and trackpads are covered on
every platform. Missing: 3D mice, tablets/digitizers, GPU-accelerated 3D (there is no 3D), and
any deliberate multi-monitor handling. Remaining: **≈ 15–25 Opus 5.5 hours**, plus a human with
the devices to test 3D mice and tablets.

Measured from the source on 2026-10-10 (`crates/ui-egui/src/gpu.rs`, `canvas.rs`,
`apps/cadcraft/src/graphics.rs`, `apps/cadcraft-web`); AutoCAD's side from Autodesk's system
requirements and graphics-performance documentation. Nothing was benchmarked in this pass.

| Feature | AutoCAD 2027 | CADCraft macOS | CADCraft Windows | CADCraft Linux / FreeBSD | CADCraft web |
|---|---|---|---|---|---|
| GPU canvas for 2D | yes (hardware acceleration, GPU smoothing, high-quality geometry) | wgpu on Metal | wgpu on DirectX 12 by default (`WGPU_BACKEND` overrides; GL avoided after AMD crashes, #31) | wgpu on Vulkan (or GL) | WebGPU, WebGL2 fallback |
| CPU fallback | software mode | yes (`tiny-skia` rasteriser) | yes | yes | — |
| GPU for 3D (shaded visual styles, ray-traced rendering) | yes | no (no 3D) | no | no | no |
| Low-latency presentation | yes | vsync or low-latency via `CADCRAFT_VSYNC` | same | no vsync queue so the crosshair keeps up (#183) | browser-controlled |
| HiDPI / Retina, display scaling | yes | yes (egui points per pixel) | yes; a Windows 11 DPI fix is queued upstream (PR #110) | yes | yes |
| Multi-monitor (palettes on other screens, per-monitor DPI) | yes | not handled deliberately; one window | same | same | — |
| Trackpad pan, pinch zoom | yes (Mac) | yes, with trackpad vs wheel detection (#58) | wheel only (precision touchpad untested) | wheel; delay reported on Linux Mint (#25) | browser events |
| Magic Mouse | yes | treated as a trackpad | — | — | — |
| Wheel mouse, middle-button pan | yes | yes | yes | yes | yes |
| 3D mouse (3Dconnexion SpaceMouse) | yes | **no** | **no** | **no** | no |
| Pen tablet / digitizer (TABLET command, calibration) | yes (Windows) | **no** | **no** | **no** | no |
| Screen magnifier following the cursor | yes | — | breaks (#39; PR #187 adds a system-cursor option) | — | — |
| Plotters and printers (PC3 devices, system printers) | yes | **no**: PDF/PNG/SVG files only; Print asks where to save the PDF (#26) | same | same | PDF bytes shown instead of a download (#134) |

## Performance on hardware (internal numbers only)

From the earlier roadmap (measured by the agents that built it, 2026-10-06, `cadcraft-cli perf`):
200,000 entities pick in 0.001 ms, about 4 ms after an edit (incremental R-tree), and the GPU
canvas draws a frame in about 4 ms. Nothing has been compared with AutoCAD on the same machine,
and 1M-entity drawings are untested. A 10 MB DWG took about two minutes to fail (#57).

Zoom and pan no longer freeze the window (2026-10-13). On a hatched 12k-entity architect's DWG
(about 340k display primitives) each zoom step used to block the UI for 2–4.5 s while the
display list, its vertex data and its GPU buffers were rebuilt. These rebuilds now run on a
worker thread while the previous list is drawn, so the longest stall is 26–64 ms (M-series Mac).
Edits still rebuild on the UI thread (about 1.8 s on that drawing): per-entity caching is the
next step.

Dense-polyline selection was measured on 2026-10-14 with a local, user-provided 40.9 MB DWG:
6,587 drawing entities (6,585 in model space), 1,630,449 model polyline vertices, and a
selected contour with 14,940 vertices.
`EntityKind::grips()` used to regenerate the entire segment vector for every vertex; both
grip drawing and grip hit-testing called it on pointer-driven frames. It now generates that
vector once, in linear time, preserving every vertex and midpoint grip and their indices.

On the same 16 GB M-series Mac, optimized development builds and Metal renderer, the same
view, selection, Properties palette and five pointer moves gave these internal UI CPU times
(`ui.inspect.perf.frameMs`, not end-to-end pointer latency or GPU presentation time):

| Selected contour | Before | After |
|---|---:|---:|
| Median frame work (5 samples) | 705.24 ms | 1.80 ms |
| Maximum frame work (5 samples) | 708.52 ms | 2.08 ms |

The display list stayed at 9,030 primitives, the selection stayed on the same object, and
the drawing remained unmodified. Synthetic tests exercise all grips of a 20,000-vertex
contour and clockwise/counterclockwise arc and closing midpoints. `cadcraft-cli perf FILE`
also reports the largest polyline's vertex/grip counts and grip-generation time for future
checks. The real DWG and screenshots are not repository fixtures.

This fixes a reproducible selection stall; it does not establish parity with AutoCAD or
prove the absence of every crash. Opening the same file no longer duplicates the DXF tag stream for sections and records.
The reader borrows slices; the existing owned DXF APIs remain available. No geometry, blocks or foreign data are removed.
Three sequential CLI imports per build on the same Mac gave these medians (`/usr/bin/time -l`,
optimized development build, before/after the borrowing change):

| DWG import | Before | After |
|---|---:|---:|
| Peak resident memory (decimal GB) | 2.40 | 1.43 |
| Wall time (seconds) | 2.69 | 2.43 |

Every imported summary matched exactly (entity types/counts, extents, blocks and layouts).
These are local measurements on one drawing, not a bound for every file; the DWG dependency
still decodes the entire document before the DXF bridge runs. Broader layout/plot fidelity
remains separate work.

Dense selections also reuse a bounded mesh of their grip rectangles (up to 128 MiB including
its grip metadata). Pointer movement only changes the affected fill colours. Document uid,
revision, selection, view, DPI, grip size and tessellation settings invalidate the cache;
deselection releases it. A selection above the budget uses the uncached path, without dropping
grips or retrying the allocation every frame. Fill and border are one egui rectangle, keeping
pixel rounding and antialiasing. Hot/hover colours and editing indices are tested against the
uncached rectangles, including overlapping grips at four DPI settings. Both paths now share
the rectangle traversal, fill colours and border. The canvas delegates this drawing to the
cache module, and unchanged frames compare a borrowed selection slice instead of allocating
a new selection vector.

For the twelve largest contours of the same DWG, selected simultaneously in the same view,
five pointer moves gave median internal UI CPU work of 17.84 → 7.70 ms and median control-channel
`ui.move` round trips of 246.34 → 126.42 ms. The latter includes scheduling and frame work;
it is not an end-to-end input benchmark or an AutoCAD comparison. Every selected grip remains
editable. Dense overlapping grips still produce substantial GPU/egui work; this does not
claim unlimited selection sizes or universal crash freedom.

## Retained drawing memory

On 2026-10-15, the public `CADCraft_Heavy_Engineering_sample.dwg` (47,844,432 bytes,
985,250 model/paper entities, plus block contents) was measured on an Apple M4 with 16 GB RAM.
Dimensions, attribute definitions, multileaders and tables now keep their large payloads in
`Box` allocations. A simple entity occupies 368 rather than 672 inline bytes on this target;
these four variants require one additional allocation, so annotation-only drawings may not
benefit. JSON and file representations remain unchanged. A synthetic size-budget regression
keeps each entity's inline data below 400 bytes.

Three fresh processes per version, interleaved before/after/after/before/before/after, compare
this change with `27c4060`. A reader-only probe retains the drawing after `cadcraft_io::read`
returns, with no canvas, takes three one-second RSS samples, and reports their median. The
table reports medians across the three processes (decimal GB). Separate release CLI `info`
processes use `/usr/bin/time -l` for import/inspection peaks and wall time.

| Public heavy DWG | Before | After |
|---|---:|---:|
| Resident memory after import, reader probe | 2.37 GB | 2.12 GB |
| CLI import/inspection peak footprint | 2.78 GB | 2.31 GB |
| CLI import/inspection maximum RSS | 2.41 GB | 2.65 GB |
| CLI import/inspection wall time | 8.43 s | 7.98 s |

Retained RSS falls by 245 MB (10.4%) in this local check. Maximum RSS remains variable and
its median increases here; this does not establish a reduction in peak RSS. These are reader
and CLI measurements, not desktop resident-memory or reference-machine targets. Existing
full-document conversion buffers and renderer/GPU allocations remain separate costs.
Imported summaries match exactly, and the exported DXF is byte-identical to the preceding
version. The sample is used only locally and is not a repository fixture.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-15 | minor | Four large annotation variants boxed; entity inline size 672 → 368 bytes on arm64, size-budget regression and public heavy-DWG retained RSS median 2.37 → 2.12 GB. Peak RSS improvement is not established |
| 2026-10-14 | minor | Cached and uncached grip drawing share rectangle traversal and styles; unchanged frames reuse the selection snapshot. Existing colour, DPI, edit-invalidation and oversized-cache regressions retained |
| 2026-10-14 | minor | Bounded grip meshes and one rectangle per marker; twelve-contour local UI CPU median 17.84 → 7.70 ms, unchanged grip indices and four-DPI colour regressions |
| 2026-10-14 | minor | Borrowed DXF sections/records; local 40.9 MB DWG peak RSS median 2.40 → 1.43 GB, preserving imported summaries |
| 2026-10-14 | minor | Dense contour grips generated once per call, preserving editing indices; selected-contour UI CPU median 705.24 → 1.80 ms in a local 40.9 MB DWG; synthetic regressions and CLI grip timing |
| 2026-10-13 | minor | Zoom and pan regenerate in the background: 2–4.5 s stalls down to 26–64 ms on a hatched DWG |
| 2026-10-10 | major | First hardware parity doc: GPU backends per platform, input devices, displays, plotters |
