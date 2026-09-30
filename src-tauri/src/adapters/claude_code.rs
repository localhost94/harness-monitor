//! Claude Code adapter.
//!
//! Source: ~/.claude/sessions/<pid>.json, one file per CLI process, rewritten
//! by the harness itself. It carries an authoritative `status` and
//! `waitingFor`, so "finished" vs "needs you" is read, never guessed.
//!
//! Three things this file must get right:
//!   1. dead pids go to the *ended* list, never to `scan` (see liveness.rs) -
//!      most files are ghosts;
//!   2. identity is the process, not `sessionId`, which repeats across pids
//!      when a session is resumed;
//!   3. the two lists are decided by one function, so a row cannot land in the
//!      live list by accident - that is what would fire a false toast.

use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::HarnessAdapter;
use crate::liveness::{self, Liveness};
use crate::model::{AgentSession, FidelityTier, HarnessId, SessionKey, SessionState, TokenCounts};
use crate::paths::PathResolver;

#[derive(Default)]
pub struct ClaudeCodeAdapter {
    /// Per session id: how far we have read its transcript, and the totals so
    /// far. Transcripts are append-only, so re-reading from the start every
    /// tick would be pure waste - we keep a byte offset and read only what is
    /// new, the same shape as opencode's event cursor.
    usage: HashMap<String, TranscriptCursor>,
}

#[derive(Default)]
struct TranscriptCursor {
    offset: u64,
    totals: TokenCounts,
}

/// A transcript larger than this is tailed from the end rather than parsed in
/// full, so a months-old session cannot stall a tick. Its earlier tokens are
/// then missing, which is why the UI shows this as a running total rather than
/// a lifetime figure.
const MAX_FULL_PARSE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionFile {
    pid: i64,
    session_id: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    started_at: Option<i64>,
    #[serde(default)]
    proc_start: Option<String>,
    #[serde(default)]
    pid_domain: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    waiting_for: Option<String>,
    #[serde(default)]
    status_updated_at: Option<i64>,
    #[serde(default)]
    updated_at: Option<i64>,
}

impl HarnessAdapter for ClaudeCodeAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::ClaudeCode
    }

    fn detect(&self, paths: &PathResolver) -> bool {
        paths.claude_root().is_dir()
    }

    fn scan(&mut self, paths: &PathResolver) -> Result<Vec<AgentSession>> {
        let mut out = Vec::new();
        for path in self.state_files(paths) {
            match parse_session_file(&path) {
                Ok(parsed) if parsed.live => {
                    let mut session = parsed.session;
                    // Claude Code's state file has no token counts; those live
                    // in the session transcript.
                    session.tokens = self.read_usage(paths, &session);
                    out.push(session)
                }
                Ok(_) => {}
                Err(err) => tracing::debug!(?path, %err, "skipping unreadable session file"),
            }
        }
        Ok(out)
    }

    /// Every state file whose pid is gone, newest first.
    ///
    /// Nothing ever deletes `~/.claude/sessions/<pid>.json`, so this is where
    /// the app's memory of past work comes from. It is deliberately a separate
    /// pass from `scan`: these rows must not reach the differ, because a file
    /// frozen at `status:"busy"` reads as a turn that finished.
    ///
    /// Token totals are not read for these. A dead session's transcript cannot
    /// grow, so folding it in costs a full parse of every historical transcript
    /// on every tick and yields a number nobody asked for. The state file
    /// carries enough to list the session; the transcript is only worth
    /// reading while the session is still going.
    fn scan_ended(&mut self, paths: &PathResolver) -> Result<Vec<AgentSession>> {
        let mut out = Vec::new();
        for path in self.state_files(paths) {
            match parse_session_file(&path) {
                Ok(parsed) if !parsed.live => out.push(parsed.session),
                Ok(_) => {}
                Err(err) => tracing::debug!(?path, %err, "skipping unreadable session file"),
            }
        }
        // read_dir order is filesystem-dependent, and an unsorted list would
        // reshuffle between ticks.
        out.sort_by(|a, b| b.state_changed_at.cmp(&a.state_changed_at));
        out.truncate(super::max_ended());
        Ok(out)
    }
}

impl ClaudeCodeAdapter {
    /// `<pid>.json` state files only - the sibling `<pid>.<sha>.key` files are
    /// a different thing with a different extension.
    fn state_files(&self, paths: &PathResolver) -> Vec<PathBuf> {
        let dir = paths.claude_sessions();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("json"))
            .collect()
    }
}

impl ClaudeCodeAdapter {
    fn read_usage(&mut self, paths: &PathResolver, session: &AgentSession) -> Option<TokenCounts> {
        let transcript = paths
            .claude_root()
            .join("projects")
            .join(project_slug(&session.cwd))
            .join(format!("{}.jsonl", session.session_id));

        let cursor = self.usage.entry(session.session_id.clone()).or_default();
        match accumulate(&transcript, cursor) {
            Ok(()) => Some(cursor.totals),
            Err(err) => {
                tracing::debug!(?transcript, %err, "transcript unreadable");
                // Zeroed totals would read as "this session used nothing".
                if cursor.offset == 0 {
                    None
                } else {
                    Some(cursor.totals)
                }
            }
        }
    }
}

/// Claude Code names a project directory after its cwd with every separator
/// flattened to a dash: /mnt/c/D/agent -> -mnt-c-D-agent.
fn project_slug(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Reads the bytes appended since last time and folds their usage into the
/// running totals.
fn accumulate(path: &Path, cursor: &mut TranscriptCursor) -> Result<()> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();

    if cursor.offset == 0 && len > MAX_FULL_PARSE_BYTES {
        cursor.offset = len;
        return Ok(());
    }
    // Truncated or replaced (a resumed session can rewrite its transcript):
    // start over rather than reading from a meaningless offset.
    if len < cursor.offset {
        cursor.offset = 0;
        cursor.totals = TokenCounts::default();
    }
    if len == cursor.offset {
        return Ok(());
    }

    file.seek(SeekFrom::Start(cursor.offset))?;
    let mut fresh = String::new();
    file.take(len - cursor.offset).read_to_string(&mut fresh)?;

    // A tick can land mid-write, so stop at the last complete line and leave
    // the partial one for next time.
    let complete_to = match fresh.rfind('\n') {
        Some(idx) => idx + 1,
        None => return Ok(()),
    };

    for line in fresh[..complete_to].lines() {
        if line.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<TranscriptLine>(line) else {
            continue;
        };
        // Subagent turns are billed to the same account but belong to their
        // own sidechain; counting them here would double-count the parent.
        if entry.is_sidechain.unwrap_or(false) || entry.entry_type.as_deref() != Some("assistant") {
            continue;
        }
        let Some(usage) = entry.message.and_then(|m| m.usage) else {
            continue;
        };
        cursor.totals.input += usage.input_tokens.unwrap_or(0);
        cursor.totals.output += usage.output_tokens.unwrap_or(0);
        cursor.totals.cache_read += usage.cache_read_input_tokens.unwrap_or(0);
        cursor.totals.cache_write += usage.cache_creation_input_tokens.unwrap_or(0);
        cursor.totals.reasoning += usage
            .output_tokens_details
            .and_then(|d| d.thinking_tokens)
            .unwrap_or(0);
    }

    cursor.offset += complete_to as u64;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct TranscriptLine {
    #[serde(rename = "type")]
    entry_type: Option<String>,
    #[serde(rename = "isSidechain")]
    is_sidechain: Option<bool>,
    message: Option<TranscriptMessage>,
}

#[derive(Debug, Deserialize)]
struct TranscriptMessage {
    usage: Option<TranscriptUsage>,
}

#[derive(Debug, Deserialize)]
struct TranscriptUsage {
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_input_tokens: Option<i64>,
    cache_creation_input_tokens: Option<i64>,
    output_tokens_details: Option<OutputDetails>,
}

#[derive(Debug, Deserialize)]
struct OutputDetails {
    thinking_tokens: Option<i64>,
}

fn parse_session_file(path: &Path) -> Result<Parsed> {
    let raw = std::fs::read_to_string(path)?;
    let file: SessionFile = serde_json::from_str(&raw)?;

    let proc_start = file.proc_start.unwrap_or_default();
    // Two different failures, and they need opposite treatment:
    //   - no procStart at all: the pid cannot be checked against anything, so
    //     it cannot be called live. macOS lands here for every session.
    //   - a checked pid that is gone: definitively over.
    // Only the first is ever `Unknown` *and* excluded from the live list.
    let (liveness, live) = if proc_start.is_empty() {
        (Liveness::Unknown, false)
    } else {
        match liveness::check(file.pid, &proc_start) {
            Liveness::Dead => (Liveness::Dead, false),
            other => (other, true),
        }
    };

    let state = map_state(file.status.as_deref(), file.waiting_for.as_deref());
    let changed_at = file
        .status_updated_at
        .or(file.updated_at)
        .or(file.started_at)
        .unwrap_or(0);

    Ok(Parsed {
        session: AgentSession {
            key: SessionKey {
                harness: HarnessId::ClaudeCode,
                pid_domain: file.pid_domain.unwrap_or_else(|| "local".into()),
                pid: file.pid,
                proc_start,
            },
            session_id: file.session_id,
            cwd: PathBuf::from(file.cwd.unwrap_or_default()),
            name: file.name,
            // A dead process's last recorded status is history, not state.
            state: if live { state } else { SessionState::Ended },
            state_changed_at: changed_at,
            started_at: file.started_at.unwrap_or(changed_at),
            waiting_for: file.waiting_for,
            model: None,
            tokens: None,
            cost: None,
            is_background: file.kind.as_deref() == Some("bg"),
            tier: FidelityTier::Full,
            jump_target: None,
            terminal_title: None,
            liveness,
        },
        live,
    })
}

/// One state file, parsed, plus which list it belongs in. The two are decided
/// in the same place so no caller can put a row in the wrong one.
struct Parsed {
    session: AgentSession,
    live: bool,
}

fn map_state(status: Option<&str>, waiting_for: Option<&str>) -> SessionState {
    match status {
        Some("busy") => SessionState::Running,
        Some("waiting") => match waiting_for {
            Some(w) if w.contains("permission") => SessionState::AwaitingPermission,
            _ => SessionState::AwaitingInput,
        },
        Some("shell") => SessionState::Shell,
        Some("idle") => SessionState::Idle,
        _ => SessionState::ActiveUnknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn permission_prompt_is_its_own_state() {
        assert_eq!(
            map_state(Some("waiting"), Some("permission prompt")),
            SessionState::AwaitingPermission
        );
        assert_eq!(
            map_state(Some("waiting"), Some("input needed")),
            SessionState::AwaitingInput
        );
        // waiting with no reason still means the user is being waited on
        assert_eq!(
            map_state(Some("waiting"), None),
            SessionState::AwaitingInput
        );
    }

    #[test]
    fn known_statuses_map_directly() {
        assert_eq!(map_state(Some("busy"), None), SessionState::Running);
        assert_eq!(map_state(Some("idle"), None), SessionState::Idle);
        assert_eq!(map_state(Some("shell"), None), SessionState::Shell);
    }

    #[test]
    fn unknown_status_never_claims_idle() {
        // Claiming Idle would fire a false "done" toast.
        assert_eq!(
            map_state(Some("teleporting"), None),
            SessionState::ActiveUnknown
        );
        assert_eq!(map_state(None, None), SessionState::ActiveUnknown);
    }

    /// Every non-alphanumeric character becomes a dash, so a WSL path and a
    /// mounted Windows one land in the same directory under
    /// `~/.claude/projects` regardless of separators or spaces.
    #[test]
    fn project_slugs_flatten_every_separator() {
        // The adapter runs on the Linux/WSL side, so the slugs it has to match
        // are POSIX paths - including the /mnt/c ones from a Windows cwd.
        assert_eq!(project_slug(Path::new("/mnt/c/D/agent")), "-mnt-c-D-agent");
        assert_eq!(
            project_slug(Path::new("/home/you/my project")),
            "-home-you-my-project"
        );
        assert_eq!(
            project_slug(Path::new("/home/you/.dotfile")),
            "-home-you--dotfile"
        );
    }

    /// Trailing newline included: a line without one is a partial write, and
    /// the reader deliberately stops before the last complete line.
    fn transcript(lines: &[&str]) -> (tempfile::NamedTempFile, TranscriptCursor) {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let mut body = lines.join("\n");
        body.push('\n');
        std::io::Write::write_all(&mut file, body.as_bytes()).unwrap();
        file.flush().unwrap();
        (file, TranscriptCursor::default())
    }

    /// One JSON object per line, exactly as the transcript stores it: the
    /// reader parses line by line, so a pretty-printed object would not parse.
    const ASSISTANT: &str = r#"{"type":"assistant","message":{"usage":{"input_tokens":100,"output_tokens":200,"cache_read_input_tokens":30,"cache_creation_input_tokens":40,"output_tokens_details":{"thinking_tokens":7}}}}"#;

    #[test]
    fn an_assistant_turn_folds_into_the_totals() {
        let (file, mut cursor) = transcript(&[ASSISTANT]);
        accumulate(file.path(), &mut cursor).unwrap();
        assert_eq!(cursor.totals.input, 100);
        assert_eq!(cursor.totals.output, 200);
        assert_eq!(cursor.totals.cache_read, 30);
        assert_eq!(cursor.totals.cache_write, 40);
        assert_eq!(cursor.totals.reasoning, 7);
    }

    #[test]
    fn only_the_newly_appended_bytes_are_read() {
        // Transcripts are append-only and grow into the hundreds of megabytes;
        // re-reading from the start on every 1.5s tick is the one thing this
        // cursor exists to prevent.
        let (file, mut cursor) = transcript(&[ASSISTANT]);
        accumulate(file.path(), &mut cursor).unwrap();
        let after_first = cursor.offset;
        assert!(after_first > 0);

        // A second, identical pass adds nothing and moves nothing.
        accumulate(file.path(), &mut cursor).unwrap();
        assert_eq!(cursor.offset, after_first);
        assert_eq!(cursor.totals.input, 100);
    }

    #[test]
    fn later_turns_accumulate_onto_the_running_total() {
        let (file, mut cursor) = transcript(&[ASSISTANT]);
        accumulate(file.path(), &mut cursor).unwrap();

        let mut appended = std::fs::OpenOptions::new()
            .append(true)
            .open(file.path())
            .unwrap();
        std::io::Write::write_all(&mut appended, format!("\n{ASSISTANT}\n").as_bytes()).unwrap();
        drop(appended);

        accumulate(file.path(), &mut cursor).unwrap();
        assert_eq!(
            cursor.totals.input, 200,
            "not 100: the total is running, not per-turn"
        );
        assert_eq!(cursor.totals.output, 400);
    }

    #[test]
    fn sidechain_turns_are_not_double_counted() {
        // A subagent's tokens are attributed to the parent turn already.
        let sidechain = r#"{"type":"assistant","isSidechain":true,"message":{"usage":{"input_tokens":9999,"output_tokens":9999}}}"#;
        let (file, mut cursor) = transcript(&[ASSISTANT, sidechain]);
        accumulate(file.path(), &mut cursor).unwrap();
        assert_eq!(cursor.totals.input, 100);
    }

    #[test]
    fn user_turns_carry_no_usage_and_are_skipped() {
        let user = r#"{"type":"user","message":{"content":"hello"}}"#;
        let (file, mut cursor) = transcript(&[user, ASSISTANT]);
        accumulate(file.path(), &mut cursor).unwrap();
        assert_eq!(cursor.totals.input, 100);
    }

    #[test]
    fn a_half_written_line_is_left_for_the_next_tick() {
        // A tick can land mid-write. Reading the partial line would either fail
        // to parse it or, worse, count half a turn.
        let (file, mut cursor) = transcript(&[ASSISTANT]);
        let mut appended = std::fs::OpenOptions::new()
            .append(true)
            .open(file.path())
            .unwrap();
        std::io::Write::write_all(&mut appended, br#"{"type":"assistant","mess"#).unwrap();
        drop(appended);

        accumulate(file.path(), &mut cursor).unwrap();
        assert_eq!(cursor.totals.input, 100, "only the complete first line");
        let complete = cursor.offset;
        assert!(complete > 0 && complete < file.path().metadata().unwrap().len());

        // Once the rest of the line lands, it is picked up.
        let mut finished = std::fs::OpenOptions::new()
            .append(true)
            .open(file.path())
            .unwrap();
        std::io::Write::write_all(&mut finished, b"age\":{}}}}}\n").unwrap();
        drop(finished);
        accumulate(file.path(), &mut cursor).unwrap();
        assert_eq!(cursor.offset, file.path().metadata().unwrap().len());
    }

    #[test]
    fn a_malformed_line_does_not_stop_the_others() {
        // One corrupt line in a month of transcript must not zero the session.
        let (file, mut cursor) = transcript(&["{ truncated", ASSISTANT]);
        accumulate(file.path(), &mut cursor).unwrap();
        assert_eq!(cursor.totals.input, 100);
    }

    #[test]
    fn a_rewritten_transcript_starts_over() {
        // A resumed session can rewrite its transcript from scratch; carrying an
        // offset into a shorter file would read from the middle of nothing.
        let (file, mut cursor) = transcript(&[ASSISTANT, ASSISTANT]);
        accumulate(file.path(), &mut cursor).unwrap();
        assert_eq!(cursor.totals.input, 200);

        let (shorter, mut cursor) = (file.path().to_path_buf(), cursor);
        std::fs::write(&shorter, format!("{ASSISTANT}\n")).unwrap();
        accumulate(&shorter, &mut cursor).unwrap();
        assert_eq!(cursor.totals.input, 100, "replaced, so recounted from zero");
    }

    #[test]
    fn a_transcript_over_the_cap_is_tailed_rather_than_parsed() {
        // A months-old session must not stall a tick. Its earlier tokens are
        // then missing, which is why the UI calls the figure a running total.
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.as_file_mut()
            .set_len(MAX_FULL_PARSE_BYTES + 1)
            .unwrap();
        let mut cursor = TranscriptCursor::default();
        accumulate(file.path(), &mut cursor).unwrap();
        assert_eq!(cursor.totals, TokenCounts::default());
        assert!(cursor.offset > 0, "the cursor moved to the end");
    }

    #[test]
    fn a_missing_transcript_is_an_error_the_caller_turns_into_none() {
        // "This session used nothing" is a lie, so read_usage returns None when
        // there is nothing to read - see the caller's offset check.
        let mut cursor = TranscriptCursor::default();
        assert!(accumulate(Path::new("/nope/never.jsonl"), &mut cursor).is_err());
    }

    #[test]
    fn an_empty_transcript_yields_zero_rather_than_an_error() {
        let (file, mut cursor) = transcript(&[]);
        accumulate(file.path(), &mut cursor).unwrap();
        assert_eq!(cursor.totals, TokenCounts::default());
    }

    /// A state file that did not make the live list.
    ///
    /// Two different reasons get here - an absent `procStart` and a checked pid
    /// that is gone - and they need opposite treatment, so the reason is left to
    /// each test to assert.
    fn not_live(path: &Path) -> AgentSession {
        let parsed = parse_session_file(path).expect("a parsed session");
        assert!(!parsed.live, "this fixture was meant to be filed as ended");
        assert_eq!(parsed.session.state, SessionState::Ended);
        parsed.session
    }

    #[test]
    fn a_session_with_no_procstart_is_filed_not_claimed_live() {
        // "Cannot be disproved" is not evidence. Showing it as live would be the
        // single largest correctness risk in the app.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("1.json");
        std::fs::write(
            &file,
            br#"{"pid":1,"sessionId":"s","cwd":"/tmp","status":"busy","statusUpdatedAt":5}"#,
        )
        .unwrap();
        let session = not_live(&file);
        assert_eq!(
            session.liveness,
            Liveness::Unknown,
            "no procStart to check against"
        );
        assert_eq!(session.session_id, "s");
        assert_eq!(session.key.proc_start, "");
    }

    #[test]
    fn a_dead_process_loses_its_recorded_state() {
        // The file still says "busy"; presenting that as a live state would be
        // a lie about a process that exited months ago.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("2.json");
        std::fs::write(
            &file,
            br#"{"pid":999999998,"sessionId":"s","cwd":"/tmp","procStart":"1","status":"busy","statusUpdatedAt":5}"#,
        )
        .unwrap();
        let session = not_live(&file);
        assert_eq!(session.state, SessionState::Ended);
        assert_eq!(session.liveness, Liveness::Dead);
    }

    #[test]
    fn a_background_session_is_labelled() {
        // "bg" is a different kind of session, and a user glancing at a row
        // needs to know they are not looking at the foreground one.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("3.json");
        std::fs::write(
            &file,
            br#"{"pid":3,"sessionId":"s","cwd":"/tmp","kind":"bg","statusUpdatedAt":5}"#,
        )
        .unwrap();
        assert!(not_live(&file).is_background);
    }

    #[test]
    fn the_timestamp_preference_is_updated_then_started_at() {
        // `state_changed_at` is what the differ watches, so it has to be the
        // most specific stamp the file carries.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("4.json");
        std::fs::write(
            &file,
            br#"{"pid":4,"sessionId":"s","startedAt":1,"updatedAt":2,"statusUpdatedAt":3}"#,
        )
        .unwrap();
        assert_eq!(not_live(&file).state_changed_at, 3);

        std::fs::write(
            &file,
            br#"{"pid":4,"sessionId":"s","startedAt":1,"updatedAt":2}"#,
        )
        .unwrap();
        assert_eq!(not_live(&file).state_changed_at, 2);

        std::fs::write(&file, br#"{"pid":4,"sessionId":"s","startedAt":1}"#).unwrap();
        let session = not_live(&file);
        assert_eq!(session.state_changed_at, 1);
        assert_eq!(session.started_at, 1);
    }

    #[test]
    fn a_file_with_no_timestamp_at_all_does_not_render_as_the_epoch() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("5.json");
        std::fs::write(&file, br#"{"pid":5,"sessionId":"s"}"#).unwrap();
        assert_eq!(not_live(&file).state_changed_at, 0);
    }

    #[test]
    fn the_two_lists_are_decided_in_one_place() {
        // The live/ended decision is returned alongside the row precisely so no
        // caller can put a session in the wrong list by forgetting a condition.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("6.json");
        std::fs::write(&file, br#"{"pid":6,"sessionId":"s","statusUpdatedAt":5}"#).unwrap();
        let parsed = parse_session_file(&file).expect("parsed");
        assert!(!parsed.live);
        assert_eq!(parsed.session.state, SessionState::Ended);
    }
}
