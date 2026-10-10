# Geometry parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (first audit of the geometry kernel's precision against AutoCAD) · **Target:** Autodesk AutoCAD 2027

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

Read from the source on 2026-10-10 (main at bd2412f), with file:line evidence in the tables. No
numeric comparison against AutoCAD has been run yet: the plan is a set of synthetic drawings whose
exact answers are known analytically, measured in both apps black-box (LIST, ID, AREA results)
and in CADCraft headless. Until then the percentages are judgements.

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

## Trim, extend, fillet, chamfer, break, join, lengthen (≈ 45%)

| Command | AutoCAD supports | CADCraft supports |
|---|---|---|
| TRIM | every curve type, quick and standard modes | line, arc, circle, polyline; ellipse and spline via sampled cutting (`modify.rs:485-656`, `modify2.rs:1996`) |
| EXTEND | every open curve | line, arc, polyline, ellipse; **no spline** |
| FILLET | lines, arcs, circles, polylines, ellipses, splines, xlines, rays; Trim/No trim | lines, arcs, circles (TTR circle), polyline mode; **no ellipse or spline** (`modify2.rs:1804`) |
| CHAMFER | lines, polylines, xlines, rays | lines (`modify.rs:756`); polyline chamfer not confirmed |
| BREAK | every curve | line, arc, circle, polyline |
| JOIN | lines, arcs, polylines, ellipses, splines, helixes | line, arc, polyline |
| LENGTHEN | lines, arcs, polylines, elliptical arcs, splines | line, arc, polyline |

Where line and arc are supported the results are exact.

## Measurement (≈ 40%)

| Quantity | AutoCAD | CADCraft |
|---|---|---|
| Closest point: line, arc, circle, polyline | exact | exact |
| Closest point: ellipse, spline | exact to tolerance | **sampled** (`curve.rs:264`, `curves.rs:637`) |
| Tangent points, common tangents | every curve | circles and arcs only (`curve.rs:170-183`) |
| AREA: circle, ellipse | exact | exact area (ellipse perimeter by Ramanujan, not exact) |
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
| Geometric constraints (12) | coincident, collinear, concentric, fix, parallel, perpendicular, horizontal, vertical, tangent, smooth (G2), symmetric, equal | all 12; **Smooth solved as tangency, not G2** |
| Dimensional constraints | linear, horizontal, vertical, aligned, angular, radial, diameter; dynamic and annotational; reference | distance (aligned/horizontal/vertical), angular, radius, diameter; parameters and expressions (`expr.rs`) |
| Geometry | lines, arcs, circles, polylines, ellipses, splines, blocks | line, circle, arc, polyline, point (`model.rs:42`); **no ellipse, spline or block** |
| Algorithm | proprietary | damped Gauss–Newton / Levenberg–Marquardt, numeric Jacobian, per connected component; MAX_ITER 200, MAX_ROWS 800 |
| Over-constraint | refused with a message | redundancy by Jacobian rank, conflicts by removal probing (up to 48 probes) |
| Degrees of freedom, fully constrained state | shown (constraint bars, status) | **not reported** |
| Inference while drawing (CONSTRAINTINFER) | yes | no |

## Tests

`#[test]` counts on 2026-10-10: geom 28, constraints 22, render 50, engine 174. No property-based
or fuzz tests of geometry, and no known-answer suite at large coordinates. Hostile-input tests
exist for parsing, not for numerics.

## What to do first (ranked)

1. Exact ellipse and spline intersection and closest point (subdivision + Newton with a
   relative tolerance); use them in snaps, TRIM, EXTEND, BREAK. 15–25 h.
2. Polyline offset with bulge recomputation, self-intersection cleanup and OFFSETGAPTYPE;
   spline/ellipse offset as fitted splines within a tolerance. 15–25 h.
3. A tolerance context relative to the drawing's extents, and a known-answer test suite at
   origin and at 1e6–1e7 coordinates. 8–12 h.
4. FILLET/JOIN/LENGTHEN/EXTEND for ellipses and splines; CHAMFER on polylines. 10–15 h.
5. Exact areas and MASSPROP for bulge polylines, regions (2D) and hatch loops. 6–10 h.
6. Hatch gap tolerance and curved boundaries. 8–12 h.
7. Solver: DOF reporting, G2 Smooth, ellipse/spline geometry, constraint inference. 20–30 h.
8. Spline fit options (end tangents, knot parameterisation, fit tolerance, periodic). 6–10 h.

Total ≈ 90–140 h; items 1–6 parallelise across two or three agents.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | major | First geometry-precision audit: tolerances, curves, intersections, offsets, modify commands, measurement, hatch, solver |
