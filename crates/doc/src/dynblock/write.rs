//! Authoring dynamic blocks: adding parameters and actions to a block, written as the objects
//! the [reader](super::dynamic_block) reads (BPARAMETER, BACTION in the Block Editor).
//!
//! Everything lands in the block's kept [`Foreign`](crate::foreign::Foreign) data, laid out as
//! AutoCAD lays out its own (observed in files it saves): the BLOCK_RECORD's extension dictionary
//! gets an `ACAD_ENHANCEDBLOCK` evaluation graph and a purge preventer, and each parameter or
//! action is a node of the graph. A parameter brings its grips, each grip two location
//! components (its X and Y), and the graph's edges say what is evaluated after what: a
//! parameter before its location components and its actions, a grip before its parameter.
//! A graph read from a file is extended the same way.

use cadcraft_geom::{Vec2, Vec3};

use crate::foreign::ForeignObject;
use crate::{Drawing, EntityStore, Handle, RawTag};

use super::{ParameterKind, dynamic_block};

/// The graph `AcDbEvalGraph` is extended only up to this many nodes.
const MAX_NODES: usize = super::MAX_NODES;

/// Most frame points and selected objects one action is written with.
pub const MAX_ACTION_ITEMS: usize = 10_000;

/// The application name of element tags: the xdata pairing a block's objects with their
/// copies in references.
const ETAG: &str = "AcDbBlockRepETag";

/// The value AutoCAD gives a grip location component it hasn't evaluated yet (a linear
/// parameter's end grip).
const UNSET: f64 = 1.797_693_134_862_314e99;

/// A parameter to add.
#[derive(Clone, Debug, PartialEq)]
pub enum ParameterSpec {
    /// A distance from `base` to `end` (Distance1), its label `label_offset` from the line (to
    /// its left, looking from base to end), with 0, 1 (end) or 2 grips.
    Linear { base: Vec3, end: Vec3, label: Option<String>, label_offset: f64, grips: u8 },
    /// A position (Position1), its label at `label_at`, with 0 or 1 grip.
    Point { at: Vec3, label: Option<String>, label_at: Vec3, grips: u8 },
}

/// Which point of a two-point parameter an action follows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ParamPoint {
    Base,
    #[default]
    End,
}

/// An action to add, following the parameter with graph id `parameter`.
#[derive(Clone, Debug, PartialEq)]
pub enum ActionSpec {
    /// Moves the listed vertices of `vertices`' objects (inside `frame`, a rectangle's two corners
    /// or a polygon) and the other selected objects whole.
    Stretch { parameter: i32, point: ParamPoint, frame: Vec<Vec2>, selection: Vec<Handle>, vertices: Vec<(Handle, Vec<i32>)>, at: Vec3 },
    /// Moves the selected objects.
    Move { parameter: i32, point: ParamPoint, selection: Vec<Handle>, at: Vec3 },
}

/// One node of the evaluation graph: its index, flags, graph id, object and the first and last
/// of its incoming and outgoing edges (edge indices, -1 for none).
#[derive(Clone, Debug, PartialEq)]
struct Node {
    index: i32,
    flags: i32,
    id: i32,
    handle: Handle,
    links: [i32; 4],
}

/// One edge: its index, flags, kind (1 from a parameter, 2 from a grip), the nodes it joins
/// (indices) and its neighbours in their lists: previous and next incoming of `to`, previous and
/// next outgoing of `from`, then one more link AutoCAD leaves at -1.
#[derive(Clone, Debug, PartialEq)]
struct Edge {
    index: i32,
    flags: i32,
    kind: i32,
    from: i32,
    to: i32,
    links: [i32; 5],
}

/// An evaluation graph object being extended.
struct Graph {
    handle: Handle,
    /// Its groups before `100 AcDbEvalGraph`, as they were.
    head: Vec<RawTag>,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

fn tag(code: i32, value: impl ToString) -> RawTag {
    RawTag { code, value: value.to_string() }
}

fn real(code: i32, v: f64) -> RawTag {
    tag(code, format!("{v:?}"))
}

fn point(out: &mut Vec<RawTag>, code: i32, p: Vec3) {
    out.extend([real(code, p.x), real(code + 10, p.y), real(code + 20, p.z)]);
}

impl Graph {
    /// The graph object `o` read back into nodes and edges, or `None` when its groups aren't
    /// laid out as a graph (it is then left alone).
    fn read(o: &ForeignObject) -> Option<Graph> {
        let at = o.tags.iter().position(|t| t.code == 100 && t.value.eq_ignore_ascii_case("AcDbEvalGraph"))?;
        let head = o.tags.get(..at)?.to_vec();
        let body = o.tags.get(at + 1..)?;
        let int = |t: Option<&RawTag>, code: i32| t.filter(|t| t.code == code).and_then(|t| t.value.trim().parse::<i32>().ok());
        let mut i = body.iter().position(|t| t.code != 96 && t.code != 97).unwrap_or(body.len());
        let (mut nodes, mut edges) = (Vec::new(), Vec::new());
        while body.get(i).is_some_and(|t| t.code == 91) {
            if nodes.len() >= MAX_NODES {
                return None;
            }
            let g = |k: usize| body.get(i + k);
            let handle = g(3).filter(|t| t.code == 360).and_then(|t| Handle::parse_hex(&t.value))?;
            let links = [int(g(4), 92)?, int(g(5), 92)?, int(g(6), 92)?, int(g(7), 92)?];
            nodes.push(Node { index: int(g(0), 91)?, flags: int(g(1), 93)?, id: int(g(2), 95)?, handle, links });
            i += 8;
        }
        while body.get(i).is_some_and(|t| t.code == 92) {
            if edges.len() >= MAX_NODES.saturating_mul(16) {
                return None;
            }
            let g = |k: usize| body.get(i + k);
            let links = [int(g(5), 92)?, int(g(6), 92)?, int(g(7), 92)?, int(g(8), 92)?, int(g(9), 92)?];
            edges.push(Edge { index: int(g(0), 92)?, flags: int(g(1), 93)?, kind: int(g(2), 94)?, from: int(g(3), 91)?, to: int(g(4), 91)?, links });
            i += 10;
        }
        // Anything after the edges is a layout this doesn't know.
        if i != body.len() {
            return None;
        }
        Some(Graph { handle: o.handle, head, nodes, edges })
    }

    /// A new, empty graph owned by dictionary `dict`.
    fn new(handle: Handle, dict: Handle) -> Graph {
        let head = vec![tag(5, handle.hex()), tag(102, "{ACAD_REACTORS"), tag(330, dict.hex()), tag(102, "}"), tag(330, dict.hex())];
        Graph { handle, head, nodes: Vec::new(), edges: Vec::new() }
    }

    fn tags(&self) -> Vec<RawTag> {
        let mut out = self.head.clone();
        out.push(tag(100, "AcDbEvalGraph"));
        out.push(tag(96, self.nodes.len()));
        out.push(tag(97, self.nodes.len()));
        for n in &self.nodes {
            out.extend([tag(91, n.index), tag(93, n.flags), tag(95, n.id), tag(360, n.handle.hex())]);
            out.extend(n.links.iter().map(|l| tag(92, l)));
        }
        for e in &self.edges {
            out.extend([tag(92, e.index), tag(93, e.flags), tag(94, e.kind), tag(91, e.from), tag(91, e.to)]);
            out.extend(e.links.iter().map(|l| tag(92, l)));
        }
        out
    }

    /// The next free graph id.
    fn next_id(&self) -> i32 {
        self.nodes.iter().map(|n| n.id).max().unwrap_or(0).saturating_add(1)
    }

    /// Add node `id` for object `handle`; its index.
    fn add_node(&mut self, id: i32, handle: Handle) -> i32 {
        let index = self.nodes.iter().map(|n| n.index).max().map_or(0, |m| m.saturating_add(1));
        self.nodes.push(Node { index, flags: 32, id, handle, links: [-1; 4] });
        index
    }

    fn node_mut(&mut self, index: i32) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.index == index)
    }

    fn edge_mut(&mut self, index: i32) -> Option<&mut Edge> {
        self.edges.iter_mut().find(|e| e.index == index)
    }

    /// The index of the node with graph id `id`.
    fn index_of(&self, id: i32) -> Option<i32> {
        self.nodes.iter().find(|n| n.id == id).map(|n| n.index)
    }

    /// Add an edge of `kind` from node index `from` to `to`, last in both nodes' lists.
    fn link(&mut self, from: i32, to: i32, kind: i32) {
        let index = self.edges.iter().map(|e| e.index).max().map_or(0, |m| m.saturating_add(1));
        let last_out = self.node_mut(from).map_or(-1, |n| n.links[3]);
        let last_in = self.node_mut(to).map_or(-1, |n| n.links[1]);
        self.edges.push(Edge { index, flags: 0, kind, from, to, links: [last_in, -1, last_out, -1, -1] });
        if let Some(e) = self.edge_mut(last_out) {
            e.links[3] = index;
        }
        if let Some(e) = self.edge_mut(last_in) {
            e.links[1] = index;
        }
        if let Some(n) = self.node_mut(from) {
            if n.links[2] < 0 {
                n.links[2] = index;
            }
            n.links[3] = index;
        }
        if let Some(n) = self.node_mut(to) {
            if n.links[0] < 0 {
                n.links[0] = index;
            }
            n.links[1] = index;
        }
    }
}

/// The groups every node object starts with: its handle, owner and graph id.
fn node_head(h: Handle, graph: Handle, id: i32) -> Vec<RawTag> {
    vec![tag(5, h.hex()), tag(330, graph.hex()), tag(100, "AcDbEvalExpr"), tag(90, id), tag(98, 33), tag(99, 301)]
}

/// [`node_head`] and the block element: its name and a flag AutoCAD writes (`1071`).
fn element_head(h: Handle, graph: Handle, id: i32, name: &str, flag: i32) -> Vec<RawTag> {
    let mut out = node_head(h, graph, id);
    out.extend([tag(100, "AcDbBlockElement"), tag(300, name), tag(98, 33), tag(99, 301), tag(1071, flag)]);
    out
}

/// The block being authored: its record's kept data, graph and the objects made so far.
struct Author<'a> {
    d: &'a mut Drawing,
    record: String,
    graph: Graph,
    /// New objects to add after the graph.
    new: Vec<ForeignObject>,
    /// Classes the new objects need: (DXF name, C++ name).
    classes: Vec<(&'static str, &'static str)>,
}

impl<'a> Author<'a> {
    /// Open block `name`'s graph for authoring, making the dictionary, graph and purge preventer
    /// when it has none.
    fn open(d: &'a mut Drawing, name: &str) -> Result<Author<'a>, String> {
        let record = name.trim().to_ascii_uppercase();
        if record.is_empty() || record.starts_with('*') {
            return Err(format!("\"{name}\" can't have parameters or actions."));
        }
        let key = ("BLOCK_RECORD".to_string(), record.clone());
        let xdict = d.foreign.records.get(&key).and_then(|x| x.xdict);
        let mut new = Vec::new();
        let mut classes = Vec::new();
        let graph = match xdict.and_then(|x| dict_entry(d, x, "ACAD_ENHANCEDBLOCK")) {
            Some(g) => {
                let o = d.foreign.objects.iter().find(|o| o.handle == g).ok_or("The block's evaluation graph is missing.")?;
                Graph::read(o).ok_or("The block's evaluation graph can't be extended.")?
            }
            None => {
                let rec = record_handle(d, &record);
                let dict = match xdict {
                    Some(x) if d.foreign.objects.iter().any(|o| o.handle == x) => x,
                    _ => {
                        let x = d.new_handle();
                        let tags = vec![tag(5, x.hex()), tag(330, rec.hex()), tag(100, "AcDbDictionary"), tag(280, 1), tag(281, 1)];
                        new.push(ForeignObject { handle: x, kind: "DICTIONARY".into(), tags });
                        x
                    }
                };
                let (g, pp) = (d.new_handle(), d.new_handle());
                let mut tags = vec![tag(5, pp.hex()), tag(102, "{ACAD_REACTORS"), tag(330, dict.hex()), tag(102, "}"), tag(330, dict.hex())];
                tags.extend([tag(100, "AcDbDynamicBlockPurgePreventer"), tag(70, 1)]);
                new.push(ForeignObject { handle: pp, kind: "ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION".into(), tags });
                classes.push(("ACAD_EVALUATION_GRAPH", "AcDbEvalGraph"));
                classes.push(("ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION", "AcDbDynamicBlockPurgePreventer"));
                let f = std::sync::Arc::make_mut(&mut d.foreign);
                // The dictionary's entries: in the new dictionary, or added to the one it had.
                let entries =
                    [tag(3, "ACAD_ENHANCEDBLOCK"), tag(360, g.hex()), tag(3, "AcDbDynamicBlockRoundTripPurgePreventer"), tag(360, pp.hex())];
                match new.iter_mut().find(|o| o.handle == dict) {
                    Some(o) => o.tags.extend(entries),
                    None => {
                        if let Some(o) = f.objects.iter_mut().find(|o| o.handle == dict) {
                            o.tags.extend(entries);
                        }
                    }
                }
                let extra = f.records.entry(key).or_default();
                extra.xdict = Some(dict);
                if !extra.xdata.iter().any(|t| t.code == 1001 && t.value.eq_ignore_ascii_case("AcDbDynamicBlockTrueName")) {
                    extra.xdata.extend([tag(1001, "AcDbDynamicBlockTrueName"), tag(1000, name.trim())]);
                    extra.xdata.extend([tag(1001, "AcDbDynamicBlockGUID"), tag(1000, guid(name.trim(), g))]);
                }
                Graph::new(g, dict)
            }
        };
        Ok(Author { d, record, graph, new, classes })
    }

    /// Write the graph and the new objects back into the drawing's kept data.
    fn finish(self) {
        let Author { d, graph, new, classes, .. } = self;
        let f = std::sync::Arc::make_mut(&mut d.foreign);
        let tags = graph.tags();
        match f.objects.iter_mut().find(|o| o.handle == graph.handle) {
            Some(o) => o.tags = tags,
            None => f.objects.push(ForeignObject { handle: graph.handle, kind: "ACAD_EVALUATION_GRAPH".into(), tags }),
        }
        f.objects.extend(new);
        for (name, cpp) in classes {
            if !f.classes.iter().any(|c| c.iter().any(|t| t.code == 1 && t.value.eq_ignore_ascii_case(name))) {
                f.classes.push(vec![tag(1, name), tag(2, cpp), tag(3, "ObjectDBX Classes"), tag(90, 1153), tag(91, 0), tag(280, 0), tag(281, 0)]);
            }
        }
    }

    fn class(&mut self, name: &'static str, cpp: &'static str) {
        if !self.classes.iter().any(|(n, _)| *n == name) {
            self.classes.push((name, cpp));
        }
    }

    /// Add object `kind` with `tags` as graph node `id`; its index.
    fn node(&mut self, kind: &str, h: Handle, id: i32, tags: Vec<RawTag>) -> i32 {
        self.new.push(ForeignObject { handle: h, kind: kind.into(), tags });
        self.graph.add_node(id, h)
    }

    /// A grip at `at` with its two location components (`x_name`, `y_name`, both valued
    /// `value`), linked after parameter node `param`: the grip's graph id.
    fn grip(&mut self, param: i32, at: Vec3, name: &str, [x_name, y_name]: [&str; 2], value: f64, linear: Option<Vec3>) -> i32 {
        let (gh, gid) = (self.d.new_handle(), self.graph.next_id());
        let (xid, yid) = (gid.saturating_add(1), gid.saturating_add(2));
        let mut tags = element_head(gh, self.graph.handle, gid, name, 0);
        tags.extend([tag(100, "AcDbBlockGrip"), tag(91, xid), tag(92, yid)]);
        point(&mut tags, 1010, at);
        tags.extend([tag(280, 1), tag(93, -1)]);
        let kind = match linear {
            Some(dir) => {
                tags.extend([tag(100, "AcDbBlockLinearGrip"), real(140, dir.x), real(141, dir.y), real(142, dir.z)]);
                self.class("BLOCKLINEARGRIP", "AcDbBlockLinearGrip");
                "BLOCKLINEARGRIP"
            }
            None => {
                tags.push(tag(100, "AcDbBlockXYGrip"));
                self.class("BLOCKXYGRIP", "AcDbBlockXYGrip");
                "BLOCKXYGRIP"
            }
        };
        let grip = self.node(kind, gh, gid, tags);
        self.class("BLOCKGRIPLOCATIONCOMPONENT", "AcDbBlockGripExpr");
        for (id, comp) in [(xid, x_name), (yid, y_name)] {
            let h = self.d.new_handle();
            let mut tags = node_head(h, self.graph.handle, id);
            tags.extend([tag(1, ""), tag(70, 40), real(140, value), tag(100, "AcDbBlockGripExpr"), tag(91, 1), tag(300, comp)]);
            let c = self.node("BLOCKGRIPLOCATIONCOMPONENT", h, id, tags);
            self.graph.link(param, c, 1);
        }
        self.graph.link(grip, param, 2);
        gid
    }

    /// An element name not used in the block yet: `base`, else `base` and the lowest free number.
    fn element_name(&self, base: &str) -> String {
        let used: Vec<String> = dynamic_block(self.d, &self.record)
            .map(|m| m.parameters.iter().map(|p| p.element.clone()).chain(m.actions.iter().map(|a| a.name.clone())).collect())
            .unwrap_or_default();
        let free = |n: &str| !used.iter().any(|u| u.eq_ignore_ascii_case(n));
        if free(base) {
            return base.to_string();
        }
        (1..).map(|i| format!("{base}{i}")).find(|n| free(n)).unwrap_or_else(|| base.to_string())
    }
}

/// The handle the kept data uses for block `record`'s BLOCK_RECORD (a new one, registered,
/// when none refers to it yet).
fn record_handle(d: &mut Drawing, record: &str) -> Handle {
    if let Some(h) = d.foreign.targets.iter().find(|(_, (k, n))| k == "BLOCK_RECORD" && n == record).map(|(h, _)| *h) {
        return h;
    }
    let h = d.new_handle();
    std::sync::Arc::make_mut(&mut d.foreign).targets.insert(h, ("BLOCK_RECORD".to_string(), record.to_string()));
    h
}

/// A GUID for block `name`'s dynamic data, made from its name and graph (stable for a drawing).
fn guid(name: &str, graph: Handle) -> String {
    // FNV-1a over the name and handle, spread over 128 bits.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut word = || {
        for b in name.bytes().chain(graph.0.to_le_bytes()) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    let (a, b) = (word(), word());
    format!("{{{:08X}-{:04X}-{:04X}-{:04X}-{:012X}}}", a >> 32, (a >> 16) & 0xFFFF, a & 0xFFFF, b >> 48, b & 0xFFFF_FFFF_FFFF)
}

/// A property name not used by the block's parameters yet: `prefix` and the lowest number from 1.
fn free_label(d: &Drawing, record: &str, prefix: &str) -> String {
    let used: Vec<String> =
        dynamic_block(d, record).map(|m| m.parameters.iter().flat_map(|p| p.kind.label().map(str::to_string)).collect()).unwrap_or_default();
    (1..).map(|i| format!("{prefix}{i}")).find(|n| !used.iter().any(|u| u.eq_ignore_ascii_case(n))).unwrap_or_else(|| format!("{prefix}1"))
}

fn finite(p: Vec3) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.z.is_finite()
}

/// Add parameter `spec` to block `name` (any case); its graph id.
pub fn add_parameter(d: &mut Drawing, name: &str, spec: &ParameterSpec) -> Result<i32, String> {
    match spec {
        ParameterSpec::Linear { base, end, label, label_offset, grips } => {
            // Two finite points far apart can still be infinitely far from each other, and the
            // direction is written into the grips.
            if !finite(*base) || !finite(*end) || !finite(*end - *base) || !label_offset.is_finite() {
                return Err("Invalid point.".into());
            }
            if (*end - *base).len() <= 1e-9 {
                return Err("The endpoint must differ from the start point.".into());
            }
            if *grips > 2 {
                return Err("A linear parameter has 0, 1 or 2 grips.".into());
            }
            let record = name.trim().to_ascii_uppercase();
            let label = label.clone().unwrap_or_else(|| free_label(d, &record, "Distance"));
            let mut a = Author::open(d, name)?;
            let element = a.element_name("Linear");
            let (h, id) = (a.d.new_handle(), a.graph.next_id());
            let param = a.graph.add_node(id, h);
            let first_grip = a.new.len();
            let dir = *end - *base;
            let end_grip = if *grips >= 1 { a.grip(param, *end, "End Grip", ["UpdatedEndX", "UpdatedEndY"], UNSET, Some(dir)) } else { 0 };
            let base_grip = if *grips >= 2 { a.grip(param, *base, "Base Grip", ["UpdatedBaseX", "UpdatedBaseY"], 0.0, Some(-dir)) } else { 0 };
            let mut tags = element_head(h, a.graph.handle, id, &element, if *grips == 2 { 32 } else { 1 });
            tags.extend([tag(100, "AcDbBlockParameter"), tag(280, 1), tag(281, 0), tag(100, "AcDbBlock2PtParameter")]);
            point(&mut tags, 1010, *base);
            point(&mut tags, 1011, *end);
            tags.extend([tag(170, 4), tag(91, base_grip), tag(91, end_grip), tag(91, 0), tag(91, 0)]);
            // Each grip's connections: a count, the grip and the value it changes.
            let connections = [
                (171, 92, 301, base_grip, "DisplacementX"),
                (172, 93, 302, base_grip, "DisplacementY"),
                (173, 94, 303, end_grip, "DisplacementX"),
                (174, 95, 304, end_grip, "DisplacementY"),
            ];
            for (count, id_code, name_code, grip, value) in connections {
                if grip == 0 {
                    tags.push(tag(count, 0));
                } else {
                    tags.extend([tag(count, 1), tag(id_code, grip), tag(name_code, value)]);
                }
            }
            tags.extend([tag(177, 0), tag(100, "AcDbBlockLinearParameter"), tag(305, &label), tag(306, ""), real(140, *label_offset), tag(307, "")]);
            tags.extend([tag(96, 1), real(141, 0.0), real(142, 0.0), real(143, 0.0), tag(175, 0)]);
            // The parameter's object goes before its grips, as AutoCAD lists them.
            a.new.insert(first_grip, ForeignObject { handle: h, kind: "BLOCKLINEARPARAMETER".into(), tags });
            a.class("BLOCKLINEARPARAMETER", "AcDbBlockLinearParameter");
            a.finish();
            Ok(id)
        }
        ParameterSpec::Point { at, label, label_at, grips } => {
            if !finite(*at) || !finite(*label_at) {
                return Err("Invalid point.".into());
            }
            if *grips > 1 {
                return Err("A point parameter has 0 or 1 grip.".into());
            }
            let record = name.trim().to_ascii_uppercase();
            let label = label.clone().unwrap_or_else(|| free_label(d, &record, "Position"));
            let mut a = Author::open(d, name)?;
            let element = a.element_name("Point");
            let (h, id) = (a.d.new_handle(), a.graph.next_id());
            let param = a.graph.add_node(id, h);
            let first_grip = a.new.len();
            let grip = if *grips == 1 { a.grip(param, *at, "Grip", ["UpdatedX", "UpdatedY"], 0.0, None) } else { 0 };
            let mut tags = element_head(h, a.graph.handle, id, &element, if *grips == 1 { 0 } else { 1 });
            tags.extend([tag(100, "AcDbBlockParameter"), tag(280, 1), tag(281, 0), tag(100, "AcDbBlock1PtParameter")]);
            point(&mut tags, 1010, *at);
            tags.push(tag(93, grip));
            if grip == 0 {
                tags.extend([tag(170, 0), tag(171, 0)]);
            } else {
                tags.extend([tag(170, 1), tag(91, grip), tag(301, "DisplacementX"), tag(171, 1), tag(92, grip), tag(302, "DisplacementY")]);
            }
            tags.extend([tag(100, "AcDbBlockPointParameter"), tag(303, &label), tag(304, "")]);
            point(&mut tags, 1011, *label_at);
            a.new.insert(first_grip, ForeignObject { handle: h, kind: "BLOCKPOINTPARAMETER".into(), tags });
            a.class("BLOCKPOINTPARAMETER", "AcDbBlockPointParameter");
            a.finish();
            Ok(id)
        }
    }
}

/// Add action `spec` to block `name` (any case); its graph id. The parameter it follows must be
/// one the action can follow (a linear or point parameter for a stretch or move).
pub fn add_action(d: &mut Drawing, name: &str, spec: &ActionSpec) -> Result<i32, String> {
    let (parameter, point_of, selection, at) = match spec {
        ActionSpec::Stretch { parameter, point, selection, at, .. } | ActionSpec::Move { parameter, point, selection, at } => {
            (*parameter, *point, selection, *at)
        }
    };
    if !finite(at) {
        return Err("Invalid point.".into());
    }
    if selection.is_empty() {
        return Err("An action needs objects to act on.".into());
    }
    if selection.len() > MAX_ACTION_ITEMS {
        return Err(format!("An action acts on at most {MAX_ACTION_ITEMS} objects."));
    }
    let m = dynamic_block(d, name).ok_or("The block has no parameters; add one first (BPARAMETER).")?;
    let p = m.parameters.iter().find(|p| p.node == parameter).ok_or_else(|| format!("No parameter {parameter} in the block."))?;
    let connections = match (&p.kind, point_of) {
        (ParameterKind::Linear { .. }, ParamPoint::End) => ["EndXDelta", "EndYDelta"],
        (ParameterKind::Linear { .. }, ParamPoint::Base) => ["BaseXDelta", "BaseYDelta"],
        (ParameterKind::Point { .. }, _) => ["XDelta", "YDelta"],
        _ => return Err(format!("A {} can't be followed by this action yet.", p.element)),
    };
    let mut a = Author::open(d, name)?;
    let pindex = a.graph.index_of(parameter).ok_or_else(|| format!("No parameter {parameter} in the block."))?;
    let (h, id) = (a.d.new_handle(), a.graph.next_id());
    let (kind, cpp, element) = match spec {
        ActionSpec::Stretch { .. } => ("BLOCKSTRETCHACTION", "AcDbBlockStretchAction", a.element_name("Stretch")),
        ActionSpec::Move { .. } => ("BLOCKMOVEACTION", "AcDbBlockMoveAction", a.element_name("Move")),
    };
    let mut tags = element_head(h, a.graph.handle, id, &element, 0);
    tags.extend([tag(100, "AcDbBlockAction"), tag(70, 0), tag(71, selection.len())]);
    tags.extend(selection.iter().map(|s| tag(330, s.hex())));
    point(&mut tags, 1010, at);
    tags.push(tag(100, if kind == "BLOCKSTRETCHACTION" { "AcDbBlockStretchAction" } else { "AcDbBlockMoveAction" }));
    tags.extend([tag(92, parameter), tag(301, connections[0]), tag(93, parameter), tag(302, connections[1])]);
    if let ActionSpec::Stretch { frame, vertices, .. } = spec {
        if frame.len() < 2 || frame.len() > MAX_ACTION_ITEMS || frame.iter().any(|p| !p.is_finite()) {
            return Err("A stretch frame needs two corners or a polygon.".into());
        }
        if vertices.len() > MAX_ACTION_ITEMS || vertices.iter().map(|(_, v)| v.len()).sum::<usize>() > MAX_ACTION_ITEMS.saturating_mul(16) {
            return Err("Too many vertices in the stretch frame.".into());
        }
        tags.push(tag(72, frame.len()));
        for p in frame {
            tags.extend([real(1011, p.x), real(1021, p.y)]);
        }
        tags.push(tag(73, vertices.len()));
        for (h, idx) in vertices {
            tags.extend([tag(331, h.hex()), tag(74, idx.len())]);
            tags.extend(idx.iter().map(|i| tag(94, i)));
        }
        tags.push(tag(75, 0));
    }
    tags.extend([real(140, 1.0), real(141, 0.0), tag(280, 0)]);
    let node = a.node(kind, h, id, tags);
    a.graph.link(pindex, node, 1);
    a.class(kind, cpp);
    a.finish();
    Ok(id)
}

/// Give every object of `store`, block `name`'s objects, an element tag (`AcDbBlockRepETag`
/// xdata: `1070 1`, `1071` tag, `1005` the object itself) when it has none or has another
/// object's (a copy), and keep the next free tag on the block's record, as AutoCAD does. Only for
/// blocks with dynamic data.
pub fn tag_elements(d: &mut Drawing, name: &str, store: &mut EntityStore) {
    let key = ("BLOCK_RECORD".to_string(), name.trim().to_ascii_uppercase());
    if dynamic_block(d, name).is_none() {
        return;
    }
    // The tag an object carries, if it is its own.
    let own = |xd: &[RawTag], h: Handle| -> Option<i64> {
        let at = xd.iter().position(|t| t.code == 1001 && t.value.eq_ignore_ascii_case(ETAG))?;
        let app: Vec<&RawTag> = xd.iter().skip(at + 1).take_while(|t| t.code != 1001).collect();
        let mine = app.iter().find(|t| t.code == 1005).and_then(|t| Handle::parse_hex(&t.value)) == Some(h);
        app.iter().find(|t| t.code == 1071).and_then(|t| t.value.trim().parse().ok()).filter(|_| mine)
    };
    let mut seen = std::collections::HashSet::new();
    let mut retag = Vec::new();
    for e in store.iter() {
        match own(&e.common.xdata, e.handle) {
            Some(t) if seen.insert(t) => {}
            _ => retag.push(e.handle),
        }
    }
    let record_next = d.foreign.records.get(&key).and_then(|x| {
        let at = x.xdata.iter().position(|t| t.code == 1001 && t.value.eq_ignore_ascii_case(ETAG))?;
        x.xdata.iter().skip(at + 1).take_while(|t| t.code != 1001).find(|t| t.code == 1071).and_then(|t| t.value.trim().parse::<i64>().ok())
    });
    let mut next = seen.iter().max().map_or(0, |m| m.saturating_add(1)).max(record_next.unwrap_or(0));
    for h in retag {
        let t = next;
        next = next.saturating_add(1);
        store.modify(h, |e| {
            strip_app(&mut e.common.xdata, ETAG);
            e.common.xdata.extend([tag(1001, ETAG), tag(1070, 1), tag(1071, t), tag(1005, h.hex())]);
        });
    }
    let f = std::sync::Arc::make_mut(&mut d.foreign);
    let x = f.records.entry(key).or_default();
    strip_app(&mut x.xdata, ETAG);
    x.xdata.extend([tag(1001, ETAG), tag(1070, 1), tag(1071, next)]);
}

/// Remove application `app`'s xdata from `xd`.
fn strip_app(xd: &mut Vec<RawTag>, app: &str) {
    let Some(at) = xd.iter().position(|t| t.code == 1001 && t.value.eq_ignore_ascii_case(app)) else { return };
    let end = xd.iter().skip(at + 1).position(|t| t.code == 1001).map_or(xd.len(), |n| at + 1 + n);
    xd.drain(at..end);
}

/// The object entry `key` of dictionary `dict` names.
fn dict_entry(d: &Drawing, dict: Handle, key: &str) -> Option<Handle> {
    let o = d.foreign.objects.iter().find(|o| o.handle == dict)?;
    let mut tags = o.tags.iter();
    while let Some(t) = tags.next() {
        if t.code == 3 && t.value.eq_ignore_ascii_case(key) {
            return tags.next().filter(|t| t.code == 350 || t.code == 360).and_then(|t| Handle::parse_hex(&t.value));
        }
    }
    None
}
