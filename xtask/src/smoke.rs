//! `cargo xtask smoke`: run the command sweep (`cadcraft-cli smoke`) and record what it cost.
//!
//! Each run writes `target/smoke/<run_id>/` (`results.jsonl`, `summary.json`, `report.md` from
//! the runner, plus `metrics.json` from here) and appends one line to `smoke/ledger.jsonl`, from
//! which `smoke/LEDGER.md` is regenerated. The ledger is the history: when the run happened, on
//! which commit, how many commands passed, how long the sweep took, and the agent-side cost of
//! the work that led to it (wall-clock, tokens, which models) — see `metrics.rs`.
//!
//! Agent cost is attributed by time window: everything since the previous ledger entry finished,
//! unless `--since` says otherwise.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::metrics::{self, commas, human_ms, iso, now_ms};

const USAGE: &str = "\
usage: cargo xtask smoke [--out DIR] [--since WHEN] [--no-metrics] [-- <cadcraft-cli smoke args>]

  --out DIR       where to write this run (default target/smoke/<run_id>)
  --since WHEN    attribute agent metrics from WHEN (ISO, epoch ms, -30m/-4h/-2d);
                  default: the end of the previous ledger entry
  --no-metrics    skip the agent token/time/model metrics
  --              everything after this is passed to `cadcraft-cli smoke`
";

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let mut out: Option<PathBuf> = None;
    let mut since: Option<i64> = None;
    let mut want_metrics = true;
    let mut passthrough: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--out" => out = Some(PathBuf::from(it.next().ok_or("--out DIR")?)),
            "--since" => since = Some(it.next().copied().and_then(metrics::parse_when).ok_or("--since WHEN: bad value")?),
            "--no-metrics" => want_metrics = false,
            "--" => {
                passthrough.extend(it.by_ref().map(|s| (*s).to_string()));
            }
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("unknown argument `{other}`\n\n{USAGE}")),
        }
    }

    let started = now_ms();
    let run_id = run_id(started);
    let out = out.unwrap_or_else(|| root.join("target").join("smoke").join(&run_id));
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;

    let ledger = root.join("smoke").join("ledger.jsonl");
    let since = since.or_else(|| last_entry(&ledger).and_then(|e| e["finishedAt"].as_str().and_then(metrics::parse_iso)));

    eprintln!("$ cadcraft-cli smoke --out {}", out.display());
    let mut c = crate::cargo();
    c.args(["run", "--release", "-q", "-p", "cadcraft-cli", "--", "smoke", "--out"]).arg(&out).args(&passthrough);
    let status = c.status().map_err(|e| format!("cadcraft-cli smoke: failed to spawn: {e}"))?;
    let finished = now_ms();
    let sweep_failed = !status.success();

    let summary: Value = std::fs::read_to_string(out.join("summary.json"))
        .map_err(|e| format!("{}: {e} (did the sweep run?)", out.join("summary.json").display()))
        .and_then(|s| serde_json::from_str(&s).map_err(|e| format!("summary.json: {e}")))?;

    let agent = if want_metrics {
        match metrics::collect(root, &metrics::Opts { since, until: Some(finished), session: None }) {
            Ok(r) => {
                println!();
                metrics::print_report(&r);
                r.to_json()
            }
            Err(e) => {
                eprintln!("note: agent metrics unavailable ({e})");
                Value::Null
            }
        }
    } else {
        Value::Null
    };

    let entry = json!({
        "schema": "cadcraft.smoke.ledger/1",
        "runId": run_id,
        "startedAt": iso(started),
        "finishedAt": iso(finished),
        "sweepMs": finished - started,
        "commit": git_commit(root),
        "branch": git_branch(root),
        "out": out.strip_prefix(root).unwrap_or(&out).to_string_lossy().replace('\\', "/"),
        "transport": summary["transport"].clone(),
        "commandsTested": summary["commands_tested"].clone(),
        "totals": summary["totals"].clone(),
        "agent": agent.clone(),
    });
    std::fs::write(out.join("metrics.json"), serde_json::to_string_pretty(&entry).unwrap_or_default()).map_err(|e| format!("metrics.json: {e}"))?;
    append_ledger(&ledger, &entry)?;
    write_ledger_md(root, &ledger)?;

    let t = &summary["totals"];
    let n = |k: &str| t[k].as_u64().unwrap_or(0);
    println!();
    println!(
        "smoke {run_id}: {} tested — {} pass, {} expected-error, {} fail, {} skip in {}",
        summary["commands_tested"].as_u64().unwrap_or(0),
        n("pass"),
        n("expected_error"),
        n("fail"),
        n("skip"),
        human_ms(finished - started)
    );
    println!("  run: {}   ledger: smoke/LEDGER.md", out.display());
    if sweep_failed || n("fail") > 0 {
        return Err(format!("{} command(s) failed — see {}", n("fail"), out.join("report.md").display()));
    }
    Ok(())
}

fn run_id(ms: i64) -> String {
    let s = iso(ms);
    s.chars().filter(|c| c.is_ascii_digit() || *c == 'T').collect::<String>().replace('T', "-")
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git").current_dir(root).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn git_commit(root: &Path) -> String {
    git(root, &["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".into())
}

fn git_branch(root: &Path) -> String {
    git(root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|| "unknown".into())
}

fn entries(ledger: &Path) -> Vec<Value> {
    std::fs::read_to_string(ledger).map(|s| s.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).collect()).unwrap_or_default()
}

fn last_entry(ledger: &Path) -> Option<Value> {
    entries(ledger).pop()
}

fn append_ledger(ledger: &Path, entry: &Value) -> Result<(), String> {
    if let Some(dir) = ledger.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut text = std::fs::read_to_string(ledger).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&serde_json::to_string(entry).unwrap_or_default());
    text.push('\n');
    std::fs::write(ledger, text).map_err(|e| format!("{}: {e}", ledger.display()))
}

/// Regenerate `smoke/LEDGER.md` from the whole ledger (newest run first).
fn write_ledger_md(root: &Path, ledger: &Path) -> Result<(), String> {
    let mut rows = entries(ledger);
    rows.reverse();
    let mut md = String::from(
        "# Smoke-test ledger\n\nOne row per `cargo xtask smoke` run, newest first. Generated — do not edit by hand.\n\n\
         `sweep` is how long the command sweep took; `agent` columns are the Claude Code work that led to the run\n\
         (wall-clock between the first and last agent message in the window, tokens including cache reads, and the\n\
         models that spent them). See `docs/smoke-tests.md`.\n\n\
         | run | commit | tested | pass | exp-err | fail | skip | sweep | agent time | agent tokens | models |\n\
         |---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for e in &rows {
        let t = &e["totals"];
        let n = |k: &str| t[k].as_u64().unwrap_or(0);
        let a = &e["agent"];
        let (atime, atok) = match a.is_object() {
            true => (human_ms(a["elapsedMs"].as_i64().unwrap_or(0)), commas(a["total"]["totalTokens"].as_u64().unwrap_or(0))),
            false => ("—".into(), "—".into()),
        };
        let models = a["byModel"]
            .as_object()
            .map(|m| m.keys().cloned().collect::<Vec<_>>().join(", "))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "—".into());
        md.push_str(&format!(
            "| {} | `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            e["runId"].as_str().unwrap_or("?"),
            e["commit"].as_str().unwrap_or("?"),
            e["commandsTested"].as_u64().unwrap_or(0),
            n("pass"),
            n("expected_error"),
            n("fail"),
            n("skip"),
            human_ms(e["sweepMs"].as_i64().unwrap_or(0)),
            atime,
            atok,
            models,
        ));
    }
    let path = root.join("smoke").join("LEDGER.md");
    std::fs::write(&path, md).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_id_is_sortable() {
        let a = run_id(metrics::parse_iso("2026-10-09T15:18:21.000Z").expect("iso"));
        let b = run_id(metrics::parse_iso("2026-10-09T15:19:00.000Z").expect("iso"));
        assert_eq!(a, "20261009-151821");
        assert!(a < b);
    }

    #[test]
    fn ledger_round_trip_and_markdown() {
        let dir = std::env::temp_dir().join(format!("cadcraft-smoke-ledger-{}", now_ms()));
        std::fs::create_dir_all(dir.join("smoke")).expect("tmp dir");
        let ledger = dir.join("smoke").join("ledger.jsonl");
        let entry = json!({
            "runId": "20261009-151821", "commit": "abc1234", "startedAt": "2026-10-09T15:18:21Z",
            "finishedAt": "2026-10-09T15:19:21Z", "sweepMs": 60_000, "commandsTested": 288,
            "totals": { "pass": 200, "expected_error": 80, "fail": 6, "skip": 2 },
            "agent": { "elapsedMs": 3_600_000, "total": { "totalTokens": 1_234_567 },
                       "byModel": { "claude-opus-5 (high)": {} } },
        });
        append_ledger(&ledger, &entry).expect("append");
        append_ledger(&ledger, &entry).expect("append again");
        assert_eq!(entries(&ledger).len(), 2);
        assert_eq!(last_entry(&ledger).expect("last")["runId"], "20261009-151821");
        write_ledger_md(&dir, &ledger).expect("markdown");
        let md = std::fs::read_to_string(dir.join("smoke").join("LEDGER.md")).expect("read md");
        assert!(md.contains("| 20261009-151821 | `abc1234` | 288 | 200 | 80 | 6 | 2 | 1m 00s | 1h 00m 00s | 1,234,567 | claude-opus-5 (high) |"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
