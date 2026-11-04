//! Immutable, bounded linked-file bytes. No path in this module is filesystem authority.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

pub const MAX_BYTES: usize = 80 * 1024 * 1024;
pub const MAX_FILES: usize = 256;
pub const MAX_EDGES: usize = 1024;
const MAX_RESIDENT_FILES: usize = 1024;
static USED: AtomicUsize = AtomicUsize::new(0);
static COUNT: AtomicUsize = AtomicUsize::new(0);
static POOL: OnceLock<Mutex<HashMap<String, Weak<Bytes>>>> = OnceLock::new();

/// One resident allocation is charged once even when captures and undo share it.
#[derive(Debug)]
pub struct Bytes {
    data: Vec<u8>,
    hash: String,
}
impl Drop for Bytes {
    fn drop(&mut self) {
        USED.fetch_sub(self.data.len(), Ordering::SeqCst);
        COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}
impl Bytes {
    pub fn new(data: Vec<u8>) -> Result<Arc<Self>, String> {
        if data.is_empty() || data.len() > MAX_BYTES {
            return Err("linked file is empty or exceeds80MiB".into());
        }
        let hash = format!("{:x}", Sha256::digest(&data));
        let pool = POOL.get_or_init(Mutex::default);
        let mut pool = pool.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        pool.retain(|_, value| value.strong_count() != 0);
        if let Some(found) = pool.get(&hash).and_then(Weak::upgrade) {
            if found.data == data {
                return Ok(found);
            }
            return Err("linked-file digest collision".into());
        }
        let size = USED.load(Ordering::SeqCst).checked_add(data.len()).ok_or("linked-file budget overflow")?;
        if size > MAX_BYTES || COUNT.load(Ordering::SeqCst) >= MAX_RESIDENT_FILES {
            return Err("resident linked-file snapshots exceed the80MiB/file-count budget".into());
        }
        USED.fetch_add(data.len(), Ordering::SeqCst);
        COUNT.fetch_add(1, Ordering::SeqCst);
        let result = Arc::new(Self { data, hash: hash.clone() });
        pool.insert(hash, Arc::downgrade(&result));
        Ok(result)
    }
    /// Conservative pre-copy charge; matching resident content is already charged globally.
    pub fn additional_charge(hash: &str, length: usize) -> usize {
        let pool = POOL.get_or_init(Mutex::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if pool.get(hash).and_then(Weak::upgrade).is_some_and(|file| file.data.len() == length) { 0 } else { length }
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    pub fn hash(&self) -> &str {
        &self.hash
    }
    pub fn retained() -> usize {
        USED.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Debug)]
pub struct File {
    pub key: String,
    pub kind: String,
    pub bytes: Arc<Bytes>,
}
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Edge {
    pub from_key: String,
    pub raw_reference: String,
    pub role: String,
    pub target_key: String,
}

/// Transport staging owns copied bytes but has not yet validated the DXF dependency graph.
#[derive(Clone, Debug)]
pub struct Input {
    pub main_key: String,
    pub name: String,
    pub main: Option<Arc<Bytes>>,
    pub main_hash: String,
    pub files: Vec<File>,
    pub edges: Vec<Edge>,
    pub version: String,
}

/// A structurally checked set. The provider consumer additionally verifies its native graph.
#[derive(Clone, Debug)]
pub struct ResourceSet {
    pub main: File,
    pub name: String,
    pub files: Vec<File>,
    pub edges: Vec<Edge>,
    pub version: String,
    pub authority: Option<(String, String)>,
    pub grant_connection: Option<String>,
    digest: String,
}
impl ResourceSet {
    pub fn new(input: Input, main: Arc<Bytes>) -> Result<Arc<Self>, String> {
        if !input.name.to_ascii_lowercase().ends_with(".dxf")
            || input.name.len() > 255
            || input.name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\'))
            || input.version.len() > 160
            || input.version.chars().any(char::is_control)
            || input.main_hash != main.hash()
        {
            return Err("invalid linked-file native name, version or main digest".into());
        }
        let declarations = input.files.iter().map(|file| (file.key.clone(), file.kind.clone())).collect::<Vec<_>>();
        validate_declarations(&input.main_key, &declarations, &input.edges)?;
        let mut total = main.data().len();
        let mut files = input.files;
        for file in &files {
            total = total.checked_add(file.bytes.data().len()).ok_or("linked-file byte count overflow")?;
            if total > MAX_BYTES {
                return Err("linked-file declared bytes exceed80MiB".into());
            }
        }
        files.sort_by(|a, b| a.key.cmp(&b.key));
        let mut edges = input.edges;
        edges.sort();
        let mut set = Self {
            main: File { key: input.main_key, kind: "dxf".into(), bytes: main },
            name: input.name,
            files,
            edges,
            version: input.version,
            authority: None,
            grant_connection: None,
            digest: String::new(),
        };
        let encoded = serde_json::to_vec(&canonical(set.manifest())).map_err(|e| e.to_string())?;
        set.digest = format!("{:x}", Sha256::digest(encoded));
        Ok(Arc::new(set))
    }
    /// Open an authorized DXF dependency as another native drawing, retaining its reachable set.
    pub fn subdrawing(&self, key: &str) -> Result<Arc<Self>, String> {
        let main = self.file(key).filter(|file| file.kind == "dxf").ok_or("xref bytes are not an authorized native DXF")?;
        let mut pending = vec![key.to_string()];
        let mut visited = std::collections::BTreeSet::new();
        let mut edges = Vec::new();
        while let Some(from) = pending.pop() {
            if !visited.insert(from.clone()) {
                continue;
            }
            for edge in self.edges.iter().filter(|edge| edge.from_key == from) {
                edges.push(edge.clone());
                pending.push(edge.target_key.clone());
            }
        }
        let files = visited
            .into_iter()
            .filter(|item| item != key)
            .map(|item| self.file(&item).cloned().ok_or_else(|| "authorized xref closure is missing bytes".to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let input = Input {
            main_key: key.into(),
            name: "Reference.dxf".into(),
            main: None,
            main_hash: main.bytes.hash().into(),
            files,
            edges,
            version: self.version.clone(),
        };
        let mut result = Self::new(input, main.bytes.clone())?;
        let subset = Arc::make_mut(&mut result);
        subset.authority = self.authority.clone();
        subset.grant_connection = self.grant_connection.clone();
        Ok(result)
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn owned_by(&self, provider: String, namespace: String, connection: String) -> Arc<Self> {
        let mut set = self.clone();
        set.authority = Some((provider, namespace));
        set.grant_connection = Some(connection);
        Arc::new(set)
    }
    /// A connection ID correlates an explicit host authorization assertion; it is not a credential.
    pub fn granted_to(&self, provider: &str, namespace: &str, connection: &str) -> bool {
        self.authority.as_ref().is_some_and(|owner| owner.0 == provider && owner.1 == namespace)
            && self.grant_connection.as_deref() == Some(connection)
    }
    pub fn committed(&self, version: String) -> Arc<Self> {
        let mut set = self.clone();
        set.version = version;
        Arc::new(set)
    }
    pub fn file(&self, key: &str) -> Option<&File> {
        if key == self.main.key { Some(&self.main) } else { self.files.iter().find(|file| file.key == key) }
    }
    /// Fixed JSON shape/field ordering and sorted arrays define the version1 content digest.
    pub fn manifest(&self) -> Value {
        let row = |f: &File| json!({"logicalKey":f.key,"kind":f.kind,"bytes":f.bytes.data().len(),"sha256":f.bytes.hash()});
        json!({"schema":"cadcraft-linked-files-v1","name":self.name,"main":row(&self.main),"files":self.files.iter().map(row).collect::<Vec<_>>(),"edges":self.edges})
    }
    pub fn content_eq(a: &Option<Arc<Self>>, b: &Option<Arc<Self>>) -> bool {
        a.as_ref().map(|v| v.digest()) == b.as_ref().map(|v| v.digest())
    }
}

pub fn logical_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 1024
        && !key.starts_with('/')
        && !key.contains(['\\', ':'])
        && !key.chars().any(char::is_control)
        && key.split('/').count() <= 32
        && key.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}
/// Stored relative paths resolve only inside a logical set. Absolute references need explicit edges.
pub fn relative_key(from: &str, raw: &str) -> Option<String> {
    let raw = raw.replace('\\', "/");
    if raw.starts_with('/') || raw.contains(':') {
        return None;
    }
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for item in raw.split('/') {
        match item {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            item => parts.push(item),
        }
    }
    let key = parts.join("/");
    logical_key(&key).then_some(key)
}

/// Recursively sort keys even if an unrelated crate enables serde_json/preserve_order.
fn canonical(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<_, _> = map.into_iter().collect();
            Value::Object(sorted.into_iter().map(|(key, value)| (key, canonical(value))).collect())
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonical).collect()),
        value => value,
    }
}
/// Structural closure admission shared by native validation and pre-copy WASM ingress.
/// It validates declarations only; the app consumer still decodes every actual native file.
pub fn validate_declarations(main: &str, files: &[(String, String)], edges: &[Edge]) -> Result<(), String> {
    if !logical_key(main) || files.len() >= MAX_FILES || edges.len() > MAX_EDGES {
        return Err("linked declaration count/root exceeds bounds".into());
    }
    let mut keys = BTreeMap::new();
    keys.insert(main, "dxf");
    let mut folded = std::collections::HashSet::new();
    folded.insert(main.to_lowercase());
    for (key, kind) in files {
        if !logical_key(key) || !matches!(kind.as_str(), "dxf" | "raster" | "font") || !folded.insert(key.to_lowercase()) {
            return Err("linked resource keys/kinds collide or exceed bounds".into());
        }
        keys.insert(key.as_str(), kind.as_str());
    }
    let mut seen = std::collections::HashSet::new();
    for edge in edges {
        declared_edge(&keys, edge)?;
        if !seen.insert((&edge.from_key, &edge.raw_reference, &edge.role)) {
            return Err("duplicate linked reference declaration".into());
        }
    }
    declared_reachability(main, &keys, edges)
}
fn declared_reachability(main: &str, keys: &BTreeMap<&str, &str>, edges: &[Edge]) -> Result<(), String> {
    let indexes: BTreeMap<_, _> = keys.keys().enumerate().map(|(index, key)| (*key, index)).collect();
    let root = indexes.get(main).copied().ok_or("linked root has no graph node")?;
    let mut next = vec![Vec::new(); keys.len()];
    for edge in edges {
        let from = indexes.get(edge.from_key.as_str()).copied().ok_or("linked edge owner is missing")?;
        let to = indexes.get(edge.target_key.as_str()).copied().ok_or("linked edge target is missing")?;
        next.get_mut(from).ok_or("linked edge owner exceeds graph bounds")?.push(to);
    }
    // Different raw relations to the same file do not create different nesting paths.
    for targets in &mut next {
        targets.sort_unstable();
        targets.dedup();
    }
    let mut depth = DepthAdmission::new(next)?;
    let reachable = depth.reachable.get(root).ok_or("linked root exceeds graph bounds")?;
    if reachable.iter().map(|word| word.count_ones() as usize).sum::<usize>() != keys.len() {
        return Err("linked declaration contains unreachable resources".into());
    }
    depth.visit(root, 0, [0; NODE_WORDS])
}

const NODE_WORDS: usize = MAX_FILES.div_ceil(64);
type NodeMask = [u64; NODE_WORDS];
const MAX_DEPTH_VISITS: usize = 1_048_576;
const MAX_DEPTH_CONTEXTS: usize = 65_536;

/// Only fully proved continuations are cached. Reachable active ancestors are part of the key:
/// a deeper visit blocked by another ancestor cannot prove an unrelated, shallower visit safe.
struct DepthAdmission {
    next: Vec<Vec<usize>>,
    reachable: Vec<NodeMask>,
    upper_bounds: Vec<usize>,
    proven: HashMap<(usize, NodeMask), usize>,
    visits: usize,
}
impl DepthAdmission {
    fn new(next: Vec<Vec<usize>>) -> Result<Self, String> {
        let reachable = (0..next.len()).map(|node| reachable_nodes(&next, node)).collect::<Result<Vec<_>, _>>()?;
        let upper_bounds = path_upper_bounds(&next, &reachable)?;
        Ok(Self { next, reachable, upper_bounds, proven: HashMap::new(), visits: 0 })
    }
    fn visit(&mut self, node: usize, depth: usize, mut active: NodeMask) -> Result<(), String> {
        if self.visits >= MAX_DEPTH_VISITS {
            return Err("linked depth analysis exceeded 1048576 visits; nesting was not certified".into());
        }
        self.visits += 1;
        if mask_contains(&active, node) {
            return Ok(());
        } // Retain a cycle-closing edge, never expand it.
        if depth > crate::doc::MAX_BLOCK_DEPTH {
            return Err("declared linked closure exceeds depth16".into());
        }
        mask_insert(&mut active, node)?;
        let reachable = self.reachable.get(node).ok_or("linked depth node is missing")?;
        let mut blocked = active;
        for (word, can_reach) in blocked.iter_mut().zip(reachable) {
            *word &= *can_reach;
        }
        if self.proven.get(&(node, blocked)).is_some_and(|prior| *prior >= depth) {
            return Ok(());
        }
        // Every continuation visits distinct, previously inactive reachable nodes. This upper
        // bound also admits dense legal cycles without enumerating their path permutations.
        let remaining = reachable.iter().zip(&active).map(|(word, used)| (*word & !*used).count_ones() as usize).sum::<usize>();
        let upper = self.upper_bounds.get(node).copied().ok_or("linked path certificate is missing")?;
        if remaining.min(upper) > crate::doc::MAX_BLOCK_DEPTH - depth {
            let next = self.next.get(node).cloned().ok_or("linked depth successors are missing")?;
            for to in next {
                self.visit(to, depth + 1, active)?;
            }
        }
        self.remember(node, blocked, depth);
        Ok(())
    }
    fn remember(&mut self, node: usize, blocked: NodeMask, depth: usize) {
        if let Some(prior) = self.proven.get_mut(&(node, blocked)) {
            *prior = (*prior).max(depth);
        } else if self.proven.len() < MAX_DEPTH_CONTEXTS {
            self.proven.insert((node, blocked), depth);
        }
        // A full cache only loses an optimization; it never supplies a positive verdict.
    }
}
/// Cheap certificates are computed once per graph, never once per DFS visit.
/// Ignoring direction and other active ancestors gives a conservative continuation superset.
fn path_upper_bounds(next: &[Vec<usize>], reachable: &[NodeMask]) -> Result<Vec<usize>, String> {
    let adjacent = undirected_neighbors(next)?;
    let cover = greedy_vertex_cover(&adjacent)?;
    reachable
        .iter()
        .enumerate()
        .map(|(node, can_reach)| {
            let vertices = can_reach.iter().map(|word| word.count_ones() as usize).sum::<usize>().saturating_sub(1);
            let component = component_after_start(&adjacent, node, can_reach)?;
            let covered = can_reach.iter().zip(&cover).map(|(word, in_cover)| (*word & *in_cover).count_ones() as usize).sum::<usize>();
            let outside_cover = covered.saturating_mul(2);
            let cover_bound = if mask_contains(&cover, node) { outside_cover.saturating_sub(1) } else { outside_cover };
            Ok(vertices.min(component).min(cover_bound))
        })
        .collect()
}
fn undirected_neighbors(next: &[Vec<usize>]) -> Result<Vec<Vec<usize>>, String> {
    let mut adjacent = vec![Vec::new(); next.len()];
    for (from, targets) in next.iter().enumerate() {
        for &to in targets {
            if from == to {
                continue;
            } // A self-loop closes the active path; it adds no fresh vertex.
            adjacent.get_mut(from).ok_or("linked undirected owner is missing")?.push(to);
            adjacent.get_mut(to).ok_or("linked undirected target is missing")?.push(from);
        }
    }
    for targets in &mut adjacent {
        targets.sort_unstable();
        targets.dedup();
    }
    Ok(adjacent)
}
fn component_after_start(adjacent: &[Vec<usize>], start: usize, can_reach: &NodeMask) -> Result<usize, String> {
    let mut seen = [0; NODE_WORDS];
    mask_insert(&mut seen, start)?;
    let mut largest = 0;
    for node in 0..adjacent.len() {
        if mask_contains(can_reach, node) && !mask_contains(&seen, node) {
            largest = largest.max(component_size(adjacent, node, can_reach, &mut seen)?);
        }
    }
    Ok(largest)
}
fn component_size(adjacent: &[Vec<usize>], start: usize, can_reach: &NodeMask, seen: &mut NodeMask) -> Result<usize, String> {
    let mut pending = vec![start];
    let mut count = 0;
    while let Some(node) = pending.pop() {
        if !mask_contains(can_reach, node) || !mask_insert(seen, node)? {
            continue;
        }
        count += 1;
        pending.extend(adjacent.get(node).ok_or("linked component node is missing")?.iter().copied());
    }
    Ok(count)
}
fn cover_candidate(adjacent: &[Vec<usize>], remaining: &NodeMask) -> Option<(usize, usize)> {
    adjacent
        .iter()
        .enumerate()
        .filter(|(node, _)| mask_contains(remaining, *node))
        .map(|(node, targets)| (node, targets.iter().filter(|to| mask_contains(remaining, **to)).count()))
        .max_by_key(|(_, degree)| *degree)
}
/// Any valid cover supplies a safe bound; greedy maximum uncovered degree need not be optimal.
fn greedy_vertex_cover(adjacent: &[Vec<usize>]) -> Result<NodeMask, String> {
    let mut remaining = [0; NODE_WORDS];
    let mut cover = [0; NODE_WORDS];
    for node in 0..adjacent.len() {
        mask_insert(&mut remaining, node)?;
    }
    for _ in 0..adjacent.len() {
        let Some((node, degree)) = cover_candidate(adjacent, &remaining) else {
            break;
        };
        if degree == 0 {
            break;
        }
        mask_insert(&mut cover, node)?;
        mask_remove(&mut remaining, node)?;
    }
    if !adjacent.iter().enumerate().all(|(from, targets)| mask_contains(&cover, from) || targets.iter().all(|to| mask_contains(&cover, *to))) {
        return Err("linked path bound did not produce a valid vertex cover".into());
    }
    Ok(cover)
}

fn mask_contains(mask: &NodeMask, node: usize) -> bool {
    mask.get(node / 64).is_some_and(|word| *word & (1u64 << (node % 64)) != 0)
}
fn mask_insert(mask: &mut NodeMask, node: usize) -> Result<bool, String> {
    let word = mask.get_mut(node / 64).ok_or("linked graph exceeds its 256-file mask")?;
    let bit = 1u64 << (node % 64);
    let fresh = *word & bit == 0;
    *word |= bit;
    Ok(fresh)
}
fn mask_remove(mask: &mut NodeMask, node: usize) -> Result<(), String> {
    let word = mask.get_mut(node / 64).ok_or("linked cover exceeds its 256-file mask")?;
    *word &= !(1u64 << (node % 64));
    Ok(())
}
fn reachable_nodes(next: &[Vec<usize>], start: usize) -> Result<NodeMask, String> {
    let mut found = [0; NODE_WORDS];
    let mut pending = vec![start];
    while let Some(node) = pending.pop() {
        if !mask_insert(&mut found, node)? {
            continue;
        }
        pending.extend(next.get(node).ok_or("linked graph successor is missing")?.iter().copied());
    }
    Ok(found)
}
fn declared_edge(keys: &BTreeMap<&str, &str>, edge: &Edge) -> Result<(), String> {
    let expected = match edge.role.as_str() {
        "xref" => "dxf",
        "image" => "raster",
        "font" | "bigFont" => "font",
        _ => return Err("unsupported reference role".into()),
    };
    if edge.raw_reference.is_empty()
        || edge.raw_reference.len() > 1024
        || edge.raw_reference.chars().any(char::is_control)
        || edge.raw_reference.split(['/', '\\']).count() > 32
        || keys.get(edge.from_key.as_str()) != Some(&"dxf")
        || keys.get(edge.target_key.as_str()) != Some(&expected)
    {
        return Err("invalid linked reference declaration".into());
    }
    let absolute = edge.raw_reference.starts_with(['/', '\\']) || edge.raw_reference.contains(':');
    if !absolute && relative_key(&edge.from_key, &edge.raw_reference).as_deref() != Some(edge.target_key.as_str()) {
        return Err("relative reference escapes or disagrees with its logical mapping".into());
    }
    Ok(())
}
/// Read raw STYLE file references that the real codec may normalize into family metadata.
/// The built-in writer's bare `txt` fallback remains metadata; explicit filenames need bytes.
pub fn native_font_references(bytes: &[u8]) -> Result<Vec<(String, String)>, String> {
    let tags = cadcraft_dxf::parse_with_limit(bytes, 4_000_000).map_err(|e| e.to_string())?;
    let mut refs = std::collections::BTreeSet::new();
    let (mut style, mut extension, mut shape) = (false, false, false);
    let (mut font, mut big) = ("", "");
    for tag in &tags {
        if tag.code == 0 {
            if style {
                raw_style_refs(&mut refs, font, big, shape)?;
            }
            style = matches!(&tag.value,cadcraft_dxf::Value::Str(value) if value=="STYLE");
            extension = false;
            shape = false;
            font = "";
            big = "";
            continue;
        }
        if tag.code >= 1000 {
            extension = true;
        }
        if !style || extension {
            continue;
        }
        match (tag.code, &tag.value) {
            (3, cadcraft_dxf::Value::Str(value)) => font = value,
            (4, cadcraft_dxf::Value::Str(value)) => big = value,
            (70, _) => shape = tag.i64() & 1 != 0,
            _ => {}
        }
    }
    if style {
        raw_style_refs(&mut refs, font, big, shape)?;
    }
    Ok(refs.into_iter().collect())
}
fn raw_style_refs(refs: &mut std::collections::BTreeSet<(String, String)>, font: &str, big: &str, shape: bool) -> Result<(), String> {
    if shape && font.trim().is_empty() {
        return Err("shape STYLE has no authorized font-file reference".into());
    }
    for (raw, role, required) in [(font, "font", shape || font.contains(['/', '\\', ':', '.'])), (big, "bigFont", !big.is_empty())] {
        if !required || raw.is_empty() {
            continue;
        }
        if raw.len() > 1024
            || raw.chars().any(char::is_control)
            || raw.split(['/', '\\']).count() > 32
            || (refs.len() >= MAX_EDGES && !refs.contains(&(raw.into(), role.into())))
        {
            return Err("raw native font references exceed resource bounds".into());
        }
        refs.insert((raw.into(), role.into()));
    }
    Ok(())
}

/// Restore only unchanged normalized STYLE file references in an explicit linked capture.
/// Ordinary native/local writers are untouched; the retained family xdata keeps decoding stable.
pub fn preserve_style_files(bytes: Vec<u8>, replacements: &BTreeMap<String, (String, String)>) -> Result<Vec<u8>, String> {
    if replacements.is_empty() {
        return Ok(bytes);
    }
    let binary = cadcraft_dxf::is_binary(&bytes);
    let tags = cadcraft_dxf::parse_with_limit(&bytes, 4_000_000).map_err(|error| error.to_string())?;
    fn record(
        tags: &mut Vec<cadcraft_dxf::Tag>,
        replacements: &BTreeMap<String, (String, String)>,
        used: &mut std::collections::BTreeSet<String>,
    ) -> Result<(), String> {
        if !tags.first().is_some_and(|tag| tag.code == 0 && tag.str() == "STYLE") {
            return Ok(());
        }
        let name =
            tags.iter().take_while(|tag| tag.code < 1000).find(|tag| tag.code == 2).map(|tag| tag.str().to_ascii_uppercase()).unwrap_or_default();
        let Some((raw, family)) = replacements.get(&name) else {
            return Ok(());
        };
        let font = tags.iter_mut().take_while(|tag| tag.code < 1000).find(|tag| tag.code == 3).ok_or("native STYLE has no font-file field")?;
        font.value = cadcraft_dxf::Value::Str(raw.clone());
        used.insert(name);
        let family_at = tags
            .windows(3)
            .position(|values| {
                values.first().is_some_and(|tag| tag.code == 1001 && tag.str() == "CADCRAFT")
                    && values.get(1).is_some_and(|tag| tag.code == 1000 && tag.str() == "FONT")
                    && values.get(2).is_some_and(|tag| tag.code == 1000)
            })
            .map(|index| index + 2);
        if let Some(tag) = family_at.and_then(|index| tags.get_mut(index)) {
            tag.value = cadcraft_dxf::Value::Str(family.clone());
        } else {
            tags.extend([cadcraft_dxf::Tag::s(1001, "CADCRAFT"), cadcraft_dxf::Tag::s(1000, "FONT"), cadcraft_dxf::Tag::s(1000, family.as_str())]);
        }
        Ok(())
    }
    let mut output = Vec::new();
    let mut current = Vec::new();
    let mut used = std::collections::BTreeSet::new();
    for tag in tags {
        if tag.code == 0 && !current.is_empty() {
            record(&mut current, replacements, &mut used)?;
            output.append(&mut current);
            if output.len() > 4_000_000 {
                return Err("preserved native STYLE metadata exceeds tag limit".into());
            }
        }
        current.push(tag);
    }
    record(&mut current, replacements, &mut used)?;
    output.append(&mut current);
    if output.len() > 4_000_000 {
        return Err("preserved native STYLE metadata exceeds tag limit".into());
    }
    if used.len() != replacements.len() {
        return Err("native encoder omitted a source STYLE required by the linked capture".into());
    }
    let bytes = if binary { cadcraft_dxf::write_binary(&output, false) } else { cadcraft_dxf::write_ascii(&output).into_bytes() };
    if bytes.len() > MAX_BYTES {
        return Err("preserved native DXF exceeds80MiB".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod depth_tests {
    use super::{DepthAdmission, MAX_DEPTH_VISITS, NODE_WORDS};

    #[test]
    fn linked_depth_analysis_budget_never_supplies_a_positive_verdict() -> Result<(), String> {
        let mut depth = DepthAdmission::new(vec![vec![]])?;
        depth.visits = MAX_DEPTH_VISITS;
        let error = depth.visit(0, 0, [0; NODE_WORDS]).err().ok_or("exhausted analysis was accepted")?;
        assert!(error.contains("nesting was not certified"));
        assert!(depth.proven.is_empty());
        Ok(())
    }
    #[test]
    fn linked_depth_dense_legal_cycle_uses_the_reachable_node_upper_bound() -> Result<(), String> {
        let next: Vec<Vec<usize>> = (0..17).map(|from| (0..17).filter(|to| *to != from).collect()).collect();
        declare_graph(&next)?;
        let mut depth = DepthAdmission::new(next)?;
        depth.visit(0, 0, [0; NODE_WORDS])?;
        assert_eq!(depth.visits, 1, "seventeen nodes cannot form a simple path deeper than16");
        Ok(())
    }

    fn small_cover_graph(large: usize, small: usize, small_clique: bool) -> Vec<Vec<usize>> {
        let total = large + small;
        (0..total)
            .map(|from| {
                let mut targets: Vec<_> = if from < large { (large..total).collect() } else { (0..large).collect() };
                if small_clique && from >= large {
                    targets.extend((large..total).filter(|to| *to != from));
                }
                targets
            })
            .collect()
    }
    fn declare_graph(next: &[Vec<usize>]) -> Result<(), String> {
        declare_graph_from(next, 0)
    }
    fn declare_graph_from(next: &[Vec<usize>], start: usize) -> Result<(), String> {
        let names: Vec<_> = (0..next.len()).map(|node| format!("n{node:03}.dxf")).collect();
        let root = names.get(start).ok_or("fixture root missing")?;
        let files = names.iter().enumerate().filter(|(node, _)| *node != start).map(|(_, key)| (key.clone(), "dxf".into())).collect::<Vec<_>>();
        let mut edges = Vec::new();
        for (from, targets) in next.iter().enumerate() {
            let owner = names.get(from).ok_or("fixture owner missing")?;
            for to in targets {
                let target = names.get(*to).ok_or("fixture target missing")?;
                edges.push(super::Edge { from_key: owner.clone(), raw_reference: target.clone(), role: "xref".into(), target_key: target.clone() });
            }
        }
        super::validate_declarations(root, &files, &edges)
    }
    fn assert_one_visit_certificate(next: Vec<Vec<usize>>, nodes: usize, edges: usize) -> Result<(), String> {
        assert_eq!(next.len(), nodes);
        assert_eq!(next.iter().map(Vec::len).sum::<usize>(), edges);
        declare_graph(&next)?;
        let mut depth = DepthAdmission::new(next)?;
        depth.visit(0, 0, [0; NODE_WORDS])?;
        assert_eq!(depth.visits, 1, "a positive general certificate must avoid exponential path enumeration");
        Ok(())
    }
    #[test]
    fn linked_depth_cover_certifies_k8_64_at_depth16() -> Result<(), String> {
        assert_one_visit_certificate(small_cover_graph(64, 8, false), 72, 1024)
    }
    #[test]
    fn linked_depth_cover_certifies_nonbipartite_small_clique_at_depth16() -> Result<(), String> {
        assert_one_visit_certificate(small_cover_graph(60, 8, true), 68, 1016)
    }
    #[test]
    fn linked_depth_cover_does_not_hide_excessive_nesting() -> Result<(), String> {
        let next = small_cover_graph(50, 9, true);
        for start in [0, 50] {
            let error = declare_graph_from(&next, start).err().ok_or("depth17 path was accepted")?;
            assert!(error.contains("depth16"), "inside/outside cover starts must retain exact depth rejection: {error}");
        }
        Ok(())
    }
    #[test]
    fn linked_depth_start_component_certifies_many_cycle_petals() -> Result<(), String> {
        let mut next = vec![Vec::new(); 37];
        for petal in 0..18 {
            let first = 1 + 2 * petal;
            let second = first + 1;
            next.first_mut().ok_or("fixture root missing")?.extend([first, second]);
            next.get_mut(first).ok_or("fixture petal missing")?.extend([0, second]);
            next.get_mut(second).ok_or("fixture petal missing")?.extend([0, first]);
        }
        assert_one_visit_certificate(next, 37, 108)
    }
    #[test]
    fn linked_depth_certificates_do_not_hide_clique18_depth17() -> Result<(), String> {
        let next: Vec<Vec<usize>> = (0..18).map(|from| (0..18).filter(|to| *to != from).collect()).collect();
        let error = declare_graph(&next).err().ok_or("clique18 depth17 path was accepted")?;
        assert!(error.contains("depth16"), "cardinality/component certificates must retain the exact boundary: {error}");
        Ok(())
    }
}
