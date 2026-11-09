//! Sidebar logic that does not draw: per-repository preferences, the folder
//! tree, natural sorting and keyboard navigation.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use eframe::egui;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SortOrder {
    #[default]
    Alphabetical,
    AlphabeticalDesc,
    /// Most recent tip commit first.
    MostRecent,
}

impl SortOrder {
    pub const ALL: [SortOrder; 3] = [
        SortOrder::Alphabetical,
        SortOrder::AlphabeticalDesc,
        SortOrder::MostRecent,
    ];

    pub fn label(self) -> String {
        match self {
            SortOrder::Alphabetical => t!("Alphabetically"),
            SortOrder::AlphabeticalDesc => t!("Alphabetically, descending"),
            SortOrder::MostRecent => t!("Most recent commit"),
        }
    }
}

/// Sidebar state saved per repository (in `Settings::repos`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RepoPrefs {
    /// Pinned refs, by full name (`refs/heads/main`, `refs/tags/v1`).
    pub pinned: Vec<String>,
    /// Sections and folders the user opened or closed, by row key.
    pub open: BTreeMap<String, bool>,
    pub branch_sort: SortOrder,
    pub tag_sort: SortOrder,
    pub folders_first: bool,
}

impl Default for RepoPrefs {
    fn default() -> Self {
        Self {
            pinned: Vec::new(),
            open: BTreeMap::new(),
            branch_sort: SortOrder::Alphabetical,
            tag_sort: SortOrder::Alphabetical,
            folders_first: true,
        }
    }
}

/// Key used for a repository in `Settings::repos`.
pub fn repo_key(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

// ----- Natural sort -----

/// Splits off the leading run of digits or of non-digits.
fn next_chunk(s: &str) -> Option<(&str, &str)> {
    let first = s.chars().next()?;
    let digit = first.is_ascii_digit();
    let end = s
        .char_indices()
        .find(|(_, c)| c.is_ascii_digit() != digit)
        .map_or(s.len(), |(i, _)| i);
    Some(s.split_at(end))
}

/// Compares names so that numbers sort by value: `v2` < `v10`. Case is
/// ignored, except to break ties.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a, b);
    loop {
        match (next_chunk(x), next_chunk(y)) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some((cx, rx)), Some((cy, ry))) => {
                let both_digits =
                    cx.as_bytes()[0].is_ascii_digit() && cy.as_bytes()[0].is_ascii_digit();
                let ord = if both_digits {
                    let (nx, ny) = (cx.trim_start_matches('0'), cy.trim_start_matches('0'));
                    nx.len().cmp(&ny.len()).then_with(|| nx.cmp(ny))
                } else {
                    cx.to_lowercase().cmp(&cy.to_lowercase())
                };
                if ord != Ordering::Equal {
                    return ord;
                }
                (x, y) = (rx, ry);
            }
        }
    }
}

pub fn sort_by<T>(
    list: &mut [T],
    order: SortOrder,
    name: impl Fn(&T) -> &str,
    time: impl Fn(&T) -> i64,
) {
    list.sort_by(|a, b| match order {
        SortOrder::Alphabetical => natural_cmp(name(a), name(b)),
        SortOrder::AlphabeticalDesc => natural_cmp(name(b), name(a)),
        SortOrder::MostRecent => time(b)
            .cmp(&time(a))
            .then_with(|| natural_cmp(name(a), name(b))),
    });
}

// ----- Folder tree -----

/// A branch folder (`feature/` in `feature/x`) or a branch.
pub enum Node<'a, T> {
    Folder {
        name: String,
        /// Folder path below the root, e.g. `feature/ui`.
        path: String,
        children: Vec<Node<'a, T>>,
        /// Branches inside, at any depth.
        count: usize,
        /// Most recent commit time inside.
        time: i64,
    },
    Leaf {
        /// Last part of the name.
        name: &'a str,
        item: &'a T,
        time: i64,
    },
}

impl<T> Node<'_, T> {
    fn name(&self) -> &str {
        match self {
            Node::Folder { name, .. } => name,
            Node::Leaf { name, .. } => name,
        }
    }

    fn time(&self) -> i64 {
        match self {
            Node::Folder { time, .. } | Node::Leaf { time, .. } => *time,
        }
    }
}

/// Builds the folder tree of `items` (name relative to the root, commit time, item).
pub fn tree<'a, T>(
    items: Vec<(&'a str, i64, &'a T)>,
    order: SortOrder,
    folders_first: bool,
) -> Vec<Node<'a, T>> {
    build(items, "", order, folders_first)
}

fn build<'a, T>(
    items: Vec<(&'a str, i64, &'a T)>,
    prefix: &str,
    order: SortOrder,
    folders_first: bool,
) -> Vec<Node<'a, T>> {
    let mut folders: BTreeMap<&str, Vec<(&'a str, i64, &'a T)>> = BTreeMap::new();
    let mut nodes = Vec::new();
    for (name, time, item) in items {
        match name.split_once('/') {
            Some((f, rest)) if !f.is_empty() && !rest.is_empty() => {
                folders.entry(f).or_default().push((rest, time, item))
            }
            _ => nodes.push(Node::Leaf { name, item, time }),
        }
    }
    for (f, list) in folders {
        let path = if prefix.is_empty() {
            f.to_owned()
        } else {
            format!("{prefix}/{f}")
        };
        let count = list.len();
        let time = list.iter().map(|i| i.1).max().unwrap_or(0);
        let children = build(list, &path, order, folders_first);
        nodes.push(Node::Folder {
            name: f.to_owned(),
            path,
            children,
            count,
            time,
        });
    }
    sort_by(&mut nodes, order, |n| n.name(), |n| n.time());
    if folders_first {
        // Stable: keeps the order within folders and within branches.
        nodes.sort_by_key(|n| !matches!(n, Node::Folder { .. }));
    }
    nodes
}

/// All folder paths of a list of names (`a/b/c` gives `a` and `a/b`).
pub fn folder_paths<'a>(names: impl Iterator<Item = &'a str>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for n in names {
        let mut end = 0;
        while let Some(i) = n[end..].find('/') {
            end += i;
            out.insert(n[..end].to_owned());
            end += 1;
        }
    }
    out
}

// ----- State and keyboard navigation -----

/// A row drawn in the sidebar, remembered for the arrow keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavRow {
    pub key: String,
    pub parent: Option<String>,
    /// For sections and folders: whether it is open.
    pub open: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavAction {
    /// Like a click: select the commit of the row.
    Select,
    /// Like a double-click: check out, open...
    Activate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKey {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Enter,
    Space,
}

/// A branch being dragged (sidebar row or graph label).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DragBranch {
    pub name: String,
    pub remote: bool,
}

/// The Merge / Rebase / Cherry-pick / Push menu shown after a drop.
#[derive(Debug, Clone)]
pub struct DropMenu {
    pub source: DragBranch,
    pub target: crate::ui::branch_dnd::Target,
    pub pos: egui::Pos2,
    pub just_opened: bool,
}

impl DropMenu {
    pub fn new(source: DragBranch, target: crate::ui::branch_dnd::Target, pos: egui::Pos2) -> Self {
        Self {
            source,
            target,
            pos,
            just_opened: true,
        }
    }
}

#[derive(Default)]
pub struct SidebarState {
    pub prefs: RepoPrefs,
    /// `prefs` changed and should be saved.
    pub dirty: bool,
    pub filter: String,
    /// Key of the row under the keyboard cursor.
    pub cursor: Option<String>,
    pub scroll_to_cursor: bool,
    /// Rows of the last frame, in order.
    pub rows: Vec<NavRow>,
    pub(crate) building: Vec<NavRow>,
    pub(crate) parents: Vec<String>,
    /// A keyboard action for the row under the cursor, run when it is drawn.
    pub action: Option<(String, NavAction)>,
    /// The sidebar has the keyboard focus (see `ui::focus`).
    pub has_focus: bool,
    /// Local branches picked for a bulk delete.
    pub selection: BTreeSet<String>,
    pub drop_menu: Option<DropMenu>,
    pub last_head: Option<String>,
    /// Scroll to the current branch (after a checkout).
    pub reveal: bool,
}

impl SidebarState {
    pub fn with_prefs(prefs: RepoPrefs) -> Self {
        Self {
            prefs,
            ..Default::default()
        }
    }

    pub fn filtering(&self) -> bool {
        !self.filter.trim().is_empty()
    }

    /// The filter matches `name` (case-insensitive).
    pub fn matches(&self, name: &str) -> bool {
        let q = self.filter.trim();
        q.is_empty() || name.to_lowercase().contains(&q.to_lowercase())
    }

    pub fn is_open(&self, key: &str, default: bool) -> bool {
        self.prefs.open.get(key).copied().unwrap_or(default)
    }

    pub fn set_open(&mut self, key: &str, open: bool) {
        if self.prefs.open.get(key) != Some(&open) {
            self.prefs.open.insert(key.to_owned(), open);
            self.dirty = true;
        }
    }

    pub fn is_pinned(&self, refname: &str) -> bool {
        self.prefs.pinned.iter().any(|p| p == refname)
    }

    pub fn toggle_pin(&mut self, refname: &str) {
        if self.is_pinned(refname) {
            self.prefs.pinned.retain(|p| p != refname);
        } else {
            self.prefs.pinned.push(refname.to_owned());
        }
        self.dirty = true;
    }

    pub fn set_sort(&mut self, tags: bool, order: SortOrder) {
        let slot = if tags {
            &mut self.prefs.tag_sort
        } else {
            &mut self.prefs.branch_sort
        };
        if *slot != order {
            *slot = order;
            self.dirty = true;
        }
    }

    /// The preferences to save, once after each change.
    pub fn take_dirty(&mut self) -> Option<RepoPrefs> {
        std::mem::take(&mut self.dirty).then(|| self.prefs.clone())
    }

    // ----- Drawing pass -----

    pub(crate) fn begin(&mut self) {
        self.building.clear();
        self.parents.clear();
    }

    pub(crate) fn end(&mut self) {
        self.rows = std::mem::take(&mut self.building);
        // Drop an action whose row was not drawn.
        self.action = None;
        if self
            .cursor
            .as_ref()
            .is_some_and(|c| !self.rows.iter().any(|r| &r.key == c))
        {
            self.cursor = None;
        }
    }

    /// Records a drawn row and returns the keyboard action meant for it.
    pub(crate) fn register(&mut self, key: &str, open: Option<bool>) -> Option<NavAction> {
        self.building.push(NavRow {
            key: key.to_owned(),
            parent: self.parents.last().cloned(),
            open,
        });
        match &self.action {
            Some((k, a)) if k == key => {
                let a = *a;
                self.action = None;
                Some(a)
            }
            _ => None,
        }
    }

    /// Handles a navigation key, using the rows of the last frame.
    pub fn navigate(&mut self, key: NavKey) {
        if self.rows.is_empty() {
            return;
        }
        let pos = self
            .cursor
            .as_ref()
            .and_then(|c| self.rows.iter().position(|r| &r.key == c));
        let last = self.rows.len() - 1;
        let Some(p) = pos else {
            if !matches!(key, NavKey::Enter | NavKey::Space) {
                let i = if key == NavKey::End { last } else { 0 };
                self.move_to(i);
            }
            return;
        };
        let row = self.rows[p].clone();
        match key {
            NavKey::Up => self.move_to(p.saturating_sub(1)),
            NavKey::Down => self.move_to((p + 1).min(last)),
            NavKey::Home => self.move_to(0),
            NavKey::End => self.move_to(last),
            NavKey::Left => match row.open {
                Some(true) => self.set_open(&row.key, false),
                _ => {
                    if let Some(i) = row
                        .parent
                        .and_then(|par| self.rows.iter().position(|r| r.key == par))
                    {
                        self.move_to(i);
                    }
                }
            },
            NavKey::Right => match row.open {
                Some(false) => self.set_open(&row.key, true),
                Some(true) => self.move_to((p + 1).min(last)),
                None => {}
            },
            NavKey::Enter | NavKey::Space => match row.open {
                Some(open) => self.set_open(&row.key, !open),
                None => {
                    let a = if key == NavKey::Enter {
                        NavAction::Activate
                    } else {
                        NavAction::Select
                    };
                    self.action = Some((row.key, a));
                }
            },
        }
    }

    fn move_to(&mut self, i: usize) {
        self.cursor = Some(self.rows[i].key.clone());
        self.scroll_to_cursor = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec!["v10", "v2", "V1", "v1.10", "v1.9", "main", "feature"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["feature", "main", "V1", "v1.9", "v1.10", "v2", "v10"]);
        assert_eq!(natural_cmp("a01", "a1"), Ordering::Less);
        assert_eq!(natural_cmp("x", "x"), Ordering::Equal);
    }

    fn names<T>(nodes: &[Node<'_, T>]) -> Vec<String> {
        nodes
            .iter()
            .map(|n| match n {
                Node::Folder { path, count, .. } => format!("{path}/ ({count})"),
                Node::Leaf { name, .. } => name.to_string(),
            })
            .collect()
    }

    #[test]
    fn builds_folders_and_sorts() {
        let items = [
            ("main", 5),
            ("feature/b", 1),
            ("feature/ui/a", 9),
            ("fix-2", 3),
            ("fix-10", 4),
        ];
        let list = || items.iter().map(|(n, t)| (*n, *t, n)).collect::<Vec<_>>();
        let t = tree(list(), SortOrder::Alphabetical, true);
        assert_eq!(names(&t), ["feature/ (2)", "fix-2", "fix-10", "main"]);
        let Node::Folder { children, time, .. } = &t[0] else {
            panic!()
        };
        assert_eq!(*time, 9);
        assert_eq!(names(children), ["feature/ui/ (1)", "b"]);

        let t = tree(list(), SortOrder::Alphabetical, false);
        assert_eq!(names(&t), ["feature/ (2)", "fix-2", "fix-10", "main"]);
        let t = tree(list(), SortOrder::AlphabeticalDesc, false);
        assert_eq!(names(&t), ["main", "fix-10", "fix-2", "feature/ (2)"]);
        let t = tree(list(), SortOrder::MostRecent, false);
        assert_eq!(names(&t), ["feature/ (2)", "main", "fix-10", "fix-2"]);
        let t = tree(list(), SortOrder::MostRecent, true);
        assert_eq!(names(&t), ["feature/ (2)", "main", "fix-10", "fix-2"]);
    }

    #[test]
    fn lists_folder_paths() {
        let f = folder_paths(["a/b/c", "a/d", "e"].into_iter());
        assert_eq!(f.into_iter().collect::<Vec<_>>(), ["a", "a/b"]);
    }

    fn row(key: &str, parent: Option<&str>, open: Option<bool>) -> NavRow {
        NavRow {
            key: key.into(),
            parent: parent.map(Into::into),
            open,
        }
    }

    #[test]
    fn keyboard_moves_opens_and_acts() {
        let mut s = SidebarState {
            rows: vec![
                row("sec:branches", None, Some(true)),
                row("dir:local:feature", Some("sec:branches"), Some(false)),
                row("ref:refs/heads/main", Some("sec:branches"), None),
            ],
            ..Default::default()
        };
        s.navigate(NavKey::Down);
        assert_eq!(s.cursor.as_deref(), Some("sec:branches"));
        s.navigate(NavKey::Down);
        s.navigate(NavKey::Right);
        assert!(s.is_open("dir:local:feature", false));
        assert!(s.dirty);
        s.navigate(NavKey::End);
        s.navigate(NavKey::Enter);
        assert_eq!(
            s.action,
            Some(("ref:refs/heads/main".into(), NavAction::Activate))
        );
        s.navigate(NavKey::Left);
        assert_eq!(s.cursor.as_deref(), Some("sec:branches"));
        s.navigate(NavKey::Left);
        assert!(!s.is_open("sec:branches", true));

        // The action reaches the row when it is drawn.
        s.action = Some(("x".into(), NavAction::Select));
        s.begin();
        assert_eq!(s.register("y", None), None);
        assert_eq!(s.register("x", None), Some(NavAction::Select));
        s.end();
        assert_eq!(s.rows.len(), 2);
    }

    #[test]
    fn pins_and_saves_once() {
        let mut s = SidebarState::default();
        s.toggle_pin("refs/heads/main");
        assert!(s.is_pinned("refs/heads/main"));
        assert!(s.take_dirty().is_some());
        assert!(s.take_dirty().is_none());
        s.toggle_pin("refs/heads/main");
        assert!(s.prefs.pinned.is_empty());
    }
}
