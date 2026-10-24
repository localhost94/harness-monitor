//! Which accounts this install watches.
//!
//! One person's machine has one home; a team lead watching marketing's Claude
//! subscription and engineering's Claude subscription needs them kept apart,
//! not summed - two accounts on the same plan tier hit two separate 5-hour
//! windows, and one number for both is worse than no number.
//!
//! An account is just a home directory. Everything the adapters read hangs off
//! one (`.claude`, `.codex`, `.local/share/opencode`, `.gemini`), so pointing
//! a second `PathResolver` at a second root is the whole mechanism - see
//! `PathResolver::for_home`.
//!
//! No config file means one account, this user's own home, and every surface
//! renders exactly as it did before accounts existed.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Shown when the user never configured accounts. Deliberately not a division
/// name: an unconfigured install has no divisions to speak of.
pub const DEFAULT_LABEL: &str = "this machine";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// Unique within one install - it is part of session identity.
    pub label: String,
    /// Free text. Empty means ungrouped, which the UI renders without a header
    /// rather than inventing a "misc" bucket.
    #[serde(default)]
    pub division: String,
    pub home: PathBuf,
}

/// `~/.config/harness-monitor/accounts.json`, or `HM_ACCOUNTS` pointing at any
/// file. Shape:
///
/// ```json
/// [
///   { "division": "marketing", "label": "claude-rina", "home": "~/accounts/rina" },
///   { "division": "engineering", "label": "claude-core", "home": "~" }
/// ]
/// ```
///
/// A harness that keeps its config somewhere other than the home it belongs to
/// (Claude Code's `CLAUDE_CONFIG_DIR`, say) is handled with a symlink inside
/// the account home - no second config shape here to get out of sync.
pub fn config_file() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("HM_ACCOUNTS") {
        return Some(PathBuf::from(explicit));
    }
    Some(dirs::config_dir()?.join("harness-monitor/accounts.json"))
}

pub fn load() -> Vec<Account> {
    let configured = config_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|raw| match serde_json::from_str::<Vec<Account>>(&raw) {
            Ok(list) => Some(list),
            // A typo in the config must not leave the user with a silently
            // empty window - fall through to the single-account default and
            // say why in the log.
            Err(err) => {
                tracing::warn!(%err, "accounts.json unreadable; watching this home only");
                None
            }
        });

    let list = configured.map(sanitize).unwrap_or_default();
    if list.is_empty() {
        return vec![default_account()];
    }
    list
}

fn default_account() -> Account {
    Account {
        label: DEFAULT_LABEL.into(),
        division: String::new(),
        home: dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")),
    }
}

/// Two accounts pointing at one home would double every session in the list
/// and fight over the same identity, so the first one wins. An unreachable
/// home is kept on purpose: "ops has nothing detected" is a finding, and
/// dropping the row would render it as "ops does not exist".
fn sanitize(list: Vec<Account>) -> Vec<Account> {
    let mut out: Vec<Account> = Vec::with_capacity(list.len());
    for mut account in list {
        account.home = expand_tilde(&account.home);
        if account.label.trim().is_empty() {
            account.label = account
                .home
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| DEFAULT_LABEL.into());
        }
        let clash = out
            .iter()
            .any(|kept| kept.home == account.home || kept.label == account.label);
        if clash {
            tracing::warn!(label = %account.label, "duplicate account label or home; skipping");
            continue;
        }
        out.push(account);
    }
    out
}

/// `~` in a hand-written JSON path is not expanded by anything upstream, and a
/// literal `./~/accounts/rina` directory is never what was meant.
fn expand_tilde(path: &std::path::Path) -> PathBuf {
    let Ok(rest) = path.strip_prefix("~") else {
        return path.to_path_buf();
    };
    match dirs::home_dir() {
        Some(home) => home.join(rest),
        None => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(label: &str, home: &str) -> Account {
        Account {
            label: label.into(),
            division: "eng".into(),
            home: PathBuf::from(home),
        }
    }

    #[test]
    fn duplicate_homes_and_labels_are_dropped() {
        let kept = sanitize(vec![
            account("a", "/home/one"),
            account("b", "/home/one"), // same home, different label
            account("a", "/home/two"), // same label, different home
            account("c", "/home/two"),
        ]);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].label, "a");
        assert_eq!(kept[1].label, "c");
    }

    #[test]
    fn a_blank_label_falls_back_to_the_home_directory_name() {
        let kept = sanitize(vec![account("   ", "/home/rina")]);
        assert_eq!(kept[0].label, "rina");
    }

    #[test]
    fn tilde_expands_but_only_as_a_leading_component() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_tilde(&PathBuf::from("~/accounts/rina")), home.join("accounts/rina"));
        assert_eq!(
            expand_tilde(&PathBuf::from("/srv/~backup")),
            PathBuf::from("/srv/~backup")
        );
    }

    #[test]
    fn an_unreachable_home_is_kept_so_the_division_still_shows() {
        let kept = sanitize(vec![account("ops", "/mnt/not-mounted-right-now")]);
        assert_eq!(kept.len(), 1);
    }
}
