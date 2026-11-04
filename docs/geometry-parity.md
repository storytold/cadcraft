# Geometry parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-26 · **Change:** minor (ELLIPSE axis endpoints, other-axis point and Included angle as AutoCAD stores them) · **Target:** Autodesk AutoCAD 2027

CAD users trust numbers: a trimmed endpoint, an offset, an intersection or an area is either right
to the last displayed digit or the tool is unusable for production drawings. This document
audits CADCraft's geometry kernel (`crates/geom`, the curve code in `crates/engine/src/cmd`,
`crates/constraints`) curve by curve against what AutoCAD users rely on. Snaps and tracking are
in [ui-parity.md](ui-parity.md); dimension and hatch behaviour in [gaps.md](gaps.md).

**Headline (estimated): geometry ≈ 40% (weighted below: 41%), of which the constraint solver ≈ 50%.** Everything built
from lines, arcs and circles is exact. Ellipses and splines are handled by tessellating them, so
intersections, offsets, trims and snaps on them are approximations; polyline offsets don't
recompute arc bulges or clean up loops; tolerances are absolute, so very large or very small
drawings behave differently from mid-sized ones. 3D geometry doesn't exist. Remaining:
**≈ 90–140 Opus 5.5 hours** for 2D precision and the solver, plus the 3D kernel (M11, see
[target-app-parity.md](target-app-parity.md)).

## How this was measured

Read from the source on 2026-10-10 (main at bd2412f), with file:line evidence in the tables. The
one numeric comparison against AutoCAD so far is ELLIPSE from axis endpoints (2026-10-26): 11
typed inputs measured black-box in AutoCAD 2024 (accoreconsole, read back with `entget`) and
pinned by `ellipse_axis_endpoints_match_autocad`. Everything else is still to be measured: the
plan is a set of synthetic drawings whose exact answers are known analytically, measured in both
apps black-box (LIST, ID, AREA results) and in CADCraft headless. Until then the percentages are
judgements.

Weights (how much AutoCAD users lean on each area in 2D drafting): numeric robustness 15%,
curve representation 10%, intersections 15%, offsets 15%, trim/extend/fillet/chamfer/break/join
15%, measurement (closest point, length, area, MASSPROP) 10%, hatch boundaries 10%,
constraints 10%.

## Numbers and tolerances (≈ 30%)

| Item | AutoCAD | CADCraft | Evidence |
|---|---|---|---|
| Representation | IEEE double, 3D throughout | f64, **2D kernel** (`Vec2`); Z kept only on entity points and 3D polylines | `geom/src/vec.rs` |
| Tolerance model | Relative to geometry (the ACIS/AcGe tolerance context), stable at survey coordinates | one absolute `EPS = 1e-9`, plus ad-hoc absolute 1e-12/1e-14 in intersections and 1e-3/1e-4 world units for tessellating curves; a few relative tolerances (line–line denominator, hatch, solver) | `geom/src/lib.rs:26`, `intersect.rs:8,19,34,50`, `cmd/modify.rs:469-474`, `snap.rs:98-110` |
| Robust predicates | Exact orientation decisions where it matters | none | — |
| Large coordinates (UTM, 1e6–1e7) | Works | untested; absolute tolerances will misjudge tangency and coincidence, and absolute tessellation steps create huge sample counts that hit caps | — |
| Point merging (JOIN, PEDIT Join fuzz, region) | Fuzz distance per command | JOIN 1e-9 absolute; PEDIT has a fuzz; region grid-key rounding at `tol` | `modify.rs:419`, `region.rs:42` |
| Display precision (LUPREC, AUPREC, units) | Decimal, Engineering, Architectural, Fractional, Scientific; 5 angle formats | units formatting and parsing of all of them | `engine/src/units.rs` |

## Curves (≈ 60%)

| Curve | AutoCAD | CADCraft |
|---|---|---|
| Line, ray, xline | yes | yes, exact |
| Circle, arc, bulge polyline | yes | yes, exact |
| Ellipse, elliptical arc | yes | yes (center, major axis, ratio, parameters) |
| Spline: CV form, rational, any degree | yes | yes: rational B-spline, basis-function evaluation |
| Spline: fit points | degree 3 with end tangents, knot parameterisation (chord, square root, uniform), fit tolerance | cubic, chord-length, averaged knots, dense solve; **no end tangents, no knot parameterisation choice, no fit tolerance** (`geom/src/spline.rs:31-88`) |
| Spline: closed/periodic | periodic | "closed" appends the first point and refits; **not periodic** (`modify2.rs:647`) |
| Spline editing: knot insertion, split, refine, rebuild, convert to polyline | SPLINEDIT, grips | knot insertion and splitting (`curves.rs:539-622`); refine/rebuild partial |
| Helix | yes | **no** |
| 3D polyline | yes | stored and drawn; no 3D operations |
| Regions, 3D solids, surfaces, meshes (ACIS/B-rep, NURBS surfaces, subdivision) | yes | **no** (M11) |

## Intersections (≈ 40%)

| Pair | AutoCAD | CADCraft |
|---|---|---|
| line–line, line–circle/arc, circle–circle | exact | closed form, exact; tangency decided by a fixed `|disc| ≤ 1e-12·a`, so near-tangent cases depend on scale |
| line/arc–ellipse | exact (quartic) | **tessellated**: 1e-4 world units in TRIM, 1e-3 in snaps; TRIM refines with 720 samples + 60-step bisection, near-exact against line/arc cutters (`modify2.rs:1946-1995`) |
| line/arc–spline | exact to tolerance (subdivision + Newton) | tessellated as above |
| ellipse–ellipse, ellipse–spline, spline–spline | exact to tolerance | **polygon approximations** only |
| Self-intersections (curves, offset loops) | yes | **no routine** |
| Apparent intersection (projected) | yes | treated as plain intersection |

## Offsets (≈ 30%)

| Curve | AutoCAD OFFSET | CADCraft (`cmd/modify.rs:276-424`) |
|---|---|---|
| Line, ray, xline | exact | exact |
| Circle, arc | exact | exact (radius change) |
| Ellipse | spline approximation within tolerance | **a polyline** from tessellation at 1e-3 × major axis |
| Spline | spline approximation within tolerance | tessellate at 1e-3, offset vertices along normals, keep about 40 points, refit: **lossy** |
| Polyline with arcs | exact, with self-intersection trimming, OFFSETGAPTYPE (extend, fillet, chamfer) | segments offset and joined at neighbours' intersections; **arc bulges not recomputed** after endpoints move (comment at L413), **no loop cleanup**, collapsed segments not removed; OFFSETGAPTYPE only echoed |
| Options: Through, Erase, Layer, Multiple | yes | see [gaps.md](gaps.md) |

## Trim, extend, fillet, chamfer, break, join, lengthen (≈ 70%)

| Command | AutoCAD supports | CADCraft supports |
|---|---|---|
| TRIM | every curve type, quick and standard modes | line, arc, circle, polyline; ellipse and spline via sampled cutting (`modify.rs:485-656`, `modify2.rs` `trim_other`); a closed spline leaves one open spline On a closed polyline a pick before the first or after the last cut keeps the run between the cuts and nothing more (#726). |
| EXTEND | every open curve | line, arc, polyline, elliptical arc, open spline (runs on along its end tangent, `modify2/curve_edits.rs`) |
| FILLET | lines, arcs, circles, polylines, ellipses, splines, xlines, rays; Trim/No trim | lines, arcs, circles (TTR circle), polyline mode, Trim/No trim, Multiple; elliptical arcs and splines with any of these (tangent circle from sampled offsets refined by Newton's method, `geom/src/fillet.rs`; arcs trimmed or extended round the ellipse, splines split at the touch point); two straight segments of one polyline (adjacent, or with one between that the corner replaces), a polyline's straight end segment with a line (the line joins it); not polyline arc segments, xlines or rays |
| CHAMFER | lines, polylines, xlines, rays | lines, Distance/Angle, whole polylines (Polyline option), polyline segments as for FILLET (`modify2/fillet_pline.rs`); refuses arcs, ellipses and splines with a message, as AutoCAD; not xlines or rays |
| BREAK | every curve | line, arc, circle, polyline |
| JOIN | lines, arcs, polylines, ellipses, splines, helixes | line, arc, polyline into a polyline; elliptical arcs on one ellipse into a larger arc or the full ellipse (gaps close); Close turns a single arc into its circle and a single elliptical arc into its full ellipse; splines with lines, arcs, polylines and elliptical arcs into one exact NURBS (degree elevation, rational arcs; `geom/src/nurbs.rs`); no helixes |
| BREAK | every curve | line, arc, circle, polyline; a closed polyline loses the stretch from the first to the second point along its vertices and is left open (#1173) |
| JOIN | lines, arcs, polylines, ellipses, splines, helixes | line, arc, polyline into a polyline; elliptical arcs on one ellipse into a larger arc or the full ellipse (gaps close); splines with lines, arcs, polylines and elliptical arcs into one exact NURBS (degree elevation, rational arcs; `geom/src/nurbs.rs`); no helixes |
| LENGTHEN | lines, arcs, polylines, elliptical arcs, splines | line, arc, polyline; elliptical arc (arc length along the ellipse); open spline (cut along the curve, or run on along its end tangent) |

Where line and arc are supported the results are exact.

## Measurement (≈ 40%)

| Quantity | AutoCAD | CADCraft |
|---|---|---|
| Closest point: line, arc, circle, polyline | exact | exact |
| Closest point: ellipse, spline | exact to tolerance | **sampled** (`curve.rs:264`, `curves.rs:637`) |
| Tangent points, common tangents | every curve | circles and arcs only (`curve.rs:170-183`) |
| AREA: circle, ellipse | exact | exact area (ellipse perimeter by Ramanujan, not exact); an open elliptical arc has no area, like an arc |
| AREA: polyline with arcs | exact (bulge segments) | **tessellated at 1e-4** (`curve.rs:491`) |
| MASSPROP (regions: area, perimeter, centroid, moments) | exact on regions and solids | shoelace over a tessellated circle, polyline or first hatch loop (`utility.rs:310-352`); no regions |
| LIST/DIST/ID precision | full double | full double |

## Hatch boundaries (≈ 45%)

`geom/src/region.rs`: curves are polygonised, crossings split on a grid, vertices merged at
`tol`, a ray picks the edge and the face is traced by sharpest left turn; islands by
connectivity (including islands made of separate lines, #163). Caps: 60,000 segments, 4,096
islands. Missing: **gap tolerance (HPGAPTOL)**, true arcs and curves in boundaries (boundaries
come out tessellated, so a hatched circle's boundary is a polygon), island detection modes
beyond the default, boundary sets, and retained associative boundaries for every case. Pattern
lines are scanline families clipped against the tessellated loops (`render/src/hatch.rs`).

## Constraint solver (≈ 50%)

| Item | AutoCAD | CADCraft (`crates/constraints`) |
|---|---|---|
| Geometric constraints (12) | coincident, collinear, concentric, fix, parallel, perpendicular, horizontal, vertical, tangent, smooth (G2), symmetric, equal | all 12; Smooth is G2 (common point, tangent and curvature, `curves.rs`) between a spline and a line, arc, elliptical arc, polyline segment or spline; between two non-spline curves it stays tangency |
| Dimensional constraints | linear, horizontal, vertical, aligned, angular, radial, diameter; dynamic and annotational; reference | distance (aligned/horizontal/vertical), angular, radius, diameter; parameters and expressions (`expr.rs`), with one depth limit across syntax trees and parameter references |
| Geometry | lines, arcs, circles, polylines, ellipses, splines, blocks | line, circle, arc, polyline, point; ellipse and elliptical arc (centre, axis ends and arc ends as points; tangent, coincident, concentric, horizontal/vertical on the major axis); spline (fit or control points as points; tangent and smooth at its ends; open clamped or fit splines); block reference (insertion point, and the block's own points riding rigidly with position and rotation) (`model.rs`) |
| Algorithm | proprietary | damped Gauss–Newton / Levenberg–Marquardt, numeric Jacobian, per connected component; MAX_ITER 200, MAX_ROWS 800; curve–curve tangency with an ellipse through auxiliary contact parameters |
| Over-constraint | refused with a message | redundancy by Jacobian rank (Householder QR with column pivoting, rows and columns balanced), including Fix; the message names the constraints the new one duplicates; conflicts by removal probing (up to 48 probes) |
| Degrees of freedom, fully constrained state | shown (constraint bars, status) | `constraints.dof` and `constraints.inspect`: free parameters minus Jacobian rank per component, per-object DOF from the null space, under/fully/over-constrained status (`dof.rs`); a status strip on each constraint bar and the drawing's state on the canvas |
| Inference while drawing (CONSTRAINTINFER) | yes | CONSTRAINTINFER (variable and command): after a drawing or editing command, what it placed exactly onto existing geometry (snaps, ortho, polar) gets Coincident (end, mid, centre, on object), Tangent, Perpendicular, Parallel and Horizontal/Vertical, filtered by the AutoConstrain types (`auto.rs` `infer_placed`) |

## Tests

`#[test]` counts on 2026-10-10: geom 28, constraints 22, render 50, engine 174. A property-based
and hostile-input suite for geometry numerics landed 2026-10-12 (`crates/geom/src/property_tests.rs`):
a deterministic xorshift generator (no new dependency) drives a few thousand cases per property —
intersections lie on both curves and are symmetric, circle pairs match the closed form, closest
points are fixed points (perpendicular inside the segment, endpoints outside), three-point arcs
keep `{start, end} = {p1, p3}` and contain p2 (the CCW-only storage swaps the pair when p2 is on
the clockwise side, as DXF does), and hostile inputs (NaN, infinities, subnormals, raw bit
patterns, 1e300) never panic and return only finite hits or none. Known answers hold at 1e6–1e7
coordinates to 1e-9 relative.

## What to do first (ranked)

1. Exact ellipse and spline intersection and closest point (subdivision + Newton with a
   relative tolerance); use them in snaps, TRIM, EXTEND, BREAK. 15–25 h.
2. Polyline offset with bulge recomputation, self-intersection cleanup and OFFSETGAPTYPE;
   spline/ellipse offset as fitted splines within a tolerance. 15–25 h.
3. A tolerance context relative to the drawing's extents, and a known-answer test suite at
   origin and at 1e6–1e7 coordinates. 8–12 h.
4. Exact areas and MASSPROP for bulge polylines, regions (2D) and hatch loops. 6–10 h.
5. Hatch gap tolerance and curved boundaries. 8–12 h.
6. ~~Solver: DOF reporting, G2 Smooth, ellipse/spline geometry, constraint inference.~~ Done (#546).
7. Spline fit options (end tangents, knot parameterisation, fit tolerance, periodic). 6–10 h.

Total ≈ 90–140 h; items 1–6 parallelise across two or three agents.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-26 | minor | ELLIPSE from axis endpoints matches AutoCAD's stored geometry: the major axis points at the first endpoint (arcs drew on the opposite side of the center), a point at the other-axis prompt gives its distance from the center, and the end-angle prompt offers Included angle |
| 2026-10-26 | minor | PEDIT Multiple at the command line (#2182): select objects, lines and arcs turned into polylines on Yes, then Close/Open/Join/Width/Fit/Spline/Decurve/Ltype gen/Reverse/Undo apply to all; Join joins the selected objects with each other within a fuzz distance |
| 2026-10-22 | minor | Constraint expressions count operator chains toward the syntax-tree depth limit; evaluation shares one depth limit across operators and parameter references, with a 1 MiB-stack regression (#1936) |
| 2026-10-14 | trivial | JOIN Close: a single arc becomes its circle, a single elliptical arc its full ellipse (#1175) |
| 2026-10-12 | trivial | TRIM on a closed polyline keeps only the complementary run when the pick lies past the last cut (#726) |
| 2026-10-12 | minor | Property-based and hostile-input tests for geometry numerics landed (`crates/geom/src/property_tests.rs`): intersections, closest points, three-point arcs, bounds, vector identities; hostile f64s never panic; known answers at 1e6–1e7 |
| 2026-10-10 | major | First geometry-precision audit: tolerances, curves, intersections, offsets, modify commands, measurement, hatch, solver |
| 2026-10-11 | minor | EXTEND, JOIN and LENGTHEN on elliptical arcs and splines; closed splines trim to one piece; trim/extend section 45% → 55% (#466) |
| 2026-10-11 | minor | FILLET with elliptical arcs and splines; trim/extend section 55% → 65% (#466) |
| 2026-10-11 | minor | FILLET/CHAMFER with polyline segments (same polyline, end segment with a line); section 65% → 70% (#466) |
| 2026-10-11 | minor | Constraint solver (#546): DOF report, G2 Smooth, ellipse/spline/block geometry, CONSTRAINTINFER |
