//! `load_dialog` and `unload_dialog` through the Lisp entry point.
//!
//! Fixtures are synthetic DCL. A load failure must leave the drawing's handle store as it was.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::value::to_text;
use crate::Session;

fn eval(s: &mut Session, code: &str) -> Result<String, String> {
    match s.lisp_eval(code, false, true, Vec::new()) {
        Ok(Some(Ok(v))) => Ok(to_text(&v, true)),
        Ok(Some(Err(e))) => Err(e),
        Ok(None) => Err("did not finish".into()),
        Err(e) => Err(e.to_string()),
    }
}

fn ok(s: &mut Session, code: &str) -> String {
    match eval(s, code) {
        Ok(v) => v,
        Err(e) => panic!("{code}: {e}"),
    }
}

fn negative(text: &str) -> i32 {
    let id: i32 = text.parse().unwrap_or_else(|_| panic!("expected an integer, got {text}"));
    assert!(id < 0, "expected a negative integer, got {id}");
    id
}

fn loaded_ids(s: &Session) -> Vec<i32> {
    let uid = s.state().unwrap().uid;
    let mut ids: Vec<i32> = s.lisp.envs.get(&uid).map(|env| env.dialogs.keys().copied().collect()).unwrap_or_default();
    ids.sort_unstable();
    ids
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{eval, loaded_ids, negative, ok};
    use crate::Session;
    use crate::lisp::{dcl, dcl_load, tests::session};

    fn positive(text: &str) -> i32 {
        let id: i32 = text.parse().unwrap_or_else(|_| panic!("expected an integer handle, got {text}"));
        assert!(id > 0, "expected a positive handle, got {id}");
        id
    }

    fn scratch(label: &str) -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("cadcraft-dcl-{label}-{}-{nanos}-{n}", std::process::id()))
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = scratch(label);
            std::fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// A quoted Lisp string for `path`.
    fn lisp_path(path: &Path) -> String {
        let text = path.to_string_lossy();
        let mut out = String::from("\"");
        for c in text.chars() {
            match c {
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                _ => out.push(c),
            }
        }
        out.push('"');
        out
    }

    const TILE: &str = "probe : dialog {\n  label = \"Probe\";\n}\n";

    /// Valid DCL of exactly `bytes` length. The padding sits in one block comment so it stays inside
    /// the parser's string and token caps.
    fn valid_dcl_of_len(bytes: usize) -> Vec<u8> {
        let head = b"/*";
        let tail = b"*/\nprobe : dialog {\n  label = \"ok\";\n}\n";
        assert!(bytes >= head.len() + tail.len());
        let mut out = vec![b'x'; bytes];
        out[..head.len()].copy_from_slice(head);
        out[bytes - tail.len()..].copy_from_slice(tail);
        out
    }

    fn definition_names(s: &Session, id: i32) -> Vec<String> {
        let uid = s.state().unwrap().uid;
        let Some(dialog) = s.lisp.envs.get(&uid).and_then(|env| env.dialogs.get(&id)) else { return Vec::new() };
        let mut names = Vec::new();
        for file in &dialog.files {
            for item in &file.parsed.items {
                if let dcl::Item::Definition(definition) = item {
                    names.push(definition.name.clone());
                }
            }
        }
        names
    }

    #[test]
    fn dcl_file_load_and_unload_return_the_documented_values() {
        let mut s = session();
        let dir = TempDir::new("life");
        let first = write(&dir, "first.dcl", TILE.as_bytes());
        let second = write(&dir, "second.dcl", b"other : dialog {\n  label = \"Other\";\n}\n");
        let a = positive(&ok(&mut s, &format!("(load_dialog {})", lisp_path(&first))));
        let b = positive(&ok(&mut s, &format!("(load_dialog {})", lisp_path(&second))));
        assert_ne!(a, b);
        let mut expect = vec![a, b];
        expect.sort_unstable();
        assert_eq!(loaded_ids(&s), expect);
        assert_eq!(ok(&mut s, &format!("(unload_dialog {a})")), "nil");
        assert_eq!(loaded_ids(&s), vec![b], "unload removes only the selected handle");
        assert_eq!(ok(&mut s, &format!("(unload_dialog {a})")), "nil");
        assert_eq!(loaded_ids(&s), vec![b]);
        assert_eq!(ok(&mut s, &format!("(unload_dialog {b})")), "nil");
        assert!(loaded_ids(&s).is_empty());
    }

    #[test]
    fn dcl_file_load_finds_a_name_without_an_extension() {
        let mut s = session();
        let dir = TempDir::new("support");
        write(&dir, "cadcraft_probe_widget.dcl", TILE.as_bytes());
        s.set_lisp_support_dir(Some(dir.path().to_path_buf()));
        let id = positive(&ok(&mut s, "(load_dialog \"cadcraft_probe_widget\")"));
        assert_eq!(loaded_ids(&s), vec![id]);
        assert_eq!(ok(&mut s, &format!("(unload_dialog {id})")), "nil");

        let bare = TempDir::new("bare");
        write(&bare, "cadcraft_probe_bare", TILE.as_bytes());
        s.set_lisp_support_dir(Some(bare.path().to_path_buf()));
        negative(&ok(&mut s, "(load_dialog \"cadcraft_probe_bare\")"));
        assert!(loaded_ids(&s).is_empty());
    }

    #[test]
    fn dcl_file_load_honors_automation_read_root() {
        let mut s = session();
        let root = TempDir::new("automation-root");
        let other = TempDir::new("automation-other");
        let allowed = write(&root, "allowed.dcl", TILE.as_bytes());
        let refused = write(&other, "refused.dcl", TILE.as_bytes());
        let source = format!("@include {}\nhost : dialog {{ ok_only; }}\n", lisp_path(&refused));
        let including = write(&root, "including.dcl", source.as_bytes());
        let roots = crate::file_access::AutomationRoots::new(Some(root.path()), None).unwrap();

        crate::file_access::confine(roots.as_ref(), || {
            let id = positive(&ok(&mut s, &format!("(load_dialog {})", lisp_path(&allowed))));
            negative(&ok(&mut s, &format!("(load_dialog {})", lisp_path(&refused))));
            negative(&ok(&mut s, &format!("(load_dialog {})", lisp_path(&including))));
            assert_eq!(loaded_ids(&s), vec![id]);
        });
    }

    #[test]
    fn dcl_file_load_resolves_a_relative_include() {
        let mut s = session();
        let beside = TempDir::new("beside");
        let child_name = "cadcraft_probe_child.dcl";
        write(&beside, child_name, TILE.as_bytes());
        let parent = format!("@include \"{child_name}\"\nhost : dialog {{\n  label = \"Host\";\n}}\n");
        let parent_path = write(&beside, "host.dcl", parent.as_bytes());
        let id = positive(&ok(&mut s, &format!("(load_dialog {})", lisp_path(&parent_path))));
        assert_eq!(definition_names(&s, id), vec!["host".to_string(), "probe".to_string()]);

        let abs_dir = TempDir::new("abs");
        let abs_child = write(&abs_dir, "abs-child.dcl", TILE.as_bytes());
        let quoted = abs_child.display().to_string().replace('\\', "\\\\").replace('"', "\\\"");
        let abs_src = format!("@include \"{quoted}\"\nabs : dialog {{\n  label = \"Abs\";\n}}\n");
        let abs_parent = write(&beside, "abs-host.dcl", abs_src.as_bytes());
        let abs_id = positive(&ok(&mut s, &format!("(load_dialog {})", lisp_path(&abs_parent))));

        let mut expect = vec![id, abs_id];
        expect.sort_unstable();
        assert_eq!(loaded_ids(&s), expect);
    }

    #[test]
    fn dcl_file_load_includes_the_exact_file_name() {
        let mut s = session();
        let dir = TempDir::new("include-name");
        let exact = write(&dir, "child", TILE.as_bytes());
        write(&dir, "child.dcl", b"wrong : dialog { ok_only; }\n");
        let source = format!("@include {}\nhost : dialog {{ ok_only; }}\n", lisp_path(&exact));
        let root = write(&dir, "host.dcl", source.as_bytes());

        let id = positive(&ok(&mut s, &format!("(load_dialog {})", lisp_path(&root))));
        assert_eq!(definition_names(&s, id), ["host", "probe"]);
    }

    #[test]
    fn dcl_file_load_counts_repeated_include_requests() {
        let mut s = session();
        let dir = TempDir::new("request-cap");
        write(&dir, "child.dcl", TILE.as_bytes());
        let includes = "@include \"child.dcl\"\n";
        let at_limit = includes.repeat(dcl_load::MAX_FILES - 1);
        let root = write(&dir, "limit.dcl", at_limit.as_bytes());
        let id = positive(&ok(&mut s, &format!("(load_dialog {})", lisp_path(&root))));

        let above_limit = at_limit + includes;
        let excess = write(&dir, "excess.dcl", above_limit.as_bytes());
        negative(&ok(&mut s, &format!("(load_dialog {})", lisp_path(&excess))));
        assert_eq!(loaded_ids(&s), [id]);
    }

    #[test]
    fn dcl_file_load_rejects_bad_files_without_storing_a_handle() {
        let mut s = session();
        assert!(loaded_ids(&s).is_empty());
        let missing = scratch("missing").join("no-such.dcl");
        negative(&ok(&mut s, &format!("(load_dialog {})", lisp_path(&missing))));
        assert!(loaded_ids(&s).is_empty());

        let dir = TempDir::new("bad");
        let broken = write(&dir, "broken.dcl", b"box : dialog {\n  /*");
        negative(&ok(&mut s, &format!("(load_dialog {})", lisp_path(&broken))));
        assert!(loaded_ids(&s).is_empty());

        let huge = vec![b'x'; dcl::MAX_INPUT_BYTES + 1];
        let oversized = write(&dir, "huge.dcl", &huge);
        negative(&ok(&mut s, &format!("(load_dialog {})", lisp_path(&oversized))));
        assert!(loaded_ids(&s).is_empty());

        let left = write(&dir, "left.dcl", b"@include \"right.dcl\"\nleft : dialog {\n  label = \"L\";\n}\n");
        write(&dir, "right.dcl", b"@include \"left.dcl\"\nright : dialog {\n  label = \"R\";\n}\n");
        negative(&ok(&mut s, &format!("(load_dialog {})", lisp_path(&left))));
        assert!(loaded_ids(&s).is_empty(), "a cycle must not leave a live DCL file");

        let big = valid_dcl_of_len(dcl::MAX_INPUT_BYTES);
        assert!(dcl::parse(std::str::from_utf8(&big).unwrap()).is_ok(), "the oversized-include fixture must be valid DCL");
        write(&dir, "big.dcl", &big);
        let host = write(&dir, "with-big.dcl", b"@include \"big.dcl\"\nhost : dialog {\n  label = \"H\";\n}\n");
        negative(&ok(&mut s, &format!("(load_dialog {})", lisp_path(&host))));
        assert!(loaded_ids(&s).is_empty());

        let good = write(&dir, "good.dcl", TILE.as_bytes());
        let id = positive(&ok(&mut s, &format!("(load_dialog {})", lisp_path(&good))));
        assert_eq!(loaded_ids(&s), vec![id], "earlier failures must leave no live DCL file");
    }

    #[test]
    fn dcl_file_load_charges_failed_reads_to_the_run() {
        let mut s = session();
        let dir = TempDir::new("run-budget");
        let mut unclosed_comment = vec![b' '; 768];
        unclosed_comment[..2].copy_from_slice(b"/*");
        let malformed = write(&dir, "malformed.dcl", &unclosed_comment);
        let invalid_utf8 = write(&dir, "invalid-utf8.dcl", &[0xff; 768]);
        let mut missing_include = valid_dcl_of_len(768);
        missing_include.extend_from_slice(b"@include \"./missing-budget-child.dcl\"\n");
        let parent = write(&dir, "missing-child.dcl", &missing_include);
        let oversize = write(&dir, "oversize.dcl", &vec![b' '; dcl_load::MAX_LOAD_BYTES + 1]);

        s.lisp.limits.string_bytes = 1024;
        for path in [malformed, invalid_utf8, parent] {
            let load = format!("(load_dialog {})", lisp_path(&path));
            negative(&ok(&mut s, &load));
            let twice = format!("(progn {load} {load})");
            assert_eq!(eval(&mut s, &twice), Err("out of memory: the routine made too much text".into()));
            assert!(loaded_ids(&s).is_empty());
        }
        let load = format!("(load_dialog {})", lisp_path(&oversize));
        assert_eq!(eval(&mut s, &load), Err("out of memory: the routine made too much text".into()));
        assert!(loaded_ids(&s).is_empty());

        let good = write(&dir, "good.dcl", TILE.as_bytes());
        let id = positive(&ok(&mut s, &format!("(load_dialog {})", lisp_path(&good))));
        assert_eq!(loaded_ids(&s), [id]);
    }

    #[test]
    fn dcl_file_load_checks_depth_of_each_include_path() {
        let mut s = session();
        let dir = TempDir::new("depth-path");
        let leaf = dcl_load::MAX_INCLUDE_DEPTH - 1;
        for step in 1..leaf {
            let include = format!("@include \"depth-chain-{}.dcl\"\n", step + 1);
            write(&dir, &format!("depth-chain-{step}.dcl"), include.as_bytes());
        }
        write(&dir, &format!("depth-chain-{leaf}.dcl"), TILE.as_bytes());

        let short = "@include \"depth-chain-1.dcl\"\n";
        let long = "@include \"depth-wrapper.dcl\"\n";
        write(&dir, "depth-wrapper.dcl", short.as_bytes());
        let at_limit = write(&dir, "at-limit.dcl", short.as_bytes());
        let short_first = write(&dir, "short-first.dcl", format!("{short}{long}").as_bytes());
        let long_first = write(&dir, "long-first.dcl", format!("{long}{short}").as_bytes());

        let id = positive(&ok(&mut s, &format!("(load_dialog {})", lisp_path(&at_limit))));
        assert_eq!(definition_names(&s, id), ["probe"]);
        let short_first_result = ok(&mut s, &format!("(load_dialog {})", lisp_path(&short_first)));
        let long_first_result = ok(&mut s, &format!("(load_dialog {})", lisp_path(&long_first)));
        negative(&short_first_result);
        negative(&long_first_result);
        assert_eq!(loaded_ids(&s), [id]);
    }

    #[test]
    fn dcl_file_load_checks_argument_types() {
        let mut s = session();
        assert_eq!(eval(&mut s, "(load_dialog)"), Err("too few arguments".into()));
        assert_eq!(eval(&mut s, "(load_dialog \"a\" \"b\")"), Err("too many arguments".into()));
        assert_eq!(eval(&mut s, "(load_dialog 1)"), Err("bad argument type: stringp: 1".into()));
        assert_eq!(eval(&mut s, "(unload_dialog)"), Err("too few arguments".into()));
        assert_eq!(eval(&mut s, "(unload_dialog \"x\")"), Err("bad argument type: fixnump: \"x\"".into()));
        assert_eq!(ok(&mut s, "(unload_dialog 1)"), "nil");
        assert!(loaded_ids(&s).is_empty());
    }

    #[test]
    fn dcl_file_handles_are_bounded_per_drawing() {
        let mut s = session();
        let first_drawing = s.active;
        let dir = TempDir::new("drawing-cap");
        let path = write(&dir, "probe.dcl", TILE.as_bytes());
        let load = format!("(load_dialog {})", lisp_path(&path));
        for _ in 0..dcl_load::MAX_DIALOGS {
            positive(&ok(&mut s, &load));
        }
        let handles = loaded_ids(&s);
        assert_eq!(handles.len(), dcl_load::MAX_DIALOGS);
        negative(&ok(&mut s, &load));
        assert_eq!(loaded_ids(&s), handles);

        s.new_drawing(false);
        assert!(loaded_ids(&s).is_empty());
        positive(&ok(&mut s, &load));
        assert_eq!(loaded_ids(&s).len(), 1);

        s.active = first_drawing;
        assert_eq!(loaded_ids(&s), handles);
        assert_eq!(ok(&mut s, &format!("(unload_dialog {})", handles[0])), "nil");
        positive(&ok(&mut s, &load));
        assert_eq!(loaded_ids(&s).len(), dcl_load::MAX_DIALOGS);
    }
}

#[cfg(target_arch = "wasm32")]
#[test]
fn dcl_file_native_lookup_on_the_web_returns_a_negative_integer() {
    let mut s = Session::new();
    negative(&ok(&mut s, "(load_dialog \"/any/dialog.dcl\")"));
    assert!(loaded_ids(&s).is_empty());
    assert_eq!(ok(&mut s, "(unload_dialog 1)"), "nil");
}
