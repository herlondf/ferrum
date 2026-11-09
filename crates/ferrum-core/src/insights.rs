//! Repository insights: hotspots (files changed often and recently), churn
//! over time, who knows which file (bus factor) and contributors over time.
//!
//! One walk over HEAD's first-parent diffs with libgit2: each non-merge
//! commit is diffed against its parent with no context lines, and the added
//! and deleted lines are counted per file and per author. Call it off the UI
//! thread; it stops when `cancel` is set.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use git2::{DiffOptions, Sort};

use crate::{Error, Result, lossy};

const DAY: i64 = 86_400;
/// A change this many days old counts half as much in the hotspot score.
const HALF_LIFE_DAYS: f64 = 90.0;
/// Files larger than this are not diffed line by line (counted as changed).
const MAX_DIFF_SIZE: i64 = 2 * 1024 * 1024;
/// Most contributors shown separately in the timeline; the rest are "others".
pub const TIMELINE_AUTHORS: usize = 5;

#[derive(Debug, Clone)]
pub struct Options {
    /// Only commits from this time on (unix seconds).
    pub since: Option<i64>,
    /// Only files under this path (`""`: everything).
    pub path: String,
    /// Commits looked at, newest first; older ones are left out.
    pub max_commits: usize,
    /// "Now" for recency weights and buckets (unix seconds).
    pub now: i64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            since: None,
            path: String::new(),
            max_commits: 5_000,
            now: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        }
    }
}

/// One file of the current tree and how it changed in the period.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileInsight {
    pub path: String,
    pub commits: u32,
    pub added: u64,
    pub deleted: u64,
    pub last_change: i64,
    /// Changes weighted by how recent they are (higher: hotter).
    pub score: f64,
    /// Lines changed per author (index into `Insights::contributors`),
    /// most first.
    pub authors: Vec<(usize, u64)>,
}

impl FileInsight {
    pub fn churn(&self) -> u64 {
        self.added + self.deleted
    }

    /// The author who changed most of the file and their share (0..=1).
    pub fn main_author(&self) -> Option<(usize, f32)> {
        let total: u64 = self.authors.iter().map(|a| a.1).sum();
        let (who, lines) = *self.authors.first()?;
        Some((
            who,
            if total == 0 {
                1.0
            } else {
                lines as f32 / total as f32
            },
        ))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Contributor {
    pub name: String,
    pub email: String,
    pub commits: u32,
    pub added: u64,
    pub deleted: u64,
    pub first: i64,
    pub last: i64,
    /// Files of the current tree this person changed most.
    pub owned_files: u32,
}

/// A time bucket of the churn and contributor charts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bucket {
    /// Start (unix seconds).
    pub start: i64,
    pub commits: u32,
    pub added: u64,
    pub deleted: u64,
    /// Commits of the top `TIMELINE_AUTHORS` contributors, then "others".
    pub by_author: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct Insights {
    /// Commits looked at (merges included).
    pub commits: usize,
    /// More commits matched than `max_commits`; the oldest were left out.
    pub truncated: bool,
    /// Files of the current tree changed in the period, hottest first.
    pub files: Vec<FileInsight>,
    /// Most commits first.
    pub contributors: Vec<Contributor>,
    /// Oldest first, `bucket_days` wide.
    pub buckets: Vec<Bucket>,
    pub bucket_days: i64,
    /// Fewest people who together are the main author of more than half
    /// of the files (0 when no file changed).
    pub bus_factor: usize,
}

impl Insights {
    /// Files whose main author wrote at least `share` of the changes, with
    /// at least `min_commits` commits: knowledge held by one person.
    pub fn knowledge_islands(&self, share: f32, min_commits: u32) -> Vec<&FileInsight> {
        let mut out: Vec<&FileInsight> = self
            .files
            .iter()
            .filter(|f| {
                f.commits >= min_commits && f.main_author().is_some_and(|(_, s)| s >= share)
            })
            .collect();
        out.sort_by(|a, b| b.churn().cmp(&a.churn()).then(a.path.cmp(&b.path)));
        out
    }
}

/// Bucket width for a period of `span` seconds: days up to six weeks, weeks
/// up to about half a year, then months, then wider so there are at most
/// ~60 bars.
pub fn bucket_days(span: i64) -> i64 {
    let days = (span / DAY).max(1);
    if days <= 45 {
        1
    } else if days <= 190 {
        7
    } else if days <= 30 * 60 {
        30
    } else {
        ((days / 60) + 29) / 30 * 30
    }
}

/// Computes the insights of the repository at `path`.
pub fn compute(path: &Path, opts: &Options, cancel: &AtomicBool) -> Result<Insights> {
    let repo = git2::Repository::open(path)?;
    let mut out = Insights::default();
    let Ok(head) = repo.head().and_then(|h| h.peel_to_commit()) else {
        return Ok(out);
    };
    let head_tree = head.tree()?;
    let mailmap = repo.mailmap().ok();
    let mut walk = repo.revwalk()?;
    walk.set_sorting(Sort::TIME)?;
    walk.push(head.id())?;

    let prefix = opts.path.trim().trim_matches('/').to_owned();
    let mut files: HashMap<String, FileInsight> = HashMap::new();
    let mut people: HashMap<String, usize> = HashMap::new();
    // Per commit: (time, author index, added, deleted).
    let mut events: Vec<(i64, usize, u64, u64)> = Vec::new();

    for id in walk {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let commit = repo.find_commit(id?)?;
        let time = commit.time().seconds();
        if opts.since.is_some_and(|s| time < s) {
            // Time-sorted: everything after is older (give or take clock skew).
            break;
        }
        if out.commits >= opts.max_commits {
            out.truncated = true;
            break;
        }
        let sig = match &mailmap {
            Some(m) => commit
                .author_with_mailmap(m)
                .unwrap_or_else(|_| commit.author()),
            None => commit.author(),
        };
        let email = lossy(sig.email_bytes()).to_lowercase();
        let name = lossy(sig.name_bytes());
        let key = if email.is_empty() {
            name.clone()
        } else {
            email.clone()
        };
        let next = out.contributors.len();
        let who = *people.entry(key).or_insert(next);
        if who == next {
            out.contributors.push(Contributor {
                name,
                email,
                first: time,
                last: time,
                ..Default::default()
            });
        }
        let c = &mut out.contributors[who];
        c.first = c.first.min(time);
        c.last = c.last.max(time);
        if commit.parent_count() > 1 {
            // Merges: their changes are counted in the merged commits.
            out.commits += 1;
            continue;
        }
        let parent_tree = match commit.parent(0) {
            Ok(p) => Some(p.tree()?),
            Err(_) => None,
        };
        let tree = commit.tree()?;
        let mut dopts = DiffOptions::new();
        dopts.context_lines(0).max_size(MAX_DIFF_SIZE);
        if !prefix.is_empty() {
            dopts.pathspec(&prefix);
        }
        let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut dopts))?;
        let (mut c_add, mut c_del) = (0u64, 0u64);
        let mut touched = false;
        for i in 0..diff.deltas().len() {
            let Some(delta) = diff.get_delta(i) else {
                continue;
            };
            let file = delta.new_file().path().or_else(|| delta.old_file().path());
            let Some(file) = file else { continue };
            let file = file.to_string_lossy().replace('\\', "/");
            if !prefix.is_empty() && !under(&file, &prefix) {
                continue;
            }
            let (add, del) = match git2::Patch::from_diff(&diff, i) {
                Ok(Some(p)) => p.line_stats().map(|(_, a, d)| (a as u64, d as u64)),
                _ => Ok((0, 0)),
            }
            .unwrap_or((0, 0));
            touched = true;
            c_add += add;
            c_del += del;
            let f = files.entry(file.clone()).or_insert_with(|| FileInsight {
                path: file,
                ..Default::default()
            });
            f.commits += 1;
            f.added += add;
            f.deleted += del;
            f.last_change = f.last_change.max(time);
            let age = ((opts.now - time).max(0) as f64) / DAY as f64;
            f.score += 0.5f64.powf(age / HALF_LIFE_DAYS);
            let lines = (add + del).max(1);
            match f.authors.iter_mut().find(|a| a.0 == who) {
                Some(a) => a.1 += lines,
                None => f.authors.push((who, lines)),
            }
        }
        out.commits += 1;
        if prefix.is_empty() || touched {
            let c = &mut out.contributors[who];
            c.commits += 1;
            c.added += c_add;
            c.deleted += c_del;
            events.push((time, who, c_add, c_del));
        }
    }

    // Only files that still exist: deleted ones are no hotspot.
    let mut list: Vec<FileInsight> = files
        .into_values()
        .filter(|f| head_tree.get_path(Path::new(&f.path)).is_ok())
        .collect();
    for f in &mut list {
        f.authors.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    }
    list.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(b.commits.cmp(&a.commits))
            .then(a.path.cmp(&b.path))
    });
    for f in &list {
        if let Some((who, _)) = f.main_author() {
            out.contributors[who].owned_files += 1;
        }
    }
    out.bus_factor = bus_factor(&out.contributors, list.len());

    // Contributors sorted by commits (people with none in the path or
    // period are left out); file author indexes follow.
    let mut order: Vec<usize> = (0..out.contributors.len())
        .filter(|&i| out.contributors[i].commits > 0)
        .collect();
    order.sort_by(|&a, &b| {
        let (ca, cb) = (&out.contributors[a], &out.contributors[b]);
        cb.commits.cmp(&ca.commits).then(ca.name.cmp(&cb.name))
    });
    let mut remap = vec![usize::MAX; out.contributors.len()];
    for (new, &old) in order.iter().enumerate() {
        remap[old] = new;
    }
    out.contributors = order.iter().map(|&i| out.contributors[i].clone()).collect();
    for f in &mut list {
        for a in &mut f.authors {
            a.0 = remap[a.0];
        }
        f.authors.retain(|a| a.0 != usize::MAX);
    }
    for e in &mut events {
        e.1 = remap[e.1];
    }
    out.files = list;
    buckets(&mut out, &events, opts);
    Ok(out)
}

fn under(file: &str, prefix: &str) -> bool {
    file == prefix
        || file
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Fewest contributors who are the main author of more than half the files.
fn bus_factor(people: &[Contributor], files: usize) -> usize {
    if files == 0 {
        return 0;
    }
    let mut owned: Vec<u32> = people.iter().map(|c| c.owned_files).collect();
    owned.sort_unstable_by(|a, b| b.cmp(a));
    let mut sum = 0usize;
    for (i, n) in owned.iter().enumerate() {
        sum += *n as usize;
        if sum * 2 > files {
            return i + 1;
        }
    }
    owned.iter().filter(|n| **n > 0).count().max(1)
}

fn buckets(out: &mut Insights, events: &[(i64, usize, u64, u64)], opts: &Options) {
    let Some(oldest) = events.iter().map(|e| e.0).min() else {
        return;
    };
    // From the oldest commit read (a young repository fills the chart).
    let start = oldest.max(0);
    let width_days = bucket_days(opts.now - start);
    let width = width_days * DAY;
    let count = (((opts.now - start).max(0) / width) + 1) as usize;
    let first = opts.now - count as i64 * width + 1;
    out.bucket_days = width_days;
    out.buckets = (0..count)
        .map(|i| Bucket {
            start: first + i as i64 * width,
            by_author: vec![0; TIMELINE_AUTHORS + 1],
            ..Default::default()
        })
        .collect();
    for &(time, who, add, del) in events {
        let i = ((time - first).max(0) / width) as usize;
        let Some(b) = out.buckets.get_mut(i.min(count - 1)) else {
            continue;
        };
        b.commits += 1;
        b.added += add;
        b.deleted += del;
        b.by_author[who.min(TIMELINE_AUTHORS)] += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    fn commit_as(t: &TestRepo, who: &str, file: &str, content: &str, msg: &str) {
        t.write(file, content);
        t.git.run(&["add", "-A"]).unwrap();
        let author = format!("{who} <{}@example.com>", who.to_lowercase());
        t.git
            .run(&["commit", "-q", "--author", &author, "-m", msg])
            .unwrap();
    }

    #[test]
    fn hotspots_authors_and_bus_factor() {
        let t = TestRepo::new();
        commit_as(&t, "Ana", "src/a.rs", "1\n", "a1");
        commit_as(&t, "Ana", "src/a.rs", "1\n2\n", "a2");
        commit_as(&t, "Ana", "src/a.rs", "1\n2\n3\n", "a3");
        commit_as(&t, "Bob", "b.txt", "x\n", "b1");
        commit_as(&t, "Bob", "gone.txt", "x\n", "g");
        t.git.run(&["rm", "-q", "gone.txt"]).unwrap();
        t.git
            .run(&[
                "commit",
                "-q",
                "--author",
                "Bob <bob@example.com>",
                "-m",
                "rm",
            ])
            .unwrap();
        commit_as(&t, "Bob", "src/a.rs", "1\n2\n3\n4\n", "b2");

        let ins = compute(t.path(), &Options::default(), &AtomicBool::new(false)).unwrap();
        assert_eq!(ins.commits, 7);
        assert!(!ins.truncated);
        let paths: Vec<&str> = ins.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["src/a.rs", "b.txt"], "deleted files are left out");
        let a = &ins.files[0];
        assert_eq!(a.commits, 4);
        assert_eq!(a.added, 4);
        let (who, share) = a.main_author().unwrap();
        assert_eq!(ins.contributors[who].name, "Ana");
        assert!((share - 0.75).abs() < 0.01);
        assert_eq!(ins.contributors[0].name, "Bob", "most commits first");
        assert_eq!(ins.contributors[0].commits, 4);
        assert_eq!(ins.bus_factor, 2);
        let islands = ins.knowledge_islands(0.7, 3);
        assert_eq!(islands.len(), 1);
        assert_eq!(islands[0].path, "src/a.rs");
        let total: u32 = ins.buckets.iter().map(|b| b.commits).sum();
        assert_eq!(total, 7);
        let by: u32 = ins.buckets.iter().flat_map(|b| b.by_author.iter()).sum();
        assert_eq!(by, 7);
    }

    #[test]
    fn path_filter_and_limits() {
        let t = TestRepo::new();
        commit_as(&t, "Ana", "src/a.rs", "1\n", "a");
        commit_as(&t, "Bob", "docs/x.md", "1\n", "b");
        commit_as(&t, "Bob", "srcx/y.rs", "1\n", "c");
        let opts = Options {
            path: "src/".into(),
            ..Default::default()
        };
        let ins = compute(t.path(), &opts, &AtomicBool::new(false)).unwrap();
        assert_eq!(ins.files.len(), 1);
        assert_eq!(ins.files[0].path, "src/a.rs");
        assert_eq!(
            ins.contributors.len(),
            1,
            "only people who touched the path"
        );
        assert_eq!(ins.bus_factor, 1);

        let opts = Options {
            max_commits: 2,
            ..Default::default()
        };
        let ins = compute(t.path(), &opts, &AtomicBool::new(false)).unwrap();
        assert!(ins.truncated);
        assert_eq!(ins.commits, 2);

        let opts = Options {
            since: Some(opts.now + 1000),
            ..Default::default()
        };
        let ins = compute(t.path(), &opts, &AtomicBool::new(false)).unwrap();
        assert_eq!(ins.commits, 0);
        assert!(ins.files.is_empty());

        assert!(matches!(
            compute(t.path(), &Options::default(), &AtomicBool::new(true)),
            Err(Error::Cancelled)
        ));
    }

    #[test]
    fn empty_repository_and_bucket_widths() {
        let t = TestRepo::new();
        let ins = compute(t.path(), &Options::default(), &AtomicBool::new(false)).unwrap();
        assert_eq!(ins.commits, 0);
        assert_eq!(bucket_days(30 * DAY), 1);
        assert_eq!(bucket_days(90 * DAY), 7);
        assert_eq!(bucket_days(365 * DAY), 30);
        assert!(bucket_days(20 * 365 * DAY) >= 90);
        assert!(20 * 365 / bucket_days(20 * 365 * DAY) <= 61);
    }
}
