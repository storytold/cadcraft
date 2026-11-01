//! Exact STEP exchange for closed Truck solids carried by CADCraft's grouped 3DFACE proxies.
//!
//! Mesh-only drawings are refused: STEP output must come from exact persisted B-rep bodies.

use std::collections::HashSet;

use cadcraft_doc::{Common, Drawing, EntityKind};
use cadcraft_geom::brep::{Body, CompressedSolid, kernel};
use truck_stepio::{r#in as step_in, out as step_out};

use crate::{IoError, Result};

const MAX_BYTES: usize = 64 << 20;
const MAX_BODIES: usize = 256;
const MAX_FACES: usize = 100_000;
const DISPLAY_TOLERANCE: f64 = 0.1;

fn error(s: impl Into<String>) -> IoError {
    IoError::Format(format!("STEP: {}", s.into()))
}

fn step_curve(curve: step_in::alias::Curve3D) -> Result<kernel::Curve> {
    use step_in::alias::Curve3D as C;
    match curve {
        C::Line(c) => Ok(kernel::Curve::Line(c)),
        C::BSplineCurve(c) => Ok(kernel::Curve::BSplineCurve(c)),
        C::NurbsCurve(c) => Ok(kernel::Curve::NurbsCurve(c)),
        _ => Err(error("unsupported STEP curve carrier in exact shell")),
    }
}

fn step_surface(surface: step_in::alias::Surface) -> Result<kernel::Surface> {
    use step_in::alias::{ElementarySurface as E, Surface as S, SweptCurve as W};
    match surface {
        S::ElementarySurface(surface) => match *surface {
            E::Plane(p) => Ok(kernel::Surface::Plane(p)),
            E::CylindricalSurface(p) | E::ConicalSurface(p) => {
                let mapped = p.map(|rev| kernel::RevolutedCurve::by_revolution(kernel::Curve::Line(*rev.entity_curve()), rev.origin(), rev.axis()));
                Ok(kernel::Surface::RevolutedCurve(mapped))
            }
            _ => Err(error("unsupported STEP elementary surface carrier in exact shell")),
        },
        S::BSplineSurface(s) => Ok(kernel::Surface::BSplineSurface(*s)),
        S::NurbsSurface(s) => Ok(kernel::Surface::NurbsSurface(*s)),
        S::SweptCurve(s) => match *s {
            W::RevolutedCurve(_) => Err(error("unsupported STEP swept surface carrier in exact shell")),
            W::ExtrudedCurve(_) => Err(error("unsupported STEP extruded surface carrier in exact shell")),
        },
    }
}

fn convert_shell(
    shell: truck_topology::compress::CompressedShell<kernel::Point3, step_in::alias::Curve3D, step_in::alias::Surface>,
) -> Result<truck_topology::compress::CompressedShell<kernel::Point3, kernel::Curve, kernel::Surface>> {
    Ok(truck_topology::compress::CompressedShell {
        vertices: shell.vertices,
        edges: shell
            .edges
            .into_iter()
            .map(|e| Ok(truck_topology::compress::CompressedEdge { vertices: e.vertices, curve: step_curve(e.curve)? }))
            .collect::<Result<Vec<_>>>()?,
        faces: shell
            .faces
            .into_iter()
            .map(|f| {
                Ok(truck_topology::compress::CompressedFace {
                    boundaries: f.boundaries,
                    orientation: f.orientation,
                    surface: step_surface(f.surface)?,
                })
            })
            .collect::<Result<Vec<_>>>()?,
    })
}

fn bodies(d: &Drawing) -> Result<Vec<Body>> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for group in d.groups.iter().filter(|g| g.description == "3D solid") {
        if out.len() >= MAX_BODIES {
            return Err(error("too many exact solid bodies"));
        }
        for h in &group.members {
            if !seen.insert(*h) {
                return Err(error("exact solid groups have overlapping display faces"));
            }
        }
        let body = cadcraft_doc::brep::read(d, &group.members).map_err(error)?.ok_or_else(|| error("solid group has no exact source geometry"))?;
        body.validate().map_err(|e| error(e.to_string()))?;
        out.push(body);
    }
    if out.is_empty() {
        return Err(error("no exact solid bodies; mesh triangles cannot be written as analytic STEP geometry"));
    }
    if d.model.iter().any(|e| !matches!(e.kind, EntityKind::Face3d(_))) {
        return Err(error("drawing contains non-solid model entities"));
    }
    let faces: Vec<_> = d.model.iter().filter(|e| matches!(e.kind, EntityKind::Face3d(_))).collect();
    if faces.len() != seen.len() || faces.iter().any(|e| !seen.contains(&e.handle)) {
        return Err(error("drawing contains ungrouped or non-solid display geometry"));
    }
    if faces.len() > MAX_FACES {
        return Err(error("display mesh exceeds STEP export limit"));
    }
    Ok(out)
}

pub fn write(d: &Drawing) -> Result<Vec<u8>> {
    let bodies = bodies(d)?;
    let mut models = step_out::StepModels::default();
    for body in &bodies {
        models.push_solid(&body.geometry);
    }
    let header = step_out::StepHeaderDescriptor { organization_system: "CADCraft".into(), ..Default::default() };
    let text = step_out::CompleteStepDisplay::new(models, header).to_string();
    if text.len() > MAX_BYTES {
        return Err(error("output exceeds 64 MiB limit"));
    }
    Ok(text.into_bytes())
}

fn read_inner(text: &str) -> Result<Drawing> {
    // The STEP parser converts topology and carriers, but does not apply shape-representation
    // transformation relationships or convert units. Normalize whitespace/case before checking
    // so harmless formatting cannot turn a refusal into silent misplacement or scaling.
    let normalized: String = text.chars().filter(|c| !c.is_ascii_whitespace()).flat_map(char::to_uppercase).collect();
    if [
        "NEXT_ASSEMBLY_USAGE_OCCURRENCE",
        "ITEM_DEFINED_TRANSFORMATION",
        "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION",
        "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
    {
        return Err(error("assembly or representation placements are unsupported; refusing to misplace exact shells"));
    }
    if normalized.contains("CONVERSION_BASED_UNIT") {
        return Err(error("conversion-based STEP units are unsupported; only millimetres are supported"));
    }
    let metre_units = normalized.matches(".METRE.").count();
    let millimetre_units = normalized.matches(".MILLI.,.METRE.").count();
    if !normalized.contains("GLOBAL_UNIT_ASSIGNED_CONTEXT") || metre_units == 0 || metre_units != millimetre_units {
        return Err(error("cannot verify STEP length unit; only millimetres are supported"));
    }
    let exchange = step_in::ruststep::parser::parse(text).map_err(|e| error(format!("invalid STEP syntax: {e}")))?;
    if exchange.data.len() != 1 {
        return Err(error("exactly one DATA section is supported"));
    }
    let section = exchange.data.first().ok_or_else(|| error("STEP file has no DATA section"))?;
    let table = step_in::Table::from_data_section(section);
    if table.shell.is_empty() {
        return Err(error("no exact shell entities found"));
    }
    if table.shell.len() > MAX_BODIES {
        return Err(error("too many STEP shells"));
    }
    let mut d = Drawing::new_metric();
    for (i, (_, shell)) in table.shell.iter().enumerate() {
        let compressed = table.to_compressed_shell(shell).map_err(|e| error(format!("unsupported shell topology: {e}")))?;
        let compressed = convert_shell(compressed)?;
        let body = Body { geometry: CompressedSolid { boundaries: vec![compressed] } };
        body.validate().map_err(|e| error(format!("STEP shell is not a supported closed solid: {e}")))?;
        let mesh = body.tessellate(DISPLAY_TOLERANCE).map_err(|e| error(e.to_string()))?;
        cadcraft_doc::brep::insert(&mut d, &body, &mesh, &Common { layer: format!("STEP solid {}", i + 1), ..Common::default() }).map_err(error)?;
    }
    d.read_notes.push(
        "STEP: imported closed shells as exact solids; product names, assembly placements, and source units are not retained by this exchange subset"
            .into(),
    );
    Ok(d)
}

pub fn read(bytes: &[u8]) -> Result<Drawing> {
    if bytes.len() > MAX_BYTES {
        return Err(error("file exceeds 64 MiB limit"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| error("STEP input must be UTF-8 text"))?;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_inner(text)))
        .unwrap_or_else(|_| Err(error("STEP parser rejected malformed or unsupported topology")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_geom::Vec3;

    #[test]
    fn exact_cylinder_round_trips_as_curved_geometry() {
        let body = Body::cylinder(Vec3::ZERO, 2.0, 4.0).unwrap();
        let mesh = body.tessellate(0.1).unwrap();
        let mut drawing = Drawing::new_metric();
        cadcraft_doc::brep::insert(&mut drawing, &body, &mesh, &Common::default()).unwrap();
        let bytes = write(&drawing).unwrap();
        let restored = read(&bytes).unwrap();
        let group = restored.groups.first().unwrap();
        let imported = cadcraft_doc::brep::read(&restored, &group.members).unwrap().unwrap();
        assert_eq!(imported.geometry.boundaries[0].faces.len(), body.geometry.boundaries[0].faces.len());
        assert!(imported.geometry.boundaries[0].edges.iter().any(|e| !format!("{:?}", e.curve).contains("Line")));
    }

    #[test]
    fn mesh_only_input_is_not_misrepresented_as_step() {
        let mut drawing = Drawing::new_metric();
        let corners = [Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)];
        drawing.add(&cadcraft_doc::Space::Model, Common::default(), EntityKind::Face3d(cadcraft_doc::Face3d { corners, hidden_edges: 0 })).unwrap();
        assert!(write(&drawing).unwrap_err().to_string().contains("mesh triangles"));
    }

    #[test]
    fn refuses_unsupported_placement_relationships_before_parsing_geometry() {
        for marker in ["NEXT_ASSEMBLY_USAGE_OCCURRENCE", "ITEM_DEFINED_TRANSFORMATION", "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION"] {
            let input = format!("ISO-10303-21;HEADER;ENDSEC;DATA;#1={marker}();ENDSEC;END-ISO-10303-21;");
            assert!(read(input.as_bytes()).unwrap_err().to_string().contains("placements are unsupported"));
        }
    }

    #[test]
    fn unit_check_is_case_and_whitespace_insensitive() {
        let input = "ISO-10303-21;HEADER;FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));ENDSEC;DATA;#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT ( $ , .METRE. ));#2=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)));ENDSEC;END-ISO-10303-21;";
        assert!(read(input.as_bytes()).unwrap_err().to_string().contains("millimetre"));
    }
}
