//! Per-repository settings and worktrees.
//!
//! The rule: what belongs to the repository itself (pinned refs, issue link
//! rules) is shared by all its worktrees and saved under the common git
//! folder's key (`shared_key`). View state (open folders, sort order, graph
//! options, collapsed merges) belongs to one worktree and is saved under the
//! worktree's path, as before. The worktree entry keeps a full copy, so
//! settings saved by older versions (worktree key only) still load: the
//! shared part falls back to it until a shared entry exists.

use crate::settings::Settings;
use crate::tab::RepoTab;
use crate::ui::history_model::{HistorySettings, RepoPrefs as HistoryPrefs};
use crate::ui::sidebar_model::{self, RepoPrefs as SidebarPrefs};

/// Key of the settings shared by every worktree of `tab`'s repository.
pub fn shared_key(tab: &RepoTab) -> String {
    let dir = tab.repo.common_dir();
    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let text = dir.to_string_lossy();
    format!("common:{}", text.trim_end_matches(['/', '\\']))
}

// ----- Sidebar (`Settings::repos`) -----

pub fn sidebar_prefs(settings: &Settings, tab: &RepoTab) -> SidebarPrefs {
    let mut prefs = settings
        .repos
        .get(&sidebar_model::repo_key(tab.path()))
        .cloned()
        .unwrap_or_default();
    if let Some(shared) = settings.repos.get(&shared_key(tab)) {
        prefs.pinned = shared.pinned.clone();
    }
    prefs
}

/// Saves the sidebar prefs of `tabs[i]`, and hands the shared part to the
/// other open worktrees of the same repository.
pub fn store_sidebar(settings: &mut Settings, tabs: &mut [RepoTab], i: usize, prefs: SidebarPrefs) {
    let key = shared_key(&tabs[i]);
    for (j, t) in tabs.iter_mut().enumerate() {
        if j != i && shared_key(t) == key {
            t.sidebar.prefs.pinned = prefs.pinned.clone();
        }
    }
    let shared = SidebarPrefs {
        pinned: prefs.pinned.clone(),
        ..Default::default()
    };
    settings.repos.insert(key, shared);
    settings
        .repos
        .insert(sidebar_model::repo_key(tabs[i].path()), prefs);
}

// ----- History (`Settings::history.repos`) -----

pub fn history_prefs(history: &HistorySettings, tab: &RepoTab) -> HistoryPrefs {
    let mut prefs = history.prefs_for(tab.path());
    if let Some(shared) = history.repos.get(&shared_key(tab)) {
        prefs.issue_rules = shared.issue_rules.clone();
    }
    prefs
}

/// Saves the history prefs of `tabs[i]`, sharing its issue rules.
pub fn store_history(history: &mut HistorySettings, tabs: &mut [RepoTab], i: usize) {
    let key = shared_key(&tabs[i]);
    let rules = tabs[i].history.ext.prefs.issue_rules.clone();
    for (j, t) in tabs.iter_mut().enumerate() {
        if j != i && shared_key(t) == key {
            t.history.ext.prefs.issue_rules = rules.clone();
        }
    }
    history.store(tabs[i].path(), &tabs[i].history.ext.prefs);
    let shared = HistoryPrefs {
        issue_rules: rules,
        ..Default::default()
    };
    history.repos.insert(key, shared);
}
