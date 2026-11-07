//! Saved layer states as AutoCAD keeps them: the LAYER table's extension dictionary holds an
//! `ACAD_LAYERSTATES` dictionary of one XRECORD per state. Measured in AutoCAD 2024 (DXFOUT of
//! states saved with -LAYER stAte), an XRECORD holds `280 1`, the mask (91), a text (301: empty
//! as -LAYER saves it; AutoCAD keeps one a file holds through restore and DXFOUT), `290 0` and
//! the current layer's name (302), then for each layer its LAYER handle (330),
//! flags (90: 1 off, 2 frozen, 4 locked, 8 plotted, 16 frozen in new viewports), colour index
//! (62), lineweight (370), for a layer of an xref 93 (0 for a newly attached one; a file can hold
//! 1, whose meaning isn't measured), LTYPE handle (331), plot style (in a drawing of colour-dependent
//! plot styles the name `Color_N` in group 1, N its colour index; of named ones the handle of its
//! placeholder object in group 390), transparency (440: 0 opaque, else `0x02000000 | alpha`) and,
//! for a true colour, 92 (`0xC2RRGGBB`).
//!
//! States AutoCAD's dictionary can't hold stay in CADCraft's own record (`CADCRAFT_LAYERSTATES`):
//! the view layer snapshots (`*VIEW name`), and a name that repeats an earlier one in another
//! case (of states `S` and `s` in one dictionary AutoCAD 2024 lists only `S`, and restoring `s`
//! restores `S`, measured).

use std::collections::{HashMap, HashSet};

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Layer, LayerState, Lineweight};
use cadcraft_dxf::Tag;

use crate::dxf_ext::{transparency_from_dxf, transparency_to_dxf};

/// The key of the states dictionary in the LAYER table's extension dictionary.
pub(crate) const KEY: &str = "ACAD_LAYERSTATES";

/// The prefix of a true colour in group 92.
const TRUE_COLOR: u32 = 0xC200_0000;

/// Which of `states` go into AutoCAD's dictionary (the others into CADCraft's record).
pub(crate) fn in_acad(states: &[LayerState]) -> Vec<bool> {
    let mut seen: HashSet<String> = HashSet::with_capacity(states.len());
    states.iter().map(|st| !st.name.trim().is_empty() && !st.name.starts_with('*') && seen.insert(st.name.to_lowercase())).collect()
}

/// The states CADCraft's own record keeps (those [`in_acad`] leaves out).
pub(crate) fn own(states: &[LayerState]) -> Vec<LayerState> {
    states.iter().zip(in_acad(states)).filter(|(_, acad)| !acad).map(|(st, _)| st.clone()).collect()
}

/// Handles of the objects that hold the drawing's states in AutoCAD's form.
#[derive(Default)]
pub(crate) struct Plan {
    /// The LAYER table, which owns the extension dictionary.
    pub table: String,
    /// The LAYER table's extension dictionary.
    pub xdict: String,
    /// The `ACAD_LAYERSTATES` dictionary.
    dict: String,
    /// (index in the drawing's states, XRECORD handle).
    records: Vec<(usize, String)>,
}

impl Plan {
    pub fn new(states: &[LayerState], mut handle: impl FnMut() -> String) -> Self {
        let acad = in_acad(states);
        if !acad.contains(&true) {
            return Plan::default();
        }
        let xdict = handle();
        let dict = handle();
        let records = acad.iter().enumerate().filter(|(_, a)| **a).map(|(i, _)| (i, handle())).collect();
        Plan { table: String::new(), xdict, dict, records }
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// What the state objects need from the rest of the file.
pub(crate) struct Refs<'a> {
    /// Upper-case layer name → LAYER record handle.
    pub layers: &'a HashMap<String, String>,
    /// Upper-case linetype name → LTYPE record handle.
    pub ltypes: &'a HashMap<String, String>,
    /// Upper-case plot style name → its placeholder object's handle.
    pub pstyles: &'a HashMap<String, String>,
    /// Plot styles by colour (`$PSTYLEMODE` 1): the plot style name is `Color_N`.
    pub colour_dependent: bool,
}

/// The objects of plan `p`: the extension dictionary, the `ACAD_LAYERSTATES` dictionary and one
/// XRECORD per state. Layers the drawing no longer has are left out of a state (AutoCAD names
/// layers by handle).
pub(crate) fn objects(p: &Plan, states: &[LayerState], refs: &Refs) -> Vec<Tag> {
    let mut t = Vec::new();
    if p.is_empty() {
        return t;
    }
    t.extend([Tag::s(0, "DICTIONARY"), Tag::s(5, &p.xdict), Tag::s(330, &p.table), Tag::s(100, "AcDbDictionary"), Tag::i(280, 1), Tag::i(281, 1)]);
    t.extend([Tag::s(3, KEY), Tag::s(360, &p.dict)]);
    t.extend([Tag::s(0, "DICTIONARY"), Tag::s(5, &p.dict), Tag::s(102, "{ACAD_REACTORS"), Tag::s(330, &p.xdict), Tag::s(102, "}")]);
    t.extend([Tag::s(330, &p.xdict), Tag::s(100, "AcDbDictionary"), Tag::i(281, 1)]);
    let planned = || p.records.iter().filter_map(|(i, h)| Some((states.get(*i)?, h)));
    for (st, h) in planned() {
        t.extend([Tag::s(3, &st.name), Tag::s(350, h)]);
    }
    for (st, h) in planned() {
        t.extend([Tag::s(0, "XRECORD"), Tag::s(5, h), Tag::s(102, "{ACAD_REACTORS"), Tag::s(330, &p.dict), Tag::s(102, "}"), Tag::s(330, &p.dict)]);
        t.extend([Tag::s(100, "AcDbXrecord"), Tag::i(280, 1), Tag::i(91, i64::from(st.mask)), Tag::s(301, &st.description), Tag::i(290, 0)]);
        t.push(Tag::s(302, &st.current));
        for l in &st.layers {
            if let Some(lh) = refs.layers.get(&l.name.to_ascii_uppercase()) {
                layer(&mut t, l, lh, refs);
            }
        }
    }
    t
}

/// One layer of a state's XRECORD.
fn layer(t: &mut Vec<Tag>, l: &Layer, handle: &str, refs: &Refs) {
    let flags = i64::from(!l.on) | i64::from(l.frozen) << 1 | i64::from(l.locked) << 2 | i64::from(l.plot) << 3 | i64::from(l.vp_freeze_new) << 4;
    let aci = match l.color {
        Color::True(rgb) => i64::from(cadcraft_color::nearest_aci(rgb)),
        c => i64::from(c.to_aci()).clamp(1, 255),
    };
    t.extend([Tag::s(330, handle), Tag::i(90, flags), Tag::i(62, aci), Tag::i(370, i64::from(l.lineweight.to_dxf()))]);
    if crate::dxf_xref::dependent(&l.name) {
        t.push(Tag::i(93, 0));
    }
    // A linetype the drawing no longer has: Continuous, which every drawing has.
    if let Some(lt) = refs.ltypes.get(&l.linetype.to_ascii_uppercase()).or_else(|| refs.ltypes.get("CONTINUOUS")) {
        t.push(Tag::s(331, lt));
    }
    if refs.colour_dependent {
        t.push(Tag::s(1, format!("Color_{aci}")));
    } else if let Some(ps) = refs.pstyles.get(&l.plot_style.trim().to_ascii_uppercase()).or_else(|| refs.pstyles.get("NORMAL")) {
        t.push(Tag::s(390, ps));
    }
    t.push(Tag::i(440, if l.transparency == 0 { 0 } else { transparency_to_dxf(l.transparency) }));
    if let Color::True(rgb) = l.color {
        t.push(Tag::i(92, i64::from(i32::from_be_bytes((TRUE_COLOR | rgb.to_u32()).to_be_bytes()))));
    }
}

/// What a state's handles name in the drawing being read.
pub(crate) struct Lookup<'a> {
    /// The drawing's layer a LAYER handle names: a state's layer starts as a copy of it (what the
    /// state doesn't hold, such as the description, is the drawing's).
    pub layer: &'a dyn Fn(&str) -> Option<Layer>,
    /// The linetype an LTYPE handle names.
    pub ltype: &'a dyn Fn(&str) -> Option<String>,
    /// The plot style a placeholder handle names.
    pub pstyle: &'a dyn Fn(&str) -> Option<String>,
    /// Plot styles by colour: group 1 (`Color_N`) says nothing the layer's colour doesn't.
    pub colour_dependent: bool,
}

/// The state `name` from its XRECORD's groups. A layer whose record is gone (AutoCAD keeps a
/// purged layer's entry, measured) is skipped.
pub(crate) fn read(name: &str, tags: &[Tag], lookup: &Lookup) -> LayerState {
    let mut st = LayerState { name: name.to_string(), ..LayerState::default() };
    // The data follows the subclass marker (an XRECORD without one holds none).
    let start = tags.iter().position(|t| t.code == 100 && t.str().trim() == "AcDbXrecord").map_or(tags.len(), |i| i + 1);
    let body = tags.get(start..).unwrap_or_default();
    let first = body.iter().position(|t| t.code == 330).unwrap_or(body.len());
    for t in body.get(..first).unwrap_or_default() {
        match t.code {
            91 => st.mask = t.str().trim().parse::<u32>().unwrap_or(LayerState::ALL),
            301 => st.description = t.str(),
            302 => st.current = t.str(),
            _ => {}
        }
    }
    let rest = body.get(first..).unwrap_or_default();
    let mut at = 0;
    while let Some(group) = rest.get(at..) {
        let Some(head) = group.first() else { break };
        let len = group.iter().skip(1).position(|t| t.code == 330).map_or(group.len(), |n| n + 1);
        at += len;
        if let Some(mut l) = (lookup.layer)(&head.str()) {
            entry(&mut l, group.get(1..len).unwrap_or_default(), lookup);
            st.layers.push(l);
        }
    }
    st
}

/// A layer's properties from the groups after its handle.
fn entry(l: &mut Layer, groups: &[Tag], lookup: &Lookup) {
    let int = |t: &Tag| t.str().trim().parse::<i64>().ok();
    let mut true_color = None;
    for t in groups {
        match t.code {
            90 => {
                if let Some(f) = int(t) {
                    (l.on, l.frozen, l.locked, l.plot, l.vp_freeze_new) = (f & 1 == 0, f & 2 != 0, f & 4 != 0, f & 8 != 0, f & 16 != 0);
                }
            }
            62 => {
                if let Some(c) = int(t).map(i64::abs).and_then(|c| u8::try_from(c).ok()).filter(|c| *c >= 1) {
                    l.color = Color::Index(c);
                }
            }
            92 => true_color = int(t).and_then(|v| u32::try_from(v & 0xFFFF_FFFF).ok()).filter(|v| v & 0xFF00_0000 == TRUE_COLOR),
            370 => {
                if let Some(w) = int(t).and_then(|w| i16::try_from(w).ok()) {
                    l.lineweight = Lineweight::from_dxf(w);
                }
            }
            331 => {
                if let Some(lt) = (lookup.ltype)(&t.str()) {
                    l.linetype = lt;
                }
            }
            390 => {
                if let Some(ps) = (lookup.pstyle)(&t.str()) {
                    l.plot_style = ps;
                }
            }
            1 if !lookup.colour_dependent => l.plot_style = t.str(),
            440 => {
                if let Some(v) = int(t) {
                    l.transparency = transparency_from_dxf(v).unwrap_or(0);
                }
            }
            _ => {}
        }
    }
    if let Some(v) = true_color {
        l.color = Color::True(Rgb::from_u32(v & 0x00FF_FFFF));
    }
}
