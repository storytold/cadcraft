//! Named profiles (Options ▸ Profiles): sets of preferences to switch between, as AutoCAD keeps
//! user profiles. The current profile's settings are the live ones; the others are kept as the
//! OPTIONS JSON of their settings and saved with [`CadApp::profile_json`]. A profile holds every
//! preference OPTIONS reports except the interface language.
//!
//! `options.profile` is their JSON form (the Profiles tab uses it too):
//!
//! | `action` | parameters | does |
//! |---|---|---|
//! | `list` (default) | | `{current, profiles}` |
//! | `set` | `name` | make `name` current: the current one's settings are kept, `name`'s applied |
//! | `add` | `name` | a new profile with the current settings |
//! | `copy` | `name`, `newName` | a new profile with `name`'s settings |
//! | `rename` | `name`, `newName` | |
//! | `delete` | `name` | not the current profile |
//! | `reset` | `name?` (default the current) | back to the defaults |
//! | `export` | `name?` (default the current), `path?` | writes a profile file; without `path` returns its `text` |
//! | `import` | `path` or `text`, `name?` (default the file's, numbered if taken) | adds the file's profile |

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::CadApp;

/// The command id of the JSON form.
pub const COMMAND: &str = "options.profile";
/// The profile a first run starts with.
pub const UNNAMED: &str = "<<Unnamed Profile>>";
/// The most profiles kept.
pub const MAX_PROFILES: usize = 64;
/// The longest profile name.
const MAX_NAME: usize = 64;
/// The largest profile file `import` reads.
const MAX_FILE: u64 = 256 * 1024;
/// The most settings one saved profile holds.
const MAX_SETTINGS: usize = 512;
/// The `format` of a profile file.
const FORMAT: &str = "cadcraft-profile";
/// Preferences a profile leaves alone.
const NOT_IN_PROFILE: &[&str] = &["interfaceLanguage"];

/// The named profiles.
#[derive(Clone, Debug, PartialEq)]
pub struct Profiles {
    /// The current profile's name.
    pub current: String,
    /// The other profiles' settings (OPTIONS JSON), by name.
    saved: BTreeMap<String, Value>,
}

impl Default for Profiles {
    fn default() -> Self {
        Profiles { current: UNNAMED.into(), saved: BTreeMap::new() }
    }
}

/// A profile's name as the dialog shows it: the first-run profile's in the interface language.
pub fn display_name(name: &str) -> &str {
    if name == UNNAMED { crate::i18n::t(UNNAMED) } else { name }
}

impl Profiles {
    /// Every profile's name, the current one included, sorted.
    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.saved.keys().filter(|n| **n != self.current).cloned().collect();
        v.push(self.current.clone());
        v.sort_by_key(|n| n.to_lowercase());
        v
    }

    /// The profile called `name` (any case), as it is spelled.
    pub fn find(&self, name: &str) -> Option<String> {
        let n = name.trim();
        if self.current.eq_ignore_ascii_case(n) {
            return Some(self.current.clone());
        }
        self.saved.keys().find(|k| k.eq_ignore_ascii_case(n)).cloned()
    }

    pub(super) fn saved_json(&self) -> Value {
        Value::Object(self.saved.iter().filter(|(n, _)| **n != self.current).map(|(n, v)| (n.clone(), v.clone())).collect())
    }

    /// Read the names and profiles [`CadApp::profile_json`] saved; bad entries are skipped.
    pub(super) fn load(&mut self, v: &Value) {
        if let Some(n) = v.get("profileName").and_then(Value::as_str).and_then(|n| valid_name(n).ok()) {
            self.current = n;
        }
        if let Some(obj) = v.get("profiles").and_then(Value::as_object) {
            self.saved = obj
                .iter()
                .filter_map(|(n, s)| Some((valid_name(n).ok()?, settings_of(s)?)))
                .filter(|(n, _)| !n.eq_ignore_ascii_case(&self.current))
                .take(MAX_PROFILES - 1)
                .collect();
        }
    }
}

/// A profile name: trimmed, 1-64 characters, no control characters.
fn valid_name(n: &str) -> Result<String, String> {
    let n = n.trim();
    if n.is_empty() || n.chars().count() > MAX_NAME || n.chars().any(char::is_control) {
        return Err(format!("a profile name has 1 to {MAX_NAME} characters"));
    }
    Ok(n.to_string())
}

/// Saved settings: an object of at most [`MAX_SETTINGS`] entries.
fn settings_of(v: &Value) -> Option<Value> {
    let obj = v.as_object()?;
    Some(Value::Object(obj.iter().take(MAX_SETTINGS).map(|(k, v)| (k.clone(), v.clone())).collect()))
}

/// The current settings as a profile keeps them.
pub fn snapshot(app: &CadApp) -> Value {
    let mut m = match super::report(app) {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    m.retain(|k, _| !NOT_IN_PROFILE.contains(&k.as_str()));
    Value::Object(m)
}

/// Make `settings` the live settings: the defaults, then each setting that is still valid.
fn apply(app: &mut CadApp, settings: &Value) {
    let _ = super::run_json(app, &json!({ "reset": true }));
    let Some(obj) = settings.as_object() else { return };
    for (k, v) in obj.iter().take(MAX_SETTINGS) {
        if k == "reset" || NOT_IN_PROFILE.contains(&k.as_str()) {
            continue;
        }
        let mut one = Map::new();
        one.insert(k.clone(), v.clone());
        let _ = super::run_json(app, &Value::Object(one));
    }
}

fn list(app: &CadApp) -> Value {
    json!({ "current": app.profiles.current, "profiles": app.profiles.names() })
}

fn name_param(p: &Value, key: &str) -> Result<String, String> {
    valid_name(p.get(key).and_then(Value::as_str).ok_or_else(|| format!("options.profile: `{key}` is required"))?)
}

/// An existing profile named by `p[key]`.
fn existing(app: &CadApp, p: &Value, key: &str) -> Result<String, String> {
    let n = name_param(p, key)?;
    app.profiles.find(&n).ok_or_else(|| format!("options.profile: there is no profile \"{n}\""))
}

/// A name no profile has yet, and room for one more.
fn fresh(app: &CadApp, p: &Value, key: &str) -> Result<String, String> {
    let n = name_param(p, key)?;
    if app.profiles.find(&n).is_some() {
        return Err(format!("options.profile: a profile named \"{n}\" already exists"));
    }
    if app.profiles.names().len() >= MAX_PROFILES {
        return Err(format!("options.profile: at most {MAX_PROFILES} profiles"));
    }
    Ok(n)
}

/// The settings of the profile called `name` (the current one's are the live settings).
fn settings(app: &CadApp, name: &str) -> Value {
    if name == app.profiles.current { snapshot(app) } else { app.profiles.saved.get(name).cloned().unwrap_or_else(|| json!({})) }
}

/// A profile file's text.
fn file_text(name: &str, settings: &Value) -> String {
    serde_json::to_string_pretty(&json!({ "format": FORMAT, "version": 1, "name": name, "settings": settings })).unwrap_or_default()
}

fn read_file(path: &str) -> Result<String, String> {
    let len = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?.len();
    if len > MAX_FILE {
        return Err(format!("{path}: too large for a profile file"));
    }
    std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
}

/// The JSON form ([module docs](self)).
pub fn run(app: &mut CadApp, p: &Value) -> Result<Value, String> {
    let action = match p.get("action") {
        None | Some(Value::Null) => "list",
        Some(Value::String(a)) => a.as_str(),
        Some(_) => return Err("options.profile: `action` must be text".into()),
    };
    match action {
        "list" => {}
        "set" => {
            let name = existing(app, p, "name")?;
            if name != app.profiles.current {
                let mine = snapshot(app);
                let theirs = app.profiles.saved.remove(&name).unwrap_or_else(|| json!({}));
                let old = std::mem::replace(&mut app.profiles.current, name);
                app.profiles.saved.insert(old, mine);
                apply(app, &theirs);
            }
        }
        "add" => {
            let name = fresh(app, p, "name")?;
            let s = snapshot(app);
            app.profiles.saved.insert(name, s);
        }
        "copy" => {
            let from = existing(app, p, "name")?;
            let name = fresh(app, p, "newName")?;
            let s = settings(app, &from);
            app.profiles.saved.insert(name, s);
        }
        "rename" => {
            let from = existing(app, p, "name")?;
            let to = name_param(p, "newName")?;
            if app.profiles.find(&to).is_some_and(|n| n != from) {
                return Err(format!("options.profile: a profile named \"{to}\" already exists"));
            }
            if from == app.profiles.current {
                app.profiles.current = to;
            } else if let Some(s) = app.profiles.saved.remove(&from) {
                app.profiles.saved.insert(to, s);
            }
        }
        "delete" => {
            let name = existing(app, p, "name")?;
            if name == app.profiles.current {
                return Err("options.profile: the current profile can't be deleted; make another one current first".into());
            }
            app.profiles.saved.remove(&name);
        }
        "reset" => {
            let name = match p.get("name") {
                Some(_) => existing(app, p, "name")?,
                None => app.profiles.current.clone(),
            };
            if name == app.profiles.current {
                super::run_json(app, &json!({ "reset": true }))?;
            } else {
                app.profiles.saved.insert(name, json!({}));
            }
        }
        "export" => {
            let name = match p.get("name") {
                Some(_) => existing(app, p, "name")?,
                None => app.profiles.current.clone(),
            };
            let text = file_text(&name, &settings(app, &name));
            match p.get("path") {
                None | Some(Value::Null) => return Ok(json!({ "name": name, "text": text })),
                Some(path) => {
                    let path = path.as_str().map(str::trim).filter(|p| !p.is_empty()).ok_or("options.profile: `path` must be a file path")?;
                    std::fs::write(path, text).map_err(|e| format!("options.profile: {path}: {e}"))?;
                    return Ok(json!({ "name": name, "path": path }));
                }
            }
        }
        "import" => {
            let text = match (p.get("text").and_then(Value::as_str), p.get("path").and_then(Value::as_str)) {
                (Some(t), _) if t.len() as u64 <= MAX_FILE => t.to_string(),
                (Some(_), _) => return Err("options.profile: `text` is too large for a profile".into()),
                (None, Some(path)) => read_file(path.trim()).map_err(|e| format!("options.profile: {e}"))?,
                (None, None) => return Err("options.profile: `path` or `text` is required".into()),
            };
            let v: Value = serde_json::from_str(&text).map_err(|e| format!("options.profile: not a profile file ({e})"))?;
            if v.get("format").and_then(Value::as_str) != Some(FORMAT) {
                return Err("options.profile: not a CADCraft profile file".into());
            }
            let s = v.get("settings").and_then(settings_of).ok_or("options.profile: the profile file has no settings")?;
            let name = match p.get("name") {
                Some(_) => fresh(app, p, "name")?,
                // The file's own name; one a profile already has gets a number.
                None => {
                    let base = v.get("name").and_then(Value::as_str).and_then(|n| valid_name(n).ok()).unwrap_or_else(|| "Imported".into());
                    let base: String = base.chars().take(MAX_NAME - 5).collect();
                    let free = (1..=MAX_PROFILES + 1)
                        .map(|i| if i == 1 { base.clone() } else { format!("{base} ({i})") })
                        .find(|n| app.profiles.find(n).is_none())
                        .ok_or("options.profile: no free profile name")?;
                    fresh(app, &json!({ "name": free }), "name")?
                }
            };
            app.profiles.saved.insert(name.clone(), s);
            let mut out = list(app);
            out["name"] = json!(name);
            return Ok(out);
        }
        other => return Err(format!("options.profile: unknown action `{other}` (list, set, add, copy, rename, delete, reset, export, import)")),
    }
    Ok(list(app))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked() {
        assert!(valid_name("  Office  ").is_ok_and(|n| n == "Office"));
        for bad in ["", "   ", "a\nb", &"x".repeat(65)] {
            assert!(valid_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn hostile_saved_profiles_are_capped() {
        let mut p = Profiles::default();
        let many: Map<String, Value> = (0..200).map(|i| (format!("P{i}"), json!({"PICKBOX": 3}))).collect();
        p.load(&json!({ "profileName": "\n", "profiles": many }));
        assert_eq!(p.current, UNNAMED, "a bad name is skipped");
        assert_eq!(p.names().len(), MAX_PROFILES);
        p.load(&json!({ "profiles": { "A": "not settings", "": {} } }));
        assert_eq!(p.names(), vec![UNNAMED.to_string()]);
    }
}
