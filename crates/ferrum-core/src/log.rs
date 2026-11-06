//! The history shown in the commit graph: which refs to walk, in which order,
//! and extra facts about the commits (body, unreachable, stashes, unpushed).

use std::collections::{HashMap, HashSet};

use git2::Sort;
use serde::{Deserialize, Serialize};

use crate::model::CommitInfo;
use crate::repo::{RS, commit_info_mailmap};
use crate::{Oid, Repo, Result, lossy};

/// What the history walks. Ref patterns are full ref names
/// (`refs/heads/main`); a pattern ending in `/` matches a whole folder or
/// remote (`refs/remotes/origin/`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LogOptions {
    /// `git log --topo-order` instead of `--date-order`.
    pub topo_order: bool,
    pub first_parent: bool,
    /// Only these refs (empty = everything).
    pub include: Vec<String>,
    /// Refs left out of the graph.
    pub hidden: Vec<String>,
    pub hide_remotes: bool,
    /// Hides tag labels; the commits stay.
    pub hide_tags: bool,
    pub hide_stashes: bool,
    /// Also walk reflog entries, which shows commits no ref points to anymore.
    pub reflog: bool,
}

impl LogOptions {
    /// The graph shows less than the whole repository.
    pub fn is_filtered(&self) -> bool {
        !self.include.is_empty() || !self.hidden.is_empty() || self.hide_remotes
    }
}

/// Whether `name` (a full ref name) matches `pattern`.
pub fn ref_matches(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix('/') {
        Some(prefix) => name
            .strip_prefix(prefix)
            .is_some_and(|r| r.starts_with('/')),
        None => pattern == name,
    }
}

fn matches_any(patterns: &[String], name: &str) -> bool {
    patterns.iter().any(|p| ref_matches(p, name))
}

#[derive(Debug, Clone, Default)]
pub struct Log {
    pub commits: Vec<CommitInfo>,
    /// More commits exist beyond the limit.
    pub truncated: bool,
    /// Commits whose message has more than a summary line.
    pub has_body: HashSet<Oid>,
    /// Commits found only through the reflog.
    pub unreachable: HashSet<Oid>,
    /// Stash commits shown in the graph, with their stash index.
    pub stashes: HashMap<Oid, usize>,
}

/// One path search result: a commit and the matching files it changed.
pub type PathHit = (Oid, Vec<String>);

impl Repo {
    /// Commits to show in the history, newest first, up to `limit`.
    pub fn log(&self, opts: &LogOptions, limit: usize) -> Result<Log> {
        let repo = self.raw();
        let mut tips = Vec::new();
        let mut head_hidden = false;
        let head_name = repo
            .find_reference("HEAD")
            .ok()
            .and_then(|h| h.symbolic_target_bytes().map(lossy));
        for r in repo.references()? {
            let r = r?;
            let name = lossy(r.name_bytes());
            let remote = name.starts_with("refs/remotes/");
            let tag = name.starts_with("refs/tags/");
            if !(remote || tag || name.starts_with("refs/heads/")) || name.ends_with("/HEAD") {
                continue;
            }
            let wanted = !(remote && opts.hide_remotes)
                && (opts.include.is_empty() || matches_any(&opts.include, &name))
                && !matches_any(&opts.hidden, &name);
            if !wanted {
                if head_name.as_deref() == Some(name.as_str()) {
                    head_hidden = true;
                }
                continue;
            }
            if let Ok(c) = r.peel_to_commit() {
                tips.push(c.id());
            }
        }
        // A detached HEAD is shown unless the graph is limited to some branches.
        if !head_hidden
            && (opts.include.is_empty() || head_name.is_none())
            && let Ok(h) = repo.head()
            && let Some(id) = h.target()
        {
            tips.push(id);
        }

        let mut reflog_tips = Vec::new();
        if opts.reflog {
            let mut names = vec!["HEAD".to_owned()];
            for r in repo.references_glob("refs/heads/*")?.flatten() {
                names.push(lossy(r.name_bytes()));
            }
            for name in names {
                let Ok(log) = repo.reflog(&name) else {
                    continue;
                };
                for e in log.iter() {
                    let id = e.id_new();
                    if !id.is_zero() && repo.find_commit(id).is_ok() {
                        reflog_tips.push(id);
                    }
                }
            }
            reflog_tips.sort();
            reflog_tips.dedup();
        }

        let mut walk = repo.revwalk()?;
        walk.set_sorting(if opts.topo_order {
            Sort::TOPOLOGICAL
        } else {
            Sort::TOPOLOGICAL | Sort::TIME
        })?;
        if opts.first_parent {
            walk.simplify_first_parent()?;
        }
        for &t in tips.iter().chain(&reflog_tips) {
            walk.push(t)?;
        }

        let mailmap = repo.mailmap().ok();
        let mut log = Log::default();
        for oid in walk {
            if log.commits.len() == limit {
                log.truncated = true;
                break;
            }
            let commit = repo.find_commit(oid?)?;
            if commit
                .body_bytes()
                .is_some_and(|b| b.iter().any(|c| !c.is_ascii_whitespace()))
            {
                log.has_body.insert(commit.id());
            }
            log.commits
                .push(commit_info_mailmap(&commit, mailmap.as_ref()));
        }

        if !reflog_tips.is_empty() {
            let mut walk = repo.revwalk()?;
            for &t in &reflog_tips {
                walk.push(t)?;
            }
            for &t in &tips {
                walk.hide(t)?;
            }
            log.unreachable = walk.take(limit).filter_map(|r| r.ok()).collect();
        }

        if !opts.hide_stashes {
            self.insert_stashes(&mut log, mailmap.as_ref());
        }
        Ok(log)
    }

    /// Puts each stash right above the commit it was made on.
    fn insert_stashes(&self, log: &mut Log, mailmap: Option<&git2::Mailmap>) {
        let repo = self.raw();
        let Ok(reflog) = repo.reflog("refs/stash") else {
            return;
        };
        let mut pos: HashMap<Oid, usize> = HashMap::new();
        let mut entries = Vec::new();
        for (index, e) in reflog.iter().enumerate() {
            let Ok(commit) = repo.find_commit(e.id_new()) else {
                continue;
            };
            let mut info = commit_info_mailmap(&commit, mailmap);
            // Only the base commit: the index and untracked parents are internal.
            info.parents.truncate(1);
            entries.push((index, info));
        }
        if entries.is_empty() {
            return;
        }
        for (i, c) in log.commits.iter().enumerate() {
            pos.insert(c.id, i);
        }
        let mut inserts: Vec<(usize, usize, CommitInfo)> = entries
            .into_iter()
            .filter_map(|(index, info)| Some((*pos.get(info.parents.first()?)?, index, info)))
            .collect();
        // Insert from the bottom so earlier positions stay valid; newer stashes on top.
        inserts.sort_by_key(|(p, index, _)| (std::cmp::Reverse(*p), std::cmp::Reverse(*index)));
        for (p, index, info) in inserts {
            log.stashes.insert(info.id, index);
            log.commits.insert(p, info);
        }
    }

    /// Commits a merge brought in: reachable from its other parents but not the first.
    pub fn merge_contents(&self, merge: Oid, limit: usize) -> Result<Vec<CommitInfo>> {
        let repo = self.raw();
        let commit = repo.find_commit(merge)?;
        let parents: Vec<Oid> = commit.parent_ids().collect();
        if parents.len() < 2 {
            return Ok(Vec::new());
        }
        let mut walk = repo.revwalk()?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;
        for &p in &parents[1..] {
            walk.push(p)?;
        }
        walk.hide(parents[0])?;
        let mailmap = repo.mailmap().ok();
        let mut out = Vec::new();
        for oid in walk.take(limit) {
            out.push(commit_info_mailmap(
                &repo.find_commit(oid?)?,
                mailmap.as_ref(),
            ));
        }
        Ok(out)
    }

    /// Commits on local branches that their upstream does not have (unpushed),
    /// and the other way round (unpulled).
    pub fn unsynced(&self, limit: usize) -> Result<(HashSet<Oid>, HashSet<Oid>)> {
        let repo = self.raw();
        let mut ahead = HashSet::new();
        let mut behind = HashSet::new();
        for item in repo.branches(Some(git2::BranchType::Local))? {
            let (branch, _) = item?;
            let (Some(local), Ok(up)) = (branch.get().target(), branch.upstream()) else {
                continue;
            };
            let Some(remote) = up.get().target() else {
                continue;
            };
            if local == remote {
                continue;
            }
            for (from, to, set) in [(local, remote, &mut ahead), (remote, local, &mut behind)] {
                let mut walk = repo.revwalk()?;
                walk.push(from)?;
                walk.hide(to)?;
                set.extend(walk.take(limit).filter_map(|r| r.ok()));
            }
        }
        Ok((ahead, behind))
    }

    /// Commits that changed a path containing `term` (case-insensitive), with
    /// the matching files. `all` searches every branch, otherwise only HEAD.
    pub fn search_paths(&self, term: &str, all: bool) -> Result<Vec<PathHit>> {
        let fmt = format!("--format={RS}%H");
        let file = format!(":(icase,glob)**/*{}*", glob_escape(term));
        let dir = format!("{file}/**");
        let scope = if all { "--all" } else { "HEAD" };
        let out = self.git().run(&[
            "log",
            scope,
            &fmt,
            "--name-only",
            "--no-renames",
            "-z",
            "--",
            &file,
            &dir,
        ])?;
        Ok(parse_path_log(&out))
    }
}

/// Escapes glob characters so the term matches literally.
fn glob_escape(term: &str) -> String {
    let mut out = String::new();
    for c in term.trim().chars() {
        if matches!(c, '*' | '?' | '[' | ']' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn parse_path_log(out: &str) -> Vec<PathHit> {
    out.split(RS)
        .filter_map(|rec| {
            let mut parts = rec
                .split(['\0', '\n'])
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let id = Oid::from_str(parts.next()?).ok()?;
            Some((id, parts.map(str::to_owned).collect()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    fn summaries(log: &Log) -> Vec<&str> {
        log.commits.iter().map(|c| c.summary.as_str()).collect()
    }

    /// main: base - m1 - merge(feature); feature: base - f1 - f2.
    fn merged_repo() -> TestRepo {
        let t = TestRepo::new();
        t.commit("a.txt", "a", "base");
        t.git.run(&["switch", "-q", "-c", "feature"]).unwrap();
        t.commit("f.txt", "1", "f1");
        t.commit("f.txt", "2", "f2");
        t.git.run(&["switch", "-q", "main"]).unwrap();
        t.commit("m.txt", "1", "m1");
        t.git
            .run(&["merge", "-q", "--no-ff", "-m", "merge feature", "feature"])
            .unwrap();
        t
    }

    #[test]
    fn matches_refs_and_folders() {
        assert!(ref_matches("refs/heads/main", "refs/heads/main"));
        assert!(!ref_matches("refs/heads/main", "refs/heads/main2"));
        assert!(ref_matches("refs/heads/feature/", "refs/heads/feature/x"));
        assert!(!ref_matches("refs/heads/feature/", "refs/heads/featurex"));
        assert!(ref_matches(
            "refs/remotes/origin/",
            "refs/remotes/origin/a/b"
        ));
    }

    #[test]
    fn filters_hides_and_first_parent() {
        let t = merged_repo();
        t.git
            .run(&["switch", "-q", "-c", "side", "HEAD~1"])
            .unwrap();
        t.commit("s.txt", "s", "side work");
        t.git.run(&["switch", "-q", "main"]).unwrap();
        let repo = t.repo();

        let all = repo.log(&LogOptions::default(), 100).unwrap();
        assert_eq!(all.commits.len(), 6);
        assert!(!all.truncated);

        let only_feature = LogOptions {
            include: vec!["refs/heads/feature".into()],
            ..Default::default()
        };
        let log = repo.log(&only_feature, 100).unwrap();
        assert_eq!(summaries(&log), ["f2", "f1", "base"]);

        let hide_side = LogOptions {
            hidden: vec!["refs/heads/side".into()],
            ..Default::default()
        };
        assert!(!summaries(&repo.log(&hide_side, 100).unwrap()).contains(&"side work"));

        let fp = LogOptions {
            first_parent: true,
            include: vec!["refs/heads/main".into()],
            ..Default::default()
        };
        assert_eq!(
            summaries(&repo.log(&fp, 100).unwrap()),
            ["merge feature", "m1", "base"]
        );

        let topo = LogOptions {
            topo_order: true,
            ..Default::default()
        };
        assert_eq!(repo.log(&topo, 100).unwrap().commits.len(), 6);

        let limited = repo.log(&LogOptions::default(), 2).unwrap();
        assert_eq!(limited.commits.len(), 2);
        assert!(limited.truncated);
    }

    #[test]
    fn merge_contents_and_unsynced() {
        let t = merged_repo();
        let repo = t.repo();
        let head = repo.head_oid().unwrap();
        let brought: Vec<String> = repo
            .merge_contents(head, 100)
            .unwrap()
            .into_iter()
            .map(|c| c.summary)
            .collect();
        assert_eq!(brought, ["f2", "f1"]);

        // A fake upstream: main tracks a remote ref one commit behind.
        let base = repo.resolve("HEAD~1").unwrap().to_string();
        t.git
            .run(&["update-ref", "refs/remotes/origin/main", &base])
            .unwrap();
        t.git.run(&["remote", "add", "origin", "."]).unwrap();
        t.git
            .run(&["config", "branch.main.remote", "origin"])
            .unwrap();
        t.git
            .run(&["config", "branch.main.merge", "refs/heads/main"])
            .unwrap();
        let (ahead, behind) = repo.unsynced(100).unwrap();
        // The merge and the feature commits it brought in.
        assert!(ahead.contains(&head));
        assert_eq!(ahead.len(), 3);
        assert!(behind.is_empty());
    }

    #[test]
    fn reflog_shows_unreachable_and_stashes_sit_on_their_base() {
        let t = TestRepo::new();
        t.commit("a.txt", "1", "one");
        t.commit("a.txt", "2", "two");
        t.git.run(&["reset", "-q", "--hard", "HEAD~1"]).unwrap();
        t.write("a.txt", "dirty");
        t.git.run(&["stash", "push", "-q", "-m", "wip"]).unwrap();
        let repo = t.repo();

        let plain = repo.log(&LogOptions::default(), 100).unwrap();
        assert_eq!(plain.commits.len(), 2, "one + the stash");
        assert_eq!(plain.stashes.len(), 1);
        assert_eq!(plain.commits[1].summary, "one");
        assert_eq!(plain.commits[0].parents, vec![plain.commits[1].id]);

        let no_stash = LogOptions {
            hide_stashes: true,
            ..Default::default()
        };
        assert_eq!(repo.log(&no_stash, 100).unwrap().commits.len(), 1);

        let with_reflog = LogOptions {
            reflog: true,
            hide_stashes: true,
            ..Default::default()
        };
        let log = repo.log(&with_reflog, 100).unwrap();
        assert_eq!(summaries(&log), ["two", "one"]);
        assert!(log.unreachable.contains(&log.commits[0].id));
        assert_eq!(log.unreachable.len(), 1);
    }

    #[test]
    fn mailmap_and_body() {
        let t = TestRepo::new();
        t.write(
            ".mailmap",
            "Real Name <real@example.com> <test@example.com>\n",
        );
        t.git.run(&["add", "-A"]).unwrap();
        t.git
            .run(&["commit", "-q", "-m", "subject", "-m", "body text"])
            .unwrap();
        t.commit("b.txt", "b", "no body");
        let log = t.repo().log(&LogOptions::default(), 10).unwrap();
        assert_eq!(log.commits[0].author, "Real Name");
        assert_eq!(log.commits[0].email, "real@example.com");
        assert!(log.has_body.contains(&log.commits[1].id));
        assert!(!log.has_body.contains(&log.commits[0].id));
    }

    #[test]
    fn searches_paths() {
        let t = TestRepo::new();
        t.commit("src/Main.rs", "1", "add main");
        t.commit("docs/readme.md", "1", "docs");
        t.commit("lib/domain.rs", "2", "domain");
        let hits = t.repo().search_paths("main", true).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].1, ["lib/domain.rs"]);
        let dir = t.repo().search_paths("docs", false).unwrap();
        assert_eq!(dir.len(), 1);
        assert_eq!(dir[0].1, ["docs/readme.md"]);
    }
}
