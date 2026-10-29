//! The `CADCRAFT_*` variables of the desktop app, read once at startup into [`Env`] and passed on
//! from there; no library crate reads them. `--help` lists them from [`VARS`]. `--control`'s
//! fallback, `CADCRAFT_CONTROL_PORT`, is clap's (see `launch_args`).

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::PathBuf;

use cadcraft_ui_egui::switch::Switch;

const CONFIG_DIR: &str = "CADCRAFT_CONFIG_DIR";
const LOCALE: &str = "CADCRAFT_LOCALE";
const VSYNC: &str = "CADCRAFT_VSYNC";
const WAYLAND: &str = "CADCRAFT_WAYLAND";
const NO_NATIVE_MENU: &str = "CADCRAFT_NO_NATIVE_MENU";
const SYSTEM_CURSOR: &str = "CADCRAFT_SYSTEM_CURSOR";
const FONTALT: &str = "CADCRAFT_FONTALT";
const FONTFALLBACK: &str = "CADCRAFT_FONTFALLBACK";

/// Every variable [`Env`] reads, with its `--help` line.
pub(crate) const VARS: &[(&str, &str)] = &[
    (CONFIG_DIR, "the settings folder (logs, autosave, workspace, plotters); empty means no settings folder"),
    (LOCALE, "the interface language, instead of the system's"),
    (VSYNC, "1 presents with vsync, 0 without it (default: without on Linux and BSD)"),
    (WAYLAND, "1 keeps the native Wayland backend instead of XWayland; 0 or unset is off"),
    (NO_NATIVE_MENU, "macOS: 1 keeps the menus inside the window; 0 or unset is off"),
    (SYSTEM_CURSOR, "1 uses the system cursor instead of the drawn crosshair; 0 or unset is off"),
    (FONTALT, "the font that replaces a missing one"),
    (FONTFALLBACK, "the fonts tried, in order, for characters a font lacks"),
];

/// The process environment, read once at startup. A blank variable counts as unset, except
/// `CADCRAFT_CONFIG_DIR`.
#[derive(Debug, PartialEq)]
pub(crate) struct Env {
    /// The settings folder (logs, autosave, workspace, plotters): `CADCRAFT_CONFIG_DIR`; else, when
    /// that is unset, the platform's (`cadcraft_engine::settings_dir::platform`). `None`: no
    /// folder, because the variable is empty or blank (the way to run without one) or the
    /// platform has none.
    pub(crate) config_dir: Option<PathBuf>,
    /// The interface language, instead of the system's.
    pub(crate) locale: Option<String>,
    /// Present with vsync (default: off on Linux and BSD, on elsewhere).
    pub(crate) vsync: Switch,
    /// Keep the native Wayland backend instead of XWayland.
    #[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
    pub(crate) wayland: Switch,
    /// macOS: keep the menus inside the window.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) no_native_menu: Switch,
    /// Use the system cursor instead of the drawn crosshair.
    pub(crate) system_cursor: Switch,
    /// FONTALT at startup.
    pub(crate) font_alt: Option<String>,
    /// FONTFALLBACK at startup.
    pub(crate) font_fallback: Option<String>,
}

impl Env {
    pub(crate) fn read() -> Env {
        Env::from_vars(|k| std::env::var_os(k))
    }

    /// [`Env::read`] with `var` looking a variable up.
    pub(crate) fn from_vars(var: impl Fn(&str) -> Option<OsString>) -> Env {
        let text = |k: &str| var(k).and_then(|v| v.into_string().ok()).filter(|v| !v.trim().is_empty());
        let switch = |k: &str| Switch::parse(k, var(k).map(|v| v.to_string_lossy().into_owned()).as_deref());
        Env {
            config_dir: match var(CONFIG_DIR) {
                Some(dir) if dir.to_str().is_some_and(|d| d.trim().is_empty()) => None,
                Some(dir) => Some(PathBuf::from(dir)),
                None => cadcraft_engine::settings_dir::platform(&var),
            },
            locale: text(LOCALE),
            vsync: switch(VSYNC),
            wayland: switch(WAYLAND),
            no_native_menu: switch(NO_NATIVE_MENU),
            system_cursor: switch(SYSTEM_CURSOR),
            font_alt: text(FONTALT),
            font_fallback: text(FONTFALLBACK),
        }
    }
}

/// The `--help` section that lists [`VARS`].
pub(crate) fn help() -> String {
    let width = VARS.iter().map(|(name, _)| name.len()).max().unwrap_or(0) + 3;
    VARS.iter().fold(String::from("Environment:"), |mut help, (name, line)| {
        let _ = write!(help, "\n  {name:<width$}{line}");
        help
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |k| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v))
    }

    #[test]
    fn every_variable_read_is_in_the_help_and_every_listed_one_is_read() {
        let asked = RefCell::new(Vec::new());
        Env::from_vars(|k| {
            asked.borrow_mut().push(k.to_owned());
            None
        });
        let ours: Vec<String> = asked.into_inner().into_iter().filter(|k| k.starts_with("CADCRAFT_")).collect();
        let help = help();
        let unset = Env::from_vars(|_| None);
        for k in &ours {
            assert!(VARS.iter().any(|(name, _)| name == k), "{k} is read but not in VARS");
        }
        for (name, line) in VARS {
            assert!(ours.iter().any(|k| k == name), "{name} is in VARS but not read");
            assert_ne!(Env::from_vars(env(&[(name, "1")])), unset, "{name} sets no field");
            assert!(help.lines().any(|l| l.trim_start().starts_with(name) && l.ends_with(line)), "{help}");
        }
    }

    #[test]
    fn values_are_read_and_blank_ones_are_unset() {
        let vars = [
            (CONFIG_DIR, "/private/cc"),
            ("HOME", "/home/u"),
            ("XDG_CONFIG_HOME", "/x"),
            ("APPDATA", "C:\\a"),
            (LOCALE, "cs-CZ"),
            (VSYNC, " On "),
            (WAYLAND, "0"),
            (NO_NATIVE_MENU, "yes"),
            (SYSTEM_CURSOR, "maybe"),
            (FONTALT, "Noto Sans"),
            (FONTFALLBACK, "Vazirmatn, msjh.ttc"),
        ];
        let read = Env::from_vars(env(&vars));
        assert_eq!(
            read,
            Env {
                config_dir: Some(PathBuf::from("/private/cc")),
                locale: Some("cs-CZ".into()),
                vsync: Switch::On,
                wayland: Switch::Off,
                no_native_menu: Switch::On,
                system_cursor: Switch::Default,
                font_alt: Some("Noto Sans".into()),
                font_fallback: Some("Vazirmatn, msjh.ttc".into()),
            }
        );
        let blank = vars.map(|(k, _)| (k, if k == FONTALT { "  " } else { "" }));
        let read = Env::from_vars(env(&blank));
        assert_eq!(read.config_dir, None, "an empty variable means no directory, not the working directory");
        assert_eq!((read.locale, read.font_alt, read.font_fallback), (None, None, None));
        assert_eq!([read.vsync, read.wayland, read.no_native_menu, read.system_cursor], [Switch::Default; 4]);
    }

    #[test]
    fn config_dir_unset_is_the_platform_folder_set_is_that_folder_blank_is_none() {
        let platform = [("HOME", "/home/u"), ("XDG_CONFIG_HOME", "/x"), ("APPDATA", "C:\\a")];
        let expected = cadcraft_engine::settings_dir::platform(env(&platform));
        assert!(expected.is_some());
        assert_eq!(Env::from_vars(env(&platform)).config_dir, expected);
        let set = [platform.as_slice(), &[(CONFIG_DIR, "/private/cc")]].concat();
        assert_eq!(Env::from_vars(env(&set)).config_dir, Some(PathBuf::from("/private/cc")));
        for blank in ["", "  "] {
            let vars = [platform.as_slice(), &[(CONFIG_DIR, blank)]].concat();
            assert_eq!(Env::from_vars(env(&vars)).config_dir, None, "{blank:?} means no settings folder, not the platform's");
        }
    }
}
