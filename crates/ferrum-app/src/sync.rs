//! Fetch, pull and push options remembered between sessions, and the
//! automatic background fetch.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use eframe::egui;
use ferrum_core::sync::{FetchOptions, PullOptions};
use serde::{Deserialize, Serialize};

use crate::command::Op;
use crate::tab::RepoTab;
use crate::ui::sidebar_model::repo_key;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncSettings {
    /// Minutes between automatic fetches of open repositories; 0 turns it off.
    pub auto_fetch_minutes: u32,
    pub fetch_prune: bool,
    pub fetch_tags: bool,
    /// The fetch dialog fetches every remote.
    pub fetch_all: bool,
    /// Pull with `--autostash`.
    pub pull_autostash: bool,
    /// Push annotated tags with the branch (`--follow-tags`).
    pub push_follow_tags: bool,
    /// Remotes left out of the automatic fetch, per repository.
    pub no_auto_fetch: BTreeMap<String, BTreeSet<String>>,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            auto_fetch_minutes: 10,
            fetch_prune: true,
            fetch_tags: true,
            fetch_all: true,
            pull_autostash: false,
            push_follow_tags: false,
            no_auto_fetch: BTreeMap::new(),
        }
    }
}

impl SyncSettings {
    pub fn fetch_options(&self, remotes: Vec<String>) -> FetchOptions {
        FetchOptions {
            remotes,
            prune: self.fetch_prune,
            tags: self.fetch_tags,
        }
    }

    pub fn auto_fetch_off(&self, tab: &RepoTab, remote: &str) -> bool {
        self.no_auto_fetch
            .get(&repo_key(tab.path()))
            .is_some_and(|s| s.contains(remote))
    }

    pub fn set_auto_fetch(&mut self, tab: &RepoTab, remote: &str, on: bool) {
        let key = repo_key(tab.path());
        let set = self.no_auto_fetch.entry(key.clone()).or_default();
        if on {
            set.remove(remote);
        } else {
            set.insert(remote.to_owned());
        }
        if set.is_empty() {
            self.no_auto_fetch.remove(&key);
        }
    }
}

/// Per-tab state of the automatic fetch.
#[derive(Default)]
pub struct TabSync {
    /// When the next automatic fetch is due.
    pub next_fetch: Option<Instant>,
    /// Remotes left out of the automatic fetch (copied from the settings).
    pub auto_off: BTreeSet<String>,
}

/// A change to the remembered options, run by the app with the active tab.
pub type SyncFn = Box<dyn FnOnce(&mut RepoTab, &mut SyncSettings)>;

/// Gives plain fetches and pulls the remembered options.
pub fn with_options(op: Op, s: &SyncSettings) -> Op {
    match op {
        Op::Fetch { remote } => Op::FetchWith(s.fetch_options(remote.into_iter().collect())),
        Op::Pull(mode) => Op::PullWith(PullOptions {
            mode,
            autostash: s.pull_autostash,
            ..Default::default()
        }),
        other => other,
    }
}

/// The first automatic fetch of a tab comes a bit after it opens, one tab
/// after the other, so they do not all hit the network together.
fn first_delay(index: usize) -> Duration {
    Duration::from_secs(30 + 20 * index as u64)
}

/// Called every frame: starts automatic fetches that are due.
pub fn auto_fetch(tabs: &mut [RepoTab], s: &SyncSettings, ctx: &egui::Context) {
    let now = Instant::now();
    let interval = Duration::from_secs(s.auto_fetch_minutes as u64 * 60);
    let mut soonest: Option<Instant> = None;
    for (i, tab) in tabs.iter_mut().enumerate() {
        tab.sync.auto_off = s
            .no_auto_fetch
            .get(&repo_key(tab.path()))
            .cloned()
            .unwrap_or_default();
        if s.auto_fetch_minutes == 0 {
            tab.sync.next_fetch = None;
            continue;
        }
        let due = *tab
            .sync
            .next_fetch
            .get_or_insert_with(|| now + first_delay(i).min(interval));
        if now < due {
            soonest = Some(soonest.map_or(due, |t| t.min(due)));
            continue;
        }
        let remotes: Vec<String> = tab
            .remotes
            .iter()
            .filter(|r| !tab.sync.auto_off.contains(&r.name))
            .map(|r| r.name.clone())
            .collect();
        let started = remotes.is_empty() || {
            let opts = FetchOptions {
                remotes,
                prune: true,
                tags: s.fetch_tags,
            };
            tab.run_background(Op::FetchWith(opts), ctx)
        };
        // Busy: try again shortly, never on top of the running operation.
        let next = now
            + if started {
                interval
            } else {
                Duration::from_secs(15)
            };
        tab.sync.next_fetch = Some(next);
        soonest = Some(soonest.map_or(next, |t| t.min(next)));
    }
    if let Some(t) = soonest {
        ctx.request_repaint_after(t.saturating_duration_since(now));
    }
}

/// How to undo a fast-forward: move the branches back (reset for HEAD).
pub fn undo_fast_forward(tab: &RepoTab, pairs: &[(String, String)]) -> Option<Vec<Op>> {
    pairs
        .iter()
        .map(|(b, _)| {
            let old = tab
                .repo
                .resolve(&format!("refs/heads/{b}"))
                .ok()?
                .to_string();
            Some(if tab.head.as_deref() == Some(b.as_str()) {
                Op::Reset {
                    rev: old,
                    mode: ferrum_core::ops::ResetMode::Keep,
                }
            } else {
                Op::MoveBranch {
                    name: b.clone(),
                    target: old,
                }
            })
        })
        .collect()
}
