//! App-owned nightly profile fences; no stable fallback, migration or process environment changes.

use std::ffi::OsString;

use crate::portable::{PORTABLE_DATA_DIR, portable_marker};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
    Windows,
    Linux,
    Mac,
}

impl Platform {
    fn current() -> Result<Self, String> {
        match std::env::consts::OS {
            "windows" => Ok(Self::Windows),
            "linux" => Ok(Self::Linux),
            "macos" => Ok(Self::Mac),
            // CADCraft also builds for the BSDs (packaging/freebsd): nightly-v1 is Windows, macOS and
            // Linux only, as the shared nightlies publish.
            _ => Err("nightly-v1 is not defined for this operating system".into()),
        }
    }

    fn ignores_case(self) -> bool {
        self != Self::Linux
    }
}

fn absolute(path: PathBuf, label: &str) -> Result<PathBuf, String> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!("{label} must be absolute and contain no parent traversal"));
    }
    Ok(path)
}

fn required_base(env: &impl Fn(&str) -> Option<OsString>, key: &str) -> Result<PathBuf, String> {
    let value = env(key).filter(|v| !v.is_empty()).ok_or_else(|| format!("cannot establish the stable {key} boundary"))?;
    absolute(PathBuf::from(value), key)
}

fn xdg_base(env: &impl Fn(&str) -> Option<OsString>, key: &str, fallback: &str) -> Result<PathBuf, String> {
    match env(key).filter(|v| !v.is_empty()) {
        Some(value) => absolute(PathBuf::from(value), key),
        None => Ok(required_base(env, "HOME")?.join(fallback)),
    }
}

/// Resolve existing ancestors before creating anything. A dangling link or inaccessible ancestor
/// is not a missing directory that may be replaced or followed later.
fn canonical_future(path: &Path) -> Result<PathBuf, String> {
    let mut parent = path;
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(parent) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(parent.file_name().ok_or("the profile root has no existing ancestor")?.to_os_string());
                parent = parent.parent().ok_or("the profile root has no existing ancestor")?;
            }
            Err(error) => return Err(format!("cannot inspect the profile boundary: {error}")),
        }
    }
    let mut resolved = std::fs::canonicalize(parent).map_err(|e| format!("cannot resolve the profile boundary: {e}"))?;
    for part in missing.into_iter().rev() {
        resolved.push(part);
    }
    Ok(resolved)
}

fn overlaps(a: &Path, b: &Path, platform: Platform) -> bool {
    // A component that is not valid UTF-8 keeps its exact bytes: a lossy comparison would fold two
    // different names into the same replacement character and call them one folder.
    let parts = |p: &Path| {
        p.components()
            .map(|c| {
                let value = c.as_os_str();
                match (platform.ignores_case(), value.to_str()) {
                    (true, Some(text)) => OsString::from(text.to_lowercase()),
                    _ => value.to_os_string(),
                }
            })
            .collect::<Vec<_>>()
    };
    let (a, b) = (parts(a), parts(b));
    a.starts_with(&b) || b.starts_with(&a)
}

fn installation_boundary(platform: Platform, exe: &Path) -> Result<PathBuf, String> {
    let parent = exe.parent().ok_or("cannot establish the installation boundary")?;
    if platform == Platform::Mac {
        // A profile in Resources is still inside the versioned install, not beside MacOS.
        if let Some(bundle) = exe.ancestors().find(|p| p.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("app"))) {
            return Ok(bundle.to_path_buf());
        }
    }
    Ok(parent.to_path_buf())
}

/// Every stable CADCraft data root of this platform, plus the install folders and the running
/// install ([`installation_boundary`]).
fn forbidden_roots(platform: Platform, env: &impl Fn(&str) -> Option<OsString>, exe: &Path) -> Result<Vec<PathBuf>, String> {
    let mut roots = match platform {
        Platform::Windows => {
            let mut roots = ["APPDATA", "LOCALAPPDATA"]
                .into_iter()
                .map(|key| required_base(env, key).map(|p| p.join("CADCraft")))
                .collect::<Result<Vec<_>, _>>()?;
            // Stable's eframe storage folder (`eframe::storage_dir`: `app.ron` under
            // `%APPDATA%\ai.storyteller.cadcraft\data`), outside the roaming `CADCraft` folder.
            roots.push(required_base(env, "APPDATA")?.join(EFRAME_APP_ID));
            // The MSI's per-machine folder (cadcraft.wxs). A custom install folder is not known
            // here; the executable's own folder is fenced below.
            for key in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
                if let Some(base) = env(key).filter(|v| !v.is_empty()) {
                    roots.push(absolute(PathBuf::from(base), key)?.join("ArtCraft").join("CADCraft"));
                }
            }
            roots
        }
        Platform::Linux => {
            let mut roots =
                [("XDG_CONFIG_HOME", ".config"), ("XDG_DATA_HOME", ".local/share"), ("XDG_CACHE_HOME", ".cache"), ("XDG_STATE_HOME", ".local/state")]
                    .into_iter()
                    .map(|(key, fallback)| xdg_base(env, key, fallback).map(|p| p.join("cadcraft")))
                    .collect::<Result<Vec<_>, _>>()?;
            roots.extend(["/usr/bin/cadcraft", "/usr/local/bin/cadcraft", "/opt/cadcraft"].map(PathBuf::from));
            // The stable Flatpak keeps its data in its own per-app folder.
            roots.push(required_base(env, "HOME")?.join(".var/app/ai.storyteller.cadcraft"));
            // Stable's eframe storage folder (`eframe::storage_dir`: window state, Options profile,
            // UI and autosave prefs in `app.ron`), which is not the XDG settings folder above.
            roots.push(xdg_base(env, "XDG_DATA_HOME", ".local/share")?.join(EFRAME_APP_ID));
            roots
        }
        Platform::Mac => {
            let home = required_base(env, "HOME")?;
            let library = home.join("Library");
            vec![
                library.join("Application Support/CADCraft"),
                library.join("Caches/CADCraft"),
                library.join("Caches/ai.storyteller.cadcraft"),
                // Stable's eframe storage folder (`eframe::storage_dir`: `app.ron` in Application
                // Support), beside the settings folder env.rs uses.
                library.join("Application Support").join(EFRAME_APP_ID),
                library.join("Preferences/CADCraft"),
                library.join("Preferences/ai.storyteller.cadcraft.plist"),
                library.join("Saved Application State/ai.storyteller.cadcraft.savedState"),
                home.join("Applications/CADCraft.app"),
                PathBuf::from("/Applications/CADCraft.app"),
            ]
        }
    };
    roots.push(installation_boundary(platform, exe)?);
    roots.into_iter().map(|p| canonical_future(&p)).collect()
}

fn private_subdirectory(root: &Path, name: impl AsRef<Path>, label: &str) -> Result<PathBuf, String> {
    let chosen = canonical_future(&absolute(root.to_path_buf(), "the nightly profile root")?)?;
    let child = canonical_future(&chosen.join(name))?;
    if child == chosen || !child.starts_with(&chosen) {
        return Err(format!("the nightly {label} must stay inside its selected profile"));
    }
    // A regular file in a folder's place is not a folder: the readers and sweepers that follow would
    // fail halfway, so refuse it while the profile is still being validated.
    match std::fs::symlink_metadata(&child) {
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(format!("the nightly {label} must be a directory, not a file or an alias"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot inspect the nightly {label}: {error}")),
    }
    Ok(child)
}

/// Every existing entry directly in the profile (logs, settings and so on) must resolve inside it,
/// so a link there cannot point Nightly's writes at another app's data.
fn entries_stay_inside(root: &Path) -> Result<(), String> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("cannot inspect the nightly profile: {error}")),
    };
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot inspect the nightly profile: {e}"))?;
        private_subdirectory(root, entry.file_name(), "profile entry")?;
    }
    Ok(())
}

/// Recognise the shared portable data namespace in both the supplied and resolved path.
/// This is not an inventory of arbitrary custom Stable overrides or a Launcher ownership proof.
fn is_portable_layout(path: &Path, platform: Platform) -> bool {
    // A folder under a portable copy belongs to that copy too. Above the chosen folder itself, a
    // marker only counts beside a CADCraft executable, so an unrelated portable.txt higher up
    // (in a user folder or at a drive root) does not refuse the profile.
    path.ancestors().enumerate().any(|(depth, ancestor)| {
        portable_marker(ancestor).is_some() && (depth == 0 || ["cadcraft.exe", "cadcraft"].iter().any(|exe| ancestor.join(exe).is_file()))
    }) || path.components().any(|component| match component {
        Component::Normal(name) if platform.ignores_case() => name.to_string_lossy().eq_ignore_ascii_case(PORTABLE_DATA_DIR),
        Component::Normal(name) => name == PORTABLE_DATA_DIR,
        _ => false,
    })
}

/// The writable hook is the app's existing exclusive probe (`portable::probe`). Keeping it injected
/// lets filesystem boundary tests exercise the real policy without starting the app or changing
/// global state.
pub fn validate(
    root: &Path,
    env: impl Fn(&str) -> Option<OsString>,
    exe: Option<&Path>,
    writable: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<PathBuf, String> {
    validate_for(Platform::current()?, root, env, exe, writable)
}

fn validate_for(
    platform: Platform,
    root: &Path,
    env: impl Fn(&str) -> Option<OsString>,
    exe: Option<&Path>,
    writable: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<PathBuf, String> {
    absolute(root.to_path_buf(), "the nightly profile root")?;
    let chosen = canonical_future(root)?;
    // The deb/rpm CLI wrapper exports CADCRAFT_CONFIG_DIR at the folder this launch already owns (the
    // GUI wrappers name the profile through `--profile-root` instead), so that value is inert:
    // `env::apply_profile` sets the settings folder from the profile, not from the variable. Any
    // other value would redirect Nightly's settings folder, so it is refused.
    if let Some(configured) = env("CADCRAFT_CONFIG_DIR").filter(|v| !v.is_empty()) {
        let configured = canonical_future(&absolute(PathBuf::from(configured), "CADCRAFT_CONFIG_DIR")?)?;
        if !configured.starts_with(&chosen) {
            return Err("nightly-v1 cannot be combined with CADCRAFT_CONFIG_DIR outside its own profile".into());
        }
    }
    if platform != Platform::Windows
        && let Some(id) = env("FLATPAK_ID").filter(|v| !v.is_empty())
    {
        // A Flatpak's data folder and runtime socket namespace belong to its app id. The nightly
        // bundle has an id of its own ([`NIGHTLY_FLATPAK_ID`], named by
        // `packaging/linux/flatpak-bundle.sh`), so its profile is already the sandbox's own; any
        // other instance is the stable app's (or another app's), whose socket this profile must
        // not inherit.
        if id.to_string_lossy() != NIGHTLY_FLATPAK_ID {
            return Err("nightly-v1 cannot inherit another Flatpak app's instance; launch through the native Launcher".into());
        }
    }
    if is_portable_layout(root, platform) || is_portable_layout(&chosen, platform) {
        return Err("the nightly root must not use a portable CADCraft installation or its CADCraftData settings folder".into());
    }
    let exe = absolute(exe.ok_or("cannot establish the installation boundary")?.to_path_buf(), "the executable")?;
    let forbidden = forbidden_roots(platform, &env, &exe)?;
    if forbidden.iter().any(|stable| overlaps(&chosen, stable, platform)) {
        return Err("the nightly root must not overlap stable data or an installation".into());
    }
    entries_stay_inside(&chosen)?;
    // Drawing Recovery reads, adopts, overwrites and sweeps entries. Never let its alias leave this
    // private profile, including toward a stable portable directory not in our known roots.
    private_subdirectory(&chosen, AUTOSAVE, "recovery folder")?;
    let mut create = std::fs::DirBuilder::new();
    create.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        create.mode(0o700);
    }
    create.create(&chosen).map_err(|e| format!("cannot create the nightly profile: {e}"))?;
    let confirmed = std::fs::canonicalize(&chosen).map_err(|e| format!("cannot resolve the created nightly profile: {e}"))?;
    if confirmed != chosen || forbidden.iter().any(|stable| overlaps(&confirmed, stable, platform)) {
        return Err("the nightly profile changed while it was being validated".into());
    }
    private_subdirectory(&confirmed, AUTOSAVE, "recovery folder")?;
    entries_stay_inside(&confirmed)?;
    writable(&confirmed).map_err(|e| format!("the nightly profile is not writable: {e}"))?;
    Ok(confirmed)
}

/// The app id of the nightly Flatpak bundle (`packaging/linux/flatpak-bundle.sh`): the one Flatpak
/// instance whose sandbox is this channel's own. The window identity ([`crate::os_window`]) and the
/// bundle id differ on purpose, as they do for stable.
pub const NIGHTLY_FLATPAK_ID: &str = "ai.storyteller.cadcraft.nightly";

/// The app id eframe derives stable's storage folder from ([`crate::main`]'s `persistence_path`).
/// A profile that sits there would overwrite stable's `app.ron`, so its folder is fenced too.
const EFRAME_APP_ID: &str = "ai.storyteller.cadcraft";

/// The subfolder autosave and Drawing Recovery write ([`crate::main`], `<settings>/autosave`).
pub const AUTOSAVE: &str = "autosave";

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture {
        root: PathBuf,
        env: BTreeMap<String, OsString>,
        exe: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = loop {
                let candidate =
                    std::env::temp_dir().join(format!("cadcraft-profile-policy-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
                match std::fs::create_dir(&candidate) {
                    Ok(()) => break std::fs::canonicalize(candidate).unwrap(),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("cannot create owned fixture: {error}"),
                }
            };
            let home = root.join("home");
            let exe = root.join("install/cadcraft");
            std::fs::create_dir_all(&home).unwrap();
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(&exe, b"fixture executable, never run").unwrap();
            let env = [("HOME", home), ("APPDATA", root.join("roaming")), ("LOCALAPPDATA", root.join("local"))]
                .into_iter()
                .map(|(key, value)| (key.into(), value.into_os_string()))
                .collect();
            Self { root, env, exe }
        }

        fn get(&self, key: &str) -> Option<OsString> {
            self.env.get(key).cloned()
        }

        fn check(&self, platform: Platform, root: &Path) -> Result<PathBuf, String> {
            validate_for(
                platform,
                root,
                |key| self.get(key),
                Some(&self.exe),
                |dir| {
                    let probe = dir.join("exclusive-test-probe");
                    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe)?;
                    drop(file);
                    std::fs::remove_file(probe)
                },
            )
        }

        fn home(&self) -> PathBuf {
            PathBuf::from(self.get("HOME").unwrap())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            // Only the directory successfully created by this fixture is owned for removal.
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn portable_settings_from_another_install_are_refused_before_any_probe() {
        let fixture = Fixture::new();
        let stable_install = fixture.root.join("stable portable install");
        let stable_data = stable_install.join(PORTABLE_DATA_DIR);
        std::fs::create_dir_all(&stable_data).unwrap();
        std::fs::write(stable_install.join("portable.txt"), b"owned portable fixture").unwrap();
        std::fs::write(stable_data.join("settings.json"), b"stable settings must not change").unwrap();
        for platform in [Platform::Windows, Platform::Linux, Platform::Mac] {
            for candidate in [&stable_data, &stable_data.join("nested nightly")] {
                let probed = std::cell::Cell::new(false);
                let result = validate_for(
                    platform,
                    candidate,
                    |key| fixture.get(key),
                    Some(&fixture.exe),
                    |_| {
                        probed.set(true);
                        Ok(())
                    },
                );
                assert!(result.is_err(), "a different portable install is not a private Nightly profile");
                assert!(!probed.get(), "reject portable data before a writable probe");
                assert!(!stable_data.join("nested nightly").exists());
                assert_eq!(std::fs::read(stable_data.join("settings.json")).unwrap(), b"stable settings must not change");
                assert_eq!(std::fs::read_dir(&stable_data).unwrap().count(), 1);
            }
        }
        let private = fixture.root.join("CADCraftData-backup-name/nightly");
        assert!(fixture.check(Platform::Linux, &private).is_ok(), "a merely similar name is not a portable settings component");
    }

    #[cfg(unix)]
    #[test]
    fn recovery_aliases_cannot_redirect_to_stable_missing_or_profile_root_data() {
        let fixture = Fixture::new();
        let stable = fixture.home().join(".local/state/cadcraft/autosave");
        std::fs::create_dir_all(&stable).unwrap();
        std::fs::write(stable.join("stable-autosave"), b"stable recovery bytes").unwrap();
        for (index, target) in [stable.clone(), fixture.root.join("missing recovery target")].into_iter().enumerate() {
            let private = fixture.root.join(format!("nightly-{index}"));
            std::fs::create_dir(&private).unwrap();
            std::os::unix::fs::symlink(&target, private.join(AUTOSAVE)).unwrap();
            let probed = std::cell::Cell::new(false);
            let result = validate_for(
                Platform::Linux,
                &private,
                |key| fixture.get(key),
                Some(&fixture.exe),
                |_| {
                    probed.set(true);
                    Ok(())
                },
            );
            assert!(result.is_err(), "autosave must not escape the private profile or follow a dangling alias");
            assert!(!probed.get());
        }
        let private = fixture.root.join("nightly-self");
        std::fs::create_dir(&private).unwrap();
        std::os::unix::fs::symlink(&private, private.join(AUTOSAVE)).unwrap();
        assert!(fixture.check(Platform::Linux, &private).is_err(), "autosave cannot treat all profile settings as recovery entries");
        assert_eq!(std::fs::read(stable.join("stable-autosave")).unwrap(), b"stable recovery bytes");
        assert_eq!(std::fs::read_dir(stable).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn recovery_alias_to_a_private_subfolder_keeps_the_profile_usable() {
        let fixture = Fixture::new();
        let private = fixture.root.join("nightly-private");
        let recovery = private.join("private-recovery");
        std::fs::create_dir_all(&recovery).unwrap();
        std::os::unix::fs::symlink(&recovery, private.join(AUTOSAVE)).unwrap();
        assert!(fixture.check(Platform::Linux, &private).is_ok());
        assert_eq!(std::fs::canonicalize(private.join(AUTOSAVE)).unwrap(), recovery);
    }

    #[test]
    fn overlap_check_handles_ancestor_descendant_and_prefix_lookalikes() {
        let fixture = Fixture::new();
        let root = fixture.home().join("nightly/profiles/cadcraft");
        for platform in [Platform::Windows, Platform::Linux, Platform::Mac] {
            assert!(overlaps(&root, &root.join(AUTOSAVE), platform));
            assert!(overlaps(&root.join(AUTOSAVE), &root, platform));
            assert!(!overlaps(&root, &root.with_file_name("cadcraft-other"), platform));
        }
    }

    #[test]
    fn linux_fences_app_xdg_subtrees_but_allows_launcher_sibling() {
        let mut fixture = Fixture::new();
        for key in ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"] {
            fixture.env.insert(key.into(), fixture.root.join(key).into_os_string());
        }
        for key in ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"] {
            let stable = PathBuf::from(fixture.get(key).unwrap()).join("cadcraft");
            std::fs::create_dir_all(&stable).unwrap();
            std::fs::write(stable.join("stable-sentinel"), b"stable bytes").unwrap();
            for candidate in [stable.clone(), stable.join("nightly"), stable.parent().unwrap().to_path_buf()] {
                assert!(fixture.check(Platform::Linux, &candidate).unwrap_err().contains("overlap"));
            }
            assert_eq!(std::fs::read(stable.join("stable-sentinel")).unwrap(), b"stable bytes");
        }
        let data = PathBuf::from(fixture.get("XDG_DATA_HOME").unwrap());
        // Stable's eframe storage folder holds `app.ron`, so a profile may not sit at or inside it.
        let storage = data.join(EFRAME_APP_ID);
        std::fs::create_dir_all(&storage).unwrap();
        std::fs::write(storage.join("app.ron"), b"stable window state").unwrap();
        for candidate in [storage.clone(), storage.join("nightly")] {
            assert!(fixture.check(Platform::Linux, &candidate).unwrap_err().contains("overlap"), "{}", candidate.display());
        }
        assert_eq!(std::fs::read(storage.join("app.ron")).unwrap(), b"stable window state");
        let nightly = data.join("ArtCraftLauncher/nightly/profiles/cadcraft");
        assert_eq!(fixture.check(Platform::Linux, &nightly).unwrap(), nightly);
        assert_eq!(std::fs::read_dir(nightly).unwrap().count(), 0);
    }

    #[test]
    fn mac_fences_only_app_library_subtrees_and_the_entire_bundle() {
        let mut fixture = Fixture::new();
        fixture.exe = fixture.root.join("install/CADCraft Nightly.app/Contents/MacOS/CADCraft");
        std::fs::create_dir_all(fixture.exe.parent().unwrap()).unwrap();
        std::fs::write(&fixture.exe, b"fixture executable, never run").unwrap();
        let library = fixture.home().join("Library");
        for suffix in [
            "Application Support/CADCraft",
            "Application Support/ai.storyteller.cadcraft",
            "Caches/CADCraft",
            "Caches/ai.storyteller.cadcraft",
            "Preferences/CADCraft",
            "Saved Application State/ai.storyteller.cadcraft.savedState",
        ] {
            let stable = library.join(suffix);
            assert!(fixture.check(Platform::Mac, &stable.join("nightly")).unwrap_err().contains("overlap"));
            assert!(!stable.exists());
        }
        let inside_resources = fixture.exe.parent().unwrap().parent().unwrap().join("Resources/profile");
        assert!(fixture.check(Platform::Mac, &inside_resources).unwrap_err().contains("overlap"));
        assert!(!inside_resources.exists());
        let valid = library.join("Application Support/ArtCraftLauncher/nightly/profiles/cadcraft");
        assert_eq!(fixture.check(Platform::Mac, &valid).unwrap(), valid);
        // The bundle path and the Library fences fold case on macOS.
        let cased = library.join("application support/cadcraft/nightly");
        assert!(fixture.check(Platform::Mac, &cased).is_err());
    }

    #[test]
    fn windows_fences_roaming_local_and_both_install_folders() {
        let mut fixture = Fixture::new();
        fixture.env.insert("ProgramFiles".into(), fixture.root.join("program files").into_os_string());
        for stable in [
            fixture.root.join("roaming/CADCraft"),
            fixture.root.join("roaming/ai.storyteller.cadcraft"),
            fixture.root.join("local/CADCraft"),
            fixture.root.join("program files/ArtCraft/CADCraft"),
        ] {
            std::fs::create_dir_all(&stable).unwrap();
            std::fs::write(stable.join("stable-sentinel"), b"stable bytes").unwrap();
            for candidate in [stable.clone(), stable.join("nightly")] {
                assert!(fixture.check(Platform::Windows, &candidate).unwrap_err().contains("overlap"), "{}", candidate.display());
            }
            // Windows folder names ignore case.
            assert!(fixture.check(Platform::Windows, &stable.with_file_name("cadcraft")).is_err());
            assert_eq!(std::fs::read(stable.join("stable-sentinel")).unwrap(), b"stable bytes");
        }
        let valid = fixture.root.join("local/ArtCraftLauncher/nightly/profiles/cadcraft");
        assert_eq!(fixture.check(Platform::Windows, &valid).unwrap(), valid);
    }

    #[test]
    fn config_dir_and_flatpak_are_refused() {
        let mut fixture = Fixture::new();
        fixture.env.insert("CADCRAFT_CONFIG_DIR".into(), fixture.root.join("custom").into_os_string());
        let private = fixture.root.join("nightly-config");
        assert!(fixture.check(Platform::Linux, &private).unwrap_err().contains("CADCRAFT_CONFIG_DIR"));
        assert!(!private.exists());
        fixture.env.remove("CADCRAFT_CONFIG_DIR");
        for inherited in ["ai.storyteller.cadcraft", "org.freedesktop.other"] {
            fixture.env.insert("FLATPAK_ID".into(), inherited.into());
            assert!(fixture.check(Platform::Linux, &private).unwrap_err().contains("Flatpak"), "{inherited}");
            assert!(!private.exists());
        }
    }

    /// The nightly bundle runs under an app id of its own, so its sandbox data folder is this
    /// channel's data root; that one instance is accepted, and its profile stays inside it.
    #[test]
    fn the_nightly_flatpaks_own_instance_accepts_its_own_profile() {
        let mut fixture = Fixture::new();
        fixture.env.insert("FLATPAK_ID".into(), NIGHTLY_FLATPAK_ID.into());
        let private = fixture.home().join(".var/app/ai.storyteller.cadcraft.nightly/data/cadcraft-nightly");
        assert_eq!(fixture.check(Platform::Linux, &private).unwrap(), private);
        // Stable's Flatpak folder is still fenced even inside the nightly instance.
        let stable = fixture.home().join(".var/app/ai.storyteller.cadcraft");
        assert!(fixture.check(Platform::Linux, &stable).unwrap_err().contains("overlap"));
    }

    /// A Linux Launcher wrapper exports `CADCRAFT_CONFIG_DIR` at the profile it launches; that is
    /// the folder this launch already owns, so it must not refuse the profile. Anything else is
    /// still a redirect.
    #[test]
    fn a_wrapper_naming_the_profile_in_config_dir_is_accepted() {
        let mut fixture = Fixture::new();
        let private = fixture.root.join("nightly-wrapper");
        fixture.env.insert("CADCRAFT_CONFIG_DIR".into(), private.clone().into_os_string());
        assert_eq!(fixture.check(Platform::Linux, &private).unwrap(), private);
        let inside = private.join("nested");
        fixture.env.insert("CADCRAFT_CONFIG_DIR".into(), inside.clone().into_os_string());
        assert!(fixture.check(Platform::Linux, &inside).is_ok(), "a folder inside the profile is the profile's own");
        fixture.env.insert("CADCRAFT_CONFIG_DIR".into(), fixture.root.join("elsewhere").into_os_string());
        assert!(fixture.check(Platform::Linux, &private).unwrap_err().contains("CADCRAFT_CONFIG_DIR"));
    }

    #[test]
    fn a_profile_that_cannot_be_written_is_refused_without_a_stable_fallback() {
        let fixture = Fixture::new();
        let private = fixture.root.join("nightly-unwritable");
        let result = validate_for(Platform::Linux, &private, |key| fixture.get(key), Some(&fixture.exe), |_| Err(std::io::Error::other("read-only")));
        assert!(result.unwrap_err().contains("not writable"));
        assert!(private.exists(), "the profile was created before the probe, and nothing was written into stable");
        assert!(!fixture.home().join(".config/cadcraft").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_entry_inside_the_profile_is_refused_before_writes() {
        let fixture = Fixture::new();
        let outside = fixture.root.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let private = fixture.root.join("nightly-dangling");
        std::fs::create_dir(&private).unwrap();
        std::os::unix::fs::symlink(outside.join("missing"), private.join("logs")).unwrap();
        let probed = std::cell::Cell::new(false);
        let result = validate_for(
            Platform::Linux,
            &private,
            |key| fixture.get(key),
            Some(&fixture.exe),
            |_| {
                probed.set(true);
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(!probed.get());
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    }
}
