//! The Purge dialog (PURGE or PU typed): the unused named objects PURGE would remove, by kind,
//! each with a check box, and the drawing's zero-length geometry and empty text. Purge Checked
//! Items and Purge All run `purge` with exact names, as one undo step; -PURGE keeps AutoCAD's
//! command-line prompts. The list comes from `purge.list` and is read again when the drawing
//! changes.

use std::collections::BTreeSet;
use std::sync::Arc;

use egui::RichText;
use serde_json::{Value, json};

use super::{no_drawing, set_temp, temp};
use crate::CadApp;
use crate::theme::Tokens;

/// The kinds the dialog lists, in the order PURGE frees them (blocks free layers, layers free
/// linetypes): their `purge.list` key, which is also PURGE's `type`, and their title.
const KINDS: &[(&str, &str)] = &[
    ("blocks", "Blocks"),
    ("groups", "Groups"),
    ("dimstyles", "Dimension styles"),
    ("mleaderstyles", "Multileader styles"),
    ("mlinestyles", "Multiline styles"),
    ("tablestyles", "Table styles"),
    ("layers", "Layers"),
    ("linetypes", "Linetypes"),
    ("textstyles", "Text styles"),
];

/// What `purge.list` reported.
#[derive(Clone, Debug, Default, PartialEq)]
struct Listing {
    /// Per kind of [`KINDS`], in that order: the names PURGE would remove.
    named: Vec<(&'static str, Vec<String>)>,
    zero_length: u64,
    empty_text: u64,
}

impl Listing {
    fn read(v: &Value) -> Listing {
        let names = |k: &str| v["named"][k].as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
        Listing {
            named: KINDS.iter().map(|(k, _)| (*k, names(k))).collect(),
            zero_length: v["zeroLength"].as_u64().unwrap_or(0),
            empty_text: v["emptyText"].as_u64().unwrap_or(0),
        }
    }
    fn named_count(&self) -> usize {
        self.named.iter().map(|(_, n)| n.len()).sum()
    }
    fn unnamed_count(&self) -> u64 {
        self.zero_length.saturating_add(self.empty_text)
    }
}

/// The drawing a listing was read from: which one ([`cadcraft_engine::DocState::uid`]), and its
/// address and revision, which change with every edit.
type Key = (u64, usize, u64);

/// What the dialog keeps between frames.
#[derive(Clone, Default)]
struct State {
    listing: Option<(Key, Listing)>,
    /// Checked items of that drawing: (kind, name).
    checked: BTreeSet<(String, String)>,
    /// Purge zero-length geometry and empty text objects too.
    unnamed: bool,
}

/// The `purge` call for the checked items: each kind with its exact names, and zero-length
/// geometry and empty text when `unnamed` asks. `None` when there is nothing to purge.
fn checked_call(listing: &Listing, checked: &BTreeSet<(String, String)>, unnamed: bool) -> Option<Value> {
    let names: serde_json::Map<String, Value> = listing
        .named
        .iter()
        .filter_map(|(kind, names)| {
            let picked: Vec<&String> = names.iter().filter(|n| checked.contains(&((*kind).to_string(), (*n).clone()))).collect();
            (!picked.is_empty()).then(|| ((*kind).to_string(), json!(picked)))
        })
        .collect();
    let unnamed = unnamed && listing.unnamed_count() > 0;
    (!names.is_empty() || unnamed).then(|| json!({ "type": "all", "names": names, "unnamed": unnamed }))
}

/// Purge All: every unused named object, and the unnamed objects when `unnamed` asks.
fn all_call(listing: &Listing, unnamed: bool) -> Value {
    json!({ "type": "all", "unnamed": unnamed && listing.unnamed_count() > 0 })
}

/// Run `call`, one command and so one undo step whatever UNDO Control says, and say how many
/// objects went.
fn run_purge(app: &mut CadApp, call: &Value) {
    let gone = match app.session.execute("purge", call) {
        Ok(r) => r["purged"].as_object().map_or(0, |m| m.values().filter_map(Value::as_array).map(Vec::len).sum()),
        Err(e) => {
            app.session.echo(e.to_string());
            return;
        }
    };
    app.session.echo(match gone {
        0 => "Nothing was purged.".to_string(),
        1 => "1 unused object purged.".to_string(),
        n => format!("{n} unused objects purged."),
    });
}

pub(super) fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) -> super::Action {
    let t = Tokens::get();
    let Ok(st) = app.session.state() else {
        no_drawing(ctx, "Purge", open);
        return None;
    };
    let key: Key = (st.uid, Arc::as_ptr(&st.doc) as usize, st.revision);
    let mut state = temp::<State>(ctx, "purge_state").unwrap_or_default();
    if state.listing.as_ref().is_none_or(|(k, _)| *k != key) {
        // Another drawing (a tab switch): nothing in it was checked, whatever the names.
        if state.listing.as_ref().is_some_and(|(k, _)| k.0 != key.0) {
            state.checked.clear();
            state.unnamed = false;
        }
        let listing = app.session.execute("purge.list", &json!({})).map(|v| Listing::read(&v)).unwrap_or_default();
        // Checked items that are no longer unused drop out.
        state.checked.retain(|(k, n)| listing.named.iter().any(|(kind, names)| kind == k && names.contains(n)));
        state.listing = Some((key, listing));
    }
    let listing = state.listing.as_ref().map(|(_, l)| l.clone()).unwrap_or_default();
    let mut call: Option<Value> = None;
    let mut close = false;
    egui::Window::new("Purge").open(open).default_width(380.0).resizable(true).show(ctx, |ui| {
        ui.label(RichText::new("Named items not used").strong());
        egui::ScrollArea::vertical().id_salt("purge_items").max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
            if listing.named_count() == 0 {
                ui.label(RichText::new("There are no unused named items.").color(t.text_dim));
            }
            for ((kind, names), (_, title)) in listing.named.iter().zip(KINDS).filter(|((_, names), _)| !names.is_empty()) {
                let id = ui.make_persistent_id(("purge_kind", *kind));
                egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
                    .show_header(ui, |ui| {
                        let is_checked = |n: &String| state.checked.contains(&((*kind).to_string(), n.clone()));
                        let mut all = names.iter().all(is_checked);
                        if ui.checkbox(&mut all, format!("{title} ({})", names.len())).changed() {
                            for n in names {
                                let item = ((*kind).to_string(), n.clone());
                                if all {
                                    state.checked.insert(item);
                                } else {
                                    state.checked.remove(&item);
                                }
                            }
                        }
                    })
                    .body(|ui| {
                        for n in names {
                            let item = ((*kind).to_string(), n.clone());
                            let mut on = state.checked.contains(&item);
                            if ui.checkbox(&mut on, n.as_str()).changed() {
                                if on {
                                    state.checked.insert(item);
                                } else {
                                    state.checked.remove(&item);
                                }
                            }
                        }
                    });
            }
        });
        ui.separator();
        ui.label(RichText::new("Unnamed objects").strong());
        ui.add_enabled_ui(listing.unnamed_count() > 0, |ui| {
            let label = format!("Purge zero-length geometry ({}) and empty text objects ({})", listing.zero_length, listing.empty_text);
            ui.checkbox(&mut state.unnamed, label);
        });
        ui.separator();
        ui.horizontal(|ui| {
            let checked = !state.checked.is_empty() || (state.unnamed && listing.unnamed_count() > 0);
            if ui.add_enabled(checked, egui::Button::new("Purge Checked Items")).clicked() {
                call = checked_call(&listing, &state.checked, state.unnamed);
            }
            let any = listing.named_count() > 0 || (state.unnamed && listing.unnamed_count() > 0);
            if ui.add_enabled(any, egui::Button::new("Purge All")).on_hover_text("Every unused named item, and what it frees in turn").clicked() {
                call = Some(all_call(&listing, state.unnamed));
            }
            if ui.button("Close").clicked() {
                close = true;
            }
        });
    });
    if call.is_some() {
        state.checked.clear();
    }
    set_temp(ctx, "purge_state", state);
    if let Some(call) = call {
        run_purge(app, &call);
    }
    if close {
        *open = false;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_engine::Session;

    /// Unused: blocks U1 and U2, layers Spare and A#1, linetype DASHED; a zero-length line.
    fn session() -> Session {
        let mut s = Session::new();
        for name in ["U1", "U2"] {
            s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap();
            s.execute("selectall", &json!({})).unwrap();
            s.execute("block", &json!({ "name": name, "base": [0, 0], "keep": "delete" })).unwrap();
        }
        for name in ["Spare", "A#1", "A51"] {
            s.execute("layer.new", &json!({ "name": name })).unwrap();
        }
        s.execute("linetype", &json!({ "load": "DASHED" })).unwrap();
        let zero = cadcraft_doc::EntityKind::Line(cadcraft_doc::Line { a: cadcraft_geom::Vec3::ZERO, b: cadcraft_geom::Vec3::ZERO });
        s.doc_mut().unwrap().add(&cadcraft_doc::Space::Model, cadcraft_doc::Common::default(), zero).unwrap();
        s
    }

    fn listing(app: &mut CadApp) -> Listing {
        Listing::read(&app.session.execute("purge.list", &json!({})).unwrap())
    }

    fn item(kind: &str, name: &str) -> (String, String) {
        (kind.to_string(), name.to_string())
    }

    #[test]
    fn typed_purge_opens_the_dialog_and_dash_purge_prompts() {
        let mut app = CadApp::new(session(), crate::Services::default());
        for id in ["purge", "pu"] {
            app.ui.dialog = None;
            assert!(super::super::route(&mut app, id, &Value::Null).is_some(), "{id}");
            assert_eq!(app.ui.dialog.as_deref(), Some("purge"), "{id}");
        }
        app.ui.dialog = None;
        assert!(super::super::route(&mut app, "-purge", &Value::Null).is_none(), "-PURGE asks at the command line");
        assert!(super::super::route(&mut app, "purge", &json!({ "type": "layers" })).is_none(), "JSON calls run the command");
    }

    #[test]
    fn checked_items_purge_by_exact_name_as_one_undo_step() {
        let mut app = CadApp::new(session(), crate::Services::default());
        let l = listing(&mut app);
        assert_eq!(l.named.iter().find(|(k, _)| *k == "layers").map(|(_, n)| n.clone()), Some(vec!["Spare".into(), "A#1".into(), "A51".into()]));
        assert_eq!((l.zero_length, l.empty_text), (1, 0));
        let checked: BTreeSet<_> = [item("layers", "A#1"), item("blocks", "U2")].into();
        let call = checked_call(&l, &checked, true);
        assert_eq!(call, Some(json!({ "type": "all", "names": { "blocks": ["U2"], "layers": ["A#1"] }, "unnamed": true })));
        // One command, so one undo step whatever UNDO Control says: with Control One only the
        // last command can be undone.
        for control in ["all", "one"] {
            let mut app = CadApp::new(session(), crate::Services::default());
            app.session.execute("undo", &json!({ "mode": "control", "control": control })).unwrap();
            run_purge(&mut app, call.as_ref().unwrap());
            let d = app.session.doc().unwrap();
            // `#` is a wildcard in a pattern; as an exact name A51 stays.
            assert!(d.layer("A#1").is_none() && d.layer("A51").is_some() && d.layer("Spare").is_some(), "{control}");
            assert!(d.blocks.contains_key("U1") && !d.blocks.contains_key("U2"), "{control}");
            assert!(app.session.log.iter().any(|l| l == "3 unused objects purged."), "{control}: {:?}", app.session.log);
            assert_eq!(listing(&mut app).zero_length, 0);
            // One undo brings all three back.
            app.session.execute("undo", &json!({})).unwrap();
            let d = app.session.doc().unwrap();
            assert!(d.layer("A#1").is_some() && d.blocks.contains_key("U2"), "{control}");
            assert_eq!(listing(&mut app).zero_length, 1, "{control}");
        }
    }

    #[test]
    fn purge_all_leaves_unnamed_objects_unless_asked() {
        let mut app = CadApp::new(session(), crate::Services::default());
        let l = listing(&mut app);
        assert_eq!(all_call(&l, false), json!({ "type": "all", "unnamed": false }));
        run_purge(&mut app, &all_call(&l, false));
        let after = listing(&mut app);
        assert_eq!(after.named_count(), 0, "{after:?}");
        assert_eq!(after.zero_length, 1, "the zero-length line stays");
        run_purge(&mut app, &all_call(&after, true));
        assert_eq!(listing(&mut app).zero_length, 0);
        // Nothing left and nothing checked: nothing to run.
        assert_eq!(checked_call(&listing(&mut app), &BTreeSet::new(), true), None);
    }

    /// Checked items belong to the drawing they were checked in: another drawing with an unused
    /// item of the same name doesn't get it purged.
    #[test]
    fn switching_drawings_clears_the_ticks() {
        let mut app = CadApp::new(session(), crate::Services::default());
        let second = app.session.new_drawing(false);
        app.session.execute("layer.new", &json!({ "name": "Spare" })).unwrap();
        app.session.active = 0;
        let ctx = egui::Context::default();
        let frame = |app: &mut CadApp| {
            let mut open = true;
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                assert!(dialog(app, ui.ctx(), &mut open).is_none());
            });
            out.textures_delta.clear();
        };
        frame(&mut app);
        let mut s = temp::<State>(&ctx, "purge_state").unwrap_or_default();
        s.checked.insert(item("layers", "Spare"));
        s.unnamed = true;
        set_temp(&ctx, "purge_state", s);
        frame(&mut app);
        let ticks = || temp::<State>(&ctx, "purge_state").map(|s| (s.checked.len(), s.unnamed));
        assert_eq!(ticks(), Some((1, true)), "an edit-free frame keeps them");
        app.session.active = second;
        frame(&mut app);
        assert_eq!(ticks(), Some((0, false)), "the other drawing's Spare and unnamed objects aren't checked");
    }

    #[test]
    fn the_dialog_draws_and_follows_the_drawing() {
        let mut app = CadApp::new(session(), crate::Services::default());
        let ctx = egui::Context::default();
        let frame = |app: &mut CadApp| {
            let mut open = true;
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                assert!(dialog(app, ui.ctx(), &mut open).is_none());
            });
            out.textures_delta.clear();
            assert!(open);
        };
        frame(&mut app);
        let state = || temp::<State>(&ctx, "purge_state").unwrap_or_default();
        let named = |s: &State| s.listing.as_ref().map_or(0, |(_, l)| l.named_count());
        assert_eq!(named(&state()), 6, "U1, U2, Spare, A#1, A51, DASHED");
        // A checked item that a command then uses drops out with the next listing.
        let mut s = state();
        s.checked.insert(item("layers", "Spare"));
        set_temp(&ctx, "purge_state", s);
        app.session.execute("layer.current", &json!({ "name": "Spare" })).unwrap();
        frame(&mut app);
        assert_eq!(named(&state()), 5);
        assert!(state().checked.is_empty());
        // No drawing: the dialog says so.
        app.session.execute("closeall", &json!({})).unwrap();
        frame(&mut app);
    }
}
