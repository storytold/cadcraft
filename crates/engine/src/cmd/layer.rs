//! Layers: LAYER (programmatic sub-ids), layer tools (LAYMCUR, LAYISO, LAYOFF, LAYFRZ…).

use cadcraft_color::Color;
use cadcraft_doc::{Layer, Lineweight};
use serde_json::{Value, json};

use super::machines::SelectRun;
use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("layer", "Layers", run_list).menu(&["Format", "Layers"]).alias(&["la", "layers"]).noundo(),
        CommandSpec::new("layer.new", "New Layer", run_new).params("{name, color?, linetype?, lineweight? (mm), current?: bool}"),
        CommandSpec::new("layer.set", "Set Layer Properties", run_set)
            .params("{name, on?, frozen?, locked?, plot?, color?, linetype?, lineweight?, transparency?, description?, newVpFreeze?, newName?}"),
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
            .interactive(|_| Ok(Box::new(SelectRun::single("layoff", "LAYOFF", "Select an object on the layer to be turned off")))),
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
        CommandSpec::new("layerstate.save", "Save Layer State", run_state_save).menu(&["Format", "Layer States Manager..."]).params("{name}"),
        CommandSpec::new("layerstate.restore", "Restore Layer State", run_state_restore).params("{name}"),
        CommandSpec::new("layerstate.list", "List Layer States", run_state_list).params("{} → states").noundo(),
        CommandSpec::new("layerstate.delete", "Delete Layer State", run_state_delete).params("{name}"),
        CommandSpec::new("layerstate.rename", "Rename Layer State", run_state_rename).params("{from, to}"),
    ]
}

fn layer_json(l: &Layer, current: bool) -> Value {
    json!({
        "name": l.name, "on": l.on, "frozen": l.frozen, "locked": l.locked, "plot": l.plot,
        "color": l.color.name(), "colorRgb": l.color.resolve(Color::Index(7), Color::Index(7)).hex(),
        "linetype": l.linetype, "lineweight": l.lineweight.name(), "transparency": l.transparency,
        "description": l.description, "current": current, "newVpFreeze": l.vp_freeze_new,
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

fn parse_lw(v: &Value) -> Option<Lineweight> {
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

fn valid_name(n: &str) -> bool {
    !n.trim().is_empty() && n.len() <= 255 && !n.chars().any(|c| "<>/\\\":;?*|,=`".contains(c))
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
    let mut l = Layer::new(&name);
    apply(&mut l, p)?;
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
    apply(&mut updated, p)?;
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
    }
    let d = s.doc_mut()?;
    let l = d.layer_mut(&old).ok_or_else(|| bad("layer.set", format!("no layer `{name}`")))?;
    *l = updated;
    if let Some(nn) = new_name {
        l.name = nn.clone();
        // Re-point entities.
        let hs: Vec<_> = d.model.iter().filter(|e| e.common.layer.eq_ignore_ascii_case(&old)).map(|e| e.handle).collect();
        for h in hs {
            d.model.modify(h, |e| e.common.layer = nn.clone());
        }
        if cur.eq_ignore_ascii_case(&old) {
            d.header.set_str("CLAYER", &nn);
        }
    }
    ok()
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
    let name = str_param(p, "name").ok_or_else(|| bad("layerstate.save", "`name` is required"))?.to_string();
    let d = s.doc_mut()?;
    let layers = d.layers.clone();
    d.layer_states.retain(|st| st.name != name);
    d.layer_states.push(cadcraft_doc::LayerState { name, layers });
    ok()
}

fn run_state_restore(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("layerstate.restore", "`name` is required"))?;
    let d = s.doc_mut()?;
    let st = d.layer_states.iter().find(|st| st.name == name).cloned().ok_or_else(|| bad("layerstate.restore", "no such layer state"))?;
    for saved in st.layers {
        if let Some(l) = d.layer_mut(&saved.name) {
            *l = saved;
        }
    }
    ok()
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
    if !d.layer_states.iter().any(|st| st.name == name) {
        return Err(bad("layerstate.delete", "no such layer state"));
    }
    d.layer_states.retain(|st| st.name != name);
    ok()
}

fn run_state_rename(s: &mut Session, p: &Value) -> Result<Value> {
    let from = str_param(p, "from").ok_or_else(|| bad("layerstate.rename", "`from` is required"))?;
    let to = str_param(p, "to").map(str::trim).ok_or_else(|| bad("layerstate.rename", "`to` is required"))?.to_string();
    if !valid_name(&to) {
        return Err(bad("layerstate.rename", "invalid name"));
    }
    let d = s.doc_mut()?;
    if d.layer_states.iter().any(|st| st.name == to) {
        return Err(bad("layerstate.rename", format!("layer state `{to}` already exists")));
    }
    let st = d.layer_states.iter_mut().find(|st| st.name == from).ok_or_else(|| bad("layerstate.rename", "no such layer state"))?;
    st.name = to;
    ok()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

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
}
