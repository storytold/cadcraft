//! External references in a written DXF (DXF Reference, BLOCK): an xref is a BLOCK flagged 4
//! (8 for an overlay) whose group 1 holds the referenced drawing's path, with no objects; its
//! objects come from that drawing when the host is opened. Symbols that came with a loaded xref
//! (styles, blocks and linetypes named `xref|name`) belong to the referenced drawing and are not
//! written as the host's own. Its layers are: their colour, freeze and the other settings made in
//! the host are the host's (AutoCAD's VISRETAIN = 1), written as xref-dependent LAYER records
//! (group 70 bits 16 and 32, DXF Reference) with the linetypes they use, and kept when the xref
//! loads again.

use std::sync::Arc;

use cadcraft_doc::Drawing;

/// Whether a symbol name belongs to an attached xref (`xref|name`).
pub(crate) fn dependent(name: &str) -> bool {
    cadcraft_doc::foreign::xref_dependent(name)
}

/// The drawing as it is saved: xref blocks emptied, xref-dependent symbols other than layers and
/// their linetypes dropped. `None` when the drawing has neither (it is saved as it is).
pub(crate) fn saved_view(d: &Drawing) -> Option<Drawing> {
    let has = d.blocks.values().any(|b| b.xref_path.is_some() || dependent(&b.name))
        || d.linetypes.iter().any(|l| dependent(&l.name))
        || d.text_styles.iter().any(|t| dependent(&t.name))
        || d.dim_styles.iter().any(|t| dependent(&t.name))
        || d.mleader_styles.iter().any(|t| dependent(&t.name))
        || d.table_styles.iter().any(|t| dependent(&t.name));
    if !has {
        return None;
    }
    let mut v = d.clone();
    v.blocks.retain(|name, _| !dependent(name));
    for b in v.blocks.values_mut() {
        if b.xref_path.is_some() && !b.entities.is_empty() {
            Arc::make_mut(b).entities = Default::default();
        }
    }
    // The linetypes of its layers stay too: a LAYER record names one that the file must have
    // (a DWG keeps the reference by handle, and falls back to Continuous without it).
    v.linetypes.retain(|t| !dependent(&t.name) || v.layers.iter().any(|l| l.linetype.eq_ignore_ascii_case(&t.name)));
    v.text_styles.retain(|t| !dependent(&t.name));
    v.dim_styles.retain(|t| !dependent(&t.name));
    v.mleader_styles.retain(|t| !dependent(&t.name));
    v.table_styles.retain(|t| !dependent(&t.name));
    Some(v)
}
