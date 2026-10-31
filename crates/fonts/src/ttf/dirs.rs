//! Where installed fonts live on Linux and the BSDs, the way fontconfig finds them: the XDG base
//! directories, the `<dir>` entries of fontconfig's configuration (NixOS and Guix keep each font
//! package in its own store path, nowhere near `/usr/share/fonts`) and the host fonts a Flatpak
//! sandbox mounts. Directories only: fontconfig's aliases and matching rules are not read.
//!
//! The resolver is a pure function of the environment and a configuration reader, so its tests
//! run the same on every machine.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Bounds on a configuration a package or a user may have made hostile: files read, `<include>`
/// nesting, bytes per file, elements per file, entries per path list, directories returned.
const MAX_CONFIG_FILES: usize = 256;
const MAX_INCLUDE_DEPTH: usize = 4;
const MAX_CONFIG_BYTES: u64 = 1 << 20;
const MAX_ELEMENTS: usize = 4096;
const MAX_PATH_LIST: usize = 64;
const MAX_DIRS: usize = 1024;

/// Where Flatpak mounts the host's system, local and user font directories in a sandbox (it also
/// lists them in `/run/host/font-dirs.xml`, which the runtime's `fonts.conf` includes).
const FLATPAK_FONT_DIRS: &[&str] = &["/run/host/fonts", "/run/host/local-fonts", "/run/host/user-fonts"];

/// What an `<include>` names: a configuration file's text, or the `[0-9]*.conf` files of a
/// directory, sorted, as fontconfig reads them.
pub(super) enum Include {
    File(String),
    Dir(Vec<PathBuf>),
}

/// Font directories in priority order, without duplicates: the user's XDG font directories,
/// `$XDG_DATA_DIRS/fonts` (`/usr/local/share/fonts` and `/usr/share/fonts` whether listed or
/// not), Flatpak's host font mounts, then the `<dir>` entries of fontconfig's configuration
/// (`$FONTCONFIG_FILE`, else `/etc/fonts/fonts.conf`, following its `<include>`s). `env` reads an
/// environment variable, `read` a configuration file or directory (`None` when missing).
pub(super) fn font_dirs(env: impl Fn(&str) -> Option<OsString>, read: impl Fn(&Path) -> Option<Include>) -> Vec<PathBuf> {
    let var = |k: &str| env(k).filter(|v| !v.is_empty());
    let home = var("HOME").map(PathBuf::from);
    let data_home = var("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".local/share")));
    let config_home = var("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".config")));
    let list = |k: &str, default: &str| -> Vec<PathBuf> {
        std::env::split_paths(&var(k).unwrap_or_else(|| default.into())).filter(|p| p.is_absolute()).take(MAX_PATH_LIST).collect()
    };
    let mut user: Vec<PathBuf> = data_home.iter().map(|d| d.join("fonts")).chain(home.iter().map(|h| h.join(".fonts"))).collect();
    user.extend(list("XDG_DATA_DIRS", "/usr/local/share:/usr/share").into_iter().map(|d| d.join("fonts")));
    user.extend(["/usr/local/share/fonts", "/usr/share/fonts"].iter().chain(FLATPAK_FONT_DIRS).map(PathBuf::from));
    let mut r = Resolver {
        home,
        data_home,
        config_home,
        config_path: list("FONTCONFIG_PATH", "/etc/fonts"),
        read: &read,
        out: Vec::new(),
        visited: HashSet::new(),
    };
    for d in user {
        r.add(d);
    }
    r.include(&var("FONTCONFIG_FILE").map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| "/etc/fonts/fonts.conf".into()), None, 0);
    r.out
}

struct Resolver<'a> {
    home: Option<PathBuf>,
    data_home: Option<PathBuf>,
    config_home: Option<PathBuf>,
    /// Where a relative `<include>` name is looked up: `$FONTCONFIG_PATH`, else `/etc/fonts`.
    config_path: Vec<PathBuf>,
    read: &'a dyn Fn(&Path) -> Option<Include>,
    out: Vec<PathBuf>,
    /// Configuration files and directories read: cycles and the file bound.
    visited: HashSet<PathBuf>,
}

impl Resolver<'_> {
    fn add(&mut self, p: PathBuf) {
        if p.is_absolute() && self.out.len() < MAX_DIRS && !self.out.contains(&p) {
            self.out.push(p);
        }
    }

    /// A path with a leading `~` meaning `$HOME`.
    fn expand_home(&self, s: &str) -> Option<PathBuf> {
        match s.strip_prefix('~') {
            Some(rest) => self.home.as_ref().map(|h| h.join(rest.trim_start_matches('/'))),
            None => Some(PathBuf::from(s)),
        }
    }

    /// Follow an `<include>` (or the top-level file): an absolute name, or a relative one looked
    /// up in the configuration path. `here` is the including file's directory.
    fn include(&mut self, name: &str, here: Option<&Path>, depth: usize) {
        match self.expand_home(name) {
            Some(p) if p.is_absolute() => {
                self.config(&p, depth);
            }
            Some(p) => {
                let bases: Vec<PathBuf> = if here.is_some() { here.map(Path::to_path_buf).into_iter().collect() } else { self.config_path.clone() };
                for base in bases {
                    if self.config(&base.join(&p), depth) {
                        break;
                    }
                }
            }
            None => {}
        }
    }

    /// Read a configuration file (or every file of a directory) and its includes, nested at most
    /// `MAX_INCLUDE_DEPTH` deep. True when something was read.
    fn config(&mut self, file: &Path, depth: usize) -> bool {
        if depth > MAX_INCLUDE_DEPTH || self.visited.len() >= MAX_CONFIG_FILES || !self.visited.insert(file.to_path_buf()) {
            return false;
        }
        let xml = match (self.read)(file) {
            Some(Include::File(xml)) => xml,
            Some(Include::Dir(files)) => {
                for f in files.iter().take(MAX_CONFIG_FILES) {
                    self.config(f, depth);
                }
                return true;
            }
            None => return false,
        };
        let here = file.parent().map(Path::to_path_buf);
        for e in elements(&xml) {
            match (e.name, e.prefix) {
                ("dir", Some("xdg")) => {
                    if let Some(d) = self.data_home.as_ref().map(|d| d.join(&e.text)) {
                        self.add(d);
                    }
                }
                ("dir", Some("relative")) => {
                    if let Some(d) = here.as_ref().map(|h| h.join(&e.text)) {
                        self.add(d);
                    }
                }
                // "default", "cwd" or none: as written; a relative directory is skipped.
                ("dir", _) => {
                    if let Some(d) = self.expand_home(&e.text) {
                        self.add(d);
                    }
                }
                ("include", Some("xdg")) => {
                    if let Some(f) = self.config_home.as_ref().map(|c| c.join(&e.text)) {
                        self.config(&f, depth + 1);
                    }
                }
                ("include", Some("relative")) => self.include(&e.text, here.as_deref(), depth + 1),
                ("include", _) => self.include(&e.text, None, depth + 1),
                _ => {}
            }
        }
        true
    }
}

struct Element<'a> {
    name: &'a str,
    prefix: Option<&'a str>,
    text: String,
}

/// The `<dir>` and `<include>` elements of a configuration file with text, in order, with their
/// `prefix` attribute; comments are skipped. A bounded scan over the text: fontconfig files are
/// small and regular.
fn elements(xml: &str) -> Vec<Element<'_>> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find('<') {
        let Some(tag) = rest.get(i + 1..) else { break };
        if let Some(c) = tag.strip_prefix("!--") {
            rest = c.find("-->").and_then(|j| c.get(j + 3..)).unwrap_or("");
            continue;
        }
        let Some(end) = tag.find('>') else { break };
        let (head, after) = (tag.get(..end).unwrap_or(""), tag.get(end + 1..).unwrap_or(""));
        rest = after;
        let (name, attrs) = head.split_once(char::is_whitespace).unwrap_or((head, ""));
        if !matches!(name, "dir" | "include") {
            continue;
        }
        let prefix = attribute(attrs, "prefix");
        let text = unescape(after.split('<').next().unwrap_or("").trim());
        if !text.is_empty() {
            out.push(Element { name, prefix, text });
        }
        if out.len() >= MAX_ELEMENTS {
            break;
        }
    }
    out
}

/// The value of the attribute `name` in a start tag's attribute list: name, optional whitespace,
/// `=`, optional whitespace, a single- or double-quoted value, in any order. `None` when absent
/// or not well-formed up to it.
fn attribute<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = attrs;
    loop {
        rest = rest.trim_start();
        let key_end = rest.find(|c: char| c == '=' || c.is_whitespace())?;
        let key = rest.get(..key_end)?;
        rest = rest.get(key_end..)?.trim_start().strip_prefix('=')?.trim_start();
        let quote = rest.chars().next().filter(|c| matches!(c, '"' | '\''))?;
        rest = rest.get(1..)?;
        let value_end = rest.find(quote)?;
        let value = rest.get(..value_end)?;
        if key == name {
            return Some(value);
        }
        rest = rest.get(value_end + 1..)?;
    }
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// Read a configuration file (skipped beyond `MAX_CONFIG_BYTES`) or list a directory's
/// `[0-9]*.conf` files, sorted.
pub(super) fn read_config(path: &Path) -> Option<Include> {
    use std::io::Read as _;
    if path.is_dir() {
        let conf =
            |p: &Path| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(|c: char| c.is_ascii_digit()) && n.ends_with(".conf"));
        let mut files: Vec<PathBuf> = std::fs::read_dir(path).ok()?.flatten().map(|e| e.path()).filter(|p| conf(p)).take(MAX_CONFIG_FILES).collect();
        files.sort();
        return Some(Include::Dir(files));
    }
    let mut xml = String::new();
    std::fs::File::open(path).ok()?.take(MAX_CONFIG_BYTES.saturating_add(1)).read_to_string(&mut xml).ok()?;
    (xml.len() as u64 <= MAX_CONFIG_BYTES).then_some(Include::File(xml))
}

/// The tests build configurations from POSIX paths such as `/usr/share/fonts`, which only a Unix
/// `Path` takes as absolute, so they run on Unix only; the resolver itself builds everywhere.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn env_of<'a>(vars: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |k| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v))
    }

    fn reader<'a>(files: &'a [(&str, &str)], dirs: &'a [(&str, &[&str])]) -> impl Fn(&Path) -> Option<Include> + 'a {
        move |p| {
            if let Some((_, xml)) = files.iter().find(|(n, _)| Path::new(n) == p) {
                return Some(Include::File((*xml).to_string()));
            }
            dirs.iter().find(|(n, _)| Path::new(n) == p).map(|(_, fs)| Include::Dir(fs.iter().map(PathBuf::from).collect()))
        }
    }

    fn paths(v: &[&str]) -> Vec<PathBuf> {
        v.iter().map(PathBuf::from).collect()
    }

    const XDG: &[&str] = &["/usr/local/share/fonts", "/usr/share/fonts", "/run/host/fonts", "/run/host/local-fonts", "/run/host/user-fonts"];

    #[test]
    fn xdg_defaults_without_variables() {
        let d = font_dirs(env_of(&[("HOME", "/home/u")]), |_| None);
        assert_eq!(d, paths(&[&["/home/u/.local/share/fonts", "/home/u/.fonts"], XDG].concat()));
        assert_eq!(font_dirs(|_| None, |_| None), paths(XDG), "no HOME: system directories only");
    }

    #[test]
    fn xdg_variables_name_the_font_dirs() {
        let env = env_of(&[
            ("HOME", "/home/u"),
            ("XDG_DATA_HOME", "/data"),
            ("XDG_DATA_DIRS", "/nix/profile/share:/run/current-system/sw/share:relative:/usr/share/:"),
        ]);
        let d = font_dirs(env, |_| None);
        assert_eq!(
            d.get(..6),
            Some(
                paths(&[
                    "/data/fonts",
                    "/home/u/.fonts",
                    "/nix/profile/share/fonts",
                    "/run/current-system/sw/share/fonts",
                    "/usr/share/fonts",
                    "/usr/local/share/fonts"
                ])
                .as_slice()
            )
        );
        assert_eq!(d.len(), 9, "{d:?}");
    }

    #[test]
    fn fontconfig_dirs_nixos_style() {
        let files = [
            (
                "/etc/fonts/fonts.conf",
                "<?xml version=\"1.0\"?>\n<!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\">\n<fontconfig>\n\
                 <!-- Font directory list -->\n<cachedir prefix=\"xdg\">fontconfig</cachedir>\n\
                 <include ignore_missing=\"yes\">/etc/fonts/conf.d</include>\n<dir prefix=\"xdg\">fonts</dir>\n\
                 <dir>/nix/store/aaa-dejavu-fonts-minimal-2.37</dir>\n</fontconfig>\n",
            ),
            (
                "/etc/fonts/conf.d/00-nixos-cache.conf",
                "<fontconfig>\n  <!-- Font directories -->\n  <dir>/nix/store/bbb-dejavu-fonts-2.37</dir>\n<dir salt=\"x\">/nix/store/ccc-unifont-16.0.03</dir>\n\
                 <!-- <dir>/commented/out</dir> -->\n  <cachedir>/nix/store/ddd-fc-cache</cachedir>\n</fontconfig>\n",
            ),
            (
                "/etc/fonts/conf.d/50-user.conf",
                "<fontconfig>\n<include ignore_missing=\"yes\" prefix=\"xdg\">fontconfig/conf.d</include>\n</fontconfig>\n",
            ),
            (
                "/home/u/.config/fontconfig/conf.d/10-hm-fonts.conf",
                "<fontconfig>\n<include ignore_missing=\"yes\">/nix/store/eee-home-manager-path/etc/fonts/conf.d</include>\n\
                 <dir>/nix/store/eee-home-manager-path/share/fonts</dir>\n<dir>/etc/profiles/per-user/u/share/fonts</dir>\n</fontconfig>\n",
            ),
        ];
        let dirs: [(&str, &[&str]); 2] = [
            ("/etc/fonts/conf.d", &["/etc/fonts/conf.d/00-nixos-cache.conf", "/etc/fonts/conf.d/50-user.conf"]),
            ("/home/u/.config/fontconfig/conf.d", &["/home/u/.config/fontconfig/conf.d/10-hm-fonts.conf"]),
        ];
        let d = font_dirs(env_of(&[("HOME", "/home/u"), ("XDG_DATA_DIRS", "/etc/profiles/per-user/u/share")]), reader(&files, &dirs));
        let expect = [
            "/home/u/.local/share/fonts",
            "/home/u/.fonts",
            "/etc/profiles/per-user/u/share/fonts",
            "/usr/local/share/fonts",
            "/usr/share/fonts",
            "/run/host/fonts",
            "/run/host/local-fonts",
            "/run/host/user-fonts",
            "/nix/store/bbb-dejavu-fonts-2.37",
            "/nix/store/ccc-unifont-16.0.03",
            "/nix/store/eee-home-manager-path/share/fonts",
            "/nix/store/aaa-dejavu-fonts-minimal-2.37",
        ];
        assert_eq!(d, paths(&expect));
    }

    #[test]
    fn fontconfig_dirs_debian_style() {
        let files = [
            (
                "/etc/fonts/fonts.conf",
                "<fontconfig>\n<dir>/usr/share/fonts</dir>\n<dir>/usr/local/share/fonts</dir>\n<dir prefix=\"xdg\">fonts</dir>\n\
                 <dir>~/.fonts</dir>\n<dir>/opt/fonts &amp; more</dir>\n<dir>relative/skipped</dir>\n<dir prefix=\"cwd\">also/skipped</dir>\n\
                 <include ignore_missing=\"yes\">conf.d</include>\n</fontconfig>\n",
            ),
            ("/etc/fonts/conf.d/51-local.conf", "<fontconfig><include ignore_missing=\"yes\">local.conf</include></fontconfig>"),
            ("/etc/fonts/conf.d/52-site.conf", "<fontconfig><include prefix=\"relative\">../site/fonts.conf</include></fontconfig>"),
            ("/etc/fonts/local.conf", "<fontconfig><dir>/srv/fonts</dir><dir prefix=\"relative\">more</dir></fontconfig>"),
            ("/etc/fonts/conf.d/../site/fonts.conf", "<fontconfig><dir>/site/fonts</dir></fontconfig>"),
        ];
        let dirs: [(&str, &[&str]); 1] = [("/etc/fonts/conf.d", &["/etc/fonts/conf.d/51-local.conf", "/etc/fonts/conf.d/52-site.conf"])];
        let d = font_dirs(env_of(&[("HOME", "/home/u")]), reader(&files, &dirs));
        let tail: Vec<PathBuf> = d.iter().skip(7).cloned().collect();
        assert_eq!(tail, paths(&["/opt/fonts & more", "/srv/fonts", "/etc/fonts/more", "/site/fonts"]));
        assert_eq!(d.len(), 11, "the directories already listed are not repeated: {d:?}");
    }

    #[test]
    fn fontconfig_file_and_path_variables() {
        let files = [("/cfg/custom.conf", "<fontconfig><dir>/custom/fonts</dir></fontconfig>")];
        let env = env_of(&[("FONTCONFIG_FILE", "custom.conf"), ("FONTCONFIG_PATH", "/nowhere:/cfg")]);
        let d = font_dirs(env, reader(&files, &[]));
        assert_eq!(d.last(), Some(&PathBuf::from("/custom/fonts")));
        let d = font_dirs(env_of(&[("FONTCONFIG_FILE", "/cfg/custom.conf")]), reader(&files, &[]));
        assert_eq!(d.last(), Some(&PathBuf::from("/custom/fonts")));
        assert_eq!(font_dirs(env_of(&[("FONTCONFIG_FILE", "/cfg/missing.conf")]), reader(&files, &[])), paths(XDG));
    }

    #[test]
    fn prefix_attributes_with_whitespace_around_equals() {
        // Legal XML: whitespace, newlines included, on either side of '=', attributes in any
        // order. /cfg is not on the configuration path, so a relative include or directory is
        // found only through its prefix.
        let files = [
            (
                "/cfg/fonts.conf",
                "<fontconfig>\n<dir prefix = \"relative\">extra-fonts</dir>\n<dir\n  prefix\n  =\n  'xdg'\n>more-fonts</dir>\n\
                 <include ignore_missing = \"yes\" prefix = \"xdg\">fontconfig/fonts.conf</include>\n\
                 <include\n\tignore_missing=\"yes\"\n\tprefix\n\t=\n\t\"relative\"\n>site.conf</include>\n</fontconfig>\n",
            ),
            ("/home/u/.config/fontconfig/fonts.conf", "<fontconfig><dir>/xdg-included</dir></fontconfig>"),
            ("/cfg/site.conf", "<fontconfig><dir>/site-included</dir></fontconfig>"),
        ];
        let env = env_of(&[("HOME", "/home/u"), ("XDG_DATA_HOME", "/data"), ("FONTCONFIG_FILE", "/cfg/fonts.conf")]);
        let d = font_dirs(env, reader(&files, &[]));
        let tail: Vec<PathBuf> = d.iter().skip(XDG.len() + 2).cloned().collect();
        assert_eq!(tail, paths(&["/cfg/extra-fonts", "/data/more-fonts", "/xdg-included", "/site-included"]));
    }

    #[test]
    fn include_cycles_and_depth_are_bounded() {
        let files = [
            ("/etc/fonts/fonts.conf", "<fontconfig><dir>/d0</dir><include>/etc/fonts/b.conf</include></fontconfig>"),
            (
                "/etc/fonts/b.conf",
                "<fontconfig><dir>/d1</dir><include>/etc/fonts/fonts.conf</include><include>/etc/fonts/b.conf</include></fontconfig>",
            ),
        ];
        let d = font_dirs(|_| None, reader(&files, &[]));
        assert_eq!(d.iter().skip(XDG.len()).cloned().collect::<Vec<_>>(), paths(&["/d0", "/d1"]));

        let chain: Vec<(String, String)> = (0..10)
            .map(|i| {
                (format!("/etc/fonts/{i}.conf"), format!("<fontconfig><dir>/depth{i}</dir><include>/etc/fonts/{}.conf</include></fontconfig>", i + 1))
            })
            .collect();
        let read = |p: &Path| chain.iter().find(|(n, _)| Path::new(n) == p).map(|(_, xml)| Include::File(xml.clone()));
        let d = font_dirs(env_of(&[("FONTCONFIG_FILE", "/etc/fonts/0.conf")]), read);
        let depths: Vec<&PathBuf> = d.iter().skip(XDG.len()).collect();
        assert_eq!(depths.len(), MAX_INCLUDE_DEPTH + 1, "{depths:?}");
        assert_eq!(depths.last().map(|p| p.as_path()), Some(Path::new("/depth4")));
    }

    #[test]
    fn many_files_are_bounded() {
        let names: Vec<String> = (0..1000).map(|i| format!("/etc/fonts/conf.d/{i:04}.conf")).collect();
        let read = |p: &Path| {
            if p == Path::new("/etc/fonts/fonts.conf") {
                return Some(Include::File("<fontconfig><include>/etc/fonts/conf.d</include></fontconfig>".into()));
            }
            if p == Path::new("/etc/fonts/conf.d") {
                return Some(Include::Dir(names.iter().map(PathBuf::from).collect()));
            }
            p.starts_with("/etc/fonts/conf.d").then(|| Include::File(format!("<fontconfig><dir>{}</dir></fontconfig>", p.display())))
        };
        let d = font_dirs(|_| None, read);
        assert!(d.len() <= XDG.len() + MAX_CONFIG_FILES, "{}", d.len());
        assert!(d.len() > XDG.len() + 200);
    }

    #[test]
    fn elements_survive_odd_markup() {
        let xml = "<dir><dir>/unterminated<include>/x</include><dir prefix='relative'>r</dir><dir></dir><!-- never closed <dir>/no</dir>";
        let e = elements(xml);
        let got: Vec<(&str, Option<&str>, &str)> = e.iter().map(|e| (e.name, e.prefix, e.text.as_str())).collect();
        assert_eq!(got, [("dir", None, "/unterminated"), ("include", None, "/x"), ("dir", Some("relative"), "r")]);
        assert!(elements("").is_empty());
        assert!(elements("<").is_empty());
        assert!(elements("<dir").is_empty());
        let huge: String = "<dir>/a</dir>".repeat(MAX_ELEMENTS * 2);
        assert_eq!(elements(&huge).len(), MAX_ELEMENTS);
    }

    #[test]
    fn attributes_in_any_order_and_spacing() {
        let prefix = |xml: &str| elements(xml).first().and_then(|e| e.prefix).map(str::to_string);
        assert_eq!(prefix("<dir salt=\"x\" prefix='xdg'>f</dir>"), Some("xdg".into()));
        assert_eq!(prefix("<dir prefix=\"relative\" salt=\"x\">f</dir>"), Some("relative".into()));
        assert_eq!(prefix("<dir salt = 'prefix=\"xdg\"' prefix = \"cwd\">f</dir>"), Some("cwd".into()), "a value is not an attribute");
        assert_eq!(prefix("<include\nignore_missing = \"yes\"\nprefix\n=\n\"xdg\"\n>f</include>"), Some("xdg".into()));
        assert_eq!(prefix("<dir prefix=xdg>f</dir>"), None, "an unquoted value is not well-formed");
        assert_eq!(prefix("<dir prefix>f</dir>"), None);
        assert_eq!(prefix("<dir prefix=\"unterminated>f</dir>"), None);
        assert_eq!(prefix("<dir salt=\"x\">f</dir>"), None);
    }

    #[test]
    fn read_config_skips_oversize_files_and_lists_conf_files() {
        let dir = std::env::temp_dir().join(format!(
            "cadcraft-fontdirs-{}-{}",
            std::process::id(),
            std::time::UNIX_EPOCH.elapsed().map(|d| d.as_nanos()).unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("10-small.conf"), "<fontconfig><dir>/small</dir></fontconfig>").unwrap();
        std::fs::write(dir.join("05-first.conf"), "<fontconfig/>").unwrap();
        std::fs::write(dir.join("README"), "not a config").unwrap();
        std::fs::write(dir.join("local.conf"), "<fontconfig/>").unwrap();
        std::fs::write(dir.join("99-big.conf"), vec![b' '; (MAX_CONFIG_BYTES + 1) as usize]).unwrap();
        match read_config(&dir) {
            Some(Include::Dir(files)) => assert_eq!(files, [dir.join("05-first.conf"), dir.join("10-small.conf"), dir.join("99-big.conf")]),
            _ => panic!("a directory lists its [0-9]*.conf files"),
        }
        assert!(matches!(read_config(&dir.join("10-small.conf")), Some(Include::File(x)) if x.contains("/small")));
        assert!(read_config(&dir.join("99-big.conf")).is_none(), "oversize file skipped");
        assert!(read_config(&dir.join("missing.conf")).is_none());
        let small = dir.join("10-small.conf").to_string_lossy().into_owned();
        let d = font_dirs(env_of(&[("FONTCONFIG_FILE", &small)]), read_config);
        assert_eq!(d.last(), Some(&PathBuf::from("/small")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
