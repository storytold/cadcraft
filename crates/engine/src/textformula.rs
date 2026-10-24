//! Formulas in text applied to drawings: a text, multiline text, multileader or dimension text
//! whose value is a formula (`=...`, one line) keeps the formula and shows its result; formulas and
//! AutoCAD fields are worked out again when a drawing is opened. The evaluator and the function
//! list are [`cadcraft_render::textformula`].

use cadcraft_doc::{Drawing, EntityKind};
pub use cadcraft_render::textformula::{Env, FUNCTIONS, evaluate, has_fields, is_formula, local_now};

/// The environment of a session's drawing.
pub fn env(s: &crate::Session) -> Env {
    let file = s
        .state()
        .ok()
        .map(|st| match &st.path {
            Some(p) => std::path::Path::new(p).file_name().map_or_else(|| p.clone(), |n| n.to_string_lossy().into_owned()),
            None => st.title.clone(),
        })
        .unwrap_or_default();
    Env { now: local_now(), file, sheet: s.space().tab_name().to_string(), measured: None }
}

// ---------- applying to a drawing ----------

/// The measurement a dimension shows (for `<>` in its formula).
pub fn measured(d: &Drawing, dm: &cadcraft_doc::Dimension) -> Option<f64> {
    let plain = cadcraft_doc::Dimension {
        text: String::new(),
        block: None,
        marks: cadcraft_doc::DimMarks { formula: None, ..dm.marks.clone() },
        ..dm.clone()
    };
    let v = cadcraft_render::dimension_in(d, &plain).value;
    let s: String = v
        .chars()
        .skip_while(|c| !c.is_ascii_digit() && *c != '-' && *c != '.')
        .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-' | ' '))
        .collect();
    s.replace(' ', "").replace(',', ".").parse().ok()
}

/// A text object's formula, worked out into what it shows: a value that is a formula (one line
/// starting with `=`) becomes its formula; a formula's result becomes the value. `edited` is a
/// value just typed: typing over an AutoCAD field turns it into plain text, typing a value that
/// is not a formula drops the formula. Text, attribute definitions, multiline text,
/// multileader text and dimension text (whose `<>` is its measurement, `env.measured`).
pub fn apply(kind: &mut EntityKind, env: &Env, edited: bool) {
    let (value, formula): (&mut String, &mut Option<String>) = match kind {
        EntityKind::Text(t) => (&mut t.value, &mut t.formula),
        EntityKind::AttDef(a) => (&mut a.text.value, &mut a.text.formula),
        EntityKind::MText(t) => (&mut t.contents, &mut t.formula),
        EntityKind::MLeader(m) => match &mut m.text {
            Some(t) => (&mut t.contents, &mut t.formula),
            None => return,
        },
        EntityKind::Dimension(d) => (&mut d.text, &mut d.marks.formula),
        _ => return,
    };
    if edited {
        let plain = cadcraft_fonts::plain_mtext(value);
        if is_formula(&plain) {
            *formula = Some(plain.trim().to_string());
        } else {
            *formula = None;
            return;
        }
    }
    if let Some(f) = formula.as_deref()
        && let Some(v) = evaluate(f, env)
    {
        *value = v;
    }
}

/// Whether an entity has a formula or field to work out.
fn has_formula(k: &EntityKind) -> bool {
    match k {
        EntityKind::Text(t) => t.formula.is_some(),
        EntityKind::AttDef(a) => a.text.formula.is_some(),
        EntityKind::MText(t) => t.formula.is_some(),
        EntityKind::MLeader(m) => m.text.as_ref().is_some_and(|t| t.formula.is_some()),
        EntityKind::Dimension(d) => d.marks.formula.is_some(),
        _ => false,
    }
}

/// Work out every formula and known field of a drawing again (when it is opened): model space,
/// layouts and block definitions. The number of texts worked out.
pub fn refresh(d: &mut Drawing, env: &Env) -> usize {
    let mut n = 0;
    // Dimensions' measurements first (they need the whole drawing).
    let mut measures: std::collections::HashMap<cadcraft_doc::Handle, Option<f64>> = std::collections::HashMap::new();
    for e in d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())) {
        if let EntityKind::Dimension(dm) = &e.kind
            && dm.marks.formula.is_some()
        {
            measures.insert(e.handle, measured(d, dm));
        }
    }
    let fix = |store: &mut cadcraft_doc::EntityStore, n: &mut usize| {
        let hs: Vec<cadcraft_doc::Handle> = store.iter().filter(|e| has_formula(&e.kind)).map(|e| e.handle).collect();
        for h in hs {
            let env = Env { measured: measures.get(&h).copied().flatten(), ..env.clone() };
            store.modify(h, |e| apply(&mut e.kind, &env, false));
            *n += 1;
        }
    };
    fix(&mut d.model, &mut n);
    for l in &mut d.layouts {
        fix(&mut l.entities, &mut n);
    }
    let names: Vec<String> = d.blocks.iter().filter(|(_, b)| b.entities.iter().any(|e| has_formula(&e.kind))).map(|(k, _)| k.clone()).collect();
    for k in names {
        if let Some(b) = d.blocks.get_mut(&k) {
            fix(&mut std::sync::Arc::make_mut(b).entities, &mut n);
        }
    }
    n
}
