//! The `store-*` subcommands end to end through `cadcraft_cli::run`: a drawing goes up to a local
//! storage folder and comes back byte-for-byte, a stale ETag is refused, and the backend flags are
//! checked by clap before anything runs.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

fn invoke(args: &[&str]) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = cadcraft_cli::run(args, &mut out, &mut err);
    (code, String::from_utf8_lossy(&out).into_owned(), String::from_utf8_lossy(&err).into_owned())
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("cc-cli-store-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("store")).unwrap();
        Scratch(dir)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_string()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_drawing_round_trips_through_a_local_store() {
    let s = Scratch::new("roundtrip");
    let (b, root, back) = (s.path("b.dxf"), s.path("store"), s.path("back.dxf"));
    assert_eq!(invoke(&["sample", "bracket", &b]).0, 0);
    let (code, out, err) = invoke(&["store-write", "plans/b.dxf", &b, "--store-root", &root, "--atomic"]);
    assert_eq!(code, 0, "{err}");
    let written: serde_json::Value = serde_json::from_str(&out).unwrap();
    let tag = written["etag"].as_str().unwrap().to_string();

    let (code, out, _) = invoke(&["store-stat", "plans/b.dxf", "--store-root", &root]);
    assert_eq!(code, 0);
    assert_eq!(serde_json::from_str::<serde_json::Value>(&out).unwrap()["etag"], tag.as_str());
    let (code, out, _) = invoke(&["store-list", "plans", "--store-root", &root]);
    assert_eq!(code, 0);
    let list: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(list.as_array().map(Vec::len), Some(1), "staging, lock and journal files stay hidden: {list}");

    assert_eq!(invoke(&["store-read", "plans/b.dxf", &back, "--store-root", &root]).0, 0);
    assert_eq!(std::fs::read(&b).unwrap(), std::fs::read(&back).unwrap());

    // A stale tag loses; the current one wins.
    let (code, _, err) = invoke(&["store-write", "plans/b.dxf", &b, "--store-root", &root, "--etag", "0000000000000000"]);
    assert_eq!(code, 1);
    assert!(err.contains("plans/b.dxf: conflict:"), "{err}");
    assert_eq!(invoke(&["store-write", "plans/b.dxf", &b, "--store-root", &root, "--etag", &tag]).0, 0);

    assert_eq!(invoke(&["store-remove", "plans/b.dxf", "--store-root", &root]).0, 0);
    assert_eq!(invoke(&["store-stat", "plans/b.dxf", "--store-root", &root]).0, 1);
    let (code, out, _) = invoke(&["store-recover", "--store-root", &root]);
    assert_eq!(code, 0);
    assert_eq!(serde_json::from_str::<serde_json::Value>(&out).unwrap()["dropped"], 0);
}

#[test]
fn backend_flags_are_checked_before_anything_runs() {
    let usage = |args: &[&str]| {
        let (code, out, err) = invoke(args);
        assert_eq!(code, 2, "{args:?}: {out}{err}");
        err
    };
    usage(&["store-list"]);
    usage(&["store-list", "--store", "webdav"]);
    usage(&["store-list", "--store", "s3", "--store-root", "x"]);
    usage(&["store-list", "--store-root", "x", "--store-url", "http://127.0.0.1/dav/"]);
    // A URL is WebDAV's: naming the local backend with it is a usage error, not a runtime one.
    usage(&["store-list", "--store", "local", "--store-url", "http://127.0.0.1/dav/"]);
    usage(&["store-list", "--store", "webdav", "--store-url", "http://127.0.0.1/dav/", "--store-user", "u"]);
    let err = usage(&[
        "store-list",
        "--store",
        "webdav",
        "--store-url",
        "http://127.0.0.1/dav/",
        "--store-token",
        "t",
        "--store-user",
        "u",
        "--store-password",
        "p",
    ]);
    assert!(err.contains("cannot be used with"), "{err}");
}

#[test]
fn webdav_refuses_https_and_cleartext_credentials_off_loopback() {
    let (code, _, err) = invoke(&["store-list", "--store", "webdav", "--store-url", "https://cloud.example/dav/"]);
    assert_eq!(code, 1);
    assert!(err.contains("only http://"), "{err}");
    let (code, _, err) = invoke(&["store-list", "--store", "webdav", "--store-url", "http://192.0.2.1:9/dav/", "--store-token", "t"]);
    assert_eq!(code, 1);
    assert!(err.contains("--store-insecure-http"), "{err}");
}

#[test]
fn a_store_url_alone_selects_webdav() {
    // Nothing listens on port 9 (discard), so this reaches the backend and fails there: a
    // runtime error (exit 1) from WebDAV, not a usage error and not "--store local needs ...".
    let (code, out, err) = invoke(&["store-list", "--store-url", "http://127.0.0.1:9/dav/"]);
    assert_eq!(code, 1, "{out}{err}");
    assert!(!err.contains("--store-root"), "{err}");
}
