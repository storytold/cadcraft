//! Complete native dependency closure over authorized in-memory files; never follows a path.
use cadcraft_doc::{Drawing, EntityKind};
use cadcraft_engine::{
    cmd::file,
    linked_files::{self, Bytes, Edge, File, Input, ResourceSet},
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

fn font_file(name: &str) -> bool {
    name.contains(['/', '\\', ':', '.'])
}
fn add_ref(refs: &mut BTreeSet<(String, String)>, raw: &str, role: &str) -> Result<(), String> {
    if raw.len() > 1024 || raw.chars().any(char::is_control) || raw.split(['/', '\\']).count() > 32 {
        return Err("native reference exceeds logical path bounds".into());
    }
    if refs.len() >= linked_files::MAX_EDGES && !refs.contains(&(raw.into(), role.into())) {
        return Err("native drawing has too many resource references".into());
    }
    refs.insert((raw.into(), role.into()));
    Ok(())
}
/// Standard emitted IMAGEDEF/classes are consumed by the codec. Retained unknown data is not guessed.
fn foreign_policy(d: &Drawing) -> Result<(), String> {
    let f = &d.foreign;
    if !d.read_notes.is_empty() || !d.skipped_records.is_empty() || !f.class_order.is_empty() || !f.data_store.is_empty() {
        return Err("linked closure cannot certify incomplete/proxy/native foreign data".into());
    }
    for record in &f.classes {
        let name = record.iter().find(|tag| tag.code == 1).map(|tag| tag.value.as_str()).unwrap_or("");
        if !matches!(name, "ACDBDICTIONARYWDFLT" | "ACDBPLACEHOLDER") || record.iter().any(|tag| !matches!(tag.code, 1 | 2 | 3 | 90 | 91 | 280 | 281))
        {
            return Err("unclassified foreign DXF class in linked set".into());
        }
    }
    for object in &f.objects {
        if !matches!(object.kind.as_str(), "DICTIONARY" | "ACDBDICTIONARYWDFLT" | "ACDBPLACEHOLDER")
            || object.tags.iter().any(|tag| !matches!(tag.code, 3 | 5 | 100 | 102 | 280 | 281 | 330 | 340 | 350 | 360))
        {
            return Err("unclassified foreign DXF object in linked set".into());
        }
    }
    if f.entities.values().chain(f.records.values()).any(|extra| !extra.xdata.is_empty() || extra.props.iter().any(|tag| tag.code != 284)) {
        return Err("unclassified foreign DXF extension data in linked set".into());
    }
    Ok(())
}
/// Only recognized resource-free header strings are certified; numeric variables are metadata.
fn metadata_policy(d: &Drawing) -> Result<(), String> {
    for (key, value) in &d.header.vars {
        if value.as_str().is_some()
            && !matches!(
                key.as_str(),
                "ACADVER"
                    | "DWGCODEPAGE"
                    | "HANDSEED"
                    | "CLAYER"
                    | "CELTYPE"
                    | "TEXTSTYLE"
                    | "DIMSTYLE"
                    | "CMLSTYLE"
                    | "CMLEADERSTYLE"
                    | "CTABLESTYLE"
                    | "CANNOSCALE"
                    | "UCSNAME"
                    | "UCSBASE"
                    | "PUCSNAME"
                    | "PUCSBASE"
                    | "UCSORTHOREF"
            )
            && !cadcraft_doc::DIMVARS.iter().any(|(name, _)| *name == key.as_str())
        {
            return Err(format!("unclassified DXF header string {key}; linked closure is unsupported"));
        }
    }
    for layout in &d.layouts {
        let name = &layout.page.plot_style_table;
        if !name.is_empty() && cadcraft_doc::PlotStyleTable::builtin(name).is_none() && !d.plot_style_tables.iter().any(|table| table.name == *name) {
            return Err("external plot-style files are outside linkedFilesV1's permitted resource classes".into());
        }
    }
    Ok(())
}
/// Match the native MTEXT font-switch spelling (f/F, semicolon, optional | style flags).
fn inline_fonts(refs: &mut BTreeSet<(String, String)>, text: &str) -> Result<(), String> {
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            continue;
        }
        if !matches!(chars.next(), Some('f' | 'F')) {
            continue;
        }
        let mut name = String::new();
        for ch in chars.by_ref() {
            if ch == ';' {
                break;
            }
            if name.len().saturating_add(ch.len_utf8()) > 1024 {
                return Err("inline font switch exceeds path bounds".into());
            }
            name.push(ch);
        }
        let name = name.split('|').next().unwrap_or("").trim();
        if font_file(name) {
            add_ref(refs, name, "font")?;
        }
    }
    Ok(())
}
fn entity_refs(refs: &mut BTreeSet<(String, String)>, kind: &EntityKind) -> Result<(), String> {
    match kind {
        EntityKind::Image(image) if !image.path.is_empty() => add_ref(refs, &image.path, "image"),
        EntityKind::MText(text) => inline_fonts(refs, &text.contents),
        EntityKind::MLeader(leader) => {
            if let Some(text) = &leader.text {
                inline_fonts(refs, &text.contents)?;
            }
            Ok(())
        }
        EntityKind::Dimension(dim) => inline_fonts(refs, &dim.text),
        EntityKind::Table(table) => {
            if table.data_link.as_ref().is_some_and(|link| !link.path.is_empty()) {
                return Err("external CSV data links are outside linkedFilesV1's permitted resource classes".into());
            }
            for cell in table.cells.iter().flatten() {
                inline_fonts(refs, &cell.text)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
fn references(d: &Drawing) -> Result<BTreeSet<(String, String)>, String> {
    foreign_policy(d)?;
    metadata_policy(d)?;
    let mut refs = BTreeSet::new();
    for entity in d
        .model
        .iter()
        .chain(d.layouts.iter().flat_map(|v| v.entities.iter()))
        .chain(d.blocks.values().filter(|v| v.xref_path.is_none() && !v.name.contains('|')).flat_map(|v| v.entities.iter()))
    {
        if !entity.common.xdata.is_empty() || matches!(&entity.kind, EntityKind::Unknown(_)) {
            return Err("unclassified entity/xdata in linked resource set".into());
        }
        entity_refs(&mut refs, &entity.kind)?;
    }
    for block in d.blocks.values().filter(|v| !v.name.contains('|')) {
        if let Some(path) = &block.xref_path
            && !path.is_empty()
        {
            add_ref(&mut refs, path, "xref")?;
        }
    }
    for style in d.text_styles.iter().filter(|v| !v.name.contains('|')) {
        if font_file(&style.font) {
            add_ref(&mut refs, &style.font, "font")?;
        }
        if !style.big_font.is_empty() {
            add_ref(&mut refs, &style.big_font, "bigFont")?;
        }
    }
    Ok(refs)
}
fn file_references(bytes: &[u8], name: &str) -> Result<BTreeSet<(String, String)>, String> {
    let raw_refs =
        std::panic::catch_unwind(|| linked_files::native_font_references(bytes)).map_err(|_| "native DXF reference scan failed".to_string())??; // Enforce the fixed tag cap before the full native decoder.
    let drawing = decode(bytes, name)?;
    let mut refs = references(&drawing)?;
    for (raw, role) in raw_refs {
        add_ref(&mut refs, &raw, &role)?;
    }
    Ok(refs)
}
pub(crate) fn check_native(drawing: &Drawing) -> Result<(), String> {
    references(drawing).map(|_| ())
}
fn decode(bytes: &[u8], name: &str) -> Result<Drawing, String> {
    if bytes.starts_with(b"AC10") {
        return Err("DWG dependency closure is unavailable in the browser; supply native DXF".into());
    }
    std::panic::catch_unwind(|| (file::io().ok_or_else(|| "file formats are unavailable".to_string())?.read)(bytes, name))
        .map_err(|_| "native linked-file decoder failed".to_string())?
}
fn leaf(file: &File) -> Result<(), String> {
    if file.kind == "font" {
        return Ok(());
    } // Authorized opaque font bytes, never installed or decoded.
    let b = file.bytes.data();
    if file.kind != "raster"
        || !(b.starts_with(b"\x89PNG\r\n\x1a\n")
            || b.starts_with(b"\xff\xd8\xff")
            || b.starts_with(b"GIF87a")
            || b.starts_with(b"GIF89a")
            || b.starts_with(b"BM")
            || b.starts_with(b"II*\0")
            || b.starts_with(b"MM\0*")
            || (b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WEBP")))
    {
        return Err("unsupported resource leaf kind; SVG/containers are not dependency-free rasters".into());
    }
    Ok(())
}
fn target(from: &str, raw: &str, edge: &Edge) -> Result<(), String> {
    let absolute = raw.starts_with(['/', '\\']) || raw.contains(':');
    if !absolute && linked_files::relative_key(from, raw).as_deref() != Some(edge.target_key.as_str()) {
        return Err("relative resource path escapes or disagrees with its logical mapping".into());
    }
    Ok(())
}
fn visit(set: &ResourceSet, key: &str, depth: usize, nodes: &mut BTreeSet<String>, used: &mut BTreeSet<Edge>) -> Result<(), String> {
    if nodes.contains(key) {
        return Ok(());
    } // Record the edge before following a cycle; never re-read it.
    if depth > cadcraft_doc::MAX_BLOCK_DEPTH {
        return Err("linked drawing closure exceeds depth16".into());
    }
    let file = set.file(key).ok_or("required resource bytes are missing")?;
    nodes.insert(key.into());
    if file.kind != "dxf" {
        return leaf(file);
    }
    for (raw, role) in file_references(file.bytes.data(), &format!("{key}.dxf"))? {
        let edge = set
            .edges
            .iter()
            .find(|e| e.from_key == key && e.raw_reference == raw && e.role == role)
            .ok_or("required native reference has no authorized resource mapping")?;
        target(key, &raw, edge)?;
        let resource = set.file(&edge.target_key).ok_or("required resource is missing")?;
        let expected = match role.as_str() {
            "xref" => "dxf",
            "image" => "raster",
            _ => "font",
        };
        if resource.kind != expected {
            return Err("native reference resource kind disagrees".into());
        }
        used.insert(edge.clone());
        visit(set, &edge.target_key, depth + 1, nodes, used)?;
    }
    Ok(())
}
/// Validate all declared bytes and the actual native graph before any app mutation/ACK.
pub(crate) fn validate(input: Input, captured_main: Option<Arc<Bytes>>) -> Result<Arc<ResourceSet>, String> {
    let main = match captured_main {
        Some(main) if input.main_hash == main.hash() && input.main.as_ref().is_none_or(|supplied| supplied.hash() == main.hash()) => main,
        Some(_) => return Err("resource reply does not match the captured native DXF".into()),
        None => input.main.clone().ok_or("open set must include native main bytes")?,
    };
    let set = ResourceSet::new(input, main)?;
    let mut nodes = BTreeSet::new();
    let mut edges = BTreeSet::new();
    visit(&set, &set.main.key, 0, &mut nodes, &mut edges)?;
    if nodes.len() != set.files.len() + 1 || edges.len() != set.edges.len() {
        return Err("linked set contains undeclared/unreachable files or reference mappings".into());
    }
    Ok(set)
}
/// Rebuild only the reachable cached closure for a new immutable native capture.
pub(crate) fn cached(main: Arc<Bytes>, name: &str, source: Option<&ResourceSet>) -> Result<Arc<ResourceSet>, String> {
    let key = source.map(|s| s.main.key.clone()).unwrap_or_else(|| name.to_string());
    let mut files = BTreeMap::new();
    let mut edges = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut queue = vec![(key.clone(), main.clone(), 0usize)];
    while let Some((from, bytes, depth)) = queue.pop() {
        if !visited.insert(from.clone()) {
            continue;
        }
        if depth > cadcraft_doc::MAX_BLOCK_DEPTH || visited.len() > linked_files::MAX_FILES {
            return Err("linked closure exceeds graph limits".into());
        }
        for (raw, role) in file_references(bytes.data(), &format!("{from}.dxf"))? {
            let prior = source.and_then(|s| s.edges.iter().find(|e| e.from_key == from && e.raw_reference == raw && e.role == role));
            let to = prior
                .map(|e| e.target_key.clone())
                .or_else(|| linked_files::relative_key(&from, &raw))
                .ok_or("resource requires an explicit authorized logical mapping")?;
            edges.insert(Edge { from_key: from.clone(), raw_reference: raw, role: role.clone(), target_key: to.clone() });
            if to == key {
                continue;
            }
            let resource = source.and_then(|s| s.file(&to)).ok_or("linked resource bytes must be supplied by the authorized host")?.clone();
            if resource.kind == "dxf" {
                queue.push((to.clone(), resource.bytes.clone(), depth + 1));
            }
            files.insert(to, resource);
        }
    }
    validate(
        Input {
            main_key: key,
            name: name.into(),
            main: None,
            main_hash: main.hash().into(),
            files: files.into_values().collect(),
            edges: edges.into_iter().collect(),
            version: String::new(),
        },
        Some(main),
    )
}

/// Retain original file spelling/relative context while the native decoded family is unchanged.
/// Metadata is captured by the actual codec even before a drawing has a provider ResourceSet.
pub(crate) fn preserve_fonts(bytes: Vec<u8>, drawing: &Drawing) -> Result<Vec<u8>, String> {
    let mut replacements = BTreeMap::new();
    let mut bounded = BTreeSet::new();
    for style in drawing.text_styles.iter().filter(|style| !style.name.contains('|')) {
        if let Some((family, raw)) = &style.source_font_file
            && style.font == *family
            && raw != family
        {
            add_ref(&mut bounded, raw, "font")?;
            replacements.insert(style.name.to_ascii_uppercase(), (raw.clone(), family.clone()));
        }
    }
    linked_files::preserve_style_files(bytes, &replacements)
}
