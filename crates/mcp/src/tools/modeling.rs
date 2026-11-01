//! Discoverable 3D editing tools; every operation shares the engine's transaction path.
use serde_json::{Value, json};

const COMMANDS: &[(&str, &str)] = &[
    ("create_box", "box"),
    ("create_cylinder", "cylinder"),
    ("create_cone", "cone"),
    ("create_sphere", "sphere"),
    ("create_torus", "torus"),
    ("create_pyramid", "pyramid"),
    ("extrude_profile", "extrude"),
    ("revolve_profile", "revolve"),
    ("sweep_profile", "sweep"),
    ("loft_profiles", "loft"),
    ("union_solids", "union"),
    ("subtract_solids", "subtract"),
    ("intersect_solids", "intersect"),
    ("move_3d", "move3d"),
    ("rotate_3d", "rotate3d"),
    ("set_3d_view", "view3d"),
    ("inspect_solid", "solidinfo"),
    ("refine_solid", "retessellate"),
    ("fillet_edges", "filletedge"),
    ("chamfer_edges", "chamferedge"),
    ("shell_solid", "shell"),
    ("slice_solid", "slice"),
    ("section_solid", "section"),
    ("scale_solid", "scale3d"),
    ("set_ucs", "ucs"),
    ("export_model", "export"),
    ("undo_edit", "undo"),
    ("redo_edit", "redo"),
];

pub(super) fn definitions() -> Vec<Value> {
    COMMANDS.iter().map(|(name, command)| {
        let point = json!({"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3});
        let handles = json!({"type":"array","items":{"type":"string"},"minItems":1,"maxItems":100000});
        let number = json!({"type":"number"});
        let mut properties = json!({});
        let mut required: Vec<&str> = Vec::new();
        let description = match *command {
            "box" => {properties=json!({"p1":point,"p2":point});required=vec!["p1","p2"]; "Create a box. Returns all face handles and its selectable solid group; retain these for editing."}
            "cylinder"|"cone" => {properties=json!({"center":point,"radius":number,"height":number,"segments":{"type":"integer","minimum":3,"maximum":512}});required=vec!["radius","height"]; "Create a cylindrical or conical solid at a 3D base center. Negative height extends downward."}
            "sphere" => {properties=json!({"center":point,"radius":number,"segments":{"type":"integer","minimum":3,"maximum":512},"rings":{"type":"integer","minimum":2,"maximum":256}});required=vec!["radius"]; "Create a sphere at a 3D center with bounded tessellation."}
            "torus" => {properties=json!({"center":point,"radius":number,"tubeRadius":number,"segments":{"type":"integer","minimum":3,"maximum":512},"tubeSegments":{"type":"integer","minimum":3,"maximum":512}});required=vec!["radius","tubeRadius"]; "Create a toroidal solid. The tube radius must be smaller than the major radius."}
            "pyramid" => {properties=json!({"center":point,"radius":number,"height":number,"sides":{"type":"integer","minimum":3,"maximum":256},"rotation":number});required=vec!["radius","height"]; "Create a regular pyramid at a 3D base center."}
            "union"|"intersect" => {properties=json!({"handles":handles});required=vec!["handles"]; "Combine solids. One face handle per operand identifies its entire solid. Consumes operands atomically; locked members are refused."}
            "subtract" => {properties=json!({"from":handles,"remove":handles});required=vec!["from","remove"]; "Subtract cutter solids from target solids. One face identifies each complete solid; all operands are consumed on success."}
            "move3d" => {properties=json!({"handles":handles,"offset":point});required=vec!["handles","offset"]; "Translate 3D entities in one undo step. Pass every face handle returned when creating a solid to move the whole solid."}
            "rotate3d" => {properties=json!({"handles":handles,"axis":{"type":"array","items":point,"minItems":2,"maxItems":2},"angle":number});required=vec!["handles","axis","angle"]; "Rotate 3D entities about an axis defined by two points; angle is degrees. Pass all solid face handles to rotate a whole solid."}
            "view3d" => {properties=json!({"preset":{"type":"string","enum":["top","bottom","front","back","left","right","swiso","seiso","neiso","nwiso"]},"azimuth":number,"elevation":number,"orbit":{"type":"boolean"},"projection":{"type":"string","enum":["orthographic","perspective"]},"fov":{"type":"number","minimum":10,"maximum":120},"style":{"type":"string","enum":["shaded","flat","realistic","wireframe"]},"lightDirection":point,"lightIntensity":{"type":"number","minimum":0,"maximum":4},"ambient":{"type":"number","minimum":0,"maximum":1},"specular":{"type":"number","minimum":0,"maximum":1},"shininess":{"type":"number","minimum":1,"maximum":256},"materialColor":{"type":"array","items":{"type":"integer","minimum":0,"maximum":255},"minItems":3,"maxItems":3}}); "Set viewport projection, visual style, lighting and material settings, or the camera with a standard preset or azimuth/elevation degrees. Does not alter geometry."}
            "solidinfo" => {properties=json!({"handles":handles});required=vec!["handles"]; "Inspect persisted exact source geometry and persistent shell:edge ids; refuses mesh-only or stale source records."}
            "retessellate" => {properties=json!({"handles":handles,"tolerance":{"type":"number","minimum":0.00001}});required=vec!["handles","tolerance"]; "Regenerate a solid's display mesh from its exact source; exact topology ids remain unchanged."}
            "filletedge"|"chamferedge" => {let key=if *command=="filletedge" {"radius"}else{"distance"};properties=json!({"handles":handles,"edges":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":256},"tolerance":{"type":"number","minimum":0.00001}});properties[key]=number;required=vec!["handles",key]; "Finish convex vertical edges of a straight polygonal exact prism. Omitted edges selects all vertical edges; ids come from inspect_solid. Unsupported edge configurations are refused atomically."}
            "shell" => {properties=json!({"handles":handles,"thickness":number,"openTop":{"type":"boolean"},"tolerance":{"type":"number","minimum":0.00001}});required=vec!["handles","thickness"]; "Hollow a straight convex polygonal exact prism with constant normal wall thickness; opens its upper face by default."}
            "slice"|"section" => {properties=json!({"handles":handles,"origin":point,"normal":point,"keep":{"type":"string","enum":["positive","negative","both"]}});required=vec!["handles","origin","normal"]; "Intersect grouped solids with a world-space plane. Slice replaces them with capped solid halves; section adds closed XYZ contours. Mesh intersection follows display tessellation."}
            "scale3d" => {properties=json!({"handles":handles,"factors":point,"base":point,"tolerance":{"type":"number","minimum":0.00001}});required=vec!["handles","factors"]; "Scale exact curves and surfaces by positive XYZ factors about a base point; topology ids stay stable."}
            "ucs" => {properties=json!({"action":{"type":"string","enum":["world","previous","set","save","named"]},"origin":point,"xAxis":point,"yAxis":point,"name":{"type":"string"}});required=vec!["action"]; "Define, save or restore an orthonormal user coordinate system. Interactive typed XYZ coordinates are interpreted in that frame; JSON geometry remains world-space."}
            "export" => {properties=json!({"path":{"type":"string"},"format":{"type":"string","enum":["dxf","dwg","svg","png","pdf","obj","stl","gltf","glb","3mf","step","stp","iges","igs"]}}); "Export by filename extension or format (returns base64 data when path is omitted). Does not rename the drawing or mark it saved."}
            "undo"|"redo" => "Undo or redo one engine edit. Returns the affected command labels.",
            _ => {properties=json!({"params":{"type":"object"}});required=vec!["params"]; "Build a solid from profiles. Parameters follow the corresponding engine command; use list_commands to discover its full contract."}
        };
        json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required},"annotations":annotations(command)})
    }).collect()
}

/// MCP behaviour hints. Read-only: only `solidinfo` leaves drawing and session alone. Additive
/// commands create objects (or set the camera) without replacing any; the rest replace, consume
/// or move existing solids, overwrite a saved UCS or a file, or step the history.
/// Every edit is one undo step, but clients should still confirm destructive ones.
fn annotations(command: &str) -> Value {
    let (read_only, destructive, idempotent) = match command {
        "solidinfo" => (true, false, true),
        "view3d" => (false, false, true),
        "box" | "cylinder" | "cone" | "sphere" | "torus" | "pyramid" | "extrude" | "revolve" | "sweep" | "loft" | "section" => (false, false, false),
        "export" => (false, true, true),
        _ => (false, true, false),
    };
    json!({"readOnlyHint": read_only, "destructiveHint": destructive, "idempotentHint": idempotent, "openWorldHint": false})
}

pub(super) fn dispatch(name: &str, args: &Value) -> Option<Value> {
    let (_, command) = COMMANDS.iter().find(|(tool, _)| *tool == name)?;
    let params = match *command {
        "extrude" | "revolve" | "sweep" | "loft" => args.get("params").cloned().unwrap_or(json!({})),
        _ => args.clone(),
    };
    Some(json!({"command":command,"params":params}))
}
