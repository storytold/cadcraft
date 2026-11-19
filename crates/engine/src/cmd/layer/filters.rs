//! Layer filters of the Layer Properties Manager: property filters (rules over layer properties)
//! and group filters (chosen layers), nested in a tree, and which one the manager shows.

use cadcraft_doc::{Drawing, FilterKind, FilterRule, Layer, LayerFilter, LayerFilters};
use serde_json::{Value, json};

use super::super::{CommandSpec, bad, ok, str_param};
use crate::{Result, Session};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("layerfilter.new", "New Layer Filter", run_new).params(
            "{name, kind?: \"property\" (default) | \"group\", parent?: filter name, rules?: [{name?, on?, frozen?, locked?, plot?, newVpFreeze?, used?, color?, linetype?, lineweight?, transparency?, plotStyle?, description?}] (property; text fields are wildcard patterns), layers?: [names] (group)}",
        ),
        CommandSpec::new("layerfilter.set", "Edit Layer Filter", run_set).params("{name, newName?, rules? (property), layers? (group: replaces its layers), parent?: filter name or null (top level)}"),
        CommandSpec::new("layerfilter.delete", "Delete Layer Filter", run_delete).params("{name} (with the filters nested in it; the layers stay)"),
        CommandSpec::new("layerfilter.add", "Add to Group Filter", run_add).params("{name, layers: [names]}"),
        CommandSpec::new("layerfilter.remove", "Remove from Group Filter", run_remove).params("{name, layers: [names]} (the layers stay in the drawing)"),
        CommandSpec::new("layerfilter.apply", "Apply Layer Filter", run_apply)
            .params("{name?: filter name, \"All\" (default) or \"All Used Layers\", invert?: bool, palette?: bool (the Layers palette lists only the filter's layers)} → layers")
            .noundo(),
        CommandSpec::new("layerfilter.list", "List Layer Filters", run_list).params("{name?: list this filter's layers (default: the current filter)} → filters, current, invert, palette, layers").noundo(),
    ]
}

const RULE_KEYS: [&str; 13] =
    ["name", "on", "frozen", "locked", "plot", "newVpFreeze", "used", "color", "linetype", "lineweight", "transparency", "plotStyle", "description"];

fn parse_rule(cmd: &str, v: &Value) -> Result<FilterRule> {
    let o = v.as_object().ok_or_else(|| bad(cmd, "each rule is an object such as {\"name\": \"A-WALL*\"}"))?;
    if let Some(k) = o.keys().find(|k| !RULE_KEYS.contains(&k.as_str())) {
        return Err(bad(cmd, format!("unknown rule field `{k}` (fields: {})", RULE_KEYS.join(", "))));
    }
    let text = |k: &str| -> Result<String> {
        match o.get(k) {
            None | Some(Value::Null) => Ok(String::new()),
            Some(Value::String(s)) => Ok(s.trim().to_string()),
            Some(Value::Number(n)) => Ok(n.to_string()),
            Some(other) => Err(bad(cmd, format!("rule field `{k}` must be text, got {other}"))),
        }
    };
    let flag = |k: &str| -> Result<Option<bool>> {
        match o.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(other) => Err(bad(cmd, format!("rule field `{k}` must be true, false or null, got {other}"))),
        }
    };
    let rule = FilterRule {
        name: text("name")?,
        on: flag("on")?,
        frozen: flag("frozen")?,
        locked: flag("locked")?,
        plot: flag("plot")?,
        new_vp_freeze: flag("newVpFreeze")?,
        used: flag("used")?,
        color: text("color")?,
        linetype: text("linetype")?,
        lineweight: text("lineweight")?,
        transparency: text("transparency")?,
        plot_style: text("plotStyle")?,
        description: text("description")?,
    };
    rule.validate().map_err(|e| bad(cmd, format!("invalid pattern: {e}")))?;
    Ok(rule)
}

fn parse_rules(cmd: &str, p: &Value) -> Result<Option<Vec<FilterRule>>> {
    match p.get("rules") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) if a.len() > cadcraft_doc::layer_filter::MAX_RULES => {
            Err(bad(cmd, format!("a property filter has at most {} rules", cadcraft_doc::layer_filter::MAX_RULES)))
        }
        Some(Value::Array(a)) => a.iter().map(|r| parse_rule(cmd, r)).collect::<Result<Vec<_>>>().map(Some),
        Some(v @ Value::Object(_)) => parse_rule(cmd, v).map(|r| Some(vec![r])),
        Some(other) => Err(bad(cmd, format!("`rules` must be an array of rule objects, got {other}"))),
    }
}

fn name_list(cmd: &str, p: &Value, key: &str) -> Result<Option<Vec<String>>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) if a.len() > cadcraft_doc::layer_filter::MAX_GROUP_LAYERS => Err(bad(cmd, format!("too many layers in `{key}`"))),
        Some(Value::Array(a)) => a
            .iter()
            .map(|v| v.as_str().map(str::to_string).ok_or_else(|| bad(cmd, format!("`{key}` holds layer names"))))
            .collect::<Result<Vec<_>>>()
            .map(Some),
        Some(Value::String(s)) => Ok(Some(vec![s.clone()])),
        Some(other) => Err(bad(cmd, format!("`{key}` must be an array of layer names, got {other}"))),
    }
}

fn drawing_layers(cmd: &str, d: &Drawing, names: &[String]) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for n in names {
        let l = d.layer(n).ok_or_else(|| bad(cmd, format!("no layer `{n}`")))?;
        if !out.iter().any(|o| o.eq_ignore_ascii_case(&l.name)) {
            out.push(l.name.clone());
        }
    }
    Ok(out)
}

fn required<'a>(cmd: &str, p: &'a Value) -> Result<&'a str> {
    str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`name` (filter name) is required"))
}

fn existing<'a>(cmd: &str, d: &'a Drawing, name: &str) -> Result<&'a LayerFilter> {
    d.layer_filters.get(name).ok_or_else(|| bad(cmd, format!("no layer filter `{name}`")))
}

fn check_parent(cmd: &str, fs: &LayerFilters, parent: &str, group: bool, own: Option<&str>) -> Result<String> {
    let p = fs.get(parent).ok_or_else(|| bad(cmd, format!("no layer filter `{parent}` to nest in")))?;
    if group && !p.is_group() {
        return Err(bad(cmd, "a group filter can only be nested in another group filter"));
    }
    if let Some(own) = own
        && fs.subtree(own).iter().any(|n| n.eq_ignore_ascii_case(&p.name))
    {
        return Err(bad(cmd, "a filter cannot be nested in itself or in a filter nested in it"));
    }
    if fs.chain(&p.name).map_err(|e| bad(cmd, e.to_string()))?.len() >= cadcraft_doc::layer_filter::MAX_FILTER_DEPTH {
        return Err(bad(cmd, "filters are nested too deep"));
    }
    Ok(p.name.clone())
}

fn run_new(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "layerfilter.new";
    let name = required(CMD, p)?.to_string();
    LayerFilters::valid_name(&name).map_err(|e| bad(CMD, e.to_string()))?;
    let group = match str_param(p, "kind").map(str::to_ascii_lowercase).as_deref() {
        None | Some("property") => false,
        Some("group") => true,
        Some(k) => return Err(bad(CMD, format!("`kind` is \"property\" or \"group\", not \"{k}\""))),
    };
    let d = s.doc()?;
    if d.layer_filters.get(&name).is_some() {
        return Err(bad(CMD, format!("layer filter `{name}` already exists")));
    }
    if d.layer_filters.filters.len() >= cadcraft_doc::layer_filter::MAX_FILTERS {
        return Err(bad(CMD, "the drawing has too many layer filters"));
    }
    let parent = str_param(p, "parent").map(|pa| check_parent(CMD, &d.layer_filters, pa, group, None)).transpose()?;
    let kind = if group {
        if p.get("rules").is_some_and(|r| !r.is_null()) {
            return Err(bad(CMD, "a group filter holds layers, not rules"));
        }
        FilterKind::Group { layers: drawing_layers(CMD, d, &name_list(CMD, p, "layers")?.unwrap_or_default())? }
    } else {
        if p.get("layers").is_some_and(|r| !r.is_null()) {
            return Err(bad(CMD, "a property filter holds rules, not layers (use kind \"group\")"));
        }
        FilterKind::Property { rules: parse_rules(CMD, p)?.unwrap_or_default() }
    };
    let d = s.doc_mut()?;
    d.layer_filters.filters.push(LayerFilter { name: name.clone(), parent, kind });
    let count = d.layer_filters.select(Some(&name), false, &d.layers, &used_fn(d)).map(|v| v.len()).unwrap_or(0);
    Ok(json!({ "name": name, "layers": count }))
}

fn run_set(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "layerfilter.set";
    let name = required(CMD, p)?;
    let d = s.doc()?;
    let mut f = existing(CMD, d, name)?.clone();
    let old = f.name.clone();
    if let Some(nn) = str_param(p, "newName").map(str::trim) {
        LayerFilters::valid_name(nn).map_err(|e| bad(CMD, e.to_string()))?;
        if !nn.eq_ignore_ascii_case(&old) && d.layer_filters.get(nn).is_some() {
            return Err(bad(CMD, format!("layer filter `{nn}` already exists")));
        }
        f.name = nn.to_string();
    }
    match p.get("parent") {
        None => {}
        Some(Value::Null) => f.parent = None,
        Some(Value::String(pa)) => f.parent = Some(check_parent(CMD, &d.layer_filters, pa, f.is_group(), Some(&old))?),
        Some(other) => return Err(bad(CMD, format!("`parent` is a filter name or null, got {other}"))),
    }
    match &mut f.kind {
        FilterKind::Property { rules } => {
            if p.get("layers").is_some_and(|r| !r.is_null()) {
                return Err(bad(CMD, format!("`{old}` is a property filter: it holds rules, not layers")));
            }
            if let Some(r) = parse_rules(CMD, p)? {
                *rules = r;
            }
        }
        FilterKind::Group { layers } => {
            if p.get("rules").is_some_and(|r| !r.is_null()) {
                return Err(bad(CMD, format!("`{old}` is a group filter: it holds layers, not rules")));
            }
            if let Some(l) = name_list(CMD, p, "layers")? {
                *layers = drawing_layers(CMD, d, &l)?;
            }
        }
    }
    let d = s.doc_mut()?;
    let fs = &mut d.layer_filters;
    if !f.name.eq(&old) {
        for c in fs.filters.iter_mut().filter(|c| c.parent.as_deref().is_some_and(|pa| pa.eq_ignore_ascii_case(&old))) {
            c.parent = Some(f.name.clone());
        }
        if fs.current.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(&old)) {
            fs.current = Some(f.name.clone());
        }
    }
    if let Some(slot) = fs.get_mut(&old) {
        *slot = f;
    }
    ok()
}

fn run_delete(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "layerfilter.delete";
    let name = required(CMD, p)?;
    let d = s.doc()?;
    let gone = d.layer_filters.subtree(&existing(CMD, d, name)?.name);
    let d = s.doc_mut()?;
    let fs = &mut d.layer_filters;
    fs.filters.retain(|f| !gone.iter().any(|g| g.eq_ignore_ascii_case(&f.name)));
    if fs.current.as_deref().is_some_and(|c| gone.iter().any(|g| g.eq_ignore_ascii_case(c))) {
        fs.current = None;
        fs.invert = false;
    }
    Ok(json!({ "deleted": gone }))
}

fn edit_group(s: &mut Session, p: &Value, cmd: &str, add: bool) -> Result<Value> {
    let name = required(cmd, p)?;
    let d = s.doc()?;
    let f = existing(cmd, d, name)?;
    let FilterKind::Group { layers } = &f.kind else {
        return Err(bad(cmd, format!("`{}` is a property filter: its layers follow its rules", f.name)));
    };
    let names = name_list(cmd, p, "layers")?.filter(|l| !l.is_empty()).ok_or_else(|| bad(cmd, "`layers` (layer names) is required"))?;
    let mut next = layers.clone();
    if add {
        for n in drawing_layers(cmd, d, &names)? {
            if !next.iter().any(|l| l.eq_ignore_ascii_case(&n)) {
                next.push(n);
            }
        }
        if next.len() > cadcraft_doc::layer_filter::MAX_GROUP_LAYERS {
            return Err(bad(cmd, "the group filter would hold too many layers"));
        }
    } else {
        if let Some(n) = names.iter().find(|n| !layers.iter().any(|l| l.eq_ignore_ascii_case(n))) {
            return Err(bad(cmd, format!("layer `{n}` is not in group filter `{}`", f.name)));
        }
        next.retain(|l| !names.iter().any(|n| n.eq_ignore_ascii_case(l)));
    }
    let fname = f.name.clone();
    let count = next.len();
    let d = s.doc_mut()?;
    if let Some(FilterKind::Group { layers }) = d.layer_filters.get_mut(&fname).map(|f| &mut f.kind) {
        *layers = next;
    }
    Ok(json!({ "name": fname, "layers": count }))
}

fn run_add(s: &mut Session, p: &Value) -> Result<Value> {
    edit_group(s, p, "layerfilter.add", true)
}

fn run_remove(s: &mut Session, p: &Value) -> Result<Value> {
    edit_group(s, p, "layerfilter.remove", false)
}

fn used_fn(d: &Drawing) -> impl Fn(&Layer) -> bool + use<> {
    let used = super::used_layers(d);
    move |l: &Layer| used.contains(&l.name.to_ascii_lowercase())
}

fn canonical(cmd: &str, d: &Drawing, name: &str) -> Result<Option<String>> {
    let n = name.trim();
    if n.is_empty() || n.eq_ignore_ascii_case(LayerFilters::ALL) {
        return Ok(None);
    }
    if n.eq_ignore_ascii_case(LayerFilters::ALL_USED) {
        return Ok(Some(LayerFilters::ALL_USED.to_string()));
    }
    Ok(Some(existing(cmd, d, n)?.name.clone()))
}

fn listed(cmd: &str, d: &Drawing, name: Option<&str>, invert: bool) -> Result<Vec<String>> {
    let idx = d.layer_filters.select(name, invert, &d.layers, &used_fn(d)).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(idx.into_iter().filter_map(|i| d.layers.get(i).map(|l| l.name.clone())).collect())
}

fn none_message(d: &Drawing, name: Option<&str>) -> Option<String> {
    let n = name?;
    Some(match d.layer_filters.get(n).map(|f| &f.kind) {
        Some(FilterKind::Group { layers }) if layers.is_empty() => format!("Group filter `{n}` holds no layers yet."),
        _ => format!("No layers match the filter `{n}`."),
    })
}

fn run_apply(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "layerfilter.apply";
    let d = s.doc()?;
    let current = match str_param(p, "name") {
        Some(n) => canonical(CMD, d, n)?,
        None if p.get("name").is_some_and(|v| !v.is_null()) => return Err(bad(CMD, "`name` is a filter name")),
        None if p.get("name").is_some() => None,
        None => d.layer_filters.current.clone(),
    };
    let invert = p.get("invert").and_then(Value::as_bool).unwrap_or(d.layer_filters.invert && current.is_some());
    let palette = p.get("palette").and_then(Value::as_bool).unwrap_or(d.layer_filters.apply_to_palette);
    let layers = listed(CMD, d, current.as_deref(), invert)?;
    let message = if layers.is_empty() { none_message(d, current.as_deref()) } else { None };
    let d = s.doc_mut()?;
    let fs = &mut d.layer_filters;
    (fs.current, fs.invert, fs.apply_to_palette) = (current.clone(), invert, palette);
    let mut out = json!({ "current": current.unwrap_or_else(|| LayerFilters::ALL.into()), "invert": invert, "palette": palette, "layers": layers });
    if let (Some(m), Some(o)) = (message, out.as_object_mut()) {
        o.insert("message".into(), json!(m));
    }
    Ok(out)
}

fn filter_json(f: &LayerFilter, count: Option<usize>) -> Value {
    let mut v = serde_json::to_value(f).unwrap_or(Value::Null);
    if let Some(o) = v.as_object_mut() {
        o.insert("count".into(), count.map_or(Value::Null, |c| json!(c)));
    }
    v
}

fn run_list(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "layerfilter.list";
    let d = s.doc()?;
    let used = used_fn(d);
    let fs = &d.layer_filters;
    let filters: Vec<Value> =
        fs.filters.iter().map(|f| filter_json(f, fs.select(Some(&f.name), false, &d.layers, &used).ok().map(|v| v.len()))).collect();
    let (name, invert) = match str_param(p, "name") {
        Some(n) => (canonical(CMD, d, n)?, false),
        None => (fs.current.clone(), fs.invert),
    };
    let layers = listed(CMD, d, name.as_deref(), invert)?;
    let mut out = json!({
        "filters": filters,
        "current": fs.current.clone().unwrap_or_else(|| LayerFilters::ALL.into()),
        "invert": fs.invert,
        "palette": fs.apply_to_palette,
        "layers": layers,
    });
    let message = if fs.filters.is_empty() {
        Some("No layer filters.".to_string())
    } else if layers.is_empty() {
        none_message(d, name.as_deref())
    } else {
        None
    };
    if let (Some(m), Some(o)) = (message, out.as_object_mut()) {
        o.insert("message".into(), json!(m));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    fn names(v: &serde_json::Value) -> Vec<String> {
        v["layers"].as_array().unwrap().iter().map(|l| l.as_str().unwrap().to_string()).collect()
    }

    fn session(layers: &[&str]) -> Session {
        let mut s = Session::new();
        for l in layers {
            s.execute("layer.new", &json!({ "name": l })).unwrap();
        }
        s
    }

    #[test]
    fn property_filter_follows_new_and_renamed_layers() {
        let mut s = session(&["A-WALL", "A-WALL-EXT", "A-ANNO", "S-ANNO-DIM", "B-WALL"]);
        s.execute("layerfilter.new", &json!({ "name": "Walls", "rules": [{ "name": "A-WALL*" }] })).unwrap();
        s.execute("layerfilter.new", &json!({ "name": "Anno", "rules": [{ "name": "*ANNO*" }] })).unwrap();
        let v = s.execute("layerfilter.apply", &json!({ "name": "Walls" })).unwrap();
        assert_eq!(names(&v), ["A-WALL", "A-WALL-EXT"]);
        s.execute("layer.new", &json!({ "name": "A-WALL-NEW" })).unwrap();
        assert_eq!(names(&s.execute("layerfilter.list", &json!({})).unwrap()), ["A-WALL", "A-WALL-EXT", "A-WALL-NEW"]);
        s.execute("layer.set", &json!({ "name": "B-WALL", "newName": "a-wall-b" })).unwrap();
        assert_eq!(names(&s.execute("layerfilter.list", &json!({})).unwrap()).len(), 4);
        assert_eq!(names(&s.execute("layerfilter.list", &json!({ "name": "Anno" })).unwrap()), ["A-ANNO", "S-ANNO-DIM"]);
        let v = s.execute("layerfilter.apply", &json!({ "name": "Walls", "invert": true })).unwrap();
        assert!(names(&v).contains(&"0".to_string()) && !names(&v).contains(&"A-WALL".to_string()));
        let v = s.execute("layerfilter.apply", &json!({ "name": "All" })).unwrap();
        assert_eq!(v["current"], "All");
        assert_eq!(v["invert"], false);
        assert_eq!(names(&v).len(), s.doc().unwrap().layers.len());
    }

    #[test]
    fn property_rules_combine_layer_properties() {
        let mut s = session(&["A", "B", "C"]);
        s.execute("layer.set", &json!({ "name": "A", "color": 1, "frozen": true })).unwrap();
        s.execute("layer.set", &json!({ "name": "B", "color": 1 })).unwrap();
        s.execute("layer.set", &json!({ "name": "C", "locked": true })).unwrap();
        s.execute("layerfilter.new", &json!({ "name": "Red thawed", "rules": [{ "color": "red", "frozen": false }] })).unwrap();
        assert_eq!(names(&s.execute("layerfilter.list", &json!({ "name": "Red thawed" })).unwrap()), ["B"]);
        s.execute("layerfilter.set", &json!({ "name": "Red thawed", "rules": [{ "color": "1", "frozen": false }, { "locked": true }] })).unwrap();
        assert_eq!(names(&s.execute("layerfilter.list", &json!({ "name": "Red thawed" })).unwrap()), ["B", "C"]);
        s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap();
        s.execute("layerfilter.new", &json!({ "name": "Used", "rules": { "used": true } })).unwrap();
        assert_eq!(names(&s.execute("layerfilter.list", &json!({ "name": "Used" })).unwrap()), ["0"]);
    }

    #[test]
    fn group_filter_holds_picked_layers() {
        let mut s = session(&["P1", "P2", "P3", "Q"]);
        s.execute("layerfilter.new", &json!({ "name": "Picked", "kind": "group", "layers": ["P1", "p2"] })).unwrap();
        s.execute("layerfilter.add", &json!({ "name": "Picked", "layers": ["P3", "P1"] })).unwrap();
        let v = s.execute("layerfilter.apply", &json!({ "name": "picked" })).unwrap();
        assert_eq!(names(&v), ["P1", "P2", "P3"]);
        assert_eq!(v["current"], "Picked");
        s.execute("layerfilter.remove", &json!({ "name": "Picked", "layers": ["P2"] })).unwrap();
        assert_eq!(names(&s.execute("layerfilter.list", &json!({})).unwrap()), ["P1", "P3"]);
        assert!(s.doc().unwrap().layer("P2").is_some(), "removing from a group keeps the layer");
        assert!(s.execute("layerfilter.remove", &json!({ "name": "Picked", "layers": ["Q"] })).is_err());
        assert!(s.execute("layerfilter.add", &json!({ "name": "Picked", "layers": ["Nope"] })).is_err());
        s.execute("layer.set", &json!({ "name": "P3", "newName": "P3-RENAMED" })).unwrap();
        s.execute("layer.delete", &json!({ "name": "P1" })).unwrap();
        assert_eq!(names(&s.execute("layerfilter.list", &json!({})).unwrap()), ["P3-RENAMED"]);
        let f = s.doc().unwrap().layer_filters.get("Picked").unwrap().clone();
        assert_eq!(f.kind, cadcraft_doc::FilterKind::Group { layers: vec!["P3-RENAMED".into()] });
    }

    #[test]
    fn nesting_rename_and_delete() {
        let mut s = session(&["A-WALL", "A-WALL-EXT", "A-DOOR"]);
        s.execute("layerfilter.new", &json!({ "name": "Arch", "rules": [{ "name": "A-*" }] })).unwrap();
        s.execute("layerfilter.new", &json!({ "name": "Ext", "parent": "Arch", "rules": [{ "name": "*EXT" }] })).unwrap();
        s.execute("layerfilter.new", &json!({ "name": "G", "kind": "group", "layers": ["A-DOOR"] })).unwrap();
        assert!(s.execute("layerfilter.new", &json!({ "name": "G2", "kind": "group", "parent": "Arch" })).is_err());
        s.execute("layerfilter.new", &json!({ "name": "G walls", "parent": "G", "rules": [{ "name": "*WALL*" }] })).unwrap();
        assert!(names(&s.execute("layerfilter.list", &json!({ "name": "G walls" })).unwrap()).is_empty());
        assert!(s.execute("layerfilter.set", &json!({ "name": "Arch", "parent": "Ext" })).is_err());
        s.execute("layerfilter.apply", &json!({ "name": "Ext" })).unwrap();
        s.execute("layerfilter.set", &json!({ "name": "Arch", "newName": "Architecture" })).unwrap();
        let d = s.doc().unwrap();
        assert_eq!(d.layer_filters.get("Ext").unwrap().parent.as_deref(), Some("Architecture"));
        assert_eq!(names(&s.execute("layerfilter.list", &json!({})).unwrap()), ["A-WALL-EXT"]);
        let v = s.execute("layerfilter.delete", &json!({ "name": "Architecture" })).unwrap();
        assert_eq!(v["deleted"], json!(["Architecture", "Ext"]));
        let d = s.doc().unwrap();
        assert_eq!(d.layer_filters.current, None);
        assert_eq!(d.layer_filters.filters.len(), 2);
        assert_eq!(d.layers.len(), 5);
        s.execute("undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().layer_filters.filters.len(), 4);
    }

    #[test]
    fn bad_input_gives_clear_messages() {
        let mut s = session(&["A"]);
        let msg = |s: &mut Session, p: serde_json::Value| s.execute("layerfilter.new", &p).unwrap_err().to_string();
        assert!(msg(&mut s, json!({ "name": "F", "rules": [{ "name": "A[" }] })).contains("closing ]"));
        assert!(msg(&mut s, json!({ "name": "F", "rules": [{ "colour": "1" }] })).contains("unknown rule field"));
        assert!(msg(&mut s, json!({ "name": "All" })).contains("built-in"));
        assert!(msg(&mut s, json!({ "name": "a*b" })).contains("filter name"));
        assert!(msg(&mut s, json!({ "name": "F", "kind": "smart" })).contains("kind"));
        assert!(msg(&mut s, json!({ "name": "F", "rules": [{ "frozen": "yes" }] })).contains("true, false"));
        s.execute("layerfilter.new", &json!({ "name": "None", "rules": [{ "name": "ZZZ*" }] })).unwrap();
        assert!(msg(&mut s, json!({ "name": "none" })).contains("already exists"));
        let v = s.execute("layerfilter.apply", &json!({ "name": "None" })).unwrap();
        assert!(v["message"].as_str().unwrap().contains("No layers match"));
        s.execute("layerfilter.new", &json!({ "name": "Empty", "kind": "group" })).unwrap();
        let v = s.execute("layerfilter.apply", &json!({ "name": "Empty" })).unwrap();
        assert!(v["message"].as_str().unwrap().contains("holds no layers"));
        assert!(s.execute("layerfilter.apply", &json!({ "name": "Missing" })).is_err());
        assert!(s.execute("layerfilter.add", &json!({ "name": "None", "layers": ["A"] })).unwrap_err().to_string().contains("property filter"));
        s.doc_mut()
            .unwrap()
            .layer_filters
            .filters
            .push(cadcraft_doc::LayerFilter::property("Broken", vec![cadcraft_doc::FilterRule { name: "[".into(), ..Default::default() }]));
        assert!(s.execute("layerfilter.apply", &json!({ "name": "Broken" })).unwrap_err().to_string().contains("closing ]"));
        assert!(s.execute("layerfilter.list", &json!({})).is_ok());
    }
}
