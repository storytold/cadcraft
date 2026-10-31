# 3D modeling

> **Last updated:** 2026-10-31 · **Change:** major (exact geometry, editing, 3D input/view controls and exchange) · **Target:** Autodesk AutoCAD 2027

CADCraft combines persisted exact boundary geometry with grouped triangle display meshes.
All operations run through engine commands, shared by the native app, command line, control
channel, CLI and MCP. The supported subsets and limits below matter: this is not a complete
industrial solid-modeling kernel or ACIS implementation. Existing parity scores have not been
re-measured for this work.

## Storage and transactions

Box, cylinder, cone, sphere, torus and supported profile builders retain shared vertices,
line/B-spline/NURBS edges and trimmed planar, spline or revolved surfaces through the Apache-2.0
Truck kernel. Topology ids use `shell:edge`; dedicated 3D transforms and display refinement
preserve their ordering. Pyramid, mesh boolean results and sliced solids remain mesh-only. A
profile solid past the exact builders' limits (more than 256 profile points, 128 sweep path
stations or 64 loft sections) or outside their supported shapes is still made, mesh-only; the
command's `notes` say why.

Display triangles are ordinary `3DFACE` objects in a selectable unnamed group whose description
is `3D solid`. Picking one face selects its group. A versioned, compressed exact body is stored
once in `CADCRAFT_BREP_V1` application xdata on an anchor face. DXF preserves the record and group.
There is no ACIS payload or AutoCAD `3DSOLID` entity. A fingerprint detects changes to proxies
made outside the exact editor; stale source records produce an error. General 2D transforms
can invalidate them: use MOVE3D/ROTATE3D/SCALE3D for exact solids.

Edits are atomic and one undo step, including source geometry, display faces and groups. Locked
members block the whole edit. Input, topology, compressed payloads and tessellation work have
explicit bounds. Failed operations leave the drawing unchanged. SOLIDINFO distinguishes exact
source geometry from mesh-only objects; RETESSELLATE changes display tolerance without changing
source geometry.

## Commands

Coordinates are drawing units and angles are degrees. JSON points use world coordinates;
`[x,y]` means Z = 0. `list_commands` exposes the complete parameter documentation.

| Command | JSON parameters, abbreviated | Interactive workflow |
|---|---|---|
| `box` | `{p1,p2}` or `{corner\|center,length,width,height}` or `{corner\|center,side}` | corner, other corner, height |
| `cylinder` / `cone` | `{center?,radius\|diameter,height,segments?}` | base center, radius, height |
| `sphere` | `{center?,radius\|diameter,segments?,rings?}` | center, radius |
| `torus` | `{center?,radius\|diameter,tubeRadius\|tubeDiameter,segments?,tubeSegments?}` | center, radius, tube radius |
| `pyramid` | `{center?,radius\|side,sides?,height,rotation?}` | center, radius, height |
| `extrude` | `{profile:[[x,y],...],height,baseZ?}` or `{handles,height,deleteSource?}` | closed objects, height |
| `revolve` | `{profile:[[radius,axialDistance],...],center?,axis?,axisX?,startAngle?,angle?,segments?}` or `{handles,...}` | closed objects, default full turn |
| `sweep` | `{profile,path:[[x,y,z],...],twist?,scaleStart?,scaleEnd?}` or `{handles,pathHandle,...}` | profile, path |
| `loft` | `{sections:[[[x,y,z],...],...]}` or `{handles,spacing?,baseZ?}` | sections in order |
| `union` / `intersect` | `{handles}` | solids |
| `subtract` | `{from:[handles],remove:[handles]}` | targets, cutters |
| `solidinfo` | `{handles?}` → exact topology and edge ids | current selection / JSON |
| `retessellate` | `{handles?,tolerance}` | solid, tolerance |
| `filletedge` | `{handles?,radius,edges?:["shell:edge",...],tolerance?}` | prism, radius; all vertical edges |
| `chamferedge` | `{handles?,distance,edges?,tolerance?}` | prism, distance; all vertical edges |
| `shell` | `{handles?,thickness,openTop?:true,tolerance?}` | convex prism, thickness |
| `slice` | `{handles?,origin,normal,keep?:positive\|negative\|both}` | solids, plane origin, positive-normal point |
| `section` | `{handles?,origin,normal}` → closed XYZ contours | solids, plane origin, positive-normal point |
| `move3d` | `{handles?,offset:[dx,dy,dz]}` or `{handles?,from,to}` | objects, base, second point |
| `rotate3d` | `{handles?,angle,axis?:"x"\|"y"\|"z"\|[point,point],base?}` | objects, axis, base, angle |
| `scale3d` | `{handles?,factors:[sx,sy,sz],base?,tolerance?}` | current selection / JSON |
| `ucs` | `{action:world\|previous\|set\|save\|named,origin?,xAxis?,yAxis?,name?}` | World, Previous, Origin, 3point, Named, Save |
| `view3d` | `{preset?\|azimuth?,elevation?,projection?,style?,...}` | standard view keyword |
| `3dorbit` | `{azimuth?,elevation?,stop?}` | primary-button orbit drag |
| `plan` | `{}` | return to 2D |
| `export` | `{path?,format?}` | native save/export filters |

Pass every returned face handle to transform an entire exact solid. Inspection, finishing and
mesh booleans accept one face to identify a solid group. Creation and finishing replies return
new face handles: update client references after edits that replace display geometry.

## Profiles and finishing

Profiles supplied by handles may be closed polylines, circles and full ellipses. Circle EXTRUDE
retains exact circular geometry. A circle REVOLVE whose meridian stays clear of its axis retains
its rational circular edges and exact revolved surfaces, including signed partial turns. Other
curved profile inputs are flattened before construction.
Exact profile builders accept at most 256 points, 64 loft sections or 128 sweep path stations,
subject to the overall topology and persistence budget.

REVOLVE accepts signed partial turns and an arbitrary axis defined by two world points. Drawn
profiles must share a plane with the axis. Raw profiles use radius and axial distance. Partial
turns have planar caps. LOFT accepts planar tilted sections with differing point counts. Smaller
sections keep every original corner and gain points on their longest edges until the counts
match, so the solid passes through each section as given; exact ruled/spline surface spans
connect them. The first vertex sets correspondence. SWEEP uses parallel-transport frames with interpolated twist and
positive scale over cumulative path length. Sharp 180° reversals are refused. Loft/sweep do not
prove that intermediate surfaces are free from self-intersection.
Sweep path curves are sampled to bounded polyline stations; the exact source retains those
stations and ruled spans, not the original curved path entity.

FILLETEDGE creates true circular corner arcs on convex vertical edges of a straight polygonal
prism. CHAMFEREDGE applies equal setbacks to those edges. SHELL supports straight convex polygonal
prisms with a normal-offset cavity and optional upper opening. Curved, tilted, holed or otherwise
unsupported finishing configurations are refused before mutation. General curved-edge blends and
arbitrary-face shelling are outside this subset.

Booleans use bounded BSP on closed triangle meshes. Plane slices cap all cut contours, including
nested holes; SECTION creates grouped closed XYZ polylines. Their accuracy follows display
faceting, and their results do not reconstruct analytic curved source surfaces. Each boolean
operand is capped at 16,384 triangles; each modeling command adds at most 50,000 faces.

## 3D input and navigation

3D prompts accept absolute `x,y,z` and relative `@dx,dy,dz` in the active UCS. JSON points always
remain world-space. Pointer picks intersect a view ray with the UCS plane; parallel rays are
refused rather than dropping elevation. Endpoint, midpoint, face-center and nearest 3D snaps
retain world Z. UCS supports an orthonormal frame, previous/world frames and named frames stored
in the drawing's UCS table. Ordinary 2D prompts keep their existing plane behavior.

Drag the ViewCube to orbit, including from plan view. Drag its compass to rotate around the
vertical axis. Faces, edges and corners snap the camera; Home selects SE isometric. Navigation
does not edit geometry or undo history. PLAN restores 2D; 3DORBIT enables primary-button orbiting.

The **3D Settings** button opens native projection, visual style, directional light and material
controls. VIEW3D accepts `projection:"orthographic"|"perspective"`, `fov` (10–120°), `style`
(`shaded`, `flat`, `realistic`, `wireframe`), `lightDirection`, `lightIntensity`, `ambient`,
`specular`, `shininess` and `materialColor` (RGB or null for object colors). Settings-only calls
preserve the camera. Perspective rendering, picking and point rays share one projector;
reciprocal depth preserves occlusion.

The native viewport, PNG export and MCP render share depth-buffered rendering. Render sizes over
2048 pixels are scaled down. SVG uses painter-ordered vector faces. Camera and appearance are
session state, not a saved material library. Lighting is one directional source plus ambient and
specular terms; no textures, shadows or path tracer.

## Exchange

DXF preserves compatible display faces/groups plus exact application data. OBJ and STL carry
triangles (ASCII STL output, ASCII/binary input; binary facet attribute bytes such as colours
are skipped with a read note). glTF 2.0 JSON/GLB and core 3MF map triangle geometry,
names/layers and base colors (`#RRGGBB` or `#RRGGBBAA`) where available; glTF node graphs must
be trees. The 3MF writer shares welded vertices so closed solids are manifold objects. Imports
become faceted 3DFACE objects; unsupported mesh primitives, encodings and extensions produce
explicit errors. IMPORT keeps the source's groups (under unused names), so exact solids from
STEP, IGES or CADCraft DXF stay exact, editable and exportable, also when `at`/`scale` move
them.

STEP reads/writes persisted closed solids through Truck's B-rep mapping. Only millimetre units
are accepted; source labels are not retained, and unsupported assembly placement relationships
are refused instead of silently relocating geometry. IGES reads/writes a bounded
exact type 186 subset with type 190 planar faces and type 110 linear edges. Curved/NURBS IGES
carriers and void shells are refused. IGES global parameters are written and read by position
(units flag 14 must be millimetres), parameters never split across records, and edge/vertex
indices resolve within the type 504/502 list each loop names. Neither exact format falls back
to display triangles.
Mixed drawings or mesh-only solids must be exported as meshes or separated before exact export.

## MCP editing

`tools/list` advertises typed tools for primitives, profiles, booleans, 3D transforms, exact
inspection, refinement, fillet/chamfer/shell, slice/section, UCS, camera/appearance, export and
undo/redo. Tools dispatch through engine commands and preserve transactions and layer locks.
`list_commands` provides full parameter contracts; generic `execute` remains available. Each
modeling tool carries MCP annotations: `inspect_solid` is read-only, creating tools are additive,
and editing, boolean, transform, UCS, export and undo/redo tools are marked destructive (every
edit is still one undo step).

```text
create_box       {"p1":[0,0,0],"p2":[40,30,20]}
inspect_solid    {"handles":[one returned face]}
fillet_edges     {"handles":[one returned face],"radius":2}
set_3d_view      {"preset":"seiso","projection":"perspective","style":"realistic"}
export_model     {"path":"part.step"}
undo_edit        {}
```

PRESSPULL, POLYSOLID, WEDGE, arbitrary sculpting and general ACIS interoperability are not
implemented. These limits are independent of the new exact geometry and editing subset.

## Development packages

Set `CADCRAFT_DISABLE_UPDATES=1` when launching a custom development package to disable both
startup and manual stock-release checks. This prevents a release update from replacing a custom
binary that carries unmerged features. Normal release packages retain their existing update flow.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-31 | minor | Review fixes: corner-preserving loft resampling, mesh-only fallback past exact limits, stale-record 3D transforms, IMPORT keeps groups/exact solids, IGES global/record/list correctness, glTF tree check, STL attributes, 3MF colours/welding, layout-tab 3D gating, MCP annotations |
| 2026-10-31 | minor | Integrated with upstream v0.15.1; development update opt-out; profile, exchange and perspective-picking regressions |
| 2026-10-30 | major | Exact source persistence, bounded finishing, slice/section, UCS/XYZ input/snaps, perspective/styles/light/material controls, STEP/IGES and GLB/glTF/3MF exchange, discoverable MCP editing |
| 2026-10-30 | major | Triangle-mesh primitives, profile commands, booleans, 3D transforms/views, depth rendering, OBJ/STL and ViewCube navigation |
