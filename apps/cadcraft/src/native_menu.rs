//! macOS: CADCraft's menu tree as the native system menu bar. Items dispatch through the same
//! command path as the in-window menus; enablement is refreshed a few times per second.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::mpsc;

use cadcraft_ui_egui::menus::{self, Entry};
use cadcraft_ui_egui::{CadApp, i18n};
use muda::accelerator::Accelerator;
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

pub struct NativeMenu {
    _menu: Menu,
    items: HashMap<String, (String, String, MenuItem)>,
    submenus: Vec<(String, Submenu)>,
    predefined: Vec<(&'static str, PredefinedMenuItem)>,
    language: &'static str,
    /// Monotonic time of the last enablement refresh. `NEG_INFINITY` makes the first poll refresh
    /// immediately: `now_ms` starts near zero, so the old `0.0` sentinel suppressed the first
    /// ~300 ms of menu enablement.
    last_refresh: f64,
    /// The recent files File ▸ Open Recent lists now (it is rebuilt when they change).
    recent: Vec<String>,
    counter: usize,
    events: mpsc::Receiver<MenuEvent>,
}

/// Accelerator for "Cmd+Shift+S" (modifier-less shortcuts stay in the app so typing works).
fn accel(sc: &str) -> Option<Accelerator> {
    if !(sc.contains("Cmd") || sc.contains("Ctrl") || sc.contains("Alt")) {
        return None;
    }
    Accelerator::from_str(&sc.replace("Cmd", "CMD").replace("Alt", "ALT").replace("Shift", "SHIFT").replace("Ctrl", "CTRL")).ok()
}

impl NativeMenu {
    /// Build and register the macOS menu (app menu, Settings and the translated command tree); returns the state holding item ids and submenus.
    pub fn install(app: &mut CadApp, ctx: &egui::Context) -> Self {
        // A menu choice (or its shortcut, such as Cmd+W) is no window event, so winit would not
        // wake the app for it: the handler does, and the choice runs in the next frame.
        let (sender, events) = mpsc::channel();
        let wake = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event| {
            let _ = sender.send(event);
            wake.request_repaint();
        }));
        let menu = Menu::new();
        let mut items = HashMap::new();
        let language = app.language();
        let mut submenus = Vec::new();
        let mut counter = 0usize;
        let app_menu = Submenu::new("CADCraft", true);
        let about = MenuItem::with_id("cc-about", i18n::tr(language, "About CADCraft"), true, None);
        let discord = MenuItem::with_id("cc-discord", i18n::tr(language, "Join the ArtCraft Discord…"), true, None);
        let predefined = vec![
            ("Services", PredefinedMenuItem::services(Some(i18n::tr(language, "Services")))),
            ("Hide CADCraft", PredefinedMenuItem::hide(Some(i18n::tr(language, "Hide CADCraft")))),
            ("Hide Others", PredefinedMenuItem::hide_others(Some(i18n::tr(language, "Hide Others")))),
            ("Show All", PredefinedMenuItem::show_all(Some(i18n::tr(language, "Show All")))),
            ("Quit CADCraft", PredefinedMenuItem::quit(Some(i18n::tr(language, "Quit CADCraft")))),
        ];
        // The Mac place for preferences: CADCraft ▸ Settings… (⌘,) opens the Options dialog.
        let settings = MenuItem::with_id("cc-settings", i18n::tr(language, "Settings..."), true, accel("Cmd+Comma"));
        let _ = app_menu.append_items(&[&about, &discord, &PredefinedMenuItem::separator(), &settings, &PredefinedMenuItem::separator()]);
        for (label, item) in &predefined {
            let _ = app_menu.append(item);
            if ["Services", "Show All"].contains(label) {
                let _ = app_menu.append(&PredefinedMenuItem::separator());
            }
        }
        items.insert("cc-about".into(), ("ui.dialog.about".into(), "About CADCraft".into(), about));
        items.insert("cc-discord".into(), ("ui.discord".into(), "Join the ArtCraft Discord…".into(), discord));
        items.insert("cc-settings".into(), ("options".into(), "Settings...".into(), settings));
        let _ = menu.append(&app_menu);
        for (title, entries) in menus::tree(app) {
            let sub = Submenu::new(i18n::tr_menu(language, &title), true);
            build(&sub, &entries, &mut items, &mut submenus, &mut counter, language);
            let _ = menu.append(&sub);
            submenus.push((title, sub));
        }
        menu.init_for_nsapp();
        Self {
            _menu: menu,
            items,
            submenus,
            predefined,
            language,
            last_refresh: f64::NEG_INFINITY,
            recent: app.ui.recent_files.clone(),
            counter,
            events,
        }
    }

    /// Rebuild File ▸ Open Recent after a drawing was opened or the list was cleared: the native
    /// menu is built once, and its items name the files by position.
    fn refresh_recent(&mut self, app: &CadApp) {
        if self.recent == app.ui.recent_files {
            return;
        }
        self.recent = app.ui.recent_files.clone();
        let Some(sub) = self.submenus.iter().find(|(label, _)| label == menus::OPEN_RECENT).map(|(_, s)| s.clone()) else { return };
        for _ in sub.items() {
            sub.remove_at(0);
        }
        let ids: Vec<String> = self.items.iter().filter(|(_, (cmd, _, _))| cmd.starts_with("ui.recent.")).map(|(id, _)| id.clone()).collect();
        for id in ids {
            self.items.remove(&id);
        }
        build(&sub, &menus::recent_entries(app), &mut self.items, &mut self.submenus, &mut self.counter, self.language);
    }

    /// Drain queued menu events into commands (Discord opens externally), rebuild Open Recent, retranslate on a language change and refresh enablement when 300 ms have elapsed.
    pub fn poll(&mut self, app: &mut CadApp, ctx: &egui::Context) {
        while let Ok(ev) = self.events.try_recv() {
            if let Some((cmd, _, _)) = self.items.get(ev.id.as_ref()) {
                if cmd == "ui.discord" {
                    ctx.open_url(egui::OpenUrl::new_tab("https://discord.gg/artcraft"));
                } else {
                    menus::activate_native(app, ctx, &cmd.clone());
                }
                ctx.request_repaint();
            }
        }
        self.refresh_recent(app);
        let language = app.language();
        if self.language != language {
            self.language = language;
            for (_, label, item) in self.items.values() {
                item.set_text(i18n::tr(language, label));
            }
            for (label, item) in &self.predefined {
                item.set_text(i18n::tr(language, label));
            }
            for (label, submenu) in &self.submenus {
                submenu.set_text(i18n::tr_menu(language, label));
            }
        }
        let now = cadcraft_ui_egui::now_ms();
        if !refresh_due(now, self.last_refresh) {
            return;
        }
        self.last_refresh = now;
        let typing = menus::text_field_window(ctx).is_some();
        for (cmd, _, item) in self.items.values() {
            let en = (typing && menus::edits_text(cmd)) || cadcraft_engine::find_command(cmd).is_none_or(|c| (c.enabled)(&app.session).is_ok());
            item.set_enabled(en);
        }
    }
}

/// The periodic enablement refresh is due every 300 ms. Split out so the first-poll sentinel is
/// testable without a macOS menu: `now_ms` starts near zero, so a `0.0` initial value would
/// suppress the first refresh for 300 ms.
fn refresh_due(now: f64, last_refresh: f64) -> bool {
    now - last_refresh >= 300.0
}

fn build(
    parent: &Submenu,
    entries: &[Entry],
    items: &mut HashMap<String, (String, String, MenuItem)>,
    submenus: &mut Vec<(String, Submenu)>,
    counter: &mut usize,
    language: &str,
) {
    for e in entries {
        match e {
            Entry::Sub { label, children } => {
                let sub = Submenu::new(i18n::tr_menu(language, label), true);
                build(&sub, children, items, submenus, counter, language);
                let _ = parent.append(&sub);
                submenus.push((label.clone(), sub));
            }
            Entry::Item { label, id, shortcut, enabled } => {
                *counter += 1;
                let mid = format!("cc{counter}");
                let i = MenuItem::with_id(mid.clone(), i18n::tr(language, label), *enabled, shortcut.as_deref().and_then(accel));
                let _ = parent.append(&i);
                items.insert(mid, (id.clone(), label.clone(), i));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::refresh_due;

    /// `refresh_due` treats `NEG_INFINITY` as due at any finite time and `0.0` as due once 300 ms have elapsed; `install` initializes a fresh menu with `last_refresh = NEG_INFINITY`.
    #[test]
    fn the_first_poll_refreshes_immediately() {
        assert!(refresh_due(0.5, f64::NEG_INFINITY), "a fresh menu refreshes on its first poll");
        assert!(!refresh_due(0.5, 0.0), "the old 0.0 sentinel delayed the first refresh");
        assert!(!refresh_due(299.9, 0.0));
        assert!(refresh_due(300.0, 0.0));
    }
}
