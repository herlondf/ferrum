//! `git bisect`: starting, marking commits and where the search stands.
//! The state is read from `refs/bisect/*` and `BISECT_LOG`, so a bisect
//! started in a terminal shows up too.

use std::collections::HashSet;

use git2::Oid;

use crate::cli::Git;
use crate::{Repo, Result, lossy};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Good,
    Bad,
    Skip,
}

impl Mark {
    pub fn word(self) -> &'static str {
        match self {
            Mark::Good => "good",
            Mark::Bad => "bad",
            Mark::Skip => "skip",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BisectStatus {
    pub bad: Option<Oid>,
    pub good: Vec<Oid>,
    pub skipped: Vec<Oid>,
    /// The commit being tested (HEAD).
    pub current: Option<Oid>,
    /// Commits that may still be the first bad one (the bad commit included).
    pub candidates: usize,
    /// Rough number of tests left (log2 of the candidates).
    pub steps_left: u32,
    /// Found: the first bad commit.
    pub first_bad: Option<Oid>,
    /// Only skipped commits are left to test.
    pub only_skipped: bool,
    /// Marks in the order they were made.
    pub history: Vec<(Mark, Oid)>,
}

impl BisectStatus {
    pub fn mark_of(&self, id: Oid) -> Option<Mark> {
        if self.bad == Some(id) {
            Some(Mark::Bad)
        } else if self.good.contains(&id) {
            Some(Mark::Good)
        } else if self.skipped.contains(&id) {
            Some(Mark::Skip)
        } else {
            None
        }
    }
}

/// Tests needed to narrow `n` candidates down to one.
pub fn steps_for(n: usize) -> u32 {
    if n <= 1 {
        0
    } else {
        usize::BITS - (n - 1).leading_zeros()
    }
}

/// Reads the marks of `BISECT_LOG` ("# good: [sha] subject" lines).
pub fn parse_log(text: &str) -> Vec<(Mark, Oid)> {
    text.lines()
        .filter_map(|l| {
            let rest = l.strip_prefix("# ")?;
            let (term, rest) = rest.split_once(": [")?;
            let mark = match term {
                "good" | "old" => Mark::Good,
                "bad" | "new" => Mark::Bad,
                "skip" => Mark::Skip,
                _ => return None,
            };
            let sha = rest.split(']').next()?;
            Some((mark, Oid::from_str(sha).ok()?))
        })
        .collect()
}

/// What `git bisect good|bad|skip` printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    /// "Bisecting: N revisions left to test after this (roughly M steps)".
    Left {
        revisions: usize,
        steps: u32,
    },
    FirstBad(String),
    Unknown,
}

pub fn parse_output(out: &str) -> Progress {
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("Bisecting: ") {
            let revisions = rest.split_whitespace().next().and_then(|n| n.parse().ok());
            let steps = rest
                .split("roughly ")
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .and_then(|n| n.parse().ok());
            if let Some(revisions) = revisions {
                return Progress::Left {
                    revisions,
                    steps: steps.unwrap_or_else(|| steps_for(revisions + 1)),
                };
            }
        }
        if let Some(sha) = line.strip_suffix(" is the first bad commit") {
            return Progress::FirstBad(sha.trim().to_owned());
        }
    }
    Progress::Unknown
}

impl Repo {
    /// Where the bisect stands, or `None` when none is running.
    pub fn bisect_status(&self) -> Option<BisectStatus> {
        let log = std::fs::read_to_string(self.git_dir().join("BISECT_LOG")).ok()?;
        let raw = self.raw();
        let mut st = BisectStatus {
            current: self.head_oid(),
            history: parse_log(&log),
            ..Default::default()
        };
        if let Ok(refs) = raw.references_glob("refs/bisect/*") {
            for r in refs.flatten() {
                let name = lossy(r.name_bytes());
                let Some(id) = r.target() else { continue };
                let name = name.trim_start_matches("refs/bisect/");
                if name == "bad" || name == "new" {
                    st.bad = Some(id);
                } else if name.starts_with("good-") || name.starts_with("old-") {
                    st.good.push(id);
                } else if name.starts_with("skip-") {
                    st.skipped.push(id);
                }
            }
        }
        if let Some(bad) = st.bad
            && !st.good.is_empty()
        {
            let skipped: HashSet<Oid> = st.skipped.iter().copied().collect();
            let mut walk = raw.revwalk().ok()?;
            walk.push(bad).ok()?;
            for g in &st.good {
                let _ = walk.hide(*g);
            }
            let all: Vec<Oid> = walk.flatten().collect();
            let testable = all.iter().filter(|id| !skipped.contains(id)).count();
            st.candidates = all.len();
            if all.len() == 1 {
                st.first_bad = Some(bad);
            } else if testable <= 1 {
                st.only_skipped = true;
            }
            st.steps_left = steps_for(testable);
        }
        Some(st)
    }
}

impl Repo {
    /// Commits a bisect from `good` to `bad` searches (`bad` included).
    pub fn bisect_candidates(&self, good: Oid, bad: Oid) -> Result<usize> {
        let mut walk = self.raw().revwalk()?;
        walk.push(bad)?;
        walk.hide(good)?;
        Ok(walk.count())
    }
}

/// Starts a bisect. `bad` defaults to HEAD; with no good commit yet, git
/// waits for one.
pub fn start(git: &Git, bad: &str, good: &[String]) -> Result<String> {
    let mut args = vec!["bisect", "start", bad];
    args.extend(good.iter().map(String::as_str));
    args.push("--");
    git.run(&args)
}

/// Marks a commit; returns git's output (see `parse_output`).
pub fn mark(git: &Git, mark: Mark, rev: &str) -> Result<String> {
    git.run(&["bisect", mark.word(), rev])
}

/// Ends the bisect and goes back to where it started.
pub fn reset(git: &Git) -> Result<()> {
    git.run(&["bisect", "reset"])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    /// c1..c8, the bug appears in c5.
    fn line() -> (TestRepo, Vec<Oid>) {
        let t = TestRepo::new();
        let mut ids = Vec::new();
        for i in 1..=8 {
            let content = if i >= 5 { "bug\n" } else { "ok\n" };
            t.commit("a.txt", &format!("{content}{i}\n"), &format!("c{i}"));
            ids.push(t.repo().head_oid().unwrap());
        }
        (t, ids)
    }

    #[test]
    fn steps() {
        assert_eq!(steps_for(0), 0);
        assert_eq!(steps_for(1), 0);
        assert_eq!(steps_for(2), 1);
        assert_eq!(steps_for(3), 2);
        assert_eq!(steps_for(8), 3);
        assert_eq!(steps_for(9), 4);
    }

    #[test]
    fn parses_output() {
        let out = "Bisecting: 3 revisions left to test after this (roughly 2 steps)\n[abc] c4\n";
        assert_eq!(
            parse_output(out),
            Progress::Left {
                revisions: 3,
                steps: 2
            }
        );
        let out = "1234567890abcdef1234567890abcdef12345678 is the first bad commit\ncommit 12345";
        assert_eq!(
            parse_output(out),
            Progress::FirstBad("1234567890abcdef1234567890abcdef12345678".into())
        );
        assert_eq!(parse_output("nothing"), Progress::Unknown);
    }

    #[test]
    fn finds_the_first_bad_commit() {
        let (t, ids) = line();
        assert!(t.repo().bisect_status().is_none());
        let out = start(&t.git, &ids[7].to_string(), &[ids[0].to_string()]).unwrap();
        assert!(matches!(parse_output(&out), Progress::Left { .. }), "{out}");
        let st = t.repo().bisect_status().unwrap();
        assert_eq!(st.bad, Some(ids[7]));
        assert_eq!(st.good, vec![ids[0]]);
        assert_eq!(st.candidates, 7);
        assert_eq!(t.repo().bisect_candidates(ids[0], ids[7]).unwrap(), 7);
        assert_eq!(st.steps_left, 3);
        assert_eq!(st.history, vec![(Mark::Bad, ids[7]), (Mark::Good, ids[0])]);
        let mut found = None;
        for _ in 0..8 {
            let repo = t.repo();
            let st = repo.bisect_status().unwrap();
            if let Some(f) = st.first_bad {
                found = Some(f);
                break;
            }
            let cur = st.current.unwrap();
            let bad = t.read("a.txt").starts_with("bug");
            let m = if bad { Mark::Bad } else { Mark::Good };
            mark(&t.git, m, &cur.to_string()).unwrap();
            assert_eq!(t.repo().bisect_status().unwrap().mark_of(cur), Some(m));
        }
        assert_eq!(found, Some(ids[4]));
        reset(&t.git).unwrap();
        assert!(t.repo().bisect_status().is_none());
        assert_eq!(t.repo().head_oid(), Some(ids[7]));
    }

    #[test]
    fn skipping_everything_left() {
        let (t, ids) = line();
        start(&t.git, &ids[2].to_string(), &[ids[0].to_string()]).unwrap();
        let _ = mark(&t.git, Mark::Skip, &ids[1].to_string());
        let st = t.repo().bisect_status().unwrap();
        assert!(st.only_skipped);
        assert_eq!(st.first_bad, None);
        assert_eq!(st.skipped, vec![ids[1]]);
    }
}
