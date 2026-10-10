//! macOS: CADCraft's menu tree as the native system menu bar. Items dispatch through the same
//! command path as the in-window menus; enablement is refreshed a few times per second.

use std::collections::HashMap;
use std::str::FromStr;

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
    last_refresh: f64,
}

/// Accelerator for "Cmd+Shift+S" (modifier-less shortcuts stay in the app so typing works).
fn accel(sc: &str) -> Option<Accelerator> {
    if !(sc.contains("Cmd") || sc.contains("Ctrl") || sc.contains("Alt")) {
        return None;
    }
    Accelerator::from_str(&sc.replace("Cmd", "CMD").replace("Alt", "ALT").replace("Shift", "SHIFT").replace("Ctrl", "CTRL")).ok()
}

impl NativeMenu {
    pub fn install(app: &mut CadApp) -> Self {
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
        let _ = app_menu.append_items(&[&about, &discord, &PredefinedMenuItem::separator()]);
        for (label, item) in &predefined {
            let _ = app_menu.append(item);
            if ["Services", "Show All"].contains(label) {
                let _ = app_menu.append(&PredefinedMenuItem::separator());
            }
        }
        items.insert("cc-about".into(), ("ui.dialog.about".into(), "About CADCraft".into(), about));
        items.insert("cc-discord".into(), ("ui.discord".into(), "Join the ArtCraft Discord…".into(), discord));
        let _ = menu.append(&app_menu);
        for (title, entries) in menus::tree(app) {
            let sub = Submenu::new(i18n::tr(language, &title), true);
            build(&sub, &entries, &mut items, &mut submenus, &mut counter, language);
            let _ = menu.append(&sub);
            submenus.push((title, sub));
        }
        menu.init_for_nsapp();
        Self { _menu: menu, items, submenus, predefined, language, last_refresh: 0.0 }
    }

    pub fn poll(&mut self, app: &mut CadApp, ctx: &egui::Context) {
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if let Some((cmd, _, _)) = self.items.get(ev.id.as_ref()) {
                if cmd == "ui.discord" {
                    ctx.open_url(egui::OpenUrl::new_tab("https://discord.gg/artcraft"));
                } else {
                    menus::activate(app, &cmd.clone());
                }
                ctx.request_repaint();
            }
        }
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
                submenu.set_text(i18n::tr(language, label));
            }
        }
        let now = cadcraft_ui_egui::now_ms();
        if now - self.last_refresh < 300.0 {
            return;
        }
        self.last_refresh = now;
        for (cmd, _, item) in self.items.values() {
            let en = cadcraft_engine::find_command(cmd).is_none_or(|c| (c.enabled)(&app.session).is_ok());
            item.set_enabled(en);
        }
    }
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
                let sub = Submenu::new(i18n::tr(language, label), true);
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
