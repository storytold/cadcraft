//! `cargo xtask stats`: tests and lines of Rust per crate.

use std::path::Path;

#[derive(Default, Clone, Copy)]
struct Counts {
    files: usize,
    lines: usize,
    tests: usize,
    proptests: usize,
}

/// Upper bound on directory entries visited in one `walk`, so even a crafted tree of aliases can
/// never make the stats command run unbounded work.
const MAX_ENTRIES: usize = 2_000_000;

/// Accumulate Rust source counts under one directory with a fresh traversal-entry budget.
fn walk(dir: &Path, c: &mut Counts) -> Result<(), String> {
    let mut budget = MAX_ENTRIES;
    walk_inner(dir, c, &mut budget)
}

/// Traverse entries without following symlinks, sharing one budget across descendant directories.
/// Skip unreadable entries and excluded directories; report budget exhaustion to the caller.
fn walk_inner(dir: &Path, c: &mut Counts, budget: &mut usize) -> Result<(), String> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for e in rd.flatten() {
        if *budget == 0 {
            return Err(format!("stats traversal budget exhausted at {}", dir.display()));
        }
        *budget -= 1;
        // Use the entry's own type and never follow symlinks: a directory symlink would otherwise
        // be re-scanned as a fresh directory, so a small graph of aliases could cause exponential
        // repeated enumeration and reads (CWE-400).
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let p = e.path();
        let name = e.file_name();
        let name = name.to_string_lossy();
        if ft.is_dir() {
            // Nested fuzz workspaces and build output are not part of the crate.
            if name != "target" && name != "fuzz" && !name.starts_with('.') {
                walk_inner(&p, c, budget)?;
            }
        } else if ft.is_file()
            && name.ends_with(".rs")
            && let Ok(s) = std::fs::read_to_string(&p)
        {
            c.files += 1;
            c.lines += s.lines().count();
            let (t, pt) = count_tests(&s);
            c.tests += t;
            c.proptests += pt;
        }
    }
    Ok(())
}

/// `#[test]` attributes and `proptest!` blocks (each block may hold many
/// property tests, and macro-generated tests are only counted once — use
/// `--exact` for harness-accurate numbers).
fn count_tests(src: &str) -> (usize, usize) {
    let mut tests = 0;
    let mut props = 0;
    for line in src.lines() {
        let t = line.trim_start();
        if t.starts_with("//") {
            continue;
        }
        tests += t.matches("#[test]").count();
        if t.starts_with("proptest!") {
            props += 1;
        }
    }
    (tests, props)
}

fn exact_tests(pkg: &str) -> Option<usize> {
    let out = crate::cargo().args(["test", "-q", "-p", pkg, "--", "--list"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).lines().filter(|l| l.ends_with(": test")).count())
}

/// Print per-package source statistics, optionally querying exact test counts, and propagate traversal errors.
pub fn run(_root: &Path, exact: bool) -> Result<(), String> {
    let meta = crate::metadata()?;
    let mut rows = Vec::new();
    for p in meta["packages"].as_array().into_iter().flatten() {
        let name = p["name"].as_str().unwrap_or("?").to_owned();
        let Some(dir) = p["manifest_path"].as_str().and_then(|m| Path::new(m).parent().map(Path::to_path_buf)) else {
            continue;
        };
        let mut c = Counts::default();
        walk(&dir, &mut c)?;
        let ex = if exact { exact_tests(&name) } else { None };
        rows.push((name, c, ex));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));

    let exact_col = if exact { format!(" {:>8}", "harness") } else { String::new() };
    println!("{:<26} {:>6} {:>8} {:>7} {:>10}{exact_col}", "crate", "files", "lines", "#[test]", "proptest!");
    println!("{}", "-".repeat(62 + if exact { 9 } else { 0 }));
    let mut total = Counts::default();
    let mut total_exact = 0;
    for (name, c, ex) in &rows {
        let ex_s = match (exact, ex) {
            (false, _) => String::new(),
            (true, Some(n)) => {
                total_exact += n;
                format!(" {n:>8}")
            }
            (true, None) => format!(" {:>8}", "error"),
        };
        println!("{name:<26} {:>6} {:>8} {:>7} {:>10}{ex_s}", c.files, c.lines, c.tests, c.proptests);
        total.files += c.files;
        total.lines += c.lines;
        total.tests += c.tests;
        total.proptests += c.proptests;
    }
    println!("{}", "-".repeat(62 + if exact { 9 } else { 0 }));
    let ex_s = if exact { format!(" {total_exact:>8}") } else { String::new() };
    println!("{:<26} {:>6} {:>8} {:>7} {:>10}{ex_s}", "TOTAL", total.files, total.lines, total.tests, total.proptests);
    if !exact {
        println!("\n(#[test] counts source attributes; macro-generated tests count once. Use --exact for harness counts.)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Counts, count_tests};

    #[test]
    fn counts_attributes_and_proptest_blocks() {
        let src = "#[test]\nfn a() {}\n// #[test] commented\n    #[test]\nfn b() {}\nproptest! {\n}\n";
        assert_eq!(count_tests(src), (2, 1));
    }

    /// A directory alias does not cause its Rust source to be counted twice.
    #[cfg(unix)]
    #[test]
    fn walk_does_not_follow_directory_symlinks() {
        use super::walk;
        use std::os::unix::fs::symlink;
        let base = std::env::temp_dir().join(format!("cadcraft-xtask-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let real = base.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("a.rs"), "fn a() {}\n").unwrap();
        // A directory symlink pointing back at `real` would be re-scanned if followed.
        symlink(&real, base.join("alias")).unwrap();
        let mut c = Counts::default();
        walk(&base, &mut c).unwrap();
        assert_eq!(c.files, 1, "the aliased directory was followed and counted twice");
        std::fs::remove_dir_all(&base).ok();
    }
    /// Traversal reports an exhausted entry budget and accepts a tree that exactly fits it.
    #[test]
    fn walk_reports_budget_exhaustion() {
        let base = std::env::temp_dir().join(format!("cadcraft-stats-budget-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("a.rs"), "fn a() {}\n").unwrap();
        let error = super::walk_inner(&base, &mut Counts::default(), &mut 0).unwrap_err();
        assert!(error.contains("budget exhausted"));
        let mut c = Counts::default();
        super::walk_inner(&base, &mut c, &mut 1).unwrap();
        assert_eq!(c.files, 1);
        std::fs::remove_dir_all(base).unwrap();
    }
}
