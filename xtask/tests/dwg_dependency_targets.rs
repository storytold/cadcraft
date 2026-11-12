//! Test builds must not reintroduce the DWG decoder on targets where the bridge excludes it.

use serde_json::Value;
use std::process::Command;

#[test]
fn dwg_test_dependency_matches_the_bridge_targets() {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("read workspace dependency metadata");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let metadata: Value = serde_json::from_slice(&output.stdout).expect("Cargo returns JSON");
    let packages = metadata["packages"].as_array().expect("workspace packages");
    let dependency = |name: &str| {
        let package = packages.iter().find(|p| p["name"] == name).expect("workspace crate");
        package["dependencies"].as_array().expect("crate dependencies").iter().find(|d| d["name"] == "acadrust").expect("DWG decoder dependency")
    };
    let bridge = dependency("cadcraft-dwg");
    let tests = dependency("cadcraft-io");
    assert_eq!(tests["kind"], "dev", "the IO crate only inspects acadrust output in tests");
    assert_eq!(tests["target"], bridge["target"], "--all-targets must not add acadrust where the native DWG bridge is unavailable");
    assert_eq!(tests["req"], bridge["req"], "tests must inspect the same decoder version as the bridge");
}
