//! Exact token counts from the logs Claude Code and Codex already keep locally.
//!
//! Plan limits are reported as percentages; these counts add the raw numbers
//! behind them for activity on this PC. Files are read incrementally (only new
//! lines), and each request is counted once:
//! - Claude Code transcripts (`<claude>/projects/**/*.jsonl`) repeat a reply's
//!   usage on several lines and across resumed sessions, so they are keyed by
//!   message id + request id, last line wins.
//! - Codex rollouts (`<codex>/sessions/**/*.jsonl`) can repeat a `token_count`
//!   event, so usage is taken as the change in the running session total.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;
use serde_json::Value;

use crate::paths;

/// History kept in memory: a week plus margin.
const KEEP_MS: i64 = 8 * 86_400_000;
const MAX_DEPTH: usize = 6;

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenTotals {
    /// Uncached input.
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub total: u64,
    pub requests: u64,
}

impl TokenTotals {
    fn add(&mut self, o: &TokenTotals) {
        self.input += o.input;
        self.output += o.output;
        self.cache_read += o.cache_read;
        self.cache_write += o.cache_write;
        self.total += o.total;
        self.requests += o.requests;
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    /// Tokens since the current session window started.
    pub session: TokenTotals,
    /// Tokens since the current weekly window started.
    pub weekly: TokenTotals,
    /// Latest request seen in the logs (epoch ms).
    pub last_activity_at: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
struct Event {
    at: i64,
    t: TokenTotals,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Claude,
    Codex,
}

struct Source {
    kind: Kind,
    offsets: HashMap<PathBuf, u64>,
    /// Claude: one entry per request.
    keyed: HashMap<String, Event>,
    /// Codex: per-request deltas.
    events: Vec<Event>,
    /// Codex: running total per rollout file.
    running: HashMap<PathBuf, TokenTotals>,
}

impl Source {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            offsets: HashMap::new(),
            keyed: HashMap::new(),
            events: Vec::new(),
            running: HashMap::new(),
        }
    }

    fn scan(&mut self, root: &Path, now: i64) {
        let mut files = Vec::new();
        walk(root, now - KEEP_MS, &mut files, 0);
        for path in files {
            // Size from the open handle: the folder listing can report a stale
            // size for files another program is still writing.
            let Ok(mut f) = File::open(&path) else { continue };
            let Ok(len) = f.metadata().map(|m| m.len()) else { continue };
            let known = self.offsets.get(&path).copied().unwrap_or(0);
            let start = if len < known {
                // File was rewritten: start over.
                self.running.remove(&path);
                0
            } else {
                known
            };
            if start == len {
                continue;
            }
            if f.seek(SeekFrom::Start(start)).is_err() {
                continue;
            }
            let mut reader = BufReader::new(f);
            let mut pos = start;
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        // A line still being written: pick it up next time.
                        if buf.last() != Some(&b'\n') {
                            break;
                        }
                        pos += n as u64;
                        let line = String::from_utf8_lossy(&buf);
                        self.ingest(&line, &path);
                    }
                }
            }
            self.offsets.insert(path, pos);
        }
        let cutoff = now - KEEP_MS;
        self.keyed.retain(|_, e| e.at >= cutoff);
        self.events.retain(|e| e.at >= cutoff);
    }

    fn ingest(&mut self, line: &str, path: &Path) {
        match self.kind {
            Kind::Claude => {
                if let Some((key, e)) = parse_claude(line) {
                    self.keyed.insert(key, e);
                }
            }
            Kind::Codex => {
                let Some((at, cumulative)) = parse_codex(line) else { return };
                let prev = self.running.insert(path.to_path_buf(), cumulative).unwrap_or_default();
                if cumulative.total <= prev.total {
                    return; // repeated event
                }
                let d = |a: u64, b: u64| a.saturating_sub(b);
                self.events.push(Event {
                    at,
                    t: TokenTotals {
                        input: d(cumulative.input, prev.input),
                        output: d(cumulative.output, prev.output),
                        cache_read: d(cumulative.cache_read, prev.cache_read),
                        cache_write: d(cumulative.cache_write, prev.cache_write),
                        total: d(cumulative.total, prev.total),
                        requests: 1,
                    },
                });
            }
        }
    }

    fn all(&self) -> impl Iterator<Item = &Event> {
        self.keyed.values().chain(self.events.iter())
    }

    fn usage(&self, session_start: i64, weekly_start: i64) -> TokenUsage {
        let mut u = TokenUsage::default();
        for e in self.all() {
            if e.at >= session_start {
                u.session.add(&e.t);
            }
            if e.at >= weekly_start {
                u.weekly.add(&e.t);
            }
            u.last_activity_at = Some(u.last_activity_at.map_or(e.at, |l| l.max(e.at)));
        }
        u
    }
}

fn walk(dir: &Path, cutoff_ms: i64, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        let path = entry.path();
        if meta.is_dir() {
            walk(&path, cutoff_ms, out, depth + 1);
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            let modified = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            if modified >= cutoff_ms {
                out.push(path);
            }
        }
    }
}

fn parse_claude(line: &str) -> Option<(String, Event)> {
    if !line.contains("\"usage\"") || !line.contains("\"assistant\"") {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let msg = v.get("message")?;
    if msg.get("model").and_then(Value::as_str) == Some("<synthetic>") {
        return None;
    }
    let u = msg.get("usage")?.as_object()?;
    let at = parse_ts_ms(v.get("timestamp")?.as_str()?)?;
    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    let t = TokenTotals {
        input: n("input_tokens"),
        output: n("output_tokens"),
        cache_read: n("cache_read_input_tokens"),
        cache_write: n("cache_creation_input_tokens"),
        total: n("input_tokens") + n("output_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens"),
        requests: 1,
    };
    let id = msg.get("id").and_then(Value::as_str).unwrap_or("");
    let req = v.get("requestId").and_then(Value::as_str).unwrap_or("");
    let key = if id.is_empty() && req.is_empty() {
        // No ids to dedupe on: fall back to the line's own identity.
        v.get("uuid").and_then(Value::as_str).unwrap_or("").to_owned()
    } else {
        format!("{id}:{req}")
    };
    if key.is_empty() {
        return None;
    }
    Some((key, Event { at, t }))
}

fn parse_codex(line: &str) -> Option<(i64, TokenTotals)> {
    if !line.contains("\"token_count\"") {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let payload = v.get("payload")?;
    if payload.get("type")?.as_str()? != "token_count" {
        return None;
    }
    let u = payload.pointer("/info/total_token_usage")?.as_object()?;
    let at = parse_ts_ms(v.get("timestamp")?.as_str()?)?;
    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    let cached = n("cached_input_tokens");
    let output = n("output_tokens");
    let input_all = n("input_tokens");
    let total = match n("total_tokens") {
        0 => input_all + output,
        t => t,
    };
    Some((
        at,
        TokenTotals {
            input: input_all.saturating_sub(cached),
            output,
            cache_read: cached,
            cache_write: n("cache_write_input_tokens"),
            total,
            requests: 0,
        },
    ))
}

/// Parse `YYYY-MM-DDTHH:MM:SS[.fff][Z|±HH:MM]` to epoch milliseconds.
pub fn parse_ts_ms(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, se) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let mut i = 19;
    let mut ms = 0;
    if b.get(i) == Some(&b'.') {
        let start = i + 1;
        let mut j = start;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        let frac = s.get(start..j.min(start + 3))?;
        if frac.is_empty() {
            return None;
        }
        ms = frac.parse::<i64>().ok()? * 10i64.pow(3 - frac.len() as u32);
        i = j;
    }
    let offset_min = match b.get(i) {
        None | Some(b'Z') | Some(b'z') => 0,
        Some(&c) if c == b'+' || c == b'-' => {
            let sign = if c == b'+' { 1 } else { -1 };
            sign * (num(i + 1..i + 3)? * 60 + num(i + 4..i + 6)?)
        }
        _ => return None,
    };
    let days = days_from_civil(y, mo, d);
    Some((days * 86_400 + h * 3_600 + mi * 60 + se - offset_min * 60) * 1_000 + ms)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub struct Scanner {
    claude: Source,
    codex: Source,
}

impl Scanner {
    pub fn new() -> Self {
        Self { claude: Source::new(Kind::Claude), codex: Source::new(Kind::Codex) }
    }

    pub fn scan(&mut self, now: i64) {
        if let Some(dir) = paths::claude_config_dir() {
            self.claude.scan(&dir.join("projects"), now);
        }
        if let Some(dir) = paths::codex_home() {
            self.codex.scan(&dir.join("sessions"), now);
        }
    }

    pub fn usage(&self, provider: &str, session_start: i64, weekly_start: i64) -> TokenUsage {
        let src = if provider == "claude" { &self.claude } else { &self.codex };
        src.usage(session_start, weekly_start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_timestamps() {
        assert_eq!(parse_ts_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_ts_ms("2026-10-30T14:49:02.724Z"), Some(1_793_371_742_724));
        assert_eq!(parse_ts_ms("2026-10-30T16:49:02.7+02:00"), Some(1_793_371_742_700));
        assert_eq!(parse_ts_ms("nope"), None);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("limi-tokens-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("proj")).unwrap();
        d
    }

    fn claude_line(id: &str, req: &str, ts: &str, out: u64) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","requestId":"{req}","message":{{"id":"{id}","model":"claude-opus-5-5","usage":{{"input_tokens":10,"output_tokens":{out},"cache_read_input_tokens":1000,"cache_creation_input_tokens":200}}}}}}"#
        ) + "\n"
    }

    #[test]
    fn claude_counts_each_request_once_and_reads_incrementally() {
        let dir = temp_dir("claude");
        let file = dir.join("proj").join("s.jsonl");
        let mut f = File::create(&file).unwrap();
        // Same reply written twice (streaming), final line has the full output.
        f.write_all(claude_line("m1", "r1", "2026-10-30T10:00:00Z", 5).as_bytes()).unwrap();
        f.write_all(claude_line("m1", "r1", "2026-10-30T10:00:01Z", 50).as_bytes()).unwrap();
        f.write_all(b"{\"type\":\"user\",\"message\":{}}\n").unwrap();
        let now = parse_ts_ms("2026-10-30T12:00:00Z").unwrap();
        let mut src = Source::new(Kind::Claude);
        src.scan(&dir, now);
        let u = src.usage(0, 0);
        assert_eq!(u.session.requests, 1);
        assert_eq!(u.session.output, 50);
        assert_eq!(u.session.total, 10 + 50 + 1000 + 200);

        // A resumed session copying the same reply into another file is not double counted.
        std::fs::write(dir.join("proj").join("resumed.jsonl"), claude_line("m1", "r1", "2026-10-30T10:00:01Z", 50)).unwrap();
        // New reply appended later, plus a half-written line.
        f.write_all(claude_line("m2", "r2", "2026-10-30T11:30:00Z", 7).as_bytes()).unwrap();
        f.write_all(b"{\"type\":\"assist").unwrap();
        src.scan(&dir, now);
        let u = src.usage(parse_ts_ms("2026-10-30T11:00:00Z").unwrap(), 0);
        assert_eq!(u.weekly.requests, 2);
        assert_eq!(u.session.requests, 1);
        assert_eq!(u.session.output, 7);
        assert_eq!(u.last_activity_at, parse_ts_ms("2026-10-30T11:30:00Z"));
    }

    fn codex_line(ts: &str, input: u64, cached: u64, output: u64) -> String {
        format!(
            r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{input},"cached_input_tokens":{cached},"output_tokens":{output},"total_tokens":{}}}}}}}}}"#,
            input + output
        ) + "\n"
    }

    #[test]
    fn codex_uses_running_total_and_ignores_repeats() {
        let dir = temp_dir("codex");
        let file = dir.join("proj").join("rollout.jsonl");
        let body = [
            codex_line("2026-10-30T10:00:00Z", 1000, 800, 50),
            codex_line("2026-10-30T10:00:00Z", 1000, 800, 50), // repeated event
            codex_line("2026-10-30T11:00:00Z", 3000, 2500, 80),
            r#"{"timestamp":"2026-10-30T11:00:01Z","type":"event_msg","payload":{"type":"token_count","info":null}}"#.to_string() + "\n",
        ]
        .concat();
        std::fs::write(&file, body).unwrap();
        let mut src = Source::new(Kind::Codex);
        src.scan(&dir, parse_ts_ms("2026-10-30T12:00:00Z").unwrap());
        let u = src.usage(parse_ts_ms("2026-10-30T10:30:00Z").unwrap(), 0);
        assert_eq!(u.weekly.requests, 2);
        assert_eq!(u.weekly.total, 3080);
        assert_eq!(u.weekly.cache_read, 2500);
        assert_eq!(u.weekly.input, 500);
        assert_eq!(u.session.requests, 1);
        assert_eq!(u.session.total, 2030);
    }
}
