//! Runs every detected adapter once per watched account and assembles a
//! snapshot. Shared by the in-process loop (Linux/macOS build) and the
//! `--agent` role (spawned inside WSL by the Windows build), so both produce
//! byte-identical snapshots.
//!
//! One account is the common case and costs one pass. Adapters are per
//! account, not shared: `ClaudeCodeAdapter` and `OpenCodeAdapter` both carry a
//! read cursor, and one cursor serving two homes would attribute one account's
//! new tokens to whichever account was scanned next.

use crate::accounts::{self, Account};
use crate::adapters::{self, HarnessAdapter};
use crate::herdr;
use crate::model::{now_ms, AccountSnapshot, AgentSession, HarnessId, QuotaSnapshot, Snapshot};
use crate::paths::PathResolver;
use crate::quota;

pub struct Scanner {
    watched: Vec<Watched>,
}

struct Watched {
    account: Account,
    paths: PathResolver,
    adapters: Vec<Box<dyn HarnessAdapter>>,
    detected: Vec<HarnessId>,
}

impl Scanner {
    pub fn new() -> Self {
        Self::for_accounts(accounts::load())
    }

    pub fn for_accounts(accounts: Vec<Account>) -> Self {
        let watched = accounts
            .into_iter()
            .map(|account| Watched::new(PathResolver::for_home(account.home.clone()), account))
            .collect();
        Self { watched }
    }

    /// Single unnamed account at an explicit root. Used by the tests, and by
    /// anything that wants today's behaviour with no config file involved.
    pub fn with_paths(paths: PathResolver) -> Self {
        let account = Account {
            label: accounts::DEFAULT_LABEL.into(),
            division: String::new(),
            home: Default::default(),
        };
        Self {
            watched: vec![Watched::new(paths, account)],
        }
    }

    /// `(account label, harnesses found)` - startup log only.
    pub fn detected(&self) -> Vec<(&str, &[HarnessId])> {
        self.watched
            .iter()
            .map(|w| (w.account.label.as_str(), w.detected.as_slice()))
            .collect()
    }

    pub fn tick(&mut self) -> Snapshot {
        let mut sessions = Vec::new();
        let mut quotas = Vec::new();
        let mut accounts = Vec::with_capacity(self.watched.len());

        for watched in self.watched.iter_mut() {
            sessions.append(&mut watched.scan());
            if let Some(found) = watched.quota() {
                quotas.push(found);
            }
            accounts.push(AccountSnapshot {
                label: watched.account.label.clone(),
                division: watched.account.division.clone(),
                detected: watched.detected.clone(),
            });
        }

        // Adapters know what a session is doing; herdr knows where it is. It
        // sees the panes on this host regardless of which account owns them,
        // so it runs once for all of them.
        let locations = herdr::locations();
        if !locations.is_empty() {
            for session in sessions.iter_mut() {
                if let Some(found) = locations.get(&session.session_id) {
                    session.jump_target = Some(found.pane_id.clone());
                    session.terminal_title = found.terminal_title.clone();
                }
            }
        }

        Snapshot {
            taken_at: now_ms(),
            accounts,
            sessions,
            quotas,
            reseed: false,
        }
    }
}

impl Watched {
    fn new(paths: PathResolver, account: Account) -> Self {
        let adapters = adapters::all();
        let detected = adapters
            .iter()
            .filter(|a| a.detect(&paths))
            .map(|a| a.id())
            .collect();
        Self {
            account,
            paths,
            adapters,
            detected,
        }
    }

    fn scan(&mut self) -> Vec<AgentSession> {
        let mut sessions = Vec::new();
        for adapter in self.adapters.iter_mut() {
            if !self.detected.contains(&adapter.id()) {
                continue;
            }
            match adapter.scan(&self.paths) {
                Ok(mut found) => sessions.append(&mut found),
                // A parse failure must degrade one harness, never kill the
                // loop - and never take the other accounts down with it.
                Err(err) => tracing::warn!(
                    account = %self.account.label,
                    harness = ?adapter.id(),
                    %err,
                    "adapter scan failed"
                ),
            }
        }
        // Stamped here rather than in every adapter: an adapter reads one
        // tree and has no idea whose it is.
        for session in sessions.iter_mut() {
            session.key.account = self.account.label.clone();
        }
        sessions
    }

    fn quota(&self) -> Option<QuotaSnapshot> {
        quota::read(&self.paths).map(|q| QuotaSnapshot {
            account: self.account.label.clone(),
            ..q
        })
    }
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}
