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
    /// Portable eframe storage file, kept with the other settings instead of the user profile.
    pub(crate) portable_storage_file: Option<PathBuf>,
    /// The settings folder (logs, autosave, workspace, plotters): `CADCRAFT_CONFIG_DIR`; else, when
    /// that is unset, the portable directory or the platform's (`cadcraft_engine::settings_dir::platform`). `None`: no
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
        let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from));
        Self::from_vars_and_exe(|k| std::env::var_os(k), exe_dir.as_deref())
    }

    fn from_vars_and_exe(var: impl Fn(&str) -> Option<OsString>, exe_dir: Option<&std::path::Path>) -> Env {
        let mut vars = Env::from_vars(&var);
        // An explicit override (including blank, which disables settings) takes precedence.
        if var(CONFIG_DIR).is_none()
            && let Some(exe) = exe_dir
        {
            match crate::portable::directory_in(exe) {
                Ok(Some(data)) => {
                    vars.config_dir = Some(data.clone());
                    vars.portable_storage_file = Some(data.join("app.ron"));
                }
                Ok(None) => {}
                Err(error) => log::warn!("portable settings cannot be written ({error}); using the user profile"),
            }
        }
        vars
    }

    /// [`Env::read`] with `var` looking a variable up.
    pub(crate) fn from_vars(var: impl Fn(&str) -> Option<OsString>) -> Env {
        let text = |k: &str| var(k).and_then(|v| v.into_string().ok()).filter(|v| !v.trim().is_empty());
        let switch = |k: &str| Switch::parse(k, var(k).map(|v| v.to_string_lossy().into_owned()).as_deref());
        Env {
            portable_storage_file: None,
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

    #[test]
    fn portable_eframe_storage_is_a_file_inside_the_settings_directory() {
        let exe = std::env::temp_dir().join(format!("cadcraft-env-portable-{}", std::process::id()));
        std::fs::create_dir_all(&exe).unwrap();
        std::fs::write(exe.join("portable.txt"), "").unwrap();
        let vars = Env::from_vars_and_exe(env(&[("APPDATA", "/profile")]), Some(&exe));
        assert_eq!(vars.config_dir, Some(exe.join("CADCraftData")));
        let storage_file = vars.portable_storage_file.unwrap();
        assert_eq!(storage_file, exe.join("CADCraftData").join("app.ron"));
        assert_eq!(storage_file.parent(), vars.config_dir.as_deref());
        std::fs::write(&storage_file, r#"{"window":"saved window state"}"#).unwrap();
        let restarted = Env::from_vars_and_exe(env(&[("APPDATA", "/profile")]), Some(&exe));
        assert_eq!(restarted.portable_storage_file.as_ref(), Some(&storage_file));
        assert_eq!(std::fs::read_to_string(restarted.portable_storage_file.unwrap()).unwrap(), r#"{"window":"saved window state"}"#);
        std::fs::remove_dir_all(exe).unwrap();
    }

    #[test]
    fn explicit_override_skips_portable_probe() {
        let exe = std::env::temp_dir().join(format!("cadcraft-env-override-{}", std::process::id()));
        std::fs::create_dir_all(&exe).unwrap();
        std::fs::write(exe.join("portable.txt"), "").unwrap();
        for value in ["/custom", "", "  "] {
            let vars = Env::from_vars_and_exe(env(&[(CONFIG_DIR, value)]), Some(&exe));
            assert_eq!(vars.config_dir, (!value.trim().is_empty()).then(|| PathBuf::from(value)));
            assert_eq!(vars.portable_storage_file, None);
            assert!(!exe.join("CADCraftData").exists());
        }
        std::fs::remove_dir_all(exe).unwrap();
    }

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
                portable_storage_file: None,
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
