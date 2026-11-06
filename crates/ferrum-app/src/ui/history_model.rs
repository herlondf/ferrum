//! Data behind the history view: loading the log with the repository's graph
//! options, collapsed merges, search matches and saved preferences.

use std::collections::{HashMap, HashSet};

use ferrum_core::graph::{self, index_of};
use ferrum_core::log::{LogOptions, PathHit};
use ferrum_core::{CommitInfo, Oid, RefKind, RefLabel};
use serde::{Deserialize, Serialize};

use crate::tab::RepoTab;

/// Commits loaded at a time; scrolling near the end loads the next page.
pub const PAGE: usize = 10_000;
/// Searches kept in the recent list.
const MAX_RECENT: usize = 10;

/// A link to an issue tracker: commits whose summary matches `pattern` get a
/// link to `url`, where `$0` is the whole match and `$1`… its groups.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IssueRule {
    pub pattern: String,
    pub url: String,
}

/// History options saved per repository.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RepoPrefs {
    pub log: LogOptions,
    /// Every merge starts collapsed.
    pub collapse_all: bool,
    /// Merges collapsed by hand (or, with `collapse_all`, expanded), as hex ids.
    pub toggled: Vec<String>,
    pub issue_rules: Vec<IssueRule>,
}

/// History options for every repository (`Settings::history`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistorySettings {
    pub show_avatars: bool,
    /// Download pictures from Gravatar / GitHub; off shows initials only.
    pub network_avatars: bool,
    pub compact_labels: bool,
    /// Commit list and details side by side.
    pub vertical_layout: bool,
    /// In the side-by-side layout, keep one line per commit.
    pub single_line_rows: bool,
    pub recent_searches: Vec<String>,
    /// Keyed by repository path.
    pub repos: HashMap<String, RepoPrefs>,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            show_avatars: true,
            network_avatars: true,
            compact_labels: false,
            vertical_layout: false,
            single_line_rows: false,
            recent_searches: Vec::new(),
            repos: HashMap::new(),
        }
    }
}

impl HistorySettings {
    pub fn prefs_for(&self, path: &std::path::Path) -> RepoPrefs {
        self.repos
            .get(&path.to_string_lossy().into_owned())
            .cloned()
            .unwrap_or_default()
    }

    /// Stores a repository's preferences (nothing when they are the defaults).
    pub fn store(&mut self, path: &std::path::Path, prefs: &RepoPrefs) {
        let key = path.to_string_lossy().into_owned();
        if *prefs == RepoPrefs::default() {
            self.repos.remove(&key);
        } else {
            self.repos.insert(key, prefs.clone());
        }
    }

    pub fn remember_search(&mut self, q: &str) {
        let q = q.trim();
        if q.is_empty() {
            return;
        }
        self.recent_searches.retain(|s| s != q);
        self.recent_searches.insert(0, q.to_owned());
        self.recent_searches.truncate(MAX_RECENT);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
    /// Message, author or hash, filtered as you type.
    #[default]
    Text,
    /// Changes that add or remove the text (`git log -S`).
    Changes,
    /// Changed file paths.
    Paths,
}

/// History state beyond the basic selection and search text.
#[derive(Default)]
pub struct HistoryExt {
    pub prefs: RepoPrefs,
    /// How many commits to load.
    pub limit: usize,
    pub truncated: bool,
    /// Every loaded commit while some are hidden by collapsed merges
    /// (empty otherwise: `RepoTab::commits` holds them all).
    full: Vec<CommitInfo>,
    pub has_body: HashSet<Oid>,
    pub unreachable: HashSet<Oid>,
    pub stashes: HashMap<Oid, usize>,
    pub unpushed: HashSet<Oid>,
    pub unpulled: HashSet<Oid>,
    /// Number of commits hidden under each collapsed merge.
    pub hidden_under: HashMap<Oid, usize>,
    /// Commits each merge brought in, for its tooltip.
    pub merge_cache: HashMap<Oid, Vec<CommitInfo>>,

    pub mode: SearchMode,
    /// Search only the current branch.
    pub scope_branch: bool,
    pub path_hits: HashMap<Oid, Vec<String>>,
    /// Text of the last search run as a job, to tell when Enter should run it again.
    pub searched: Option<String>,

    pub highlight_author: Option<String>,
    /// Horizontal scroll of the graph, in points, when it is wider than its column.
    pub graph_offset: f32,
    /// Commit being dragged to reorder or squash.
    pub drag: Option<Oid>,
    /// Text of the "Go to commit" box, when open.
    pub goto: Option<String>,
    pub issue_window: bool,
    pub stats: crate::ui::stats::StatsState,
}

impl HistoryExt {
    pub fn is_collapsed(&self, id: Oid) -> bool {
        let toggled = self.prefs.toggled.iter().any(|t| t == &id.to_string());
        self.prefs.collapse_all != toggled
    }

    /// Number of commits loaded, including those hidden under collapsed merges.
    pub fn loaded(&self, shown: usize) -> usize {
        if self.full.is_empty() {
            shown
        } else {
            self.full.len()
        }
    }

    pub fn any_collapsed(&self) -> bool {
        self.prefs.collapse_all || !self.prefs.toggled.is_empty()
    }
}

/// Loads the history with the tab's options (refs, order, filters, limit).
pub fn load(tab: &mut RepoTab) -> Result<(), ferrum_core::Error> {
    let ext = &mut tab.history.ext;
    if ext.limit == 0 {
        ext.limit = PAGE;
    }
    let log = tab.repo.log(&ext.prefs.log, ext.limit)?;
    ext.truncated = log.truncated;
    ext.has_body = log.has_body;
    ext.unreachable = log.unreachable;
    ext.stashes = log.stashes;
    let (ahead, behind) = tab.repo.unsynced(2_000).unwrap_or_default();
    ext.unpushed = ahead;
    ext.unpulled = behind;
    ext.full.clear();
    tab.commits = log.commits;
    apply_collapse(tab);
    Ok(())
}

/// Hides the commits under collapsed merges and lays out the graph.
pub fn apply_collapse(tab: &mut RepoTab) {
    let ext = &mut tab.history.ext;
    let all = if ext.full.is_empty() {
        std::mem::take(&mut tab.commits)
    } else {
        std::mem::take(&mut ext.full)
    };
    ext.hidden_under.clear();
    let first_parent = ext.prefs.log.first_parent;
    if !ext.any_collapsed() || first_parent {
        tab.graph = graph::layout_with(&all, |_, n, _| !(first_parent && n > 0));
        tab.index = index_of(&all);
        tab.commits = all;
        return;
    }
    let toggled: HashSet<Oid> = ext
        .prefs
        .toggled
        .iter()
        .filter_map(|t| Oid::from_str(t).ok())
        .collect();
    let all_shut = ext.prefs.collapse_all;
    let collapsed = |c: &CommitInfo| c.parents.len() > 1 && all_shut != toggled.contains(&c.id);
    let shown = visible(&all, collapsed);
    // Credit each hidden commit to the nearest collapsed merge above it.
    let mut owner: HashMap<Oid, Oid> = HashMap::new();
    for (i, c) in all.iter().enumerate() {
        let parent_owner = if shown[i] {
            collapsed(c).then_some(c.id)
        } else {
            owner.get(&c.id).copied()
        };
        if let Some(o) = parent_owner {
            for p in c.parents.iter().skip(usize::from(shown[i])) {
                owner.entry(*p).or_insert(o);
            }
        }
        if !shown[i]
            && let Some(o) = owner.get(&c.id)
        {
            *ext.hidden_under.entry(*o).or_default() += 1;
        }
    }
    let hidden: HashSet<Oid> = all
        .iter()
        .zip(&shown)
        .filter(|(_, s)| !**s)
        .map(|(c, _)| c.id)
        .collect();
    let commits: Vec<CommitInfo> = all
        .iter()
        .zip(&shown)
        .filter(|(_, s)| **s)
        .map(|(c, _)| c.clone())
        .collect();
    tab.graph = graph::layout_with(&commits, |row, n, p| {
        !(n > 0 && collapsed(&commits[row])) && !hidden.contains(&p)
    });
    tab.index = index_of(&commits);
    tab.commits = commits;
    ext.full = all;
}

/// Which commits stay visible: tips, and parents reached from visible commits,
/// except the merged-in parents of collapsed merges. `all` must be in
/// topological order (children first).
fn visible(all: &[CommitInfo], collapsed: impl Fn(&CommitInfo) -> bool) -> Vec<bool> {
    let index = index_of(all);
    let mut has_child = vec![false; all.len()];
    for c in all {
        for p in &c.parents {
            if let Some(&i) = index.get(p) {
                has_child[i] = true;
            }
        }
    }
    let mut reached = vec![false; all.len()];
    let mut shown = vec![false; all.len()];
    for (i, c) in all.iter().enumerate() {
        shown[i] = !has_child[i] || reached[i];
        if !shown[i] {
            continue;
        }
        let parents = if collapsed(c) {
            &c.parents[..1]
        } else {
            &c.parents[..]
        };
        for p in parents {
            if let Some(&j) = index.get(p) {
                reached[j] = true;
            }
        }
    }
    shown
}

/// Collapses or expands one merge.
pub fn toggle_merge(tab: &mut RepoTab, id: Oid) {
    let key = id.to_string();
    let toggled = &mut tab.history.ext.prefs.toggled;
    if let Some(i) = toggled.iter().position(|t| *t == key) {
        toggled.remove(i);
    } else {
        toggled.push(key);
    }
    after_collapse_change(tab);
}

pub fn set_merge_collapsed(tab: &mut RepoTab, id: Oid, collapse: bool) {
    let is_merge = tab
        .index
        .get(&id)
        .is_some_and(|&i| tab.commits[i].parents.len() > 1);
    if is_merge && tab.history.ext.is_collapsed(id) != collapse {
        toggle_merge(tab, id);
    }
}

pub fn set_all_collapsed(tab: &mut RepoTab, collapse: bool) {
    let prefs = &mut tab.history.ext.prefs;
    prefs.collapse_all = collapse;
    prefs.toggled.clear();
    after_collapse_change(tab);
}

fn after_collapse_change(tab: &mut RepoTab) {
    apply_collapse(tab);
    tab.apply_search();
    if let Some(id) = tab.history.selected
        && !tab.index.contains_key(&id)
    {
        tab.history.selected = None;
        tab.commit_view = None;
    }
}

/// Reloads after the graph options changed.
pub fn reload(tab: &mut RepoTab) {
    let started = std::time::Instant::now();
    if let Err(e) = load(tab) {
        tab.error(e.to_string());
    }
    tab.apply_search();
    if std::env::var_os("FERRUM_TIMING").is_some() {
        eprintln!(
            "history load: {:?} ({} commits)",
            started.elapsed(),
            tab.commits.len()
        );
    }
}

/// Loads the next page of commits.
pub fn load_more(tab: &mut RepoTab) {
    if !tab.history.ext.truncated {
        return;
    }
    tab.history.ext.limit = tab.history.ext.limit.max(PAGE) + PAGE;
    reload(tab);
}

/// Commits reachable from HEAD among the loaded ones.
fn head_set(tab: &RepoTab) -> HashSet<Oid> {
    let mut out = HashSet::new();
    let mut stack: Vec<Oid> = tab.head_oid.into_iter().collect();
    while let Some(id) = stack.pop() {
        let Some(&i) = tab.index.get(&id) else {
            continue;
        };
        if out.insert(id) {
            stack.extend(tab.commits[i].parents.iter().copied());
        }
    }
    out
}

/// Filters the list by the search text (or the hits of a search job).
pub fn apply_search(tab: &mut RepoTab) {
    let q = tab.history.search.trim().to_lowercase();
    if q.is_empty() {
        tab.history.visible = None;
        return;
    }
    let ext = &tab.history.ext;
    let scope = ext.scope_branch.then(|| head_set(tab));
    let in_scope = |c: &CommitInfo| scope.as_ref().is_none_or(|s| s.contains(&c.id));
    let visible = match ext.mode {
        SearchMode::Changes | SearchMode::Paths => match &tab.history.content_hits {
            Some(hits) => (0..tab.commits.len())
                .filter(|&i| hits.contains(&tab.commits[i].id) && in_scope(&tab.commits[i]))
                .collect(),
            None => Vec::new(),
        },
        SearchMode::Text => tab
            .commits
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                in_scope(c)
                    && (c.summary.to_lowercase().contains(&q)
                        || c.author.to_lowercase().contains(&q)
                        || c.email.to_lowercase().contains(&q)
                        || c.id.to_string().starts_with(&q))
            })
            .map(|(i, _)| i)
            .collect(),
    };
    tab.history.visible = Some(visible);
}

/// Stores the result of a path search.
pub fn set_path_hits(tab: &mut RepoTab, hits: Vec<PathHit>) {
    tab.history.content_hits = Some(hits.iter().map(|(id, _)| *id).collect());
    tab.history.ext.path_hits = hits.into_iter().collect();
    apply_search(tab);
}

/// Moves the selection to the next (or previous) commit in the filtered list.
pub fn step_match(tab: &mut RepoTab, forward: bool) {
    let Some(rows) = tab.history.visible.clone() else {
        return;
    };
    if rows.is_empty() {
        return;
    }
    let current = tab
        .history
        .selected
        .and_then(|id| tab.index.get(&id))
        .and_then(|idx| rows.iter().position(|r| r == idx));
    let next = match (current, forward) {
        (None, true) => 0,
        (None, false) => rows.len() - 1,
        (Some(p), true) => (p + 1) % rows.len(),
        (Some(p), false) => (p + rows.len() - 1) % rows.len(),
    };
    let id = tab.commits[rows[next]].id;
    tab.history.selected = Some(id);
    tab.load_commit(id, false);
    tab.history.scroll_to_selected = true;
}

/// Selects HEAD's commit, loading more history if needed.
pub fn jump_to(tab: &mut RepoTab, id: Oid) -> bool {
    while !tab.index.contains_key(&id) && tab.history.ext.truncated {
        load_more(tab);
    }
    if tab.index.contains_key(&id) {
        tab.select_commit(id);
        true
    } else {
        false
    }
}

/// Full ref name of a label, for matching graph filters.
pub fn label_ref(l: &RefLabel) -> String {
    match l.kind {
        RefKind::Local => format!("refs/heads/{}", l.name),
        RefKind::Remote => format!("refs/remotes/{}", l.name),
        RefKind::Tag => format!("refs/tags/{}", l.name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(n: u8) -> Oid {
        Oid::from_bytes(&[n; 20]).unwrap()
    }

    fn commit(id: u8, parents: &[u8]) -> CommitInfo {
        CommitInfo {
            id: oid(id),
            summary: String::new(),
            author: String::new(),
            email: String::new(),
            time: 0,
            offset_minutes: 0,
            parents: parents.iter().map(|&p| oid(p)).collect(),
        }
    }

    #[test]
    fn collapsed_merge_hides_what_it_brought_in() {
        // 5 merges 4 (feature: 4 <- 3) into 2; all from 1.
        let all = [
            commit(5, &[2, 4]),
            commit(4, &[3]),
            commit(3, &[1]),
            commit(2, &[1]),
            commit(1, &[]),
        ];
        let open = visible(&all, |_| false);
        assert!(open.iter().all(|v| *v));
        let shut = visible(&all, |c| c.id == oid(5));
        assert_eq!(shut, [true, false, false, true, true]);
    }
}
