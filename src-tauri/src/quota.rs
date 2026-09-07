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
            chrono::DateTime::from_timestamp_millis(ms as i64)
                .map(|dt| dt.to_rfc3339())
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
        .filter_map(|e| e.metadata().ok().and_then(|m| m.modified().ok()).map(|t| (t, e.path())))
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
}
