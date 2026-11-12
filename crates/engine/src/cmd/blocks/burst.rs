//! BURST (Express Tools): explode block references as EXPLODE does, but their visible attributes
//! become text with the values they show instead of the block's attribute definitions.

use cadcraft_color::Color;
use cadcraft_doc::{Common, Drawing, Entity, EntityKind, Handle, Insert};
use serde_json::{Value, json};

use crate::cmd::machines::SelectRun;
use crate::cmd::modify::{explode_kind, explode_with};
use crate::cmd::{CommandSpec, targets};
use crate::{Result, Session};

pub(super) fn burst_spec() -> CommandSpec {
    CommandSpec::new("burst", "Explode Attributes to Text", run_burst)
        .menu(&["Tools", "Express Tools", "Explode Attributes to Text"])
        .params("{handles?} (block references; other objects are left alone) → {handles}")
        .interactive(|_| Ok(Box::new(SelectRun::new("burst", "BURST"))))
}

fn run_burst(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let r = explode_with(s, &hs, burst_pieces)?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

/// What a block reference bursts into: EXPLODE's pieces without the attribute definitions, then
/// a text (multiline text for a multiline attribute) for each visible attribute.
fn burst_pieces(d: &Drawing, e: &Entity) -> Option<Vec<Entity>> {
    let EntityKind::Insert(ins) = &e.kind else { return None };
    let mut out: Vec<Entity> = explode_kind(d, e)?.into_iter().filter(|p| !matches!(p.kind, EntityKind::AttDef(_))).collect();
    for a in ins.attribs.iter().filter(|a| !a.invisible) {
        let kind = match a.multiline_text() {
            Some(m) => EntityKind::MText(m),
            None => EntityKind::Text(a.text.clone()),
        };
        out.push(Entity { handle: Handle(0), common: attrib_common(d, e, ins, &a.tag, &a.props), kind });
    }
    Some(out)
}

/// The properties an attribute is drawn with: its own, else its definition's; layer 0 is the
/// reference's layer and ByBlock colour the reference's colour, as EXPLODE gives the block's
/// objects.
fn attrib_common(d: &Drawing, e: &Entity, ins: &Insert, tag: &str, props: &cadcraft_doc::AttribProps) -> Common {
    let def =
        d.block(&ins.block).and_then(|b| b.entities.iter().find(|be| matches!(&be.kind, EntityKind::AttDef(ad) if ad.tag.eq_ignore_ascii_case(tag))));
    // An attribute without a definition follows the reference.
    let base = def.map(|de| de.common.clone()).unwrap_or_else(|| Common { color: Color::ByBlock, ..Common::default() });
    let mut c = props.over(&base);
    if c.layer == "0" {
        c.layer.clone_from(&e.common.layer);
    }
    if c.color == Color::ByBlock {
        c.color = e.common.color;
    }
    c
}
