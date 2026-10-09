//! `cargo xtask metrics`: how much agent work went into this repo — wall-clock time, token
//! usage and which model spent it.
//!
//! The numbers come from the Claude Code session transcripts in
//! `~/.claude/projects/<slug>/*.jsonl` (one JSON object per line). Only `assistant` records
//! carry `message.model` and `message.usage`; a single reply is written as several lines (one
//! per content block) sharing `message.id`, so usage is counted once per message id and tool
//! calls once per `tool_use` id. `isSidechain` marks a subagent's turns.
//!
//! Nothing here talks to the network and no transcript content is copied — only counters.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

#[derive(Default, Clone, Copy)]
pub struct Usage {
    pub messages: u64,
    pub input: u64,
    pub output: u64,
    pub cache_create: u64,
    pub cache_read: u64,
    pub thinking: u64,
}

impl Usage {
    /// Everything the model read or wrote (cache reads included).
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_create + self.cache_read
    }
    /// Tokens that were not served from the prompt cache.
    pub fn fresh(&self) -> u64 {
        self.input + self.output + self.cache_create
    }
    fn add(&mut self, o: &Usage) {
        self.messages += o.messages;
        self.input += o.input;
        self.output += o.output;
        self.cache_create += o.cache_create;
        self.cache_read += o.cache_read;
        self.thinking += o.thinking;
    }
}

#[derive(Default)]
pub struct Report {
    pub sessions: Vec<String>,
    pub total: Usage,
    /// Keyed by `model` or `model (effort)`.
    pub by_model: BTreeMap<String, Usage>,
    /// `"main"` and `"subagent"`.
    pub by_agent: BTreeMap<String, Usage>,
    pub tools: BTreeMap<String, u64>,
    pub first_ms: Option<i64>,
    pub last_ms: Option<i64>,
    pub window: (Option<i64>, Option<i64>),
}

impl Report {
    pub fn elapsed_ms(&self) -> i64 {
        match (self.first_ms, self.last_ms) {
            (Some(a), Some(b)) => (b - a).max(0),
            _ => 0,
        }
    }
    pub fn tool_calls(&self) -> u64 {
        self.tools.values().sum()
    }
    pub fn to_json(&self) -> Value {
        let usage = |u: &Usage| {
            json!({
                "messages": u.messages, "inputTokens": u.input, "outputTokens": u.output,
                "cacheCreationTokens": u.cache_create, "cacheReadTokens": u.cache_read,
                "thinkingTokens": u.thinking, "totalTokens": u.total(), "freshTokens": u.fresh(),
            })
        };
        json!({
            "schema": "cadcraft.metrics.agent/1",
            "sessions": self.sessions.clone(),
            "windowStart": self.window.0.map(iso),
            "windowEnd": self.window.1.map(iso),
            "firstMessageAt": self.first_ms.map(iso),
            "lastMessageAt": self.last_ms.map(iso),
            "elapsedMs": self.elapsed_ms(),
            "total": usage(&self.total),
            "byModel": self.by_model.iter().map(|(k, u)| (k.clone(), usage(u))).collect::<BTreeMap<_, _>>(),
            "byAgent": self.by_agent.iter().map(|(k, u)| (k.clone(), usage(u))).collect::<BTreeMap<_, _>>(),
            "toolCalls": self.tool_calls(),
            "toolsByName": self.tools.clone(),
        })
    }
}

// ---------------- transcript discovery ----------------

fn home() -> Option<PathBuf> {
    for k in ["USERPROFILE", "HOME"] {
        if let Ok(v) = std::env::var(k)
            && !v.is_empty()
        {
            return Some(PathBuf::from(v));
        }
    }
    None
}

/// Claude Code's directory name for a working directory: every character that is not
/// alphanumeric or `-` becomes `-` (`C:\Users\x\repo` → `C--Users-x-repo`).
fn slug(path: &Path) -> String {
    path.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect()
}

/// The transcript directory for `root`: the slug directory if it exists, otherwise the project
/// directory whose records report `root` as their `cwd`.
pub fn project_dir(root: &Path) -> Result<PathBuf, String> {
    let base = home().map(|h| h.join(".claude").join("projects")).ok_or("no home directory (USERPROFILE/HOME)")?;
    let direct = base.join(slug(root));
    if direct.is_dir() {
        return Ok(direct);
    }
    let want = root.to_string_lossy().to_lowercase();
    let rd = std::fs::read_dir(&base).map_err(|e| format!("{}: {e}", base.display()))?;
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        if transcripts(&p).iter().any(|f| cwd_of(f).is_some_and(|c| c.to_lowercase() == want)) {
            return Ok(p);
        }
    }
    Err(format!("no Claude Code transcripts for {} under {}", root.display(), base.display()))
}

/// The project's session transcripts, plus the subagent transcripts each session keeps in
/// `<session-id>/subagents/agent-*.jsonl` — a worker's tokens are only in there.
fn transcripts(dir: &Path) -> Vec<PathBuf> {
    let jsonl = |p: &Path| p.extension().is_some_and(|x| x == "jsonl");
    let mut v: Vec<PathBuf> = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            let subs = p.join("subagents");
            v.extend(std::fs::read_dir(&subs).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| jsonl(p)));
        } else if jsonl(&p) {
            v.push(p);
        }
    }
    v.sort();
    v
}

fn cwd_of(file: &Path) -> Option<String> {
    let s = std::fs::read_to_string(file).ok()?;
    for line in s.lines().take(50) {
        if let Ok(v) = serde_json::from_str::<Value>(line)
            && let Some(c) = v.get("cwd").and_then(Value::as_str)
        {
            return Some(c.to_string());
        }
    }
    None
}

// ---------------- time ----------------

/// Parse `2026-10-09T15:18:21.099Z` to epoch milliseconds.
pub fn parse_iso(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |a: usize, z: usize| s.get(a..z)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let ms = s.get(20..23).and_then(|f| f.parse::<i64>().ok()).unwrap_or(0);
    Some((days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + sec) * 1000 + ms)
}

/// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Epoch milliseconds as `2026-10-09T15:18:21Z`.
pub fn iso(ms: i64) -> String {
    let (days, rem) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000) / 1000);
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, (rem % 3600) / 60, rem % 60)
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// `--since` / `--until` values: an ISO timestamp, epoch milliseconds, or `-<N>(m|h|d)` ago.
pub fn parse_when(s: &str) -> Option<i64> {
    if let Some(rest) = s.strip_prefix('-') {
        let (n, unit) = rest.split_at(rest.len().saturating_sub(1));
        let mult = match unit {
            "m" => 60_000,
            "h" => 3_600_000,
            "d" => 86_400_000,
            _ => return s.parse::<i64>().ok(),
        };
        return n.parse::<i64>().ok().map(|v| now_ms() - v * mult);
    }
    if s.contains('-') && s.contains('T') {
        return parse_iso(s);
    }
    s.parse::<i64>().ok()
}

/// Human duration: `1h 12m 03s`.
pub fn human_ms(ms: i64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}h {:02}m {:02}s", s / 3600, (s % 3600) / 60, s % 60)
    } else if s >= 60 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{}.{:01}s", s, (ms % 1000) / 100)
    }
}

/// Thousands separators: `1234567` → `1,234,567`.
pub fn commas(n: u64) -> String {
    let d = n.to_string();
    let mut out = String::with_capacity(d.len() + d.len() / 3);
    for (i, c) in d.chars().enumerate() {
        if i > 0 && (d.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

// ---------------- aggregation ----------------

pub struct Opts {
    pub since: Option<i64>,
    pub until: Option<i64>,
    /// Only this session id (transcript file stem); `None` = every session in the project.
    pub session: Option<String>,
}

pub fn collect(root: &Path, opts: &Opts) -> Result<Report, String> {
    let dir = project_dir(root)?;
    let files = transcripts(&dir);
    if files.is_empty() {
        return Err(format!("no transcripts in {}", dir.display()));
    }
    let mut r = Report { window: (opts.since, opts.until), ..Report::default() };
    let mut seen_msgs: HashSet<String> = HashSet::new();
    let mut seen_tools: HashSet<String> = HashSet::new();
    for f in files {
        let stem = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let stem = match stem.strip_prefix("agent-") {
            Some(id) => format!("subagent {}", id.get(..8).unwrap_or(id)),
            None => stem,
        };
        if opts.session.as_ref().is_some_and(|w| &stem != w) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let mut counted = false;
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            if v.get("type").and_then(Value::as_str) != Some("assistant") {
                continue;
            }
            let Some(ts) = v.get("timestamp").and_then(Value::as_str).and_then(parse_iso) else { continue };
            if opts.since.is_some_and(|a| ts < a) || opts.until.is_some_and(|b| ts > b) {
                continue;
            }
            let msg = v.get("message").unwrap_or(&Value::Null);
            let model = msg.get("model").and_then(Value::as_str).unwrap_or("unknown");
            let effort = v.get("effort").and_then(Value::as_str);
            let key = match effort {
                Some(e) => format!("{model} ({e})"),
                None => model.to_string(),
            };
            let agent = if v.get("isSidechain").and_then(Value::as_bool).unwrap_or(false) { "subagent" } else { "main" };
            counted = true;
            r.first_ms = Some(r.first_ms.map_or(ts, |a: i64| a.min(ts)));
            r.last_ms = Some(r.last_ms.map_or(ts, |b: i64| b.max(ts)));

            // Usage once per message id (a reply spans several lines, one per content block).
            let id = msg.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            if id.is_empty() || seen_msgs.insert(id) {
                let u = usage_of(msg.get("usage").unwrap_or(&Value::Null));
                r.total.add(&u);
                r.by_model.entry(key).or_default().add(&u);
                r.by_agent.entry(agent.to_string()).or_default().add(&u);
            }
            // Tool calls once per tool_use id.
            for blk in msg.get("content").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
                if blk.get("type").and_then(Value::as_str) != Some("tool_use") {
                    continue;
                }
                let tid = blk.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                if !tid.is_empty() && !seen_tools.insert(tid) {
                    continue;
                }
                let name = blk.get("name").and_then(Value::as_str).unwrap_or("?");
                *r.tools.entry(name.to_string()).or_default() += 1;
            }
        }
        if counted {
            r.sessions.push(stem);
        }
    }
    Ok(r)
}

fn usage_of(u: &Value) -> Usage {
    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    Usage {
        messages: 1,
        input: n("input_tokens"),
        output: n("output_tokens"),
        cache_create: n("cache_creation_input_tokens"),
        cache_read: n("cache_read_input_tokens"),
        thinking: u.get("output_tokens_details").and_then(|d| d.get("thinking_tokens")).and_then(Value::as_u64).unwrap_or(0),
    }
}

// ---------------- CLI ----------------

const USAGE: &str = "\
usage: cargo xtask metrics [--since WHEN] [--until WHEN] [--session ID] [--json]

  WHEN is an ISO timestamp (2026-10-09T12:00:00Z), epoch milliseconds, or -30m / -4h / -2d.
  Reads the Claude Code transcripts for this working directory and reports wall-clock time,
  token usage and model use (main session and subagents separately).
";

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let mut o = Opts { since: None, until: None, session: None };
    let mut as_json = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--since" => o.since = Some(it.next().copied().and_then(parse_when).ok_or("--since WHEN: bad value")?),
            "--until" => o.until = Some(it.next().copied().and_then(parse_when).ok_or("--until WHEN: bad value")?),
            "--session" => o.session = Some(it.next().ok_or("--session ID")?.to_string()),
            "--json" => as_json = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("unknown argument `{other}`\n\n{USAGE}")),
        }
    }
    let r = collect(root, &o)?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&r.to_json()).unwrap_or_default());
        return Ok(());
    }
    print_report(&r);
    Ok(())
}

pub fn print_report(r: &Report) {
    let span = match (r.first_ms, r.last_ms) {
        (Some(a), Some(b)) => format!("{} → {}  ({})", iso(a), iso(b), human_ms(b - a)),
        _ => "no messages in window".into(),
    };
    println!("Agent metrics — {} session(s): {}", r.sessions.len(), r.sessions.join(", "));
    println!("  span: {span}");
    println!("  {} messages, {} tool calls", commas(r.total.messages), commas(r.tool_calls()));
    println!();
    println!("{:<34} {:>8} {:>12} {:>12} {:>12} {:>12}", "model", "msgs", "in", "out", "cache rd", "total");
    for (k, u) in &r.by_model {
        println!(
            "{k:<34} {:>8} {:>12} {:>12} {:>12} {:>12}",
            commas(u.messages),
            commas(u.input + u.cache_create),
            commas(u.output),
            commas(u.cache_read),
            commas(u.total())
        );
    }
    println!("{:-<96}", "");
    println!(
        "{:<34} {:>8} {:>12} {:>12} {:>12} {:>12}",
        "all",
        commas(r.total.messages),
        commas(r.total.input + r.total.cache_create),
        commas(r.total.output),
        commas(r.total.cache_read),
        commas(r.total.total())
    );
    println!("  thinking tokens: {}   fresh (uncached) tokens: {}", commas(r.total.thinking), commas(r.total.fresh()));
    if r.by_agent.len() > 1 {
        println!();
        for (k, u) in &r.by_agent {
            println!("  {k:<10} {:>8} msgs {:>14} tokens", commas(u.messages), commas(u.total()));
        }
    }
    let mut tools: Vec<(&String, &u64)> = r.tools.iter().collect();
    tools.sort_by(|a, b| b.1.cmp(a.1));
    if !tools.is_empty() {
        println!();
        let top: Vec<String> = tools.iter().take(8).map(|(n, c)| format!("{n} {c}")).collect();
        println!("  tools: {}", top.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_round_trip() {
        let s = "2026-10-09T15:18:21.099Z";
        let ms = parse_iso(s).expect("parses");
        assert_eq!(iso(ms), "2026-10-09T15:18:21Z");
        assert_eq!(parse_iso("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn slug_matches_claude_code() {
        assert_eq!(slug(Path::new(r"C:\Users\x\local-ai\storytold\cadcraft")), "C--Users-x-local-ai-storytold-cadcraft");
    }

    #[test]
    fn usage_is_counted_once_per_message_id() {
        let u = usage_of(&json!({
            "input_tokens": 2, "output_tokens": 356, "cache_creation_input_tokens": 14006,
            "cache_read_input_tokens": 37121, "output_tokens_details": { "thinking_tokens": 101 }
        }));
        assert_eq!((u.input, u.output, u.cache_create, u.cache_read, u.thinking), (2, 356, 14006, 37121, 101));
        assert_eq!(u.total(), 2 + 356 + 14006 + 37121);
        assert_eq!(u.fresh(), 2 + 356 + 14006);
    }

    #[test]
    fn when_shorthands() {
        assert_eq!(parse_when("1760000000000"), Some(1_760_000_000_000));
        let ago = parse_when("-2h").expect("relative");
        assert!((now_ms() - ago - 7_200_000).abs() < 5_000);
    }

    #[test]
    fn formatting() {
        assert_eq!(commas(1_234_567), "1,234,567");
        assert_eq!(commas(42), "42");
        assert_eq!(human_ms(3_723_000), "1h 02m 03s");
        assert_eq!(human_ms(65_000), "1m 05s");
    }
}
