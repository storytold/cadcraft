# Block preview validation

Validated locally on Windows, 2026-10-28. The baseline was `39f576a9`.
No library drawing, extracted geometry or screenshots are committed. Automated regression tests
use synthetic drawings. The opt-in release audit reads `CADCRAFT_LIBRARY` and writes diagnostics
to `CADCRAFT_AUDIT_DIR`; it is ignored in the normal test suite.

## Behaviour and limits

- Preview results distinguish ready geometry, no visible content and refusal. Only a definition
  with zero entities is labelled **Empty block**. Refusals have translated hover explanations;
  pending results are labelled separately. Palette insertion continues through the existing
  INSERT command regardless of thumbnail success.
- Input is bounded separately in collection elements, text bytes and metadata bytes. Curve work
  counts planned evaluations, spline basis iterations and input scans; expansion counts entities
  and array copies. Output has independent primitive and vertex limits, reserved before growing
  display-list buffers. Text shaping retains its existing bounded input and font-renderer limits.
- Splines share the geometry crate's tessellation and bounds-sampling plans. Bounds are charged
  before sampling; render planning uses a 320-pixel reference extent, covering the largest tile.
  Bulged polylines and hatch contours use the renderer's segment-count rules. Wide polylines
  reserve triangles as well as their centre line.
- Hatches and polygon fills retain a shared work allowance before expensive fill operations.
  A thumbnail whose allowance is exhausted is refused whole, including when the renderer could
  return emergency contours. Normal canvas rendering keeps its existing behaviour.
- The LRU holds at most 512 results and an estimated 64 MiB, including refusals and empty results.
  Estimates include collection capacities, owned keys and conservative index overhead. Drawing
  UID or revision changes invalidate everything. Visibility influences eviction priority but
  never overrides the caps. Results too large to fit become small cached refusals.
- At most three synchronous constructions start per frame. A 30 ms elapsed-time check defers
  subsequent constructions; it cannot interrupt one already running.

## Library audit

The local DWG contains 364 named imported block definitions:

| Result | Before | After |
|---|---:|---:|
| Ready preview | 358 | 360 |
| No visible content in a nonempty definition | 0 | 0 |
| Refused preview | 2 (visually ambiguous) | 0 |
| Imported definition with no entities | 4 | 4 |

The four zero-entity definitions are `BALLON`, `CHAUDIERE MURALE`, `MITIGEUR BY-PASS` and
`Système de remplissage`. In particular, `MITIGEUR BY-PASS` has no imported geometry to preview;
this patch does not repair the DWG import or make claims about the original application's block.

All named blocks were enumerated in the per-block CSV. Visually checked at 512 pixels and in
actual palette widgets: `GMAT_PAC_VIESSMANN_PURPLE`, `DIV_01_LEGENDE_ACCESSOIRES` and
`TUY_DEPART_DIRECT_POMPE_DOUBLE_ENERGIE`. The first now draws its equipment and connections;
the legend retains its symbols and labels, and the pump assembly retains its geometry and colours.
A separate French screenshot checks ready, empty, invisible, refused and pending tiles.

## Release measurements

Single release runs on the same local Windows machine (not latency guarantees):

| Measurement | Before | After |
|---|---:|---:|
| Sum of all 364 first constructions | 163.434 ms | 146.399 ms |
| Median construction | 47.5 µs | 41 µs |
| Worst first construction (`04 ARRIVEE GAZ POSTE`, font initialisation) | 87.751 ms | 79.855 ms |
| `GMAT_PAC_VIESSMANN_PURPLE` | refused in 1.630 ms | ready in 2.599 ms |
| `DIV_01_LEGENDE_ACCESSOIRES` | 1.781 ms | 1.306 ms |
| `TUY_DEPART_DIRECT_POMPE_DOUBLE_ENERGIE` | 0.240 ms | 0.279 ms |
| Return-scroll reconstructions, pages of twelve blocks | 360 | 0 |

With font caches already warm, all 364 constructions totalled 53.400 ms; the worst was
`GMAT_10_GROUPE_DOSAGE_EF`, 6.283 ms. The new cache retained all 364 results in an estimated
10,886,253 bytes (10.38 MiB). A cached twelve-thumbnail frame, including CPU tessellation,
took 1.012 ms at the median and 4.609 ms at worst. The old-policy reproduction started 364
builds on the outward pass and 360 on return (127.762 ms total construction time with warm
font caches, under the renderer-linking qualification below).

The baseline scroll-policy reproduction uses the budget/bounds/build functions from `39f576a9`
and the old `retain_visible` policy, linked against the local release geometry/render libraries.
Its construction counts are independent of that renderer difference; its elapsed time is not
presented as a strict renderer before/after benchmark. New-cache timings exercise actual palette
tiles, including egui CPU tessellation, with twelve visible thumbnails per frame; they exclude
GPU presentation and the rest of the application.

Local diagnostic artifacts are under `target/preview-audit-before/` and
`target/preview-audit-after/`: per-block `library.csv`, the new-cache `cache.txt`, warmed-build
`warm.csv`, the palette image, French status image and the three detailed example images.
Reproduce the audit with:

```text
cargo test --release -p cadcraft-ui-egui --lib local_library_audit -- --ignored --nocapture
```

Set the two environment variables first; no machine-specific path is required by automated tests.

## Validation and remaining limits

- 25 targeted block-palette tests passed; the added geometry sampling-plan regression also passed.
- `cargo xtask ci` passed all seven steps: fmt, clippy with warnings denied, workspace tests,
  i18n checks, asset attribution, dependency layers and wasm. Tests needing sockets and atomic
  temporary-file writes were run in the ordinary Windows session because the sandbox denied them.
- Two authenticated local CodeRabbit reviews completed. Their minor findings (timing-sensitive
  test setup and a split Markdown revision table) were corrected.
- The local release audit and actual palette/status screenshots were inspected. All 364 results
  fit the bounded cache and an outward/return tile traversal performed zero reconstructions.

The first font-using block can still visibly pause a synchronous frame while fonts initialise.
No new block-name exception or arbitrary timeout hides that cost. There is no background worker
or persistent cache. The 64 MiB estimate covers this thumbnail cache, not font caches, temporary
rendering allocations, egui meshes, GPU memory or total application memory. Per-run timings are
measurements on this machine, not latency guarantees. Imported empty definitions remain empty.
