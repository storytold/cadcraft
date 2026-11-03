//! 3D mouse button bindings: the model, its validation and the pure "which bindings fire" step.
//!
//! A binding maps a device button (numbered from 1, as the Buttons section of the 3D Mouse
//! dialog shows them) to a command id and optional JSON parameters, run through `app.run`. The
//! wire form, used by the saved preferences and the `ui.spacemouse` control method, is
//!
//! ```json
//! "buttons": {"1": {"command": "zoom", "params": {"mode": "extents"}}, "2": {"command": "ui.spacemouse.calibrate"}}
//! ```
//!
//! The table is complete: a button that is missing (or `null`) has no binding. Everything here is
//! host-independent; the desktop host only reads the device and calls [`fired`].

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// Most buttons a device report can carry (see `motion::decode` in the desktop app).
pub const MAX_BUTTONS: u8 = 32;
/// Longest command id accepted in a binding.
const MAX_COMMAND_LEN: usize = 64;
/// Bytes of JSON a binding's parameters may take (counted without serialising them).
const MAX_PARAMS_BYTES: usize = 4096;
/// Deepest nesting a binding's parameters may have.
const MAX_PARAMS_DEPTH: usize = 8;

/// The UI command that recalibrates the device's resting position.
pub const CALIBRATE: &str = "ui.spacemouse.calibrate";

/// What one button runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binding {
    /// A command id from the engine registry or `UI_COMMANDS`.
    pub command: String,
    /// A JSON object passed as the command's parameters; absent runs it with `{}`.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

impl Binding {
    pub fn new(command: &str) -> Self {
        Self { command: command.into(), params: Value::Null }
    }

    pub fn with_params(command: &str, params: Value) -> Self {
        Self { command: command.into(), params }
    }

    /// The parameters to run with: the object, or `{}` when there are none.
    pub fn run_params(&self) -> Value {
        if self.params.is_null() { Value::Object(serde_json::Map::new()) } else { self.params.clone() }
    }
}

/// Button number (1-based) to what it runs. Buttons not in the map do nothing.
pub type Buttons = BTreeMap<u8, Binding>;

/// What the buttons did before they were configurable: button 1 zooms to the extents, button 2
/// recalibrates the device.
pub fn default_buttons() -> Buttons {
    Buttons::from([(1, Binding::with_params("zoom", serde_json::json!({"mode": "extents"}))), (2, Binding::new(CALIBRATE))])
}

/// Deserialises `{"1": {..}, "3": null}`; a `null` entry is the same as a missing one.
pub fn deserialize_buttons<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Buttons, D::Error> {
    let raw = BTreeMap::<u8, Option<Binding>>::deserialize(deserializer)?;
    Ok(raw.into_iter().filter_map(|(index, binding)| binding.map(|b| (index, b))).collect())
}

/// Every command a button can be bound to: `(id, English label)`, sorted by label then id.
pub fn bindable_commands() -> &'static [(&'static str, &'static str)] {
    static LIST: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
    LIST.get_or_init(|| {
        let mut list: Vec<_> = cadcraft_engine::command_specs()
            .iter()
            .map(|c| (c.id, c.label))
            .chain(crate::menus::UI_COMMANDS.iter().map(|c| (c.0, c.1)))
            .filter(|(id, _)| is_bindable(id))
            .collect();
        list.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()).then(a.0.cmp(b.0)));
        list.dedup_by_key(|c| c.0);
        list
    })
}

/// Whether `id` (already lower case) names a command a button may run. The navigate command and
/// the settings method are refused so that a button can never change or drive the 3D mouse
/// configuration itself; calibrate is the one 3D mouse command that makes sense on a button.
fn is_bindable(id: &str) -> bool {
    if id.starts_with("ui.spacemouse") {
        return id == CALIBRATE;
    }
    // Quitting from a stray button press is never wanted; `ui.noop` does nothing and has no label.
    if matches!(id, "ui.quit" | "ui.noop") || id.starts_with("ui.recent.") {
        return false;
    }
    cadcraft_engine::find_command(id).is_some() || crate::menus::is_ui_command(id)
}

/// Checks the nesting depth and size of `value` without recursing past the caps or serialising it.
fn within_limits(value: &Value, depth: usize, budget: &mut usize) -> bool {
    let cost = |budget: &mut usize, n: usize| match budget.checked_sub(n) {
        Some(left) => {
            *budget = left;
            true
        }
        None => false,
    };
    if depth > MAX_PARAMS_DEPTH || !cost(budget, 1) {
        return false;
    }
    match value {
        Value::String(s) => cost(budget, s.len()),
        Value::Number(_) => cost(budget, 8),
        Value::Array(items) => items.iter().all(|item| within_limits(item, depth + 1, budget)),
        Value::Object(map) => map.iter().all(|(key, item)| cost(budget, key.len()) && within_limits(item, depth + 1, budget)),
        Value::Null | Value::Bool(_) => true,
    }
}

/// Normalises (lower-cases the id) and checks one binding for button `index`.
pub fn validate_binding(index: u8, binding: &mut Binding) -> Result<(), String> {
    if !(1..=MAX_BUTTONS).contains(&index) {
        return Err(format!("3D mouse button {index} is out of range (1 to {MAX_BUTTONS})"));
    }
    let id = binding.command.trim().to_ascii_lowercase();
    if id.is_empty() || id.len() > MAX_COMMAND_LEN {
        return Err(format!("3D mouse button {index}: the command must be 1 to {MAX_COMMAND_LEN} characters"));
    }
    if !is_bindable(&id) {
        let shown: String = id.chars().take(40).collect();
        return Err(format!("3D mouse button {index}: \"{shown}\" is not a command a button can run"));
    }
    binding.command = id;
    if !binding.params.is_null() {
        if !binding.params.is_object() {
            return Err(format!("3D mouse button {index}: params must be a JSON object"));
        }
        let mut budget = MAX_PARAMS_BYTES;
        if !within_limits(&binding.params, 0, &mut budget) {
            return Err(format!(
                "3D mouse button {index}: params are too large or nested too deeply ({MAX_PARAMS_BYTES} bytes, {MAX_PARAMS_DEPTH} levels)"
            ));
        }
    }
    Ok(())
}

/// Validates and normalises every binding of a table.
pub fn validate_buttons(buttons: &mut Buttons) -> Result<(), String> {
    for (index, binding) in buttons.iter_mut() {
        validate_binding(*index, binding)?;
    }
    Ok(())
}

/// The bindings to run for the buttons in `pressed` (bit 0 is button 1), in button order.
///
/// `allowed` is the host's "may navigate and run commands now" guard (focused window, no dialog
/// open, ...). Recalibrating only sets a request flag for the device thread, so, as before
/// buttons were configurable, it also works while the guard is closed.
pub fn fired(buttons: &Buttons, pressed: u32, allowed: bool) -> Vec<&Binding> {
    (1..=MAX_BUTTONS)
        .filter(|index| pressed & (1u32 << (index - 1)) != 0)
        .filter_map(|index| buttons.get(&index))
        .filter(|binding| allowed || binding.command == CALIBRATE)
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn defaults_are_the_old_hard_coded_actions() {
        let defaults = default_buttons();
        assert_eq!(defaults.len(), 2);
        assert_eq!(defaults.get(&1), Some(&Binding::with_params("zoom", json!({"mode": "extents"}))));
        assert_eq!(defaults.get(&2).map(|b| b.command.as_str()), Some(CALIBRATE));
        let mut checked = defaults.clone();
        assert_eq!(validate_buttons(&mut checked), Ok(()));
        assert_eq!(checked, defaults, "defaults survive validation unchanged");
        // The old behaviour: button 1 zooms to the extents, button 2 recalibrates.
        let first: Vec<_> = fired(&defaults, 0b01, true).iter().map(|b| (b.command.as_str(), b.run_params())).collect();
        assert_eq!(first, [("zoom", json!({"mode": "extents"}))]);
        let second: Vec<_> = fired(&defaults, 0b10, true).iter().map(|b| b.command.as_str()).collect();
        assert_eq!(second, [CALIBRATE]);
    }

    #[test]
    fn fired_selects_pressed_bound_buttons_in_order() {
        let mut buttons = default_buttons();
        buttons.insert(3, Binding::new("undo"));
        buttons.insert(32, Binding::new("redo"));
        let commands = |pressed, allowed| fired(&buttons, pressed, allowed).iter().map(|b| b.command.clone()).collect::<Vec<_>>();
        assert_eq!(commands(0, true), Vec::<String>::new());
        assert_eq!(commands(0b100, true), ["undo"]);
        assert_eq!(commands(1 << 31 | 0b101, true), ["zoom", "undo", "redo"]);
        assert_eq!(commands(0b1000, true), Vec::<String>::new(), "button 4 is unbound");
        assert_eq!(commands(u32::MAX, true).len(), 4);
        // With the guard closed only recalibration still runs.
        assert_eq!(commands(0b111, false), [CALIBRATE]);
        assert!(fired(&Buttons::new(), u32::MAX, true).is_empty());
    }

    #[test]
    fn validation_accepts_known_commands_and_normalises_the_id() {
        let mut ok = Binding::with_params(" ZOOM.Extents ", json!({}));
        assert_eq!(validate_binding(5, &mut ok), Ok(()));
        assert_eq!(ok.command, "zoom.extents");
        for id in ["undo", "ui.toggle.cmdline", CALIBRATE] {
            assert_eq!(validate_binding(1, &mut Binding::new(id)), Ok(()), "{id}");
        }
    }

    #[test]
    fn validation_rejects_unknown_and_self_referencing_commands() {
        for id in [
            "",
            "no.such.command",
            "ui.spacemouse.navigate",
            "ui.spacemouse",
            "ui.spacemouse.future",
            "UI.SpaceMouse.Navigate",
            "ui.quit",
            "ui.noop",
            "ui.recent.0",
            &"z".repeat(MAX_COMMAND_LEN + 1),
        ] {
            let error = validate_binding(1, &mut Binding::new(id)).expect_err(id);
            assert!(error.starts_with("3D mouse button 1"), "{error}");
        }
        assert!(validate_binding(0, &mut Binding::new("undo")).is_err());
        assert!(validate_binding(MAX_BUTTONS + 1, &mut Binding::new("undo")).is_err());
        assert!(validate_binding(MAX_BUTTONS, &mut Binding::new("undo")).is_ok());
    }

    #[test]
    fn validation_rejects_bad_params() {
        for params in [json!([1]), json!("zoom"), json!(3), json!(true)] {
            let error = validate_binding(1, &mut Binding::with_params("zoom", params)).expect_err("not an object");
            assert!(error.contains("JSON object"), "{error}");
        }
        let big = json!({"text": "x".repeat(MAX_PARAMS_BYTES)});
        assert!(validate_binding(1, &mut Binding::with_params("zoom", big)).is_err(), "oversized");
        let many: Vec<Value> = (0..MAX_PARAMS_BYTES).map(|n| json!(n)).collect();
        assert!(validate_binding(1, &mut Binding::with_params("zoom", json!({"v": many}))).is_err(), "many small items");
        let mut deep = json!(1);
        for _ in 0..MAX_PARAMS_DEPTH + 1 {
            deep = json!({"a": deep});
        }
        assert!(validate_binding(1, &mut Binding::with_params("zoom", deep)).is_err(), "too deep");
        let mut shallow = json!(1);
        for _ in 0..MAX_PARAMS_DEPTH - 1 {
            shallow = json!({"a": shallow});
        }
        assert!(validate_binding(1, &mut Binding::with_params("zoom", shallow)).is_ok());
    }

    #[test]
    fn hostile_nesting_is_bounded() {
        // As deep as serde_json itself parses: the walk stops at the depth cap, not at the bottom.
        let nested: Value = serde_json::from_str(&format!("{}{}", "[".repeat(120), "]".repeat(120))).expect("within serde_json's own depth limit");
        let mut budget = MAX_PARAMS_BYTES;
        assert!(!within_limits(&nested, 0, &mut budget));
        // Anything deeper never reaches a binding: parsing the request already fails.
        assert!(serde_json::from_str::<Value>(&"[".repeat(100_000)).is_err());
    }

    #[test]
    fn bindable_list_is_sorted_unique_and_free_of_self_references() {
        let list = bindable_commands();
        assert!(list.len() > 50);
        assert!(list.iter().any(|c| c.0 == "zoom.extents") && list.iter().any(|c| c.0 == CALIBRATE));
        assert!(!list.iter().any(|c| c.0 == "ui.spacemouse" || c.0 == "ui.spacemouse.navigate" || c.0 == "ui.quit"));
        let mut ids: Vec<_> = list.iter().map(|c| c.0).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), list.len());
        for (id, _) in list {
            assert!(validate_binding(1, &mut Binding::new(id)).is_ok(), "{id}");
        }
    }

    #[test]
    fn wire_form_treats_null_as_unbound_and_omits_empty_params() {
        let buttons: Buttons =
            deserialize_buttons(&json!({"1": {"command": "undo"}, "2": null, "3": {"command": "zoom", "params": {"mode": "in"}}})).expect("valid");
        assert_eq!(buttons.keys().copied().collect::<Vec<_>>(), [1, 3]);
        assert_eq!(
            serde_json::to_value(&buttons).expect("serialises"),
            json!({"1": {"command": "undo"}, "3": {"command": "zoom", "params": {"mode": "in"}}})
        );
        assert!(deserialize_buttons(&json!({"x": {"command": "undo"}})).is_err(), "non-numeric button");
        assert!(deserialize_buttons(&json!({"1": {"command": "undo", "extra": 1}})).is_err(), "unknown field");
        assert!(deserialize_buttons(&json!({"1": "undo"})).is_err());
        assert!(deserialize_buttons(&json!([1, 2])).is_err());
    }
}
