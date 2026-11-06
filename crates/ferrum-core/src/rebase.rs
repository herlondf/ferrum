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

/// Writes the todo list (and reword messages) into `work_dir`.
pub fn todo_list(steps: &[Step], work_dir: &Path) -> Result<String> {
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
    }
    Ok(todo)
}

/// Rebases the commits after `base` (or all commits, with `None`) following the plan.
/// Returns normally when the rebase stops for an `edit` or a conflict: check the
/// repository state afterwards.
pub fn run(git: &Git, base: Option<Oid>, steps: &[Step]) -> Result<()> {
    validate(steps).map_err(crate::Error::Other)?;
    let work = tempfile_dir()?;
    let todo = todo_list(steps, &work)?;
    let todo_file = work.join("todo");
    std::fs::write(&todo_file, todo)?;

    let editor = format!("cp {}", sh_quote(&todo_file.to_string_lossy()));
    let git = git.clone().with_env("GIT_SEQUENCE_EDITOR", editor);
    let base = base.map(|b| b.to_string());
    let mut args = vec!["rebase", "-i", "--autostash"];
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

    #[test]
    fn rejects_squash_first() {
        let (_, _, mut steps) = setup();
        steps[0].action = Action::Squash;
        assert!(validate(&steps).is_err());
    }
}
