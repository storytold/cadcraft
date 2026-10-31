//! IGES 5.3 exact exchange for bounded planar, linear manifold solids.
//!
//! The subset writes the standard type 186/514/510/508/504/502 topology graph,
//! type 190 plane surfaces and type 110 line curves. Curved/NURBS bodies are
//! refused before output; they are never replaced with the display tessellation.

use std::collections::HashMap;

use cadcraft_doc::{Common, Drawing};
use cadcraft_geom::brep::{Body, kernel};
use kernel::{InnerSpace, ParametricSurface3D};

use crate::{IoError, Result};

const MAX_BYTES: usize = 64 << 20;
const MAX_ENTITIES: usize = 100_000;
const MAX_FACES: usize = 4096;
const MAX_VERTICES: usize = 16_384;
const MAX_EDGES: usize = 32_768;

fn error(s: impl Into<String>) -> IoError {
    IoError::Format(format!("IGES: {}", s.into()))
}

#[derive(Clone)]
struct Entity {
    kind: u32,
    form: i32,
    params: String,
}

fn number(v: f64) -> Result<String> {
    if !v.is_finite() || v.abs() > 1.0e9 {
        return Err(error("coordinates must be finite and within ±1e9"));
    }
    Ok(format!("{v:.17}"))
}

fn xyz(p: kernel::Point3) -> Result<[String; 3]> {
    Ok([number(p.x)?, number(p.y)?, number(p.z)?])
}

fn export_bodies(d: &Drawing) -> Result<Vec<Body>> {
    if d.groups.is_empty() {
        return Err(error("no persisted exact solids; mesh triangle export would lose geometry"));
    }
    let mut out = Vec::new();
    let mut members = std::collections::HashSet::new();
    for group in d.groups.iter().filter(|g| g.description == "3D solid") {
        if out.len() >= 256 {
            return Err(error("too many exact solid bodies"));
        }
        for h in &group.members {
            if !members.insert(*h) {
                return Err(error("overlapping exact solid groups"));
            }
        }
        let body = cadcraft_doc::brep::read(d, &group.members).map_err(error)?.ok_or_else(|| error("solid group has no exact source geometry"))?;
        body.validate().map_err(|e| error(e.to_string()))?;
        for shell in &body.geometry.boundaries {
            if shell.vertices.len() > MAX_VERTICES || shell.edges.len() > MAX_EDGES || shell.faces.len() > MAX_FACES {
                return Err(error("body exceeds IGES topology limits"));
            }
            if shell.edges.iter().any(|e| !matches!(e.curve, kernel::Curve::Line(_))) {
                return Err(error("this IGES subset supports exact line edges only; curved and NURBS edges require STEP"));
            }
            if shell.faces.iter().any(|f| !matches!(f.surface, kernel::Surface::Plane(_)) || f.boundaries.len() != 1) {
                return Err(error("this IGES subset supports single-loop planar faces only; curved/NURBS or holed faces require STEP"));
            }
        }
        out.push(body);
    }
    if out.is_empty() {
        return Err(error("no persisted exact solids; mesh triangle export would lose geometry"));
    }
    if d.model.iter().any(|e| !members.contains(&e.handle)) {
        return Err(error("drawing contains entities outside the exact solid groups"));
    }
    Ok(out)
}

fn add(entities: &mut Vec<Entity>, kind: u32, form: i32, params: impl Into<String>) -> Result<usize> {
    if entities.len() >= MAX_ENTITIES {
        return Err(error("too many IGES entities"));
    }
    entities.push(Entity { kind, form, params: params.into() });
    Ok(entities.len())
}

fn push_body(entities: &mut Vec<Entity>, body: &Body) -> Result<()> {
    let shell = body.geometry.boundaries.first().ok_or_else(|| error("body has no shell"))?;
    if body.geometry.boundaries.len() != 1 {
        return Err(error("IGES subset supports one closed shell per body"));
    }
    let mut vertex_params = format!("{}", shell.vertices.len());
    for p in &shell.vertices {
        for coordinate in xyz(*p)? {
            vertex_params.push(',');
            vertex_params.push_str(&coordinate);
        }
    }
    let vertex_list = add(entities, 502, 1, vertex_params)?;

    let mut edge_params = format!("{}", shell.edges.len());
    let mut curve_ids = Vec::with_capacity(shell.edges.len());
    for edge in &shell.edges {
        let (a, b) = edge.vertices;
        let p0 = *shell.vertices.get(a).ok_or_else(|| error("edge references missing vertex"))?;
        let p1 = *shell.vertices.get(b).ok_or_else(|| error("edge references missing vertex"))?;
        if (p1 - p0).magnitude2() <= 1e-20 {
            return Err(error("zero-length edge is unsupported"));
        }
        let [x1, y1, z1] = xyz(p0)?;
        let [x2, y2, z2] = xyz(p1)?;
        curve_ids.push(add(entities, 110, 0, format!("{x1},{y1},{z1},{x2},{y2},{z2}"))?);
    }
    let vertex_ptr = pointer(vertex_list)?;
    for (i, edge) in shell.edges.iter().enumerate() {
        let (a, b) = edge.vertices;
        edge_params.push_str(&format!(",{},{vertex_ptr},{},{vertex_ptr},{}", pointer(curve_ids[i])?, a + 1, b + 1));
    }
    let edge_list = add(entities, 504, 1, edge_params)?;
    let edge_ptr = pointer(edge_list)?;

    let mut face_ids = Vec::new();
    for face in &shell.faces {
        let kernel::Surface::Plane(plane) = &face.surface else {
            return Err(error("unsupported non-planar IGES surface"));
        };
        let n = plane.normal();
        let x = plane.u_axis().normalize();
        let origin = plane.origin();
        let loc = add(entities, 116, 0, xyz(origin)?.join(",").to_string())?;
        let norm = add(entities, 123, 0, [number(n.x)?, number(n.y)?, number(n.z)?].join(",").to_string())?;
        let reference = add(entities, 123, 0, [number(x.x)?, number(x.y)?, number(x.z)?].join(",").to_string())?;
        let surf = add(entities, 190, 1, format!("{},{},{}", pointer(loc)?, pointer(norm)?, pointer(reference)?))?;
        let wire = face.boundaries.first().ok_or_else(|| error("face has no outer loop"))?;
        let mut loop_params = format!("{}", wire.len());
        for use_edge in wire {
            if use_edge.index >= shell.edges.len() {
                return Err(error("face references missing edge"));
            }
            loop_params.push_str(&format!(",0,{edge_ptr},{}, {},0", use_edge.index + 1, if use_edge.orientation { "1" } else { "0" }));
        }
        let loop_id = add(entities, 508, 1, loop_params)?;
        face_ids.push(add(
            entities,
            510,
            1,
            format!("{surf},{loop_count},{outer},{loop_ptr}", surf = pointer(surf)?, loop_count = 1, outer = 1, loop_ptr = pointer(loop_id)?),
        )?);
    }
    let mut shell_params = format!("{}", face_ids.len());
    for (i, face_id) in face_ids.iter().enumerate() {
        shell_params.push_str(&format!(",{},{}", pointer(*face_id)?, if shell.faces[i].orientation { "1" } else { "0" }));
    }
    let shell_id = add(entities, 514, 1, shell_params)?;
    add(entities, 186, 0, format!("{},1,0", pointer(shell_id)?))?;
    Ok(())
}

fn pointer(index: usize) -> Result<usize> {
    index.checked_mul(2).and_then(|v| v.checked_sub(1)).ok_or_else(|| error("entity pointer overflow"))
}

fn write_record(out: &mut String, payload: &str, section: char, sequence: usize) -> Result<()> {
    if payload.len() > 72 {
        return Err(error("internal IGES record overflow"));
    }
    out.push_str(payload);
    out.extend(std::iter::repeat_n(' ', 72 - payload.len()));
    out.push(section);
    out.push_str(&format!("{sequence:>7}\n"));
    Ok(())
}

fn write_entities(entities: &[Entity]) -> Result<Vec<u8>> {
    let mut output = String::new();
    let mut s_count = 0usize;
    let mut g_count = 0usize;
    {
        let line = "CADCraft exact IGES subset 5.3";
        s_count += 1;
        write_record(&mut output, line, 'S', s_count)?;
    }
    // Delimiters: comma and semicolon. Millimetres, model scale 1.0, fixed 80-column ASCII.
    for line in [
        "1H,,1H;",
        "4HCADCraft,8HCADCraft,6,308,15,308,15,1.0,2,2HMM,1,1.0,15H20261030.000000,1.0E-6,1.0E9,6HCADCAD,6HCADCAD,11,0,15H20261030.000000;",
    ] {
        for part in line.as_bytes().chunks(72) {
            let part = std::str::from_utf8(part).map_err(|_| error("global data is not ASCII"))?;
            g_count += 1;
            write_record(&mut output, part, 'G', g_count)?;
        }
    }
    let mut pd_lines: Vec<(usize, String)> = Vec::new();
    let mut de_meta = Vec::new();
    let mut pseq = 1usize;
    for (i, e) in entities.iter().enumerate() {
        let dseq = 2 * i + 1;
        let data = format!("{},{};", e.kind, e.params);
        let mut start = 0;
        while start < data.len() {
            let end = (start + 64).min(data.len());
            let part = data.get(start..end).ok_or_else(|| error("parameter data split is not UTF-8"))?.to_string();
            pd_lines.push((dseq, part));
            start = end;
        }
        let lines = pd_lines.len().saturating_sub(pseq - 1);
        de_meta.push((dseq, e.kind, e.form, pseq, lines));
        pseq = pd_lines.len() + 1;
    }
    for (dseq, kind, form, first_p, line_count) in &de_meta {
        let status = if *kind == 186 { "00000000" } else { "00010001" };
        let first = format!("{kind:>8}{first_p:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{status}", 0, 0, 0, 0, 0, 0);
        // Directory line 1 fields are fixed at 8 columns, plus 8-character status field.
        if first.len() != 72 {
            return Err(error("internal directory-entry width error"));
        }
        let mut second = format!("{kind:>8}{:>8}{:>8}{line_count:>8}{form:>8}{:>8}{:>8}{:>8}{:>8}", 0, 0, 0, 0, 0, 0);
        second.truncate(72);
        second.push('D');
        second.push_str(&format!("{:>7}\n", dseq + 1));
        let mut first = first;
        first.push('D');
        first.push_str(&format!("{:>7}\n", dseq));
        output.push_str(&first);
        output.push_str(&second);
    }
    for (i, (dseq, data)) in pd_lines.iter().enumerate() {
        write_record(&mut output, &format!("{data:<64}{dseq:>8}"), 'P', i + 1)?;
    }
    let tline = format!("S{:>7}G{:>7}D{:>7}P{:>7}", s_count, g_count, entities.len() * 2, pd_lines.len());
    write_record(&mut output, &tline, 'T', 1)?;
    if output.len() > MAX_BYTES {
        return Err(error("output exceeds 64 MiB limit"));
    }
    Ok(output.into_bytes())
}

pub fn write(d: &Drawing) -> Result<Vec<u8>> {
    let bodies = export_bodies(d)?;
    let mut entities = Vec::new();
    for body in &bodies {
        push_body(&mut entities, body)?;
    }
    write_entities(&entities)
}

// The parser below deliberately accepts the same topology/carrier subset as the writer.
#[derive(Debug)]
struct Parsed {
    kind: u32,
    form: i32,
    params: Vec<String>,
}
fn split_params(s: &str) -> Result<Vec<String>> {
    let s = s.trim_end_matches([';', ',']);
    let values = s.split(',').map(str::trim).map(str::to_string).collect::<Vec<_>>();
    if values.len() > 1_000_000 {
        return Err(error("too many entity parameters"));
    }
    Ok(values)
}
fn parse_usize(v: &str) -> Result<usize> {
    v.parse().map_err(|_| error("invalid integer parameter"))
}
fn parse_f64(v: &str) -> Result<f64> {
    let n = v.parse::<f64>().map_err(|_| error("invalid numeric parameter"))?;
    if !n.is_finite() || n.abs() > 1.0e9 {
        return Err(error("numeric parameter outside finite bounds"));
    }
    Ok(n)
}
fn parse_bool(v: &str) -> Result<bool> {
    match v {
        "1" => Ok(true),
        "0" => Ok(false),
        _ => Err(error("invalid IGES logical")),
    }
}

fn read_inner(text: &str) -> Result<Drawing> {
    let mut sections = [0usize; 5];
    let mut directory: HashMap<usize, (u32, i32, usize, usize)> = HashMap::new();
    let mut pd: HashMap<usize, String> = HashMap::new();
    let mut parameter_meta: HashMap<usize, (usize, usize)> = HashMap::new();
    let mut records: Vec<(char, Vec<u8>)> = Vec::new();
    for line in text.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let b = line.as_bytes();
        if b.len() != 80 {
            return Err(error("record is not exactly 80 columns"));
        }
        let ch = b[72] as char;
        let idx = match ch {
            'S' => 0,
            'G' => 1,
            'D' => 2,
            'P' => 3,
            'T' => 4,
            _ => return Err(error("invalid section code")),
        };
        sections[idx] += 1;
        if sections[idx] > MAX_ENTITIES * 4 {
            return Err(error("too many IGES records"));
        }
        records.push((ch, b.to_vec()));
    }
    if sections[0] == 0 || sections[1] == 0 || sections[2] == 0 || sections[3] == 0 || sections[4] != 1 {
        return Err(error("incomplete IGES section framing"));
    }
    // Millimetres only. Global parameter 2 selects comma and semicolon delimiters.
    let global = records.iter().filter(|(c, _)| *c == 'G').map(|(_, b)| String::from_utf8_lossy(&b[..72]).to_string()).collect::<String>();
    if !global.contains("2HMM") {
        return Err(error("only millimetre IGES files are supported"));
    }
    for (ch, b) in &records {
        if *ch == 'D' {
            let seq = parse_usize(std::str::from_utf8(&b[73..80]).map_err(|_| error("bad D sequence"))?.trim())?;
            let first = std::str::from_utf8(&b[..8]).map_err(|_| error("bad directory entry"))?;
            let kind = parse_usize(first.trim())? as u32;
            if seq % 2 == 1 {
                let ptr = parse_usize(std::str::from_utf8(&b[8..16]).map_err(|_| error("bad parameter pointer"))?.trim())?;
                directory.insert(seq, (kind, 0, ptr, 0));
            } else {
                let count = parse_usize(std::str::from_utf8(&b[24..32]).map_err(|_| error("bad parameter line count"))?.trim())?;
                let form = parse_usize(std::str::from_utf8(&b[32..40]).map_err(|_| error("bad form field"))?.trim())? as i32;
                let entry = directory.get_mut(&(seq - 1)).ok_or_else(|| error("orphan second directory record"))?;
                if entry.0 != kind {
                    return Err(error("directory entity type mismatch"));
                }
                entry.1 = form;
                entry.3 = count;
            }
        } else if *ch == 'P' {
            let directory_ptr = parse_usize(std::str::from_utf8(&b[64..72]).map_err(|_| error("bad parameter pointer"))?.trim())?;
            let sequence = parse_usize(std::str::from_utf8(&b[73..80]).map_err(|_| error("bad P sequence"))?.trim())?;
            let meta = parameter_meta.entry(directory_ptr).or_insert((sequence, 0));
            meta.0 = meta.0.min(sequence);
            meta.1 += 1;
            pd.entry(directory_ptr).or_default().push_str(std::str::from_utf8(&b[..64]).map_err(|_| error("non-ASCII parameter record"))?.trim_end());
        }
    }
    let mut entities = HashMap::new();
    for (seq, (kind, form, first_parameter_line, expected_lines)) in &directory {
        let raw = pd.get(seq).ok_or_else(|| error("directory entry references missing parameter data"))?;
        let (first_parameter_sequence, found_lines) =
            parameter_meta.get(seq).copied().ok_or_else(|| error("directory entry has no parameter records"))?;
        if found_lines != *expected_lines {
            return Err(error("parameter line count does not match directory entry"));
        }
        if first_parameter_sequence != *first_parameter_line {
            return Err(error("directory parameter pointer does not reference its first P record"));
        }
        let params = split_params(raw)?;
        if params.first().and_then(|s| s.parse::<u32>().ok()) != Some(*kind) {
            return Err(error("directory and parameter entity types disagree"));
        }
        entities.insert(*seq, Parsed { kind: *kind, form: *form, params });
    }
    if entities.len() > MAX_ENTITIES {
        return Err(error("too many IGES entities"));
    }
    if entities.values().any(|e| !matches!(e.kind, 110 | 116 | 123 | 186 | 190 | 502 | 504 | 508 | 510 | 514)) {
        return Err(error("unsupported exact IGES entity type in manifold solid graph"));
    }
    let mut d = Drawing::new_metric();
    let mut solid_count = 0usize;
    for e in entities.values() {
        if e.kind != 186 {
            continue;
        }
        solid_count += 1;
        if solid_count > 256 {
            return Err(error("too many solids"));
        }
        let p = &e.params;
        if p.len() != 4 {
            return Err(error("unsupported 186 void-shell topology"));
        }
        let shell_ptr = parse_usize(&p[1])?;
        let shell_orientation = parse_bool(&p[2])?;
        if parse_usize(&p[3])? != 0 {
            return Err(error("void shells are unsupported"));
        }
        let shell = entities.get(&shell_ptr).ok_or_else(|| error("186 references missing shell"))?;
        if shell.kind != 514 || shell.form != 1 {
            return Err(error("186 must reference closed type 514 shell"));
        }
        let sp = &shell.params;
        let nf = parse_usize(&sp[1])?;
        if nf == 0 || nf > MAX_FACES || sp.len() != 2 + 2 * nf {
            return Err(error("invalid shell face list"));
        }
        let mut verts: Vec<kernel::Point3> = Vec::new();
        let mut edges: Vec<(usize, usize, kernel::Curve)> = Vec::new();
        let mut loops_data = Vec::new();
        let mut surfaces = Vec::new();
        for i in 0..nf {
            let fptr = parse_usize(&sp[2 + 2 * i])?;
            let face_use = parse_bool(&sp[3 + 2 * i])?;
            let f = entities.get(&fptr).ok_or_else(|| error("shell references missing face"))?;
            if f.kind != 510 || f.form != 1 || f.params.len() != 5 {
                return Err(error("unsupported face entity form"));
            }
            let surface_ptr = parse_usize(&f.params[1])?;
            let nloops = parse_usize(&f.params[2])?;
            if !parse_bool(&f.params[3])? {
                return Err(error("single-loop faces must identify their outer loop"));
            }
            if nloops != 1 {
                return Err(error("only one-loop planar faces are supported"));
            }
            let loop_ptr = parse_usize(&f.params[4])?;
            let surf = entities.get(&surface_ptr).ok_or_else(|| error("face references missing surface"))?;
            if surf.kind != 190 || surf.form != 1 || surf.params.len() != 4 {
                return Err(error("only parameterized type 190 planes are supported"));
            }
            let location = point_entity(&entities, parse_usize(&surf.params[1])?)?;
            let normal = vector_entity(&entities, parse_usize(&surf.params[2])?)?;
            let reference = vector_entity(&entities, parse_usize(&surf.params[3])?)?;
            let plane = kernel::Plane::new(location, location + reference, location + normal.cross(reference));
            let lp = entities.get(&loop_ptr).ok_or_else(|| error("face references missing loop"))?;
            if lp.kind != 508 || lp.form != 1 {
                return Err(error("unsupported loop entity form"));
            }
            let n = parse_usize(&lp.params[1])?;
            if !(3..=MAX_EDGES).contains(&n) || lp.params.len() != 2 + 5 * n {
                return Err(error("invalid loop edge count"));
            }
            let mut wi = Vec::new();
            for j in 0..n {
                let off = 2 + 5 * j;
                if parse_usize(&lp.params[off])? != 0 {
                    return Err(error("vertex pole loop edges are unsupported"));
                }
                let eptr = parse_usize(&lp.params[off + 1])?;
                let index = parse_usize(&lp.params[off + 2])?;
                let orient = parse_bool(&lp.params[off + 3])?;
                if parse_usize(&lp.params[off + 4])? != 0 {
                    return Err(error("parameter-space trimming curves are unsupported"));
                }
                let edge_list = entities.get(&eptr).ok_or_else(|| error("loop references missing edge list"))?;
                if edge_list.kind != 504 || edge_list.form != 1 {
                    return Err(error("unsupported edge-list entity"));
                }
                let ne = parse_usize(&edge_list.params[1])?;
                if index == 0 || index > ne || edge_list.params.len() != 2 + 5 * ne {
                    return Err(error("edge-list index out of range"));
                }
                if edges.len() < ne {
                    for k in 0..ne {
                        let eo = 2 + 5 * k;
                        let cptr = parse_usize(&edge_list.params[eo])?;
                        let a_ptr = parse_usize(&edge_list.params[eo + 1])?;
                        let a = parse_usize(&edge_list.params[eo + 2])?;
                        let b_ptr = parse_usize(&edge_list.params[eo + 3])?;
                        let b = parse_usize(&edge_list.params[eo + 4])?;
                        if a_ptr != b_ptr {
                            return Err(error("separate vertex lists per edge are unsupported"));
                        }
                        let vl = entities.get(&a_ptr).ok_or_else(|| error("edge references missing vertex list"))?;
                        if vl.kind != 502 || vl.form != 1 {
                            return Err(error("unsupported vertex-list entity"));
                        }
                        let count = parse_usize(&vl.params[1])?;
                        if a == 0 || b == 0 || a > count || b > count || vl.params.len() != 2 + 3 * count {
                            return Err(error("vertex-list index out of range"));
                        }
                        if verts.is_empty() {
                            for q in 0..count {
                                let o = 2 + 3 * q;
                                verts.push(kernel::Point3::new(
                                    parse_f64(&vl.params[o])?,
                                    parse_f64(&vl.params[o + 1])?,
                                    parse_f64(&vl.params[o + 2])?,
                                ));
                            }
                        }
                        let (p0, p1) = line_entity(&entities, cptr)?;
                        let start = verts[a - 1];
                        let end = verts[b - 1];
                        if (p0 - start).magnitude() > 1e-6 || (p1 - end).magnitude() > 1e-6 {
                            return Err(error("line carrier does not match its topological endpoints"));
                        }
                        edges.push((a - 1, b - 1, kernel::Curve::Line(kernel::Line(p0, p1))));
                    }
                }
                let edge_index = index - 1;
                let (a, b, _) = edges.get(edge_index).ok_or_else(|| error("edge index unavailable"))?;
                let (start, end) = if orient { (*a, *b) } else { (*b, *a) };
                wi.push((edge_index, orient, start, end));
            }
            for pair in wi.iter().zip(wi.iter().cycle().skip(1)).take(wi.len()) {
                if pair.0.3 != pair.1.2 {
                    return Err(error("loop is not connected and closed"));
                }
            }
            loops_data.push((wi, face_use));
            surfaces.push(plane);
        }
        if edges.len() > MAX_EDGES || verts.len() > MAX_VERTICES {
            return Err(error("solid exceeds topology limits"));
        }
        let vertices = verts.iter().copied().map(kernel::builder::vertex).collect::<Vec<_>>();
        let built_edges = edges.iter().map(|(a, b, _)| kernel::builder::line(&vertices[*a], &vertices[*b])).collect::<Vec<_>>();
        let mut faces = Vec::new();
        for ((wire, face_use), surface) in loops_data.into_iter().zip(surfaces) {
            let mut oriented = Vec::with_capacity(wire.len());
            for (edge_index, edge_orient, _, _) in wire {
                let base = built_edges.get(edge_index).ok_or_else(|| error("missing constructed edge"))?.clone();
                oriented.push(if edge_orient { base } else { base.inverse() });
            }
            let w = kernel::Wire::from_iter(oriented);
            let face = kernel::builder::try_attach_plane(&[w]).map_err(|_| error("IGES planar loop could not be attached to a plane"))?;
            let face_normal = face.surface().normal(0.5, 0.5);
            let desired = surface.normal();
            let aligned = if face_normal.dot(desired) < 0.0 { face.inverse() } else { face };
            faces.push(if face_use { aligned } else { aligned.inverse() });
        }
        // IGES type 186 stores the shell orientation relative to the face orientations in
        // its type 514 shell. Ignoring a false flag preserves the boundary but reverses the
        // represented solid's inside/outside convention.
        let faces = if shell_orientation { faces } else { faces.into_iter().map(|face| face.inverse()).collect() };
        let shell = kernel::Shell::from_iter(faces);
        let solid = kernel::Solid::try_new(vec![shell]).map_err(|_| error("IGES topology is not a closed solid"))?;
        let body = Body::from_model(&solid).map_err(|e| error(e.to_string()))?;
        let mesh = body.tessellate(0.1).map_err(|e| error(e.to_string()))?;
        cadcraft_doc::brep::insert(&mut d, &body, &mesh, &Common { layer: format!("IGES solid {solid_count}"), ..Common::default() })
            .map_err(error)?;
    }
    if solid_count == 0 {
        return Err(error("file contains no type 186 exact solids"));
    }
    d.read_notes.push("IGES 5.3: imported bounded type 186 planar/linear solids; source labels and unsupported geometry are not retained".into());
    Ok(d)
}

fn point_entity(e: &HashMap<usize, Parsed>, ptr: usize) -> Result<kernel::Point3> {
    let p = e.get(&ptr).ok_or_else(|| error("missing point entity"))?;
    if p.kind != 116 || p.params.len() != 4 {
        return Err(error("expected type 116 point"));
    }
    Ok(kernel::Point3::new(parse_f64(&p.params[1])?, parse_f64(&p.params[2])?, parse_f64(&p.params[3])?))
}
fn vector_entity(e: &HashMap<usize, Parsed>, ptr: usize) -> Result<kernel::Vector3> {
    let p = e.get(&ptr).ok_or_else(|| error("missing direction entity"))?;
    if p.kind != 123 || p.params.len() != 4 {
        return Err(error("expected type 123 direction"));
    }
    let v = kernel::Vector3::new(parse_f64(&p.params[1])?, parse_f64(&p.params[2])?, parse_f64(&p.params[3])?);
    if v.magnitude() <= 1e-12 {
        return Err(error("zero direction vector"));
    }
    Ok(v.normalize())
}
fn line_entity(e: &HashMap<usize, Parsed>, ptr: usize) -> Result<(kernel::Point3, kernel::Point3)> {
    let p = e.get(&ptr).ok_or_else(|| error("missing line entity"))?;
    if p.kind != 110 || p.params.len() != 7 {
        return Err(error("expected type 110 line"));
    }
    Ok((
        kernel::Point3::new(parse_f64(&p.params[1])?, parse_f64(&p.params[2])?, parse_f64(&p.params[3])?),
        kernel::Point3::new(parse_f64(&p.params[4])?, parse_f64(&p.params[5])?, parse_f64(&p.params[6])?),
    ))
}
pub fn read(bytes: &[u8]) -> Result<Drawing> {
    if bytes.len() > MAX_BYTES {
        return Err(error("file exceeds 64 MiB limit"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| error("input must be ASCII text"))?;
    if !text.is_ascii() {
        return Err(error("input must be ASCII text"));
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_inner(text)))
        .unwrap_or_else(|_| Err(error("parser rejected malformed or unsupported topology")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_geom::Vec3;
    fn drawing(body: Body) -> Drawing {
        let mesh = body.tessellate(0.1).unwrap();
        let mut d = Drawing::new_metric();
        cadcraft_doc::brep::insert(&mut d, &body, &mesh, &Common::default()).unwrap();
        d
    }
    #[test]
    fn planar_solid_round_trips_as_type_186_exact_geometry() {
        let body = Body::box_solid(Vec3::ZERO, Vec3::new(4.0, 3.0, 2.0)).unwrap();
        let d = drawing(body.clone());
        let bytes = write(&d).unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        for ty in ["186", "514", "510", "508", "504", "502", "190", "110"] {
            assert!(text.contains(ty));
        }
        let restored = read(&bytes).unwrap();
        let group = restored.groups.first().unwrap();
        let imported = cadcraft_doc::brep::read(&restored, &group.members).unwrap().unwrap();
        assert_eq!(imported.geometry.boundaries[0].faces.len(), body.geometry.boundaries[0].faces.len());
    }
    #[test]
    fn curves_are_refused_before_any_tessellated_export() {
        let body = Body::cylinder(Vec3::ZERO, 2.0, 4.0).unwrap();
        let d = drawing(body);
        assert!(write(&d).unwrap_err().to_string().contains("curved and NURBS edges"));
    }
    #[test]
    fn malformed_oversized_and_mesh_only_files_fail_clearly() {
        let mut d = Drawing::new_metric();
        assert!(write(&d).unwrap_err().to_string().contains("mesh triangle"));
        assert!(read(&vec![0; MAX_BYTES + 1]).is_err());
        assert!(read(b"bad").is_err());
        d.groups.clear();
    }

    #[test]
    fn type_186_shell_orientation_is_applied() {
        let body = Body::box_solid(Vec3::ZERO, Vec3::new(4.0, 3.0, 2.0)).unwrap();
        let bytes = write(&drawing(body)).unwrap();
        let mut text = String::from_utf8(bytes).unwrap();
        let start = text.find("186,").expect("writer emits type 186");
        let end = text[start..].find(';').map(|n| start + n).expect("entity terminator");
        let record = &text[start..end];
        let inverted = record.replace(",1,0", ",0,0");
        assert_ne!(record, inverted);
        text.replace_range(start..end, &inverted);
        let normal_bytes = write(&drawing(Body::box_solid(Vec3::ZERO, Vec3::new(4.0, 3.0, 2.0)).unwrap())).unwrap();
        let normal = read(&normal_bytes).unwrap();
        let flipped = read(text.as_bytes()).unwrap();
        let body = |drawing: &Drawing| {
            let group = drawing.groups.first().unwrap();
            cadcraft_doc::brep::read(drawing, &group.members).unwrap().unwrap()
        };
        let normal_body = body(&normal);
        let flipped_body = body(&flipped);
        let normal_faces = &normal_body.geometry.boundaries[0].faces;
        let flipped_faces = &flipped_body.geometry.boundaries[0].faces;
        assert_eq!(normal_faces.len(), flipped_faces.len());
        assert!(normal_faces.iter().zip(flipped_faces).all(|(a, b)| a.orientation != b.orientation));
    }
}
