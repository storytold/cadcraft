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

use super::{ParameterKind, VisibilityState, dynamic_block};

/// The graph `AcDbEvalGraph` is extended only up to this many nodes.
const MAX_NODES: usize = super::MAX_NODES;

/// Most frame points and selected objects one action is written with.
pub const MAX_ACTION_ITEMS: usize = 10_000;

/// The application name of element tags: the xdata pairing a block's objects with their
/// copies in references.
pub(super) const ETAG: &str = "AcDbBlockRepETag";

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
    /// Named visibility states (Visibility1) of `objects`, at `at`, with 0 or 1 grip: one state,
    /// `VisibilityState0`, showing them all. A block has one at most.
    Visibility { at: Vec3, label: Option<String>, objects: Vec<Handle>, grips: u8 },
    /// A value picked from a lookup table (Lookup1), at `at`, with 0 or 1 grip. `name` is its name
    /// in the Block Editor (default Lookup), `description` the property's, and `show` whether
    /// references show it in Properties. Its table comes with a lookup action (BACTION).
    Lookup { at: Vec3, label: Option<String>, name: Option<String>, description: String, show: bool, grips: u8 },
}

/// What a grip is: a linear parameter's (pointing along `Vec3`), a point's, or a visibility or
/// lookup parameter's (a list to pick from).
#[derive(Clone, Copy, Debug, PartialEq)]
enum GripKind {
    Linear(Vec3),
    Xy,
    Visibility,
    Lookup,
}

impl GripKind {
    fn is_list(self) -> bool {
        matches!(self, GripKind::Visibility | GripKind::Lookup)
    }
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
    /// The lookup table of lookup parameter `parameter`, empty until it is filled
    /// ([`set_lookup_table`](super::set_lookup_table)). A lookup parameter has one at most.
    Lookup { parameter: i32, at: Vec3 },
}

/// One node of the evaluation graph: its index, flags, graph id, object and the first and last
/// of its incoming and outgoing edges (edge indices, -1 for none).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Node {
    pub(super) index: i32,
    pub(super) flags: i32,
    pub(super) id: i32,
    pub(super) handle: Handle,
    pub(super) links: [i32; 4],
}

/// One edge: its index, flags, kind (1 from a parameter, 2 from a grip; between a lookup action
/// and a parameter, how many of the parameter's values the table has), the nodes it joins
/// (indices) and its neighbours in their lists: previous and next incoming of `to`, previous and
/// next outgoing of `from`, then the edge running the other way between the same nodes (a lookup
/// table's, which reads and sets its parameters), -1 for none.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Edge {
    pub(super) index: i32,
    pub(super) flags: i32,
    pub(super) kind: i32,
    pub(super) from: i32,
    pub(super) to: i32,
    pub(super) links: [i32; 5],
}

/// An evaluation graph object being extended.
pub(super) struct Graph {
    pub(super) handle: Handle,
    /// Its groups before `100 AcDbEvalGraph`, as they were.
    pub(super) head: Vec<RawTag>,
    /// The highest graph id handed out (96, 97), which AutoCAD keeps after nodes are erased:
    /// ids aren't used again.
    pub(super) last_id: i32,
    pub(super) nodes: Vec<Node>,
    pub(super) edges: Vec<Edge>,
}

pub(super) fn tag(code: i32, value: impl ToString) -> RawTag {
    RawTag { code, value: value.to_string() }
}

pub(super) fn real(code: i32, v: f64) -> RawTag {
    tag(code, format!("{v:?}"))
}

fn point(out: &mut Vec<RawTag>, code: i32, p: Vec3) {
    out.extend([real(code, p.x), real(code + 10, p.y), real(code + 20, p.z)]);
}

impl Graph {
    /// The graph object `o` read back into nodes and edges, or `None` when its groups aren't
    /// laid out as a graph (it is then left alone).
    pub(super) fn read(o: &ForeignObject) -> Option<Graph> {
        let at = o.tags.iter().position(|t| t.code == 100 && t.value.eq_ignore_ascii_case("AcDbEvalGraph"))?;
        let head = o.tags.get(..at)?.to_vec();
        let body = o.tags.get(at + 1..)?;
        let int = |t: Option<&RawTag>, code: i32| t.filter(|t| t.code == code).and_then(|t| t.value.trim().parse::<i32>().ok());
        let last_id = body.iter().take_while(|t| t.code == 96 || t.code == 97).filter_map(|t| t.value.trim().parse::<i32>().ok()).max().unwrap_or(0);
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
        Some(Graph { handle: o.handle, head, last_id, nodes, edges })
    }

    /// A new, empty graph owned by dictionary `dict`.
    fn new(handle: Handle, dict: Handle) -> Graph {
        let head = vec![tag(5, handle.hex()), tag(102, "{ACAD_REACTORS"), tag(330, dict.hex()), tag(102, "}"), tag(330, dict.hex())];
        Graph { handle, head, last_id: 0, nodes: Vec::new(), edges: Vec::new() }
    }

    pub(super) fn tags(&self) -> Vec<RawTag> {
        let mut out = self.head.clone();
        out.push(tag(100, "AcDbEvalGraph"));
        let last = self.nodes.iter().map(|n| n.id).max().unwrap_or(0).max(self.last_id);
        out.push(tag(96, last));
        out.push(tag(97, last));
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
        self.nodes.iter().map(|n| n.id).max().unwrap_or(0).max(self.last_id).saturating_add(1)
    }

    /// Add node `id` for object `handle`; its index.
    fn add_node(&mut self, id: i32, handle: Handle) -> i32 {
        let index = self.nodes.iter().map(|n| n.index).max().map_or(0, |m| m.saturating_add(1));
        self.nodes.push(Node { index, flags: 32, id, handle, links: [-1; 4] });
        index
    }

    /// Link `edges` again, in order, after the nodes' links were cleared: (from, to, kind, flags,
    /// pair), `pair` the position in `edges` of the edge running the other way (-1 for none).
    pub(super) fn relink(&mut self, edges: &[(i32, i32, i32, i32, i32)]) {
        self.edges.clear();
        for n in &mut self.nodes {
            n.links = [-1; 4];
        }
        for &(from, to, kind, flags, _) in edges {
            self.link(from, to, kind);
            if let Some(e) = self.edges.last_mut() {
                e.flags = flags;
            }
        }
        // Edge indices are the positions in `edges` now.
        for (e, &(.., pair)) in self.edges.iter_mut().zip(edges) {
            e.links[4] = if pair >= 0 && usize::try_from(pair).is_ok_and(|p| p < edges.len()) { pair } else { -1 };
        }
    }

    /// The graph's edges as [`relink`](Self::relink) takes them, except those `drop` refuses,
    /// with node indices mapped by `index` (an edge to a node it maps to `None` is dropped too).
    pub(super) fn kept_edges(&self, drop: impl Fn(&Edge) -> bool, index: impl Fn(i32) -> Option<i32>) -> Vec<(i32, i32, i32, i32, i32)> {
        let kept: Vec<(&Edge, i32, i32)> = self.edges.iter().filter(|e| !drop(e)).filter_map(|e| Some((e, index(e.from)?, index(e.to)?))).collect();
        let position = |old: i32| kept.iter().position(|(e, ..)| e.index == old).and_then(|p| i32::try_from(p).ok()).unwrap_or(-1);
        kept.iter().map(|(e, from, to)| (*from, *to, e.kind, e.flags, if e.links[4] >= 0 { position(e.links[4]) } else { -1 })).collect()
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
    pub(super) fn link(&mut self, from: i32, to: i32, kind: i32) {
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
    fn grip(&mut self, param: i32, at: Vec3, name: &str, [x_name, y_name]: [&str; 2], value: f64, kind: GripKind) -> i32 {
        let (gh, gid) = (self.d.new_handle(), self.graph.next_id());
        let (xid, yid) = (gid.saturating_add(1), gid.saturating_add(2));
        let mut tags = element_head(gh, self.graph.handle, gid, name, 0);
        tags.extend([tag(100, "AcDbBlockGrip"), tag(91, xid), tag(92, yid)]);
        point(&mut tags, 1010, at);
        // A visibility or lookup grip is the only one AutoCAD writes 280 0 on.
        tags.extend([tag(280, i32::from(!kind.is_list())), tag(93, -1)]);
        let dxf = match kind {
            GripKind::Linear(dir) => {
                tags.extend([tag(100, "AcDbBlockLinearGrip"), real(140, dir.x), real(141, dir.y), real(142, dir.z)]);
                self.class("BLOCKLINEARGRIP", "AcDbBlockLinearGrip");
                "BLOCKLINEARGRIP"
            }
            GripKind::Xy => {
                tags.push(tag(100, "AcDbBlockXYGrip"));
                self.class("BLOCKXYGRIP", "AcDbBlockXYGrip");
                "BLOCKXYGRIP"
            }
            GripKind::Visibility => {
                tags.push(tag(100, "AcDbBlockVisibilityGrip"));
                self.class("BLOCKVISIBILITYGRIP", "AcDbBlockVisibilityGrip");
                "BLOCKVISIBILITYGRIP"
            }
            GripKind::Lookup => {
                tags.push(tag(100, "AcDbBlockLookUpGrip"));
                self.class("BLOCKLOOKUPGRIP", "AcDbBlockLookupGrip");
                "BLOCKLOOKUPGRIP"
            }
        };
        let grip = self.node(dxf, gh, gid, tags);
        // AutoCAD links a list grip to its parameter before the location components.
        if kind.is_list() {
            self.graph.link(grip, param, 1);
        }
        self.class("BLOCKGRIPLOCATIONCOMPONENT", "AcDbBlockGripExpr");
        // Each component names the parameter it reads (its graph id).
        let owner = self.graph.nodes.iter().find(|n| n.index == param).map_or(0, |n| n.id);
        for (id, comp) in [(xid, x_name), (yid, y_name)] {
            let h = self.d.new_handle();
            let mut tags = node_head(h, self.graph.handle, id);
            tags.extend([tag(1, ""), tag(70, 40), real(140, value), tag(100, "AcDbBlockGripExpr"), tag(91, owner), tag(300, comp)]);
            let c = self.node("BLOCKGRIPLOCATIONCOMPONENT", h, id, tags);
            self.graph.link(param, c, 1);
        }
        if !kind.is_list() {
            self.graph.link(grip, param, 2);
        }
        gid
    }

    /// An element name not used in the block yet: `base`, else `base` and the lowest free number.
    fn element_name(&self, base: &str) -> String {
        if self.name_free(base) { base.to_string() } else { self.numbered_name(base) }
    }

    /// `base` and the lowest number from 1 not used as an element name in the block yet
    /// (AutoCAD numbers lookup actions from Lookup1).
    fn numbered_name(&self, base: &str) -> String {
        (1..).map(|i| format!("{base}{i}")).find(|n| self.name_free(n)).unwrap_or_else(|| format!("{base}1"))
    }

    /// No parameter or action of the block has element name `name` (any case).
    fn name_free(&self, name: &str) -> bool {
        dynamic_block(self.d, &self.record)
            .is_none_or(|m| !m.parameters.iter().map(|p| &p.element).chain(m.actions.iter().map(|a| &a.name)).any(|u| u.eq_ignore_ascii_case(name)))
    }
}

/// The handle the kept data uses for block `record`'s BLOCK_RECORD (a new one, registered,
/// when none refers to it yet).
pub(super) fn record_handle(d: &mut Drawing, record: &str) -> Handle {
    if let Some(h) = d.foreign.targets.iter().find(|(_, (k, n))| k == "BLOCK_RECORD" && n == record).map(|(h, _)| *h) {
        return h;
    }
    let h = d.new_handle();
    std::sync::Arc::make_mut(&mut d.foreign).targets.insert(h, ("BLOCK_RECORD".to_string(), record.to_string()));
    h
}

/// A GUID for block `name`'s dynamic data, made from its name and graph (stable for a drawing).
pub(super) fn guid(name: &str, graph: Handle) -> String {
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

pub(super) fn finite(p: Vec3) -> bool {
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
            let end_grip =
                if *grips >= 1 { a.grip(param, *end, "End Grip", ["UpdatedEndX", "UpdatedEndY"], UNSET, GripKind::Linear(dir)) } else { 0 };
            let base_grip =
                if *grips >= 2 { a.grip(param, *base, "Base Grip", ["UpdatedBaseX", "UpdatedBaseY"], 0.0, GripKind::Linear(-dir)) } else { 0 };
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
            let grip = if *grips == 1 { a.grip(param, *at, "Grip", ["UpdatedX", "UpdatedY"], 0.0, GripKind::Xy) } else { 0 };
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
        ParameterSpec::Visibility { at, label, objects, grips } => {
            if !finite(*at) {
                return Err("Invalid point.".into());
            }
            if *grips > 1 {
                return Err("A visibility parameter has 0 or 1 grip.".into());
            }
            if objects.len() > MAX_ACTION_ITEMS {
                return Err(format!("A visibility parameter controls at most {MAX_ACTION_ITEMS} objects."));
            }
            let record = name.trim().to_ascii_uppercase();
            let m = dynamic_block(d, &record);
            if m.as_ref().is_some_and(|m| m.parameters.iter().any(|p| matches!(p.kind, ParameterKind::Visibility { .. }))) {
                return Err("The block already has a visibility parameter.".into());
            }
            // The other parameters and their grips show in the first state.
            let active: Vec<Handle> = m.map(|m| parameter_objects(&m, None)).unwrap_or_default();
            let label = label.clone().unwrap_or_else(|| free_label(d, &record, "Visibility"));
            let mut a = Author::open(d, name)?;
            let element = a.element_name("Visibility State");
            let (h, id) = (a.d.new_handle(), a.graph.next_id());
            let param = a.graph.add_node(id, h);
            let first_grip = a.new.len();
            let grip = if *grips == 1 { a.grip(param, *at, "Grip", ["UpdatedX", "UpdatedY"], 0.0, GripKind::Visibility) } else { 0 };
            let mut tags = element_head(h, a.graph.handle, id, &element, 16);
            tags.extend([tag(100, "AcDbBlockParameter"), tag(280, 1), tag(281, 0), tag(100, "AcDbBlock1PtParameter")]);
            point(&mut tags, 1010, *at);
            tags.extend([tag(93, grip), tag(170, 0), tag(171, 0)]);
            let state = VisibilityState { name: "VisibilityState0".into(), visible: objects.clone(), active };
            tags.extend(visibility_tags(&label, "", objects, std::slice::from_ref(&state)));
            a.new.insert(first_grip, ForeignObject { handle: h, kind: "BLOCKVISIBILITYPARAMETER".into(), tags });
            a.class("BLOCKVISIBILITYPARAMETER", "AcDbBlockVisibilityParameter");
            a.finish();
            Ok(id)
        }
        ParameterSpec::Lookup { at, label, name: element, description, show, grips } => {
            if !finite(*at) {
                return Err("Invalid point.".into());
            }
            if *grips > 1 {
                return Err("A lookup parameter has 0 or 1 grip.".into());
            }
            let record = name.trim().to_ascii_uppercase();
            let label = label.clone().unwrap_or_else(|| free_label(d, &record, "Lookup"));
            let mut a = Author::open(d, name)?;
            let element = match element.as_deref().map(str::trim).filter(|e| !e.is_empty()) {
                Some(e) if !a.name_free(e) => return Err(format!("A parameter or action is already named \"{e}\".")),
                Some(e) => e.to_string(),
                None => a.element_name("Lookup"),
            };
            let (h, id) = (a.d.new_handle(), a.graph.next_id());
            let param = a.graph.add_node(id, h);
            let first_grip = a.new.len();
            let grip = if *grips == 1 { a.grip(param, *at, "Grip", ["UpdatedX", "UpdatedY"], 0.0, GripKind::Lookup) } else { 0 };
            let mut tags = element_head(h, a.graph.handle, id, &element, if *grips == 1 { 0 } else { 1 });
            tags.extend([tag(100, "AcDbBlockParameter"), tag(280, i32::from(*show)), tag(281, 0), tag(100, "AcDbBlock1PtParameter")]);
            point(&mut tags, 1010, *at);
            tags.extend([tag(93, grip), tag(170, 0), tag(171, 0)]);
            // 94: its lookup action's graph id, none yet.
            tags.extend([tag(100, "AcDbBlockLookUpParameter"), tag(303, &label), tag(304, description.trim()), tag(94, 0)]);
            a.new.insert(first_grip, ForeignObject { handle: h, kind: "BLOCKLOOKUPPARAMETER".into(), tags });
            a.class("BLOCKLOOKUPPARAMETER", "AcDbBlockLookupParameter");
            a.finish();
            Ok(id)
        }
    }
}

/// Most visibility states one parameter is written with.
pub const MAX_VISIBILITY_STATES: usize = 1_000;

/// The visibility parameter's own section: label, description, the objects it controls and its
/// states (each the objects shown in it and the parameters and grips active in it).
fn visibility_tags(label: &str, description: &str, controlled: &[Handle], states: &[VisibilityState]) -> Vec<RawTag> {
    let mut out = vec![tag(100, "AcDbBlockVisibilityParameter"), tag(281, 1), tag(301, label), tag(302, description), tag(91, 0)];
    out.push(tag(93, controlled.len()));
    out.extend(controlled.iter().map(|h| tag(331, h.hex())));
    out.push(tag(92, states.len()));
    for s in states {
        out.extend([tag(303, &s.name), tag(94, s.visible.len())]);
        out.extend(s.visible.iter().map(|h| tag(332, h.hex())));
        out.push(tag(95, s.active.len()));
        out.extend(s.active.iter().map(|h| tag(333, h.hex())));
    }
    out
}

/// The objects of block `m`'s parameters other than its visibility parameter, their grips and
/// the block's actions (all of them, or parameter or action `only`'s): what a visibility state
/// lists as active in it.
pub fn parameter_objects(m: &super::DynamicBlock, only: Option<i32>) -> Vec<Handle> {
    let grip = |id: &i32| m.grip_objects.iter().find(|(g, _)| g == id).map(|(_, h)| *h);
    let params = m
        .parameters
        .iter()
        .filter(|p| !matches!(p.kind, ParameterKind::Visibility { .. }) && only.is_none_or(|n| n == p.node))
        .flat_map(|p| std::iter::once(p.handle).chain(p.grips.iter().filter(|g| **g != 0).filter_map(grip)));
    params.chain(m.actions.iter().filter(|a| only.is_none_or(|n| n == a.node)).map(|a| a.handle)).collect()
}

/// Keep block `name`'s visibility parameter in step with `objects`, the block's objects: as
/// AutoCAD does, one it doesn't control yet (drawn in the Block Editor) becomes controlled and
/// shown in state `current` only (the first state when `None` or not found), and objects no
/// longer in the block leave its lists. Nothing happens without a visibility parameter.
pub fn adopt_objects(d: &mut Drawing, name: &str, objects: &[Handle], current: Option<&str>) -> Result<(), String> {
    let Some(m) = dynamic_block(d, name) else { return Ok(()) };
    let Some(ParameterKind::Visibility { controlled, states, .. }) =
        m.parameters.into_iter().map(|p| p.kind).find(|k| matches!(k, ParameterKind::Visibility { .. }))
    else {
        return Ok(());
    };
    let alive: std::collections::HashSet<Handle> = objects.iter().copied().collect();
    let known: std::collections::HashSet<Handle> = controlled.iter().copied().collect();
    let new: Vec<Handle> = objects.iter().filter(|h| !known.contains(h)).copied().collect();
    let gone = controlled.iter().any(|h| !alive.contains(h)) || states.iter().any(|s| s.visible.iter().any(|h| !alive.contains(h)));
    if new.is_empty() && !gone {
        return Ok(());
    }
    let at = current.and_then(|c| states.iter().position(|s| s.name.eq_ignore_ascii_case(c))).unwrap_or(0);
    let mut controlled: Vec<Handle> = controlled.into_iter().filter(|h| alive.contains(h)).collect();
    controlled.extend(new.iter().copied());
    let mut states = states;
    for (i, s) in states.iter_mut().enumerate() {
        s.visible.retain(|h| alive.contains(h));
        if i == at {
            s.visible.extend(new.iter().copied());
        }
    }
    set_visibility(d, name, &controlled, &states)
}

/// Rewrite block `name`'s visibility parameter: the objects it `controlled` and its `states`
/// (in order; the first is the one new references start in).
pub fn set_visibility(d: &mut Drawing, name: &str, controlled: &[Handle], states: &[VisibilityState]) -> Result<(), String> {
    if states.is_empty() {
        return Err("A visibility parameter needs at least one visibility state.".into());
    }
    if states.len() > MAX_VISIBILITY_STATES {
        return Err(format!("A visibility parameter has at most {MAX_VISIBILITY_STATES} states."));
    }
    let too_many = |v: &[Handle]| v.len() > MAX_ACTION_ITEMS;
    if too_many(controlled) || states.iter().any(|s| too_many(&s.visible) || too_many(&s.active)) {
        return Err(format!("A visibility state lists at most {MAX_ACTION_ITEMS} objects."));
    }
    for (i, s) in states.iter().enumerate() {
        if s.name.trim().is_empty() {
            return Err("A visibility state needs a name.".into());
        }
        if states.iter().take(i).any(|o| o.name.eq_ignore_ascii_case(&s.name)) {
            return Err(format!("Visibility state \"{}\" already exists.", s.name));
        }
    }
    let m = dynamic_block(d, name).ok_or("The block has no visibility parameter.")?;
    let (handle, label, description) = m
        .parameters
        .iter()
        .find_map(|p| match &p.kind {
            ParameterKind::Visibility { label, description, .. } => Some((p.handle, label.clone(), description.clone())),
            _ => None,
        })
        .ok_or("The block has no visibility parameter.")?;
    let f = std::sync::Arc::make_mut(&mut d.foreign);
    let o = f.objects.iter_mut().find(|o| o.handle == handle).ok_or("The block's visibility parameter is missing.")?;
    let at = o
        .tags
        .iter()
        .position(|t| t.code == 100 && t.value.eq_ignore_ascii_case("AcDbBlockVisibilityParameter"))
        .ok_or("The block's visibility parameter can't be changed.")?;
    // Its section is the object's last; anything after it would be a layout this doesn't know.
    if o.tags.iter().skip(at + 1).any(|t| t.code == 100) {
        return Err("The block's visibility parameter can't be changed.".into());
    }
    o.tags.truncate(at);
    o.tags.extend(visibility_tags(&label, &description, controlled, states));
    Ok(())
}

/// Add action `spec` to block `name` (any case); its graph id. The parameter it follows must be
/// one the action can follow (a linear or point parameter for a stretch or move).
pub fn add_action(d: &mut Drawing, name: &str, spec: &ActionSpec) -> Result<i32, String> {
    let (parameter, point_of, selection, at) = match spec {
        ActionSpec::Stretch { parameter, point, selection, at, .. } | ActionSpec::Move { parameter, point, selection, at } => {
            (*parameter, *point, selection, *at)
        }
        ActionSpec::Lookup { parameter, at } => return add_lookup_action(d, name, *parameter, *at),
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
        // (A lookup action returned above.)
        ActionSpec::Move { .. } | ActionSpec::Lookup { .. } => ("BLOCKMOVEACTION", "AcDbBlockMoveAction", a.element_name("Move")),
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

/// Add an empty lookup table to lookup parameter `parameter` of block `name`; its graph id. As in
/// AutoCAD, it reads nothing yet and gives the parameter's value (one lookup column), and the
/// parameter names it (`94`).
fn add_lookup_action(d: &mut Drawing, name: &str, parameter: i32, at: Vec3) -> Result<i32, String> {
    if !finite(at) {
        return Err("Invalid point.".into());
    }
    let m = dynamic_block(d, name).ok_or("The block has no parameters; add one first (BPARAMETER).")?;
    let p = m.parameters.iter().find(|p| p.node == parameter).ok_or_else(|| format!("No parameter {parameter} in the block."))?;
    if !matches!(p.kind, ParameterKind::Lookup { .. }) {
        return Err(format!("A {} can't have a lookup action.", p.element));
    }
    if super::lookup::table_of(&m, parameter).is_some() {
        return Err("Lookup parameter already associated with a lookup table.".into());
    }
    let param_handle = p.handle;
    let mut a = Author::open(d, name)?;
    let pindex = a.graph.index_of(parameter).ok_or_else(|| format!("No parameter {parameter} in the block."))?;
    let (h, id) = (a.d.new_handle(), a.graph.next_id());
    let element = a.numbered_name("Lookup");
    let column = super::TableColumn { node: parameter, property: "lookupString".into(), lookup: true, unmatched: "Custom".into(), reverse: false };
    // 1071 is 8 on a table with no inputs yet, 0 once it has some.
    let mut tags = element_head(h, a.graph.handle, id, &element, 8);
    tags.extend([tag(100, "AcDbBlockAction"), tag(70, 0), tag(71, 0)]);
    point(&mut tags, 1010, at);
    tags.extend(super::lookup::table_tags(&[column], &[]));
    let node = a.node("BLOCKLOOKUPACTION", h, id, tags);
    a.graph.link(node, pindex, 1);
    a.class("BLOCKLOOKUPACTION", "AcDbBlockLookupAction");
    a.finish();
    super::lookup::set_action_of(d, param_handle, id);
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
pub(super) fn strip_app(xd: &mut Vec<RawTag>, app: &str) {
    let Some(at) = xd.iter().position(|t| t.code == 1001 && t.value.eq_ignore_ascii_case(app)) else { return };
    let end = xd.iter().skip(at + 1).position(|t| t.code == 1001).map_or(xd.len(), |n| at + 1 + n);
    xd.drain(at..end);
}

/// The object entry `key` of dictionary `dict` names.
pub(super) fn dict_entry(d: &Drawing, dict: Handle, key: &str) -> Option<Handle> {
    let o = d.foreign.objects.iter().find(|o| o.handle == dict)?;
    let mut tags = o.tags.iter();
    while let Some(t) = tags.next() {
        if t.code == 3 && t.value.eq_ignore_ascii_case(key) {
            return tags.next().filter(|t| t.code == 350 || t.code == 360).and_then(|t| Handle::parse_hex(&t.value));
        }
    }
    None
}
