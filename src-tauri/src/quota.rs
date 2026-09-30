//! Claude plan rate-limit window.
//!
//! Two sources, both push-only from a live Claude Code process:
//!   1. ~/.local/state/harness-monitor/quota.json - written by our statusline
//!      shim on every statusline render (fresh, sub-second);
//!   2. ~/.claude/llm-analytics-usage/*.jsonl - written by the user's Stop
//!      hook at the end of each turn (coarser, but works with no install).
//!
//! Whichever record is newer wins. The percentage is always MIRRORED, never
//! derived from token counts: the server's own numbers show why (extra_used
//! 50253 against a 10000 limit, with the pct field clamped at 100).

use crate::model::{now_ms, QuotaSnapshot};
use crate::paths::PathResolver;
use serde::Deserialize;

/// Past this age the reading is shown greyed out. It is never extrapolated
/// forward - a confidently wrong percentage is worse than an absent one.
pub const STALE_AFTER_MS: i64 = 15 * 60 * 1000;

pub fn read(paths: &PathResolver) -> Option<QuotaSnapshot> {
    let shim = read_shim(paths);
    let hook = read_hook(paths);
    match (shim, hook) {
        (Some(a), Some(b)) => Some(if a.at >= b.at { a } else { b }),
        (a, b) => a.or(b),
    }
}

pub fn is_stale(q: &QuotaSnapshot, now: i64) -> bool {
    now - q.at > STALE_AFTER_MS
}

#[derive(Debug, Deserialize)]
struct ShimFile {
    /// jq's `now`: epoch seconds, fractional.
    at: f64,
    rate_limits: Option<RateLimits>,
}

#[derive(Debug, Deserialize)]
struct RateLimits {
    five_hour: Option<Window>,
    seven_day: Option<Window>,
}

#[derive(Debug, Deserialize)]
struct Window {
    used_percentage: Option<f64>,
    resets_at: Option<serde_json::Value>,
}

fn read_shim(paths: &PathResolver) -> Option<QuotaSnapshot> {
    let raw = std::fs::read_to_string(paths.quota_state()).ok()?;
    let file: ShimFile = serde_json::from_str(&raw).ok()?;
    let limits = file.rate_limits?;
    Some(QuotaSnapshot {
        at: (file.at * 1000.0) as i64,
        source: "statusline".into(),
        tier: None,
        five_hour_pct: limits.five_hour.as_ref().and_then(|w| w.used_percentage),
        five_hour_resets_at: limits
            .five_hour
            .as_ref()
            .and_then(|w| w.resets_at.as_ref())
            .and_then(normalize_reset),
        seven_day_pct: limits.seven_day.as_ref().and_then(|w| w.used_percentage),
        seven_day_resets_at: limits
            .seven_day
            .as_ref()
            .and_then(|w| w.resets_at.as_ref())
            .and_then(normalize_reset),
    })
}

/// Claude Code sends `resets_at` to the statusline as an epoch NUMBER, while
/// the Stop-hook JSONL writes an RFC3339 string. Normalise to RFC3339 here so
/// the UI has exactly one shape to render a countdown from.
fn normalize_reset(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) => {
            let raw = n.as_f64()?;
            // Seconds or milliseconds - anything past ~5138 AD in seconds is
            // really milliseconds.
            let ms = if raw > 1e11 { raw } else { raw * 1000.0 };
            chrono::DateTime::from_timestamp_millis(ms as i64).map(|dt| dt.to_rfc3339())
        }
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
struct HookLine {
    at: String,
    #[serde(default)]
    tier: Option<String>,
    #[serde(default)]
    five_hour_pct: Option<f64>,
    #[serde(default)]
    five_hour_resets_at: Option<String>,
    #[serde(default)]
    seven_day_pct: Option<f64>,
    #[serde(default)]
    seven_day_resets_at: Option<String>,
}

fn read_hook(paths: &PathResolver) -> Option<QuotaSnapshot> {
    let dir = paths.claude_analytics();
    let newest = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("jsonl"))
        .filter_map(|e| {
            e.metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| (t, e.path()))
        })
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)?;

    let raw = std::fs::read_to_string(newest).ok()?;
    let line = raw.lines().rev().find(|l| !l.trim().is_empty())?;
    let parsed: HookLine = serde_json::from_str(line).ok()?;
    Some(QuotaSnapshot {
        at: parse_iso(&parsed.at).unwrap_or_else(now_ms),
        source: "stop-hook".into(),
        tier: parsed.tier,
        five_hour_pct: parsed.five_hour_pct,
        five_hour_resets_at: parsed.five_hour_resets_at,
        seven_day_pct: parsed.seven_day_pct,
        seven_day_resets_at: parsed.seven_day_resets_at,
    })
}

pub fn parse_iso(s: &str) -> Option<i64> {
    // Values seen in the wild: "2026-09-06T04:30:09.146523Z" and
    // "2026-09-06T06:49:59.869678+00:00".
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_timestamp_shapes() {
        assert!(parse_iso("2026-09-06T04:30:09.146523Z").is_some());
        assert!(parse_iso("2026-09-06T06:49:59.869678+00:00").is_some());
        assert!(parse_iso("not a date").is_none());
    }

    #[test]
    fn reset_stamps_normalise_to_rfc3339() {
        use serde_json::json;
        // What the statusline actually sends.
        let from_seconds = normalize_reset(&json!(1788750000)).unwrap();
        assert_eq!(parse_iso(&from_seconds), Some(1_788_750_000_000));
        // Millisecond epochs and plain strings both survive.
        let from_millis = normalize_reset(&json!(1788750000000i64)).unwrap();
        assert_eq!(parse_iso(&from_millis), Some(1_788_750_000_000));
        assert_eq!(
            normalize_reset(&json!("2026-09-06T06:49:59Z")).as_deref(),
            Some("2026-09-06T06:49:59Z")
        );
        assert_eq!(normalize_reset(&json!(null)), None);
        assert_eq!(normalize_reset(&json!("")), None);
    }

    #[test]
    fn staleness_uses_reading_age() {
        let q = QuotaSnapshot {
            at: 1_000_000,
            source: "stop-hook".into(),
            tier: None,
            five_hour_pct: Some(20.0),
            five_hour_resets_at: None,
            seven_day_pct: None,
            seven_day_resets_at: None,
        };
        assert!(!is_stale(&q, 1_000_000 + STALE_AFTER_MS - 1));
        assert!(is_stale(&q, 1_000_000 + STALE_AFTER_MS + 1));
    }

    /// A reading is exactly as stale as the boundary allows.
    #[test]
    fn the_staleness_boundary_is_inclusive_of_fresh() {
        let q = QuotaSnapshot {
            at: 0,
            source: "statusline".into(),
            tier: None,
            five_hour_pct: None,
            five_hour_resets_at: None,
            seven_day_pct: None,
            seven_day_resets_at: None,
        };
        assert!(
            !is_stale(&q, STALE_AFTER_MS),
            "exactly at the edge is still fresh"
        );
        assert!(is_stale(&q, STALE_AFTER_MS + 1));
    }

    /// The staleness window is duplicated in `src/types/index.ts` as
    /// `QUOTA_STALE_MS`. Nothing in either language can see the other, so the
    /// literal is pinned on this side of the boundary.
    #[test]
    fn the_stale_window_is_fifteen_minutes() {
        assert_eq!(STALE_AFTER_MS, 15 * 60 * 1000);
    }

    mod read {
        use super::*;
        use std::path::PathBuf;
        use tempfile::TempDir;

        /// The shim writes `{"at": <epoch seconds, fractional>, "rate_limits": …}`.
        /// The timestamp is given as RFC3339 and converted here, so a test never
        /// has to hand-compute an epoch constant that a wrong comment would then
        /// quietly agree with.
        fn shim_file(dir: &PathBuf, at: &str, five_hour_pct: f64) {
            let secs = parse_iso(at).expect("valid fixture timestamp") as f64 / 1000.0;
            std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
            std::fs::write(
                dir,
                format!(
                    r#"{{"at":{secs},"rate_limits":{{"five_hour":{{"used_percentage":{five_hour_pct},
                        "resets_at":1788750000}},"seven_day":{{"used_percentage":11}}}}}}"#
                ),
            )
            .unwrap();
        }

        /// The Stop hook writes RFC3339 strings, one JSON object per line.
        fn hook_file(dir: &PathBuf, name: &str, at: &str, five_hour_pct: f64) {
            std::fs::create_dir_all(dir).unwrap();
            let body = format!(
                concat!(
                    r#"{{"at":"2026-09-06T04:00:00.000000Z","five_hour_pct":1}}"#,
                    "\n",
                    r#"{{"at":"{at}","tier":"default_claude_max_5x","five_hour_pct":{five_hour_pct}}}"#,
                    "\n"
                ),
                at = at,
                five_hour_pct = five_hour_pct
            );
            std::fs::write(dir.join(name), body).unwrap();
        }

        fn resolver(tmp: &TempDir) -> PathResolver {
            PathResolver::for_home(tmp.path().to_path_buf())
        }

        #[test]
        fn no_sources_means_no_reading() {
            let tmp = TempDir::new().unwrap();
            assert_eq!(read(&resolver(&tmp)), None);
        }

        #[test]
        fn the_shim_alone_is_enough() {
            let tmp = TempDir::new().unwrap();
            let paths = resolver(&tmp);
            shim_file(&paths.quota_state(), "2026-09-06T06:49:59.869Z", 64.0);
            let q = read(&paths).expect("shim reading");
            assert_eq!(q.source, "statusline");
            assert_eq!(q.five_hour_pct, Some(64.0));
            assert_eq!(q.seven_day_pct, Some(11.0));
            // jq's `now` is fractional *seconds*; the model is milliseconds.
            assert_eq!(q.at, parse_iso("2026-09-06T06:49:59.869Z").unwrap());
        }

        #[test]
        fn the_stop_hook_alone_is_enough() {
            let tmp = TempDir::new().unwrap();
            let paths = resolver(&tmp);
            hook_file(
                &paths.claude_analytics(),
                "a.jsonl",
                "2026-09-06T06:49:59.869678+00:00",
                12.0,
            );
            let q = read(&paths).expect("hook reading");
            assert_eq!(q.source, "stop-hook");
            assert_eq!(q.five_hour_pct, Some(12.0));
            assert_eq!(q.tier.as_deref(), Some("default_claude_max_5x"));
        }

        #[test]
        fn the_newer_source_wins_whichever_it_is() {
            let tmp = TempDir::new().unwrap();
            let paths = resolver(&tmp);
            // The hook is a day old, the shim a moment old: the shim wins.
            hook_file(
                &paths.claude_analytics(),
                "a.jsonl",
                "2026-09-05T07:00:00.000000Z",
                12.0,
            );
            shim_file(&paths.quota_state(), "2026-09-06T07:00:00.000Z", 64.0);
            let q = read(&paths).expect("a reading");
            assert_eq!(q.source, "statusline", "the newer shim should win");
            assert_eq!(q.five_hour_pct, Some(64.0));

            // And the other way round.
            hook_file(
                &paths.claude_analytics(),
                "a.jsonl",
                "2026-09-07T07:00:00.000000Z",
                12.0,
            );
            shim_file(&paths.quota_state(), "2026-09-06T07:00:00.000Z", 64.0);
            let q = read(&paths).expect("a reading");
            assert_eq!(q.source, "stop-hook", "the newer hook should win");
            assert_eq!(q.five_hour_pct, Some(12.0));
        }

        #[test]
        fn a_tie_goes_to_the_shim() {
            // The shim is written on every statusline render and is therefore
            // the finer-grained of the two; on equal age it is the better read.
            let tmp = TempDir::new().unwrap();
            let paths = resolver(&tmp);
            let at = "2026-09-06T06:35:00.000Z";
            hook_file(&paths.claude_analytics(), "a.jsonl", at, 12.0);
            shim_file(&paths.quota_state(), at, 64.0);
            let q = read(&paths).expect("a reading");
            assert_eq!(q.source, "statusline");
        }

        #[test]
        fn a_malformed_shim_falls_through_to_the_hook() {
            // One bad JSON file must not cost the user the reading they have.
            let tmp = TempDir::new().unwrap();
            let paths = resolver(&tmp);
            std::fs::create_dir_all(paths.quota_state().parent().unwrap()).unwrap();
            std::fs::write(paths.quota_state(), "{ this is not json").unwrap();
            hook_file(
                &paths.claude_analytics(),
                "a.jsonl",
                "2026-09-06T06:49:59Z",
                12.0,
            );
            let q = read(&paths).expect("the hook reading should survive");
            assert_eq!(q.source, "stop-hook");
        }

        #[test]
        fn a_shim_with_no_rate_limits_is_not_a_reading() {
            // A statusline render before the first API response has `at` but
            // nothing to report; reporting 0% would be a lie.
            let tmp = TempDir::new().unwrap();
            let paths = resolver(&tmp);
            std::fs::create_dir_all(paths.quota_state().parent().unwrap()).unwrap();
            std::fs::write(paths.quota_state(), r#"{"at":1788750000.0}"#).unwrap();
            assert_eq!(read(&paths), None);
        }

        #[test]
        fn the_shim_percentage_is_mirrored_never_clamped_or_derived() {
            // The server clamps its own pct at 100 while reporting
            // extra_used 50253 against a 10000 limit. Mirroring is the whole
            // point; deriving from tokens would show something else entirely.
            let tmp = TempDir::new().unwrap();
            let paths = resolver(&tmp);
            shim_file(&paths.quota_state(), "2026-09-06T07:00:00.000Z", 100.0);
            let q = read(&paths).expect("a reading");
            assert_eq!(q.five_hour_pct, Some(100.0));
        }

        #[test]
        fn the_last_non_empty_line_of_the_hook_file_wins() {
            // A turn appends a line; the newest one is the reading.
            let tmp = TempDir::new().unwrap();
            let paths = resolver(&tmp);
            let dir = paths.claude_analytics();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("a.jsonl"),
                concat!(
                    r#"{"at":"2026-09-06T04:00:00Z","five_hour_pct":1}"#,
                    "\n",
                    r#"{"at":"2026-09-06T05:00:00Z","five_hour_pct":2}"#,
                    "\n",
                    "\n"
                ),
            )
            .unwrap();
            let q = read(&paths).expect("a reading");
            assert_eq!(q.five_hour_pct, Some(2.0));
        }

        #[test]
        fn a_malformed_hook_line_is_not_a_reading() {
            let tmp = TempDir::new().unwrap();
            let paths = resolver(&tmp);
            let dir = paths.claude_analytics();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("a.jsonl"), "not json\n").unwrap();
            assert_eq!(read(&paths), None);
        }

        #[test]
        fn a_missing_shim_timestamp_falls_back_to_now() {
            // Better a reading with an approximate age than none: the UI greys
            // the block out by age, and wrong-by-a-bit beats absent.
            let tmp = TempDir::new().unwrap();
            let paths = resolver(&tmp);
            let dir = paths.claude_analytics();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("a.jsonl"), "{\"at\":\"nonsense\"}\n").unwrap();
            let before = crate::model::now_ms();
            let q = read(&paths).expect("a reading");
            let after = crate::model::now_ms();
            assert!((before..=after).contains(&q.at), "at was {}", q.at);
        }
    }
}
