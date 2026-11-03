//! `roundtrip`: the corpus harness of `cadcraft_io::roundtrip::corpus` over a file or folder.

use super::args::RoundtripArgs;
use super::error::{CliError, Result};
use super::output::Streams;

/// Why `roundtrip` stopped.
#[derive(Debug, thiserror::Error)]
pub(crate) enum RoundtripError {
    #[error("roundtrip: give a FILE or FOLDER, or set CADCRAFT_CORPUS_DIR")]
    NoPath,
    #[error("roundtrip: no such file or folder: {}", .0.display())]
    Missing(std::path::PathBuf),
    #[error("roundtrip: no DXF or DWG files in {}", .0.display())]
    Empty(std::path::PathBuf),
    #[error("roundtrip: can't write the report to {}: {source}", .path.display())]
    Report { path: std::path::PathBuf, source: std::io::Error },
    #[error("roundtrip: {} strict failures:\n{}", .0.len(), .0.join("\n"))]
    Strict(Vec<String>),
}

pub(crate) fn roundtrip(a: &RoundtripArgs, io: &mut Streams) -> Result {
    use cadcraft_io::roundtrip::corpus;
    let path = a.path.clone().or_else(|| std::env::var_os("CADCRAFT_CORPUS_DIR").map(Into::into)).ok_or(RoundtripError::NoPath)?;
    let (root, files, out) = if path.is_dir() {
        let out = a.out.clone().unwrap_or_else(|| path.join("report"));
        let files = corpus::corpus_files(&path, std::slice::from_ref(&out));
        (path, files, out)
    } else if path.is_file() {
        let root = path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
        let out = a.out.clone().unwrap_or_else(|| root.join("report"));
        (root, vec![path], out)
    } else {
        return Err(RoundtripError::Missing(path).into());
    };
    if files.is_empty() {
        return Err(RoundtripError::Empty(root).into());
    }
    let mut opts = corpus::CorpusOptions { cross_format: !a.same_format, census: !a.no_census, ..Default::default() };
    opts.compare.abs_tol = a.tolerance;
    if a.keep_saved {
        opts.keep_saved = Some(out.join("saved"));
    }
    let report = corpus::run(&root, &files, &opts, &mut |i, n, rel| eprintln!("[{}/{n}] {rel}", i + 1));
    for f in &report.files {
        let routes: Vec<String> = f
            .routes
            .iter()
            .map(|r| match (&r.save_error, &r.reopen_error) {
                (Some(_), _) => format!("{}: save failed", r.route),
                (None, Some(_)) => format!("{}: reopen failed", r.route),
                (None, None) => {
                    let t = r.comparison.total();
                    format!("{}: {} lost, {} added, {} changed", r.route, t.lost, t.added, t.changed)
                }
            })
            .collect();
        match &f.open_error {
            Some(e) => io.outln(format_args!("{}: open failed: {e}", f.path)),
            None => io.outln(format_args!("{}: opened; {}", f.path, routes.join("; "))),
        }
    }
    let (md, json) = report.write(&out).map_err(|source| RoundtripError::Report { path: out.clone(), source })?;
    io.outln(format_args!("report: {}", md.display()));
    io.outln(format_args!("        {}", json.display()));
    // CADCRAFT_CORPUS_STRICT=1: a file that doesn't open, save or reopen, or records a writer
    // leaves out, fail the run (the report is written first).
    if std::env::var("CADCRAFT_CORPUS_STRICT").is_ok_and(|v| v == "1") {
        let failed = report.strict_failures();
        if !failed.is_empty() {
            return Err(CliError::from(RoundtripError::Strict(failed)));
        }
    }
    Ok(())
}
