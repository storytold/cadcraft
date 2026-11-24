//! Proxy graphics name layers by their index in the file's LAYER table (#3094). AutoCAD keeps
//! layers that only proxy graphics use, but saves the graphics with their old indices after
//! PURGE removed a layer before them, which moves the graphics to other layers; CADCraft turns the
//! indices into the written table's on save.

use cadcraft_doc::{EntityKind, Unknown};
use cadcraft_io::{read_dxf, write_dxf};

/// A proxy graphics chunk of type `kind`, its data padded to four bytes.
fn chunk(kind: u32, data: &[u8]) -> Vec<u8> {
    let mut d = data.to_vec();
    d.resize(d.len().div_ceil(4) * 4, 0);
    let mut out = ((8 + d.len()) as u32).to_le_bytes().to_vec();
    out.extend(kind.to_le_bytes());
    out.extend(d);
    out
}

/// Four circles, each on the layer index given, in colour ByLayer.
fn graphics(indices: [u32; 4]) -> Vec<u8> {
    let mut chunks = Vec::new();
    for (i, index) in indices.into_iter().enumerate() {
        let mut circle: Vec<u8> = [25.0 + 50.0 * i as f64, 70.0, 0.0, 15.0, 0.0, 0.0, 1.0].iter().flat_map(|v| v.to_le_bytes()).collect();
        circle.truncate(56);
        chunks.push(chunk(16, &index.to_le_bytes()));
        chunks.push(chunk(14, &256u32.to_le_bytes()));
        chunks.push(chunk(2, &circle));
    }
    let body = chunks.concat();
    let mut out = ((8 + body.len()) as u32).to_le_bytes().to_vec();
    out.extend((chunks.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

/// The test drawing checked against AutoCAD: the LAYER table 0, Defpoints, A_LEER (unused),
/// B_NOTIZ, C_EXTRA, and a proxy on layer 0 drawing circles on indices 0, 3, 4 and 99 (past the
/// table). `a_leer_handle: false` leaves A_LEER's handle out.
fn file(a_leer_handle: bool) -> String {
    let g = graphics([0, 3, 4, 99]);
    let mut tags: Vec<(i32, String)> = Vec::new();
    let mut push = |list: &[(i32, &str)]| tags.extend(list.iter().map(|(c, v)| (*c, v.to_string())));
    push(&[(0, "SECTION"), (2, "CLASSES"), (0, "CLASS"), (1, "SYNLAYERTEST"), (2, "SynLayerTest"), (3, "SyntheticProxyApp")]);
    push(&[(90, "0"), (280, "0"), (281, "1"), (0, "ENDSEC"), (0, "SECTION"), (2, "TABLES"), (0, "TABLE"), (2, "LAYER")]);
    for (handle, name, color) in [("10", "0", "7"), ("11", "Defpoints", "7"), ("12", "A_LEER", "1"), ("13", "B_NOTIZ", "3"), ("14", "C_EXTRA", "5")] {
        push(&[(0, "LAYER")]);
        if a_leer_handle || name != "A_LEER" {
            push(&[(5, handle)]);
        }
        push(&[(2, name), (62, color)]);
    }
    push(&[(0, "ENDTAB"), (0, "ENDSEC"), (0, "SECTION"), (2, "ENTITIES"), (0, "ACAD_PROXY_ENTITY"), (5, "21"), (100, "AcDbEntity"), (8, "0")]);
    push(&[(100, "AcDbProxyEntity"), (90, "498"), (91, "500"), (95, "0"), (70, "0")]);
    tags.push((92, g.len().to_string()));
    tags.extend(g.chunks(127).map(|c| (310, c.iter().map(|b| format!("{b:02X}")).collect())));
    tags.extend([(93, "0".to_string()), (94, "0".to_string()), (0, "ENDSEC".to_string()), (0, "EOF".to_string())]);
    tags.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect()
}

fn proxy(d: &cadcraft_doc::Drawing) -> &Unknown {
    d.model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Unknown(u) => Some(u),
            _ => None,
        })
        .expect("proxy")
}

/// The layers the four circles draw on.
fn circle_layers(d: &cadcraft_doc::Drawing) -> Vec<String> {
    proxy(d).graphics.iter().map(|g| g.common.layer.clone()).collect()
}

/// The layer indices in the proxy's graphics bytes.
fn indices(d: &cadcraft_doc::Drawing) -> Vec<u32> {
    let b = proxy(d).proxy_graphics_bytes();
    let mut at = 8;
    let mut out = Vec::new();
    while at + 8 <= b.len() {
        let size = u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize;
        if u32::from_le_bytes(b[at + 4..at + 8].try_into().unwrap()) == 16 {
            out.push(u32::from_le_bytes(b[at + 8..at + 12].try_into().unwrap()));
        }
        at += size;
    }
    out
}

#[test]
fn indices_resolve_in_the_files_table_and_one_past_it_keeps_the_layer() {
    let d = read_dxf(file(true).as_bytes()).expect("read");
    // As AutoCAD draws it: index 99 keeps the layer before it.
    assert_eq!(circle_layers(&d), ["0", "B_NOTIZ", "C_EXTRA", "C_EXTRA"]);
    // Saved unchanged when the table is.
    let again = read_dxf(write_dxf(&d).as_bytes()).expect("reopen");
    assert_eq!((indices(&again), circle_layers(&again)), (vec![0, 3, 4, 99], circle_layers(&d)));
}

/// AutoCAD's PURGE removes A_LEER and Defpoints and keeps the two layers the graphics use; saved
/// as AutoCAD saves it, indices 3 and 4 would point past the three-entry table. CADCraft writes
/// them as 1 and 2.
#[test]
fn removing_a_layer_before_the_ones_the_graphics_use_keeps_them_on_save() {
    let mut d = read_dxf(file(true).as_bytes()).expect("read");
    d.layers.retain(|l| l.name != "A_LEER" && l.name != "Defpoints");
    let again = read_dxf(write_dxf(&d).as_bytes()).expect("reopen");
    let names: Vec<&str> = again.layers.iter().map(|l| l.name.as_str()).take(3).collect();
    assert_eq!(names, ["0", "B_NOTIZ", "C_EXTRA"]);
    assert_eq!(circle_layers(&again), ["0", "B_NOTIZ", "C_EXTRA", "C_EXTRA"]);
    // 99 stays past the table.
    assert_eq!(indices(&again), [0, 1, 2, 99]);
}

/// Renaming a layer the graphics use moves them with it at once, and saving keeps them there.
#[test]
fn renaming_a_layer_the_graphics_use_keeps_them_on_it() {
    let mut d = read_dxf(file(true).as_bytes()).expect("read");
    d.layer_mut("B_NOTIZ").expect("layer").name = "B_NEU".into();
    d.rename_proxy_layer("B_NOTIZ", "B_NEU");
    assert_eq!(circle_layers(&d), ["0", "B_NEU", "C_EXTRA", "C_EXTRA"]);
    let again = read_dxf(write_dxf(&d).as_bytes()).expect("reopen");
    assert_eq!(circle_layers(&again), ["0", "B_NEU", "C_EXTRA", "C_EXTRA"]);
}

/// A LAYER record without a handle (AutoCAD rejects such a file) still counts in the index
/// list: the layers after it don't move up by one.
#[test]
fn a_layer_record_without_a_handle_keeps_its_place_in_the_index() {
    let d = read_dxf(file(false).as_bytes()).expect("read");
    assert_eq!(circle_layers(&d), ["0", "B_NOTIZ", "C_EXTRA", "C_EXTRA"]);
}

/// A proxy copied into a drawing whose LAYER table has another order keeps the layers it shows:
/// its indices are named by its own graphics, not by the other file's table.
#[test]
fn a_proxy_copied_from_another_drawing_keeps_its_layers() {
    let source = read_dxf(file(true).as_bytes()).expect("read");
    let proxy = source.model.iter().find(|e| matches!(e.kind, EntityKind::Unknown(_))).expect("proxy").as_ref().clone();
    // The target: the same layers in another order (C_EXTRA, B_NOTIZ before A_LEER), read from a
    // file so it has a table of its own.
    let mut target = read_dxf(file(true).as_bytes()).expect("read");
    target.model = Default::default();
    target.proxy_layers = vec!["0".into(), "C_EXTRA".into(), "B_NOTIZ".into(), "Defpoints".into(), "A_LEER".into()];
    let order = ["0", "C_EXTRA", "B_NOTIZ", "Defpoints", "A_LEER"];
    target.layers.sort_by_key(|l| order.iter().position(|n| *n == l.name).unwrap_or(99));
    target.add(&cadcraft_doc::Space::Model, proxy.common.clone(), proxy.kind.clone()).expect("add");
    let again = read_dxf(write_dxf(&target).as_bytes()).expect("reopen");
    assert_eq!(circle_layers(&again), ["0", "B_NOTIZ", "C_EXTRA", "C_EXTRA"]);
}
