//! Interactive rebase driven by a plan built in the UI.
//!
//! git asks an editor for the todo list. We point `GIT_SEQUENCE_EDITOR` at a
//! `cp` of our own todo file (git always runs editors through its bundled
//! shell, so this works on every platform). Rewording uses `exec git commit
//! --amend -F <file>` right after the pick, so no editor is ever opened.

use std::path::Path;

use crate::cli::Git;
use crate::model::CommitInfo;
use crate::{Oid, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Pick,
    /// Keep the commit with a new message.
    Reword(String),
    /// Meld into the previous commit, keeping both messages.
    Squash,
    /// Meld into the previous commit, discarding this message.
    Fixup,
    /// Stop after this commit so it can be amended.
    Edit,
    Drop,
}

impl Action {
    pub fn label(&self) -> &'static str {
        match self {
            Action::Pick => "pick",
            Action::Reword(_) => "reword",
            Action::Squash => "squash",
            Action::Fixup => "fixup",
            Action::Edit => "edit",
            Action::Drop => "drop",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Step {
    pub commit: CommitInfo,
    pub action: Action,
}

/// Checks the plan before running it.
pub fn validate(steps: &[Step]) -> std::result::Result<(), String> {
    let first_kept = steps.iter().find(|s| s.action != Action::Drop);
    if let Some(s) = first_kept
        && matches!(s.action, Action::Squash | Action::Fixup)
    {
        return Err(
            "The first commit cannot be squashed: there is no earlier commit to meld into.".into(),
        );
    }
    Ok(())
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Writes the todo list (and reword messages) into `work_dir`. `refs` are
/// branches to move along with the commits they point at (`update-ref`).
pub fn todo_list(steps: &[Step], work_dir: &Path, refs: &[(Oid, String)]) -> Result<String> {
    let mut todo = String::new();
    for (i, step) in steps.iter().enumerate() {
        let id = step.commit.id.to_string();
        let summary = &step.commit.summary;
        match &step.action {
            Action::Reword(msg) => {
                let file = work_dir.join(format!("reword-{i}.txt"));
                std::fs::write(&file, msg)?;
                todo.push_str(&format!("pick {id} {summary}\n"));
                todo.push_str(&format!(
                    "exec git commit --amend --allow-empty -q -F {}\n",
                    sh_quote(&file.to_string_lossy())
                ));
            }
            other => todo.push_str(&format!("{} {id} {summary}\n", other.label())),
        }
        for (_, name) in refs.iter().filter(|(at, _)| *at == step.commit.id) {
            todo.push_str(&format!("update-ref refs/heads/{name}\n"));
        }
    }
    Ok(todo)
}

/// Extra choices for an interactive rebase.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// Local branches (other than the rebased one) pointing at commits of the
    /// plan, moved along with them.
    pub update_refs: Vec<(Oid, String)>,
    /// Create this branch at HEAD first, to get back to the current state.
    pub backup: Option<String>,
}

/// Name for a backup branch of `branch`.
pub fn backup_name(branch: &str) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    format!("backup/{branch}-{secs}")
}

/// A change to one commit (or a few) made without opening the planner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Quick {
    /// Meld into the parent, keeping both messages.
    Squash,
    /// Meld into the parent, keeping the parent's message.
    Fixup,
    Edit,
    Drop,
    Reword(String),
}

/// Plans a rebase applying `action` to `targets`, commits of the current
/// branch. With one target, squash and fixup meld it into its parent; with
/// several, the others are moved right after the oldest and melded into it.
/// Returns the base and the steps.
pub fn quick_plan(
    repo: &crate::Repo,
    targets: &[Oid],
    action: &Quick,
) -> std::result::Result<(Option<Oid>, Vec<Step>), String> {
    let infos = targets
        .iter()
        .map(|id| repo.find_commit_info(*id).map_err(|e| e.to_string()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if infos.is_empty() {
        return Err("No commit selected.".into());
    }
    if infos.iter().any(|c| c.parents.len() > 1) {
        return Err("Merge commits cannot be changed this way.".into());
    }
    let head = repo.head_oid().ok_or("There is no commit yet.")?;
    if infos.iter().any(|c| !repo.is_ancestor(c.id, head)) {
        return Err("The commit is not on the current branch.".into());
    }
    let oldest = infos
        .iter()
        .find(|c| infos.iter().all(|o| repo.is_ancestor(c.id, o.id)))
        .ok_or("The commits are not on one line of history.")?;
    let melds = matches!(action, Quick::Squash | Quick::Fixup);
    let into_parent = melds && infos.len() == 1;
    let first = if into_parent {
        let parent = *oldest
            .parents
            .first()
            .ok_or("The first commit has no parent to meld into.")?;
        repo.find_commit_info(parent).map_err(|e| e.to_string())?
    } else {
        oldest.clone()
    };
    let base = first.parents.first().copied();
    let commits = repo.commits_since(base).map_err(|e| e.to_string())?;
    if commits.iter().any(|c| c.parents.len() > 1) {
        return Err("There are merge commits after it: use the interactive rebase instead.".into());
    }
    let action_for = |id: Oid| {
        if !targets.contains(&id) {
            return Action::Pick;
        }
        match action {
            Quick::Squash => Action::Squash,
            Quick::Fixup => Action::Fixup,
            Quick::Edit => Action::Edit,
            Quick::Drop => Action::Drop,
            Quick::Reword(m) => Action::Reword(m.clone()),
        }
    };
    let mut steps: Vec<Step> = commits
        .into_iter()
        .map(|commit| Step {
            action: action_for(commit.id),
            commit,
        })
        .collect();
    if melds && !into_parent {
        // The oldest target stays a pick; the other targets follow it.
        if let Some(s) = steps.iter_mut().find(|s| s.commit.id == oldest.id) {
            s.action = Action::Pick;
        }
        let (mut picked, rest): (Vec<Step>, Vec<Step>) = steps
            .into_iter()
            .partition(|s| targets.contains(&s.commit.id));
        picked.extend(rest);
        steps = picked;
    }
    validate(&steps)?;
    Ok((base, steps))
}

/// Rebases the commits after `base` (or all commits, with `None`) following the plan.
/// Returns normally when the rebase stops for an `edit` or a conflict: check the
/// repository state afterwards.
pub fn run(git: &Git, base: Option<Oid>, steps: &[Step]) -> Result<()> {
    run_with(git, base, steps, &Options::default())
}

pub fn run_with(git: &Git, base: Option<Oid>, steps: &[Step], opts: &Options) -> Result<()> {
    validate(steps).map_err(crate::Error::Other)?;
    if let Some(name) = &opts.backup {
        git.run(&["branch", name, "HEAD"])?;
    }
    let work = tempfile_dir()?;
    let todo = todo_list(steps, &work, &opts.update_refs)?;
    let todo_file = work.join("todo");
    std::fs::write(&todo_file, todo)?;

    let editor = format!("cp {}", sh_quote(&todo_file.to_string_lossy()));
    let git = git.clone().with_env("GIT_SEQUENCE_EDITOR", editor);
    let base = base.map(|b| b.to_string());
    let mut args = vec!["rebase", "-i", "--autostash"];
    if !opts.update_refs.is_empty() {
        args.push("--update-refs");
    }
    match &base {
        Some(b) => args.push(b),
        None => args.push("--root"),
    }
    let result = git.run(&args);
    // Reword files are needed until the rebase finishes, which may be after an
    // `edit` stop, so they are only removed when the rebase is done.
    if result.is_ok() && !git.dir().join(".git/rebase-merge").exists() {
        let _ = std::fs::remove_dir_all(&work);
    }
    match result {
        Ok(_) => Ok(()),
        // Stopping for a conflict leaves a rebase in progress, which the UI shows.
        Err(e) if git.dir().join(".git/rebase-merge").exists() => {
            let _ = e;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn tempfile_dir() -> Result<std::path::PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let dir = std::env::temp_dir().join(format!("ferrum-rebase-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RepoState;
    use crate::testutil::TestRepo;

    fn log(t: &TestRepo) -> Vec<String> {
        t.git
            .run(&["log", "--format=%s"])
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn setup() -> (TestRepo, Oid, Vec<Step>) {
        let t = TestRepo::new();
        t.commit("base.txt", "base\n", "base");
        let base = t.repo().head_oid().unwrap();
        t.commit("a.txt", "a\n", "A");
        t.commit("b.txt", "b\n", "B");
        t.commit("c.txt", "c\n", "C");
        let steps = t
            .repo()
            .commits_since(Some(base))
            .unwrap()
            .into_iter()
            .map(|commit| Step {
                commit,
                action: Action::Pick,
            })
            .collect();
        (t, base, steps)
    }

    #[test]
    fn reorders_rewords_squashes_and_drops() {
        let (t, base, mut steps) = setup();
        assert_eq!(
            steps
                .iter()
                .map(|s| s.commit.summary.as_str())
                .collect::<Vec<_>>(),
            ["A", "B", "C"]
        );
        // New order: C (reworded), A, B squashed into A. Then drop nothing.
        steps.swap(0, 2); // C B A
        steps.swap(1, 2); // C A B
        steps[0].action = Action::Reword("C renamed".into());
        steps[2].action = Action::Fixup;
        run(&t.git, Some(base), &steps).unwrap();
        assert_eq!(t.repo().state(), RepoState::Clean);
        assert_eq!(log(&t), ["A", "C renamed", "base"]);
        assert!(t.path().join("b.txt").exists());
    }

    #[test]
    fn drops_and_stops_for_edit() {
        let (t, base, mut steps) = setup();
        steps[1].action = Action::Drop;
        steps[2].action = Action::Edit;
        run(&t.git, Some(base), &steps).unwrap();
        assert_eq!(t.repo().state(), RepoState::Rebase);
        crate::ops::continue_operation(&t.git, RepoState::Rebase).unwrap();
        assert_eq!(log(&t), ["C", "A", "base"]);
        assert!(!t.path().join("b.txt").exists());
    }

    fn id(t: &TestRepo, rev: &str) -> Oid {
        t.repo().resolve(rev).unwrap()
    }

    fn quick(t: &TestRepo, targets: &[&str], action: Quick) {
        let ids: Vec<Oid> = targets.iter().map(|r| id(t, r)).collect();
        let (base, steps) = quick_plan(&t.repo(), &ids, &action).unwrap();
        run(&t.git, base, &steps).unwrap();
    }

    #[test]
    fn quick_actions() {
        // HEAD = C, HEAD~1 = B, HEAD~2 = A.
        let (t, _, _) = setup();
        quick(&t, &["HEAD~1"], Quick::Fixup);
        assert_eq!(log(&t), ["C", "A", "base"]);
        assert!(t.path().join("b.txt").exists());

        quick(&t, &["HEAD~1"], Quick::Reword("A2".into()));
        assert_eq!(log(&t), ["C", "A2", "base"]);

        quick(&t, &["HEAD"], Quick::Drop);
        assert_eq!(log(&t), ["A2", "base"]);

        quick(&t, &["HEAD"], Quick::Edit);
        assert_eq!(t.repo().state(), RepoState::Rebase);
        crate::ops::continue_operation(&t.git, RepoState::Rebase).unwrap();

        // The root commit has no parent to meld into.
        let root = id(&t, "HEAD~1");
        assert!(quick_plan(&t.repo(), &[root], &Quick::Squash).is_err());
        // But it can be reworded.
        quick(&t, &["HEAD~1"], Quick::Reword("root".into()));
        assert_eq!(log(&t), ["A2", "root"]);
    }

    #[test]
    fn squashes_and_drops_several() {
        let (t, _, _) = setup();
        t.commit("d.txt", "d\n", "D");
        // Squash A and C (not next to each other): C moves up into A.
        quick(&t, &["HEAD~3", "HEAD~1"], Quick::Squash);
        assert_eq!(log(&t), ["D", "B", "A", "base"]);
        let msg = t.git.run(&["log", "-1", "--format=%B", "HEAD~2"]).unwrap();
        assert!(msg.contains('A') && msg.contains('C'), "{msg}");
        assert!(t.path().join("c.txt").exists());

        quick(&t, &["HEAD", "HEAD~1"], Quick::Drop);
        assert_eq!(log(&t), ["A", "base"]);
    }

    #[test]
    fn keeps_a_backup_and_moves_other_branches() {
        let (t, base, mut steps) = setup();
        let old_head = t.repo().head_oid().unwrap();
        let b = steps[1].commit.id;
        t.git.run(&["branch", "at-b", &b.to_string()]).unwrap();
        steps[0].action = Action::Reword("A!".into());
        let opts = Options {
            update_refs: vec![(b, "at-b".into())],
            backup: Some("backup/main".into()),
        };
        run_with(&t.git, Some(base), &steps, &opts).unwrap();
        assert_eq!(log(&t), ["C", "B", "A!", "base"]);
        assert_eq!(id(&t, "backup/main"), old_head);
        assert_eq!(id(&t, "at-b"), id(&t, "HEAD~1"), "at-b moved along");
    }

    #[test]
    fn rejects_squash_first() {
        let (_, _, mut steps) = setup();
        steps[0].action = Action::Squash;
        assert!(validate(&steps).is_err());
    }
}
