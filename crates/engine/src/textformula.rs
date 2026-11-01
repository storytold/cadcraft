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

/// The measurement a dimension shows (for `<>` in its formula): the measured value with
/// DIMLFAC, DIMRND and the shown resolution applied (degrees for an angle), taken from the
/// dimension itself, not read back from its formatted text (a DIMPOST prefix, architectural
/// units or a decimal comma would give the wrong number).
pub fn measured(d: &Drawing, dm: &cadcraft_doc::Dimension) -> Option<f64> {
    let plain = cadcraft_doc::Dimension {
        text: String::new(),
        block: None,
        marks: cadcraft_doc::DimMarks { formula: None, ..dm.marks.clone() },
        ..dm.clone()
    };
    cadcraft_render::dimension_in(d, &plain).formula_value.filter(|v| v.is_finite())
}

/// A text object's formula, worked out into what it shows: a value that is a formula (one line
/// starting with `=`) becomes its formula; a formula's result becomes the value. `edited` is a
/// value just typed: typing over an AutoCAD field turns it into plain text, typing a value that
/// is not a formula drops the formula. Text, attribute definitions, multiline text,
/// multileader text and dimension text (whose `<>` is its measurement, `env.measured`).
pub fn apply(kind: &mut EntityKind, env: &Env, edited: bool) {
    // Multiline text keeps a result's `\`, `{` and `}` as letters, not formatting codes.
    let (value, formula, mtext): (&mut String, &mut Option<String>, bool) = match kind {
        EntityKind::Text(t) => (&mut t.value, &mut t.formula, false),
        EntityKind::AttDef(a) => (&mut a.text.value, &mut a.text.formula, false),
        EntityKind::MText(t) => (&mut t.contents, &mut t.formula, true),
        EntityKind::MLeader(m) => match &mut m.text {
            Some(t) => (&mut t.contents, &mut t.formula, true),
            None => return,
        },
        EntityKind::Dimension(d) => (&mut d.text, &mut d.marks.formula, false),
        _ => return,
    };
    if edited {
        // Multiline text is read without its formatting codes; one-line text and dimension text
        // as typed (their braces and backslashes are letters).
        let plain = if mtext { cadcraft_fonts::plain_mtext(value) } else { value.clone() };
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
        // A formula's result is plain text: in multiline text its `\`, `{` and `}` are letters.
        // An AutoCAD field's text is the multiline text around the fields, codes included.
        *value = if mtext && is_formula(f) { cadcraft_doc::mtext_literal(&v) } else { v };
    }
}

/// Whether a drawing has any formula or field to work out (UPDATEFIELD on a drawing without
/// one changes nothing, not even its saved state).
pub fn has_any(d: &Drawing) -> bool {
    d.model
        .iter()
        .chain(d.layouts.iter().flat_map(|l| l.entities.iter()))
        .chain(d.blocks.values().flat_map(|b| b.entities.iter()))
        .any(|e| has_formula(&e.kind))
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

/// A dimension's text, to see whether working out its formula changed it.
fn dim_text(k: &EntityKind) -> Option<&str> {
    match k {
        EntityKind::Dimension(d) => Some(d.text.as_str()),
        _ => None,
    }
}

/// Work out every formula and known field of a drawing again (when it is opened): model space,
/// layouts and block definitions. The number of texts worked out.
pub fn refresh(d: &mut Drawing, env: &Env) -> usize {
    let mut n = 0;
    // Dimensions' measurements first (they need the whole drawing).
    let mut measures: std::collections::HashMap<cadcraft_doc::Handle, Option<f64>> = std::collections::HashMap::new();
    // Dimensions in block definitions too: their `<>` is their own measurement.
    for e in d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())).chain(d.blocks.values().flat_map(|b| b.entities.iter())) {
        if let EntityKind::Dimension(dm) = &e.kind
            && dm.marks.formula.is_some()
        {
            measures.insert(e.handle, measured(d, dm));
        }
    }
    // Each space with its own sheet name (SHEET()): model space is "Model", a layout its name;
    // block definitions keep the name given.
    let fix = |store: &mut cadcraft_doc::EntityStore, sheet: Option<&str>, n: &mut usize| {
        let hs: Vec<cadcraft_doc::Handle> = store.iter().filter(|e| has_formula(&e.kind)).map(|e| e.handle).collect();
        for h in hs {
            let mut env = Env { measured: measures.get(&h).copied().flatten(), ..env.clone() };
            if let Some(sheet) = sheet {
                env.sheet = sheet.to_string();
            }
            store.modify(h, |e| {
                let before = dim_text(&e.kind).map(str::to_string);
                apply(&mut e.kind, &env, false);
                // A dimension whose text changed draws itself again (not its old block).
                if before.as_deref() != dim_text(&e.kind)
                    && let EntityKind::Dimension(dm) = &mut e.kind
                {
                    dm.block = None;
                }
            });
            *n += 1;
        }
    };
    fix(&mut d.model, Some("Model"), &mut n);
    for l in &mut d.layouts {
        let name = l.name.clone();
        fix(&mut l.entities, Some(&name), &mut n);
    }
    let names: Vec<String> = d.blocks.iter().filter(|(_, b)| b.entities.iter().any(|e| has_formula(&e.kind))).map(|(k, _)| k.clone()).collect();
    for k in names {
        if let Some(b) = d.blocks.get_mut(&k) {
            fix(&mut std::sync::Arc::make_mut(b).entities, None, &mut n);
        }
    }
    n
}
