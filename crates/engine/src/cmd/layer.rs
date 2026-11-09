//! Layers: LAYER (programmatic sub-ids, -LAYER prompts), layer tools (LAYMCUR, LAYISO, LAYOFF, LAYFRZ…).

mod prompts;
mod state_prompts;

use cadcraft_color::Color;
use cadcraft_doc::{Layer, Lineweight};
use serde_json::{Value, json};

use super::machines::SelectRun;
use super::{CommandSpec, bad, bool_or, ok, str_param, targets};

use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("layer", "Layers", run_list)
            .menu(&["Format", "Layers"])
            .alias(&["la", "layers", "-layer"])
            .params("{} → layers, states (typed -LAYER: option prompts)")
            .interactive(|_| Ok(Box::new(prompts::LayerM::default()))),
        CommandSpec::new("layer.new", "New Layer", run_new).params("{name, color?, linetype?, lineweight? (mm), current?: bool}"),
        CommandSpec::new("layer.set", "Set Layer Properties", run_set)
            .params("{name, on?, frozen?, locked?, plot?, color?, linetype?, lineweight?, transparency?, description?, newVpFreeze?, plotStyle? (named plot style), newName?}"),
        CommandSpec::new("layer.current", "Make Current", run_current)
            .menu(&["Format", "Layer Tools", "Make Current"])
            .alias(&["clayer"])
            .params("{name}"),
        CommandSpec::new("layer.delete", "Delete Layer", run_delete).params("{name}"),
        CommandSpec::new("laymcur", "Make Object's Layer Current", run_laymcur).params("{handles?}"),
        CommandSpec::new("laymch", "Layer Match", run_laymch).menu(&["Format", "Layer Tools", "Layer Match"]).params("{handles?, layer}"),
        CommandSpec::new("laycur", "Change to Current Layer", run_laycur)
            .menu(&["Format", "Layer Tools", "Change to Current Layer"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(SelectRun::new("laycur", "LAYCUR")))),
        CommandSpec::new("layiso", "Isolate Layer", run_layiso).menu(&["Format", "Layer Tools", "Isolate Layer"]).params("{handles?}"),
        CommandSpec::new("layuniso", "Unisolate Layer", run_layuniso).menu(&["Format", "Layer Tools", "Unisolate Layer"]),
        CommandSpec::new("layfrz", "Freeze Layer", |s, p| set_obj_layers(s, p, |l| l.frozen = true))
            .menu(&["Format", "Layer Tools", "Freeze Layer"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(SelectRun::single("layfrz", "LAYFRZ", "Select an object on the layer to be frozen")))),
        CommandSpec::new("layoff", "Layer Off", |s, p| set_obj_layers(s, p, |l| l.on = false))
            .menu(&["Format", "Layer Tools", "Layer Off"])
            .params("{handles?}")
            // Layer Off stays active until Enter or Esc, as in AutoCAD (#1418): one object per
            // pick and the prompt comes straight back.
            .interactive(|_| Ok(Box::new(SelectRun::repeat("layoff", "LAYOFF", "Select an object on the layer to be turned off")))),
        CommandSpec::new("laylck", "Lock Layer", |s, p| set_obj_layers(s, p, |l| l.locked = true))
            .menu(&["Format", "Layer Tools", "Lock Layer"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(SelectRun::single("laylck", "LAYLCK", "Select an object on the layer to be locked")))),
        CommandSpec::new("layulk", "Unlock Layer", |s, p| set_obj_layers(s, p, |l| l.locked = false))
            .menu(&["Format", "Layer Tools", "Unlock Layer"])
            .params("{handles?}")
            .interactive(|_| Ok(Box::new(SelectRun::single("layulk", "LAYULK", "Select an object on the layer to be unlocked")))),
        CommandSpec::new("layon", "Turn All Layers On", |s, _| all_layers(s, |l| l.on = true)),
        CommandSpec::new("laythw", "Thaw All Layers", |s, _| all_layers(s, |l| l.frozen = false)),
        CommandSpec::new("layerp", "Previous Layer", run_layerp).menu(&["Format", "Layer Tools", "Previous Layer"]),
        CommandSpec::new("layerstate.save", "Save Layer State", run_state_save)
            .menu(&["Format", "Layer States Manager..."])
            .params("{name, mask? (properties restoring sets: 1 on, 2 frozen, 4 locked, 8 plot, 16 new viewport freeze, 32 color, 64 linetype, 128 lineweight, 256 plot style, 1024 transparency; default all)}"),
        CommandSpec::new("layerstate.restore", "Restore Layer State", run_state_restore).params("{name}"),
        CommandSpec::new("layerstate.list", "List Layer States", run_state_list).params("{} → states").noundo(),
        CommandSpec::new("layerstate.delete", "Delete Layer State", run_state_delete).params("{name}"),
        CommandSpec::new("layerstate.rename", "Rename Layer State", run_state_rename).params("{from, to}"),
        CommandSpec::new("laymrg", "Layer Merge", merge::run_laymrg)
            .menu(&["Format", "Layer Tools", "Layer Merge"])
            .params("{from: [names], to} (the from layers' objects move to `to`, then the from layers are deleted)")
            .interactive(|_| Ok(Box::new(merge::LayerRemove::merge()))),
        CommandSpec::new("laydel", "Layer Delete", merge::run_laydel)
            .menu(&["Format", "Layer Tools", "Layer Delete"])
            .params("{names: [...]} (deletes the layers and every object on them)")
            .interactive(|_| Ok(Box::new(merge::LayerRemove::delete()))),
    ]
}

fn layer_json(l: &Layer, current: bool) -> Value {
    json!({
        "name": l.name, "on": l.on, "frozen": l.frozen, "locked": l.locked, "plot": l.plot,
        "color": l.color.name(), "colorRgb": l.color.resolve(Color::Index(7), Color::Index(7)).hex(),
        "linetype": l.linetype, "lineweight": l.lineweight.name(), "transparency": l.transparency,
        "description": l.description, "current": current, "newVpFreeze": l.vp_freeze_new, "plotStyle": l.plot_style,
    })
}

fn run_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let cur = d.header.str("CLAYER", "0");
    let used = used_layers(d);
    let layers: Vec<Value> = d
        .layers
        .iter()
        .map(|l| {
            let mut v = layer_json(l, l.name.eq_ignore_ascii_case(&cur));
            if let Some(o) = v.as_object_mut() {
                o.insert("used".into(), json!(used.contains(&l.name.to_ascii_lowercase())));
            }
            v
        })
        .collect();
    Ok(json!({ "layers": layers, "states": d.layer_states.iter().map(|st| st.name.clone()).collect::<Vec<_>>() }))
}

/// Lower-case names of layers that have objects on them (model, layouts and block definitions).
pub fn used_layers(d: &cadcraft_doc::Drawing) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    let ents = d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())).chain(d.blocks.values().flat_map(|b| b.entities.iter()));
    for e in ents.take(5_000_000) {
        if !set.contains(&e.common.layer.to_ascii_lowercase()) {
            set.insert(e.common.layer.to_ascii_lowercase());
        }
    }
    set
}

pub(super) fn parse_lw(v: &Value) -> Option<Lineweight> {
    if let Some(f) = v.as_f64() {
        return Some(Lineweight::Mm100((f * 100.0).round().clamp(0.0, 211.0) as u16));
    }
    match v.as_str()?.to_ascii_lowercase().as_str() {
        "bylayer" => Some(Lineweight::ByLayer),
        "byblock" => Some(Lineweight::ByBlock),
        "default" => Some(Lineweight::Default),
        s => s.trim_end_matches("mm").trim().parse::<f64>().ok().map(|f| Lineweight::Mm100((f * 100.0).round().clamp(0.0, 211.0) as u16)),
    }
}

fn apply(l: &mut Layer, p: &Value) -> Result<()> {
    if let Some(v) = p.get("on").and_then(Value::as_bool) {
        l.on = v;
    }
    if let Some(v) = p.get("frozen").and_then(Value::as_bool) {
        l.frozen = v;
    }
    if let Some(v) = p.get("locked").and_then(Value::as_bool) {
        l.locked = v;
    }
    if let Some(v) = p.get("plot").and_then(Value::as_bool) {
        l.plot = v;
    }
    if let Some(v) = str_param(p, "plotStyle").map(str::trim) {
        if v.is_empty() || v.eq_ignore_ascii_case("bylayer") || v.eq_ignore_ascii_case("byblock") {
            return Err(bad("layer", "a layer's plot style is a style name (such as Normal)"));
        }
        l.plot_style = v.chars().take(255).collect();
    }
    if let Some(c) = p.get("color") {
        let c = c
            .as_str()
            .and_then(Color::parse)
            .or_else(|| c.as_u64().and_then(|i| u8::try_from(i).ok()).filter(|i| *i > 0).map(Color::Index))
            .ok_or_else(|| bad("layer", "bad colour"))?;
        if matches!(c, Color::ByLayer | Color::ByBlock) {
            return Err(bad("layer", "a layer colour must be an index or true colour"));
        }
        l.color = c;
    }
    if let Some(lt) = str_param(p, "linetype") {
        l.linetype = lt.to_string();
    }
    if let Some(lw) = p.get("lineweight") {
        l.lineweight = parse_lw(lw).ok_or_else(|| bad("layer", "bad lineweight"))?;
    }
    if let Some(t) = p.get("transparency").and_then(Value::as_u64) {
        l.transparency = t.min(90) as u8;
    }
    if let Some(v) = p.get("newVpFreeze").and_then(Value::as_bool) {
        l.vp_freeze_new = v;
    }
    if let Some(d) = str_param(p, "description") {
        l.description = d.to_string();
    }
    Ok(())
}

/// The loaded linetype named by `linetype`, if given: a layer cannot use ByLayer, ByBlock or a
/// linetype the drawing does not have.
fn loaded_linetype(d: &cadcraft_doc::Drawing, cmd: &str, p: &Value) -> Result<Option<String>> {
    let Some(lt) = str_param(p, "linetype") else { return Ok(None) };
    if lt.eq_ignore_ascii_case("ByLayer") || lt.eq_ignore_ascii_case("ByBlock") {
        return Err(bad(cmd, "a layer linetype cannot be ByLayer or ByBlock"));
    }
    let t = d.linetype(lt).ok_or_else(|| bad(cmd, format!("linetype `{lt}` is not loaded (load it with LINETYPE)")))?;
    Ok(Some(t.name.clone()))
}

fn valid_name(n: &str) -> bool {
    !n.trim().is_empty() && n.len() <= 255 && !n.chars().any(|c| c.is_control() || "<>/\\\":;?*|,=`".contains(c))
}

fn run_new(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("layer.new", "`name` is required"))?.to_string();
    if !valid_name(&name) {
        return Err(bad("layer.new", "invalid layer name"));
    }
    let d = s.doc_mut()?;
    if d.layer(&name).is_some() {
        return Err(bad("layer.new", format!("layer `{name}` already exists")));
    }
    let lt = loaded_linetype(d, "layer.new", p)?;
    let mut l = Layer::new(&name);
    apply(&mut l, p)?;
    if let Some(lt) = lt {
        l.linetype = lt;
    }
    d.layers.push(l);
    if bool_or(p, "current", false) {
        d.header.set_str("CLAYER", &name);
    }
    ok()
}

fn run_set(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("layer.set", "`name` is required"))?.to_string();
    let new_name = str_param(p, "newName").map(str::to_string);
    // Validate on a copy before borrowing the drawing mutably, so a rejected call
    // leaves the layer untouched and records no undo step.
    let d = s.doc()?;
    let cur = d.header.str("CLAYER", "0");
    let mut updated = d.layer(&name).cloned().ok_or_else(|| bad("layer.set", format!("no layer `{name}`")))?;
    let old = updated.name.clone();
    let lt = loaded_linetype(d, "layer.set", p)?;
    apply(&mut updated, p)?;
    if let Some(lt) = lt {
        updated.linetype = lt;
    }
    if p.get("frozen").and_then(Value::as_bool) == Some(true) && old.eq_ignore_ascii_case(&cur) {
        return Err(bad("layer.set", "cannot freeze the current layer"));
    }
    if let Some(nn) = &new_name {
        if old == "0" || old.eq_ignore_ascii_case("Defpoints") {
            return Err(bad("layer.set", "cannot rename layer 0 or Defpoints"));
        }
        if !valid_name(nn) {
            return Err(bad("layer.set", "invalid layer name"));
        }
        if !nn.eq_ignore_ascii_case(&old) && d.layer(nn).is_some() {
            return Err(bad("layer.set", format!("layer `{nn}` already exists")));
        }
    }
    let d = s.doc_mut()?;
    let l = d.layer_mut(&old).ok_or_else(|| bad("layer.set", format!("no layer `{name}`")))?;
    *l = updated;
    if let Some(nn) = new_name {
        l.name = nn.clone();
        // Re-point entities in model space, every layout and every block definition.
        repoint(&mut d.model, &old, &nn);
        for layout in &mut d.layouts {
            repoint(&mut layout.entities, &old, &nn);
        }
        for block in d.blocks.values_mut() {
            if block.entities.iter().any(|e| e.common.layer.eq_ignore_ascii_case(&old)) {
                repoint(&mut std::sync::Arc::make_mut(block).entities, &old, &nn);
            }
        }
        merge::rename_in_viewports(d, &old, &nn);
        if cur.eq_ignore_ascii_case(&old) {
            d.header.set_str("CLAYER", &nn);
        }
    }
    ok()
}

fn repoint(store: &mut cadcraft_doc::EntityStore, old: &str, new: &str) {
    let hs: Vec<_> = store.iter().filter(|e| e.common.layer.eq_ignore_ascii_case(old)).map(|e| e.handle).collect();
    for h in hs {
        store.modify(h, |e| e.common.layer = new.to_string());
    }
}

fn run_current(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("layer.current", "`name` is required"))?.to_string();
    let d = s.doc_mut()?;
    let l = d.layer_mut(&name).ok_or_else(|| bad("layer.current", format!("no layer `{name}`")))?;
    l.frozen = false;
    let n = l.name.clone();
    prev_push(d);
    d.header.set_str("CLAYER", &n);
    ok()
}

fn prev_push(d: &mut cadcraft_doc::Drawing) {
    let snap = serde_json::to_string(&d.layers).unwrap_or_default();
    d.header.set_str("CADCRAFT_LAYERP", &snap);
}

fn run_layerp(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc_mut()?;
    let snap = d.header.str("CADCRAFT_LAYERP", "");
    let layers: Vec<Layer> = serde_json::from_str(&snap).map_err(|_| bad("layerp", "No previous layer state."))?;
    d.layers = layers;
    Ok(json!({"message": "Restored previous layer states."}))
}

fn run_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("layer.delete", "`name` is required"))?.to_string();
    let d = s.doc_mut()?;
    if name == "0" || name.eq_ignore_ascii_case("Defpoints") || d.header.str("CLAYER", "0").eq_ignore_ascii_case(&name) {
        return Err(bad("layer.delete", "cannot delete layer 0, Defpoints or the current layer"));
    }
    let used = d.model.iter().any(|e| e.common.layer.eq_ignore_ascii_case(&name))
        || d.layouts.iter().any(|l| l.entities.iter().any(|e| e.common.layer.eq_ignore_ascii_case(&name)))
        || d.blocks.values().any(|b| b.entities.iter().any(|e| e.common.layer.eq_ignore_ascii_case(&name)));
    if used {
        return Err(bad("layer.delete", "layer has objects on it"));
    }
    d.layers.retain(|l| !l.name.eq_ignore_ascii_case(&name));
    merge::forget_in_viewports(d, &std::collections::HashSet::from([name.to_ascii_lowercase()]));
    ok()
}

fn obj_layers(s: &Session, p: &Value) -> Result<Vec<String>> {
    let hs = targets(s, p)?;
    let d = s.doc()?;
    let mut v: Vec<String> = hs.iter().filter_map(|h| d.entity(*h).map(|e| e.common.layer.clone())).collect();
    v.sort();
    v.dedup();
    Ok(v)
}

fn run_laymcur(s: &mut Session, p: &Value) -> Result<Value> {
    let ls = obj_layers(s, p)?;
    let l = ls.first().cloned().ok_or_else(|| bad("laymcur", "select an object"))?;
    s.doc_mut()?.header.set_str("CLAYER", &l);
    Ok(json!({ "message": format!("{l} is now the current layer.") }))
}

fn run_laymch(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let layer = str_param(p, "layer").ok_or_else(|| bad("laymch", "`layer` is required"))?.to_string();
    let d = s.doc_mut()?;
    d.ensure_layer(&layer);
    for h in &hs {
        d.modify_entity(*h, |e| e.common.layer = layer.clone())?;
    }
    Ok(json!({ "changed": hs.len() }))
}

fn run_laycur(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let d = s.doc_mut()?;
    let cur = d.header.str("CLAYER", "0");
    for h in &hs {
        d.modify_entity(*h, |e| e.common.layer = cur.clone())?;
    }
    Ok(json!({ "changed": hs.len() }))
}

fn run_layiso(s: &mut Session, p: &Value) -> Result<Value> {
    let keep = obj_layers(s, p)?;
    if keep.is_empty() {
        return Err(bad("layiso", "select objects on the layers to isolate"));
    }
    let d = s.doc_mut()?;
    prev_push(d);
    d.header.set_str("CADCRAFT_LAYISO", &serde_json::to_string(&d.layers).unwrap_or_default());
    for l in &mut d.layers {
        if !keep.iter().any(|k| k.eq_ignore_ascii_case(&l.name)) {
            l.on = false;
        }
    }
    if let Some(k) = keep.first() {
        d.header.set_str("CLAYER", k);
    }
    Ok(json!({ "message": format!("Layer(s) {} have been isolated.", keep.join(", ")) }))
}

fn run_layuniso(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc_mut()?;
    let snap = d.header.str("CADCRAFT_LAYISO", "");
    let layers: Vec<Layer> = serde_json::from_str(&snap).map_err(|_| bad("layuniso", "No isolated layers."))?;
    d.layers = layers;
    ok()
}

fn set_obj_layers(s: &mut Session, p: &Value, f: fn(&mut Layer)) -> Result<Value> {
    let ls = obj_layers(s, p)?;
    let d = s.doc_mut()?;
    let cur = d.header.str("CLAYER", "0");
    prev_push(d);
    for n in &ls {
        if let Some(l) = d.layer_mut(n) {
            let was_frozen = l.frozen;
            f(l);
            if l.frozen && !was_frozen && l.name.eq_ignore_ascii_case(&cur) {
                l.frozen = false;
            }
        }
    }
    s.set_selection(Vec::new());
    Ok(json!({ "layers": ls }))
}

fn all_layers(s: &mut Session, f: fn(&mut Layer)) -> Result<Value> {
    let d = s.doc_mut()?;
    prev_push(d);
    d.layers.iter_mut().for_each(f);
    ok()
}

fn run_state_save(s: &mut Session, p: &Value) -> Result<Value> {
    use cadcraft_doc::LayerState as S;
    let name = str_param(p, "name").ok_or_else(|| bad("layerstate.save", "`name` is required"))?.to_string();
    // As -LAYER stAte Save and layerstate.rename take names: none empty, over 255 characters, with a
    // control character or with one AutoCAD 2024 refuses (measured for Save).
    if !valid_name(&name) {
        return Err(bad("layerstate.save", "invalid name"));
    }
    // The properties restoring sets; bit 512, which no property toggles, is always set, as AutoCAD
    // 2024 saves it (measured: On alone is 513).
    let mask = match p.get("mask") {
        None | Some(Value::Null) => S::ALL,
        Some(v) => {
            let m = v.as_u64().filter(|m| m & !u64::from(S::ALL) == 0);
            m.and_then(|m| u32::try_from(m).ok()).ok_or_else(|| bad("layerstate.save", "`mask` must be a sum of layer state bits (0 to 2047)"))? | 512
        }
    };
    let d = s.doc_mut()?;
    let layers = d.layers.clone();
    let current = d.header.str("CLAYER", "0");
    // Replace the state Restore and Delete reach (the same spelling, else another case), so a
    // drawing keeps one state per name: AutoCAD 2024 finds states without regard to case. Its
    // -LAYER stAte Save and layerstate-save refuse `s1` after `S1` (measured), as the command line
    // here does; this command replaces, as it does a state of the same spelling. When a file holds
    // both `S1` and `s1`, only the one spelled so is replaced.
    if let Some(i) = state_index(d, &name) {
        d.layer_states.remove(i);
    }
    d.layer_states.push(S { mask, ..S::new(name, layers, current) });
    ok()
}

/// The saved state called `name`: the one spelled so, else the first whose name differs only in
/// case. AutoCAD 2024 finds layer states without regard to case (measured: Restore, Edit, Name
/// and Delete of `s1` reach `S1`); case is compared as for layers and view snapshots.
pub(super) fn state_index(d: &cadcraft_doc::Drawing, name: &str) -> Option<usize> {
    d.layer_states.iter().position(|st| st.name == name).or_else(|| d.layer_states.iter().position(|st| st.name.eq_ignore_ascii_case(name)))
}

fn run_state_restore(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("layerstate.restore", "`name` is required"))?;
    let d = s.doc_mut()?;
    let st = state_index(d, name).and_then(|i| d.layer_states.get(i)).cloned().ok_or_else(|| bad("layerstate.restore", "no such layer state"))?;
    restore_state(d, &st);
    ok()
}

/// Restore a saved layer state as AutoCAD 2024 does (measured): first the current layer it
/// names, when the drawing has it and the restore doesn't leave it frozen, then on each drawing
/// layer the state names (without regard to case) the properties its mask covers. The current
/// layer is never frozen; other properties (descriptions, names) stay as they are.
pub(crate) fn restore_state(d: &mut cadcraft_doc::Drawing, st: &cadcraft_doc::LayerState) {
    use cadcraft_doc::LayerState as S;
    // A frozen layer only thaws with Frozen in the mask and an entry of its own: without one
    // AutoCAD 2024 keeps the current layer (measured).
    let thaws = |l: &Layer| st.mask & S::FROZEN != 0 && st.layers.iter().any(|saved| saved.name.eq_ignore_ascii_case(&l.name));
    if let Some(l) = d.layer(&st.current).filter(|l| !l.frozen || thaws(l)) {
        let name = l.name.clone();
        d.header.set_str("CLAYER", &name);
    }
    let current = d.header.str("CLAYER", "0");
    let index = layer_index(d);
    let has = |bit: u32| st.mask & bit != 0;
    for saved in &st.layers {
        let Some(l) = index.get(&saved.name.to_ascii_lowercase()).and_then(|&i| d.layers.get_mut(i)) else { continue };
        if has(S::ON) {
            l.on = saved.on;
        }
        if has(S::FROZEN) {
            l.frozen = saved.frozen && !l.name.eq_ignore_ascii_case(&current);
        }
        if has(S::LOCKED) {
            l.locked = saved.locked;
        }
        if has(S::PLOT) {
            l.plot = saved.plot;
        }
        if has(S::VP_FREEZE_NEW) {
            l.vp_freeze_new = saved.vp_freeze_new;
        }
        if has(S::COLOR) {
            l.color = saved.color;
        }
        if has(S::LINETYPE) {
            l.linetype.clone_from(&saved.linetype);
        }
        if has(S::LINEWEIGHT) {
            l.lineweight = saved.lineweight;
        }
        if has(S::PLOT_STYLE) {
            l.plot_style.clone_from(&saved.plot_style);
        }
        if has(S::TRANSPARENCY) {
            l.transparency = saved.transparency;
        }
    }
}

/// Put the saved layers back over the drawing's layers of the same name (without regard to case);
/// saved layers the drawing no longer has are skipped. Both lists can come from a file, so the
/// drawing's layers are looked up in an index rather than scanned per saved layer.
pub(crate) fn restore_layers(d: &mut cadcraft_doc::Drawing, saved: Vec<Layer>) {
    let index = layer_index(d);
    for layer in saved {
        if let Some(l) = index.get(&layer.name.to_ascii_lowercase()).and_then(|&i| d.layers.get_mut(i)) {
            *l = layer;
        }
    }
}

/// The drawing's layers by lower-case name: the first of names that differ only in case, as
/// `Drawing::layer_mut` finds it.
fn layer_index(d: &cadcraft_doc::Drawing) -> std::collections::HashMap<String, usize> {
    let mut index = std::collections::HashMap::with_capacity(d.layers.len());
    for (i, l) in d.layers.iter().enumerate() {
        index.entry(l.name.to_ascii_lowercase()).or_insert(i);
    }
    index
}

fn run_state_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let states: Vec<Value> = d.layer_states.iter().map(|st| json!({ "name": st.name, "layers": st.layers.len() })).collect();
    let msg = if states.is_empty() {
        "No saved layer states.".to_string()
    } else {
        d.layer_states.iter().map(|st| st.name.clone()).collect::<Vec<_>>().join(", ")
    };
    Ok(json!({ "states": states, "message": msg }))
}

fn run_state_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("layerstate.delete", "`name` is required"))?;
    let d = s.doc_mut()?;
    let i = state_index(d, name).ok_or_else(|| bad("layerstate.delete", "no such layer state"))?;
    d.layer_states.remove(i);
    ok()
}

fn run_state_rename(s: &mut Session, p: &Value) -> Result<Value> {
    let from = str_param(p, "from").ok_or_else(|| bad("layerstate.rename", "`from` is required"))?;
    let to = str_param(p, "to").map(str::trim).ok_or_else(|| bad("layerstate.rename", "`to` is required"))?.to_string();
    if !valid_name(&to) {
        return Err(bad("layerstate.rename", "invalid name"));
    }
    let d = s.doc_mut()?;
    // A name in another case counts, the state's own too (AutoCAD 2024 refuses `S1` to `s1`,
    // measured).
    if d.layer_states.iter().any(|st| st.name.eq_ignore_ascii_case(&to)) {
        return Err(bad("layerstate.rename", format!("layer state `{to}` already exists")));
    }
    let st = state_index(d, from).and_then(|i| d.layer_states.get_mut(i)).ok_or_else(|| bad("layerstate.rename", "no such layer state"))?;
    st.name = to;
    ok()
}

// LAYMRG and LAYDEL.
mod merge;

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    /// Restoring many saved layers, most of them missing, takes one pass and matches without case.
    #[test]
    fn restore_layers_matches_by_name_in_linear_time() {
        let mut d = cadcraft_doc::Drawing::new_imperial();
        d.layers = (0..20_000).map(|i| cadcraft_doc::Layer::new(&format!("L{i}"))).collect();
        let mut saved: Vec<cadcraft_doc::Layer> = (0..20_000).map(|i| cadcraft_doc::Layer::new(&format!("missing{i}"))).collect();
        let mut back = cadcraft_doc::Layer::new("l7");
        back.frozen = true;
        saved.push(back);
        let started = std::time::Instant::now();
        super::restore_layers(&mut d, saved);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert!(d.layer("L7").is_some_and(|l| l.frozen));
        assert_eq!(d.layers.len(), 20_000);
    }

    #[test]
    fn layer_states_save_list_restore_delete() {
        let mut s = Session::new();
        s.execute("layer.new", &json!({ "name": "A", "color": 1 })).unwrap();
        s.execute("layerstate.save", &json!({ "name": "base" })).unwrap();
        s.execute("layer.set", &json!({ "name": "A", "on": false, "color": 3 })).unwrap();
        let l = s.execute("layerstate.list", &json!({})).unwrap();
        assert_eq!(l["states"][0]["name"], "base");
        s.execute("layerstate.restore", &json!({ "name": "base" })).unwrap();
        let a = s.doc().unwrap().layer("A").unwrap().clone();
        assert!(a.on);
        assert_eq!(a.color, cadcraft_color::Color::Index(1));
        s.execute("layerstate.rename", &json!({ "from": "base", "to": "start" })).unwrap();
        assert!(s.execute("layerstate.delete", &json!({ "name": "base" })).is_err());
        s.execute("layerstate.delete", &json!({ "name": "start" })).unwrap();
        assert!(s.execute("layerstate.list", &json!({})).unwrap()["states"].as_array().unwrap().is_empty());
        // `used` flags in the layer list.
        s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap();
        let v = s.execute("layer", &json!({})).unwrap();
        let used: Vec<bool> = v["layers"].as_array().unwrap().iter().map(|l| l["used"].as_bool().unwrap()).collect();
        assert!(used.contains(&true) && used.contains(&false));
    }

    /// Restoring follows AutoCAD 2024 (measured with -LAYER stAte and states written into a DXF):
    /// the current layer first, then only the properties the mask covers; the current layer stays
    /// thawed; a current layer that is empty or gone leaves the current layer as it is.
    #[test]
    fn layer_state_restore_follows_the_mask_and_current_layer() {
        use cadcraft_color::Color;
        use cadcraft_doc::LayerState;
        let mut s = Session::new();
        s.execute("layer.new", &json!({ "name": "A", "color": 1, "current": true })).unwrap();
        s.execute("layer.new", &json!({ "name": "B" })).unwrap();
        s.execute("layerstate.save", &json!({ "name": "all" })).unwrap();
        let saved = s.doc().unwrap().layer_states[0].clone();
        assert_eq!((saved.mask, saved.current.as_str()), (LayerState::ALL, "A"));
        // Only On: A's colour stays green, B comes back on (AutoCAD's S7, mask 513).
        let d = s.doc_mut().unwrap();
        d.layer_states.push(LayerState { name: "on".into(), mask: LayerState::ON | 512, ..saved.clone() });
        s.execute("layer.set", &json!({ "name": "A", "color": 3 })).unwrap();
        s.execute("layer.set", &json!({ "name": "B", "on": false, "locked": true })).unwrap();
        s.execute("layer.current", &json!({ "name": "B" })).unwrap();
        s.execute("layerstate.restore", &json!({ "name": "on" })).unwrap();
        let d = s.doc().unwrap();
        assert_eq!(d.header.str("CLAYER", ""), "A");
        let (a, b) = (d.layer("A").unwrap(), d.layer("B").unwrap());
        assert_eq!((a.color, b.on, b.locked), (Color::Index(3), true, true));
        // Every property.
        s.execute("layerstate.restore", &json!({ "name": "all" })).unwrap();
        let d = s.doc().unwrap();
        assert_eq!((d.layer("A").unwrap().color, d.layer("B").unwrap().locked), (Color::Index(1), false));
        // A state that freezes B with no current layer of its own (or one that is gone), restored
        // with B current: B stays current and thawed; A is turned off.
        for current in ["", "NOSUCH"] {
            let mut frozen = s.doc().unwrap().layers.clone();
            for l in &mut frozen {
                l.frozen = l.name == "B";
                l.on = l.name != "A";
            }
            s.doc_mut().unwrap().layer_states.push(LayerState::new(format!("f{current}"), frozen.clone(), current));
            s.execute("layer.current", &json!({ "name": "B" })).unwrap();
            s.execute("layerstate.restore", &json!({ "name": format!("f{current}") })).unwrap();
            let d = s.doc().unwrap();
            assert_eq!(d.header.str("CLAYER", ""), "B", "{current:?}");
            assert!(!d.layer("B").unwrap().frozen && !d.layer("A").unwrap().on, "{current:?}");
            // Naming A current, it becomes current and B freezes.
            s.doc_mut().unwrap().layer_states.push(LayerState::new("fA", frozen, "A"));
            s.execute("layerstate.restore", &json!({ "name": "fA" })).unwrap();
            let d = s.doc().unwrap();
            assert_eq!(d.header.str("CLAYER", ""), "A");
            assert!(d.layer("B").unwrap().frozen);
            s.doc_mut().unwrap().layer_states.retain(|st| st.name != "fA");
            s.execute("layerstate.restore", &json!({ "name": "all" })).unwrap();
        }
        // A current layer the state's mask leaves frozen doesn't become current (AutoCAD's S9: On
        // only, current B; restored with A current and B frozen).
        let mut on_only = s.doc().unwrap().layer_states[0].clone();
        (on_only.name, on_only.mask, on_only.current) = ("S9".into(), LayerState::ON | 512, "B".into());
        s.doc_mut().unwrap().layer_states.push(on_only);
        s.execute("layer.current", &json!({ "name": "A" })).unwrap();
        s.execute("layer.set", &json!({ "name": "B", "frozen": true })).unwrap();
        s.execute("layerstate.restore", &json!({ "name": "S9" })).unwrap();
        let d = s.doc().unwrap();
        assert_eq!(d.header.str("CLAYER", ""), "A");
        assert!(d.layer("B").unwrap().frozen);
        // With Frozen in the mask B is thawed and made current.
        let mut thaw = s.doc().unwrap().layer_states[0].clone();
        (thaw.name, thaw.current) = ("thaw".into(), "B".into());
        s.doc_mut().unwrap().layer_states.push(thaw);
        s.execute("layerstate.restore", &json!({ "name": "thaw" })).unwrap();
        let d = s.doc().unwrap();
        assert_eq!(d.header.str("CLAYER", ""), "B");
        assert!(!d.layer("B").unwrap().frozen);
        // Frozen in the mask, but no entry for B (a state written elsewhere): nothing would thaw
        // B, so A stays current and B frozen (AutoCAD 2024, measured).
        let mut no_entry = s.doc().unwrap().layer_states[0].clone();
        (no_entry.name, no_entry.current) = ("no entry".into(), "B".into());
        no_entry.layers.retain(|l| l.name != "B");
        s.doc_mut().unwrap().layer_states.push(no_entry);
        s.execute("layer.current", &json!({ "name": "A" })).unwrap();
        s.execute("layer.set", &json!({ "name": "B", "frozen": true })).unwrap();
        s.execute("layerstate.restore", &json!({ "name": "no entry" })).unwrap();
        let d = s.doc().unwrap();
        assert_eq!(d.header.str("CLAYER", ""), "A");
        assert!(d.layer("B").unwrap().frozen);
        // An entry spelled in another case is B's (layer names ignore case, as the restore of the
        // properties does): B is thawed and made current.
        let mut other_case = s.doc().unwrap().layer_states[0].clone();
        (other_case.name, other_case.current) = ("other case".into(), "B".into());
        for l in other_case.layers.iter_mut().filter(|l| l.name == "B") {
            l.name = "b".into();
        }
        s.doc_mut().unwrap().layer_states.push(other_case);
        s.execute("layerstate.restore", &json!({ "name": "other case" })).unwrap();
        let d = s.doc().unwrap();
        assert_eq!(d.header.str("CLAYER", ""), "B");
        assert!(!d.layer("B").unwrap().frozen);
        // What a state doesn't hold stays: the description.
        s.execute("layer.set", &json!({ "name": "A", "description": "kept" })).unwrap();
        s.execute("layerstate.restore", &json!({ "name": "all" })).unwrap();
        assert_eq!(s.doc().unwrap().layer("A").unwrap().description, "kept");
    }

    /// Plotting, freezing in new viewports, linetype, lineweight, plot style and transparency each
    /// come back under their own mask bit only: a state of one bit restores that property and
    /// leaves the other five as they are.
    #[test]
    fn layer_state_restores_each_property_under_its_own_bit_only() {
        use cadcraft_doc::{Drawing, LayerState as S, Lineweight};
        let mut s = Session::new();
        s.execute("layer.new", &json!({ "name": "A" })).unwrap();
        s.execute("layerstate.save", &json!({ "name": "all" })).unwrap();
        let saved = s.doc().unwrap().layer_states[0].clone();
        let props = |d: &Drawing| {
            let l = d.layer("A").unwrap();
            (l.plot, l.vp_freeze_new, l.linetype.clone(), l.lineweight, l.plot_style.clone(), l.transparency)
        };
        let change = |d: &mut Drawing| {
            let l = d.layer_mut("A").unwrap();
            (l.plot, l.vp_freeze_new, l.linetype, l.lineweight, l.plot_style, l.transparency) =
                (false, true, "DASHED".into(), Lineweight::from_dxf(50), "Thick".into(), 40);
        };
        let before = props(s.doc().unwrap());
        change(s.doc_mut().unwrap());
        let after = props(s.doc().unwrap());
        for bit in [S::PLOT, S::VP_FREEZE_NEW, S::LINETYPE, S::LINEWEIGHT, S::PLOT_STYLE, S::TRANSPARENCY] {
            let name = format!("b{bit}");
            s.doc_mut().unwrap().layer_states.push(S { name: name.clone(), mask: bit | 512, ..saved.clone() });
            change(s.doc_mut().unwrap());
            s.execute("layerstate.restore", &json!({ "name": name })).unwrap();
            let mut want = after.clone();
            match bit {
                S::PLOT => want.0 = before.0,
                S::VP_FREEZE_NEW => want.1 = before.1,
                S::LINETYPE => want.2.clone_from(&before.2),
                S::LINEWEIGHT => want.3 = before.3,
                S::PLOT_STYLE => want.4.clone_from(&before.4),
                _ => want.5 = before.5,
            }
            assert_eq!(props(s.doc().unwrap()), want, "mask {bit}");
        }
    }

    #[test]
    fn rejected_layer_set_changes_nothing() {
        let mut s = Session::new();
        // Layer 0 is current; freezing it is rejected and must not apply the colour either.
        let before = s.doc().unwrap().layer("0").unwrap().clone();
        let undo = s.state().unwrap().undo.len();
        assert!(s.execute("layer.set", &json!({ "name": "0", "frozen": true, "color": 3 })).is_err());
        assert_eq!(s.doc().unwrap().layer("0").unwrap(), &before);
        assert_eq!(s.state().unwrap().undo.len(), undo, "no undo step for a rejected call");
        // Same for a rejected rename.
        assert!(s.execute("layer.set", &json!({ "name": "0", "newName": "X", "color": 3 })).is_err());
        assert_eq!(s.doc().unwrap().layer("0").unwrap(), &before);
        assert_eq!(s.state().unwrap().undo.len(), undo, "no undo step for a rejected call");
        // A valid colour-only edit of the current layer still applies.
        s.execute("layer.set", &json!({ "name": "0", "color": 3 })).unwrap();
        assert_ne!(s.doc().unwrap().layer("0").unwrap(), &before);
    }

    #[test]
    fn delete_and_purge_keep_layers_used_by_block_definitions() {
        let mut s = Session::new();
        s.execute("layer.new", &json!({ "name": "A", "color": 1 })).unwrap();
        s.execute("layer.current", &json!({ "name": "A" })).unwrap();
        let l = s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap()["handles"][0].as_str().unwrap().to_string();
        s.execute("block", &json!({ "name": "B", "base": [0, 0], "handles": [l], "keep": "delete" })).unwrap();
        s.execute("layer.current", &json!({ "name": "0" })).unwrap();
        s.execute("insert", &json!({ "name": "B", "at": [5, 5] })).unwrap();
        assert!(s.execute("layer.delete", &json!({ "name": "A" })).is_err());
        s.execute("purge", &json!({})).unwrap();
        assert!(s.doc().unwrap().layer("A").is_some());
    }

    #[test]
    fn rename_repoints_paper_space_and_block_entities() {
        let mut s = Session::new();
        s.execute("layer.new", &json!({ "name": "A", "color": "red", "current": true })).unwrap();
        s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap();
        s.execute("selectall", &json!({})).unwrap();
        s.execute("block", &json!({ "name": "Part", "base": [0, 0], "keep": "delete" })).unwrap();
        s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
        s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap();
        s.execute("layer.set", &json!({ "name": "A", "newName": "B" })).unwrap();
        let d = s.doc().unwrap();
        let on_layer = |name: &str| {
            let paper = d.layouts.iter().flat_map(|l| l.entities.iter());
            let blocks = d.blocks.values().flat_map(|b| b.entities.iter());
            paper.chain(blocks).filter(|e| e.common.layer == name).count()
        };
        assert_eq!(on_layer("A"), 0);
        assert_eq!(on_layer("B"), 2);
    }
}
