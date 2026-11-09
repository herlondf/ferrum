//! Learning mode: a panel with the git commands each action ran (from the
//! Activity log), with a copy button and a short explanation, and the
//! command in the tooltips of the main buttons.

use std::path::Path;

use eframe::egui::{self, RichText};
use ferrum_core::cli::command_line;

use crate::command::{Command, Commands, Op};
use crate::icons;
use crate::jobs::{self, Outcome};
use crate::theme;

fn enabled_id() -> egui::Id {
    egui::Id::unique("ferrum-learning-mode")
}

/// Set every frame from the settings.
pub fn set_enabled(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(enabled_id(), on));
}

pub fn enabled(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp(enabled_id())).unwrap_or(false)
}

/// `tip`, plus the git command of `op` in learning mode.
pub fn tip(ctx: &egui::Context, tip: String, op: Option<&Op>) -> String {
    match op.filter(|_| enabled(ctx)).and_then(command_for) {
        Some(cmd) => format!("{tip}\n\n$ {cmd}"),
        None => tip,
    }
}

fn line(args: &[&str]) -> Option<String> {
    Some(command_line(args))
}

/// The main git command an operation runs (the same arguments as `ferrum_core`).
pub fn command_for(op: &Op) -> Option<String> {
    use ferrum_core::ops::ResetMode;
    match op {
        Op::StageAll => line(&["add", "-A"]),
        Op::UnstageAll => line(&["reset", "-q"]),
        Op::Fetch { remote } => {
            let mut a = vec!["fetch", "--prune", "--progress", "--tags"];
            a.push(remote.as_deref().unwrap_or("--all"));
            line(&a)
        }
        Op::FetchWith(o) => {
            let mut a = vec!["fetch", "--progress"];
            if o.prune {
                a.push("--prune");
            }
            if o.tags {
                a.push("--tags");
            }
            match o.remotes.len() {
                0 => a.push("--all"),
                1 => {}
                _ => a.push("--multiple"),
            }
            a.extend(o.remotes.iter().map(String::as_str));
            line(&a)
        }
        Op::Pull(_) | Op::PullWith(_) => {
            let (mode, autostash) = match op {
                Op::PullWith(o) => (o.mode, o.autostash),
                Op::Pull(m) => (*m, false),
                _ => return None,
            };
            let mut a = vec!["pull", "--progress"];
            match mode {
                ferrum_core::ops::PullMode::Default => {}
                ferrum_core::ops::PullMode::Merge => a.push("--no-rebase"),
                ferrum_core::ops::PullMode::Rebase => a.push("--rebase"),
                ferrum_core::ops::PullMode::FastForwardOnly => a.push("--ff-only"),
            }
            if autostash {
                a.push("--autostash");
            }
            line(&a)
        }
        Op::PushRefs(r) => {
            let args = r.args();
            line(&args.iter().map(String::as_str).collect::<Vec<_>>())
        }
        Op::Push(o) => {
            let spec = format!("refs/heads/{0}:refs/heads/{0}", o.branch);
            let mut a = vec!["push", "--progress"];
            if o.set_upstream {
                a.push("--set-upstream");
            }
            if o.force_with_lease {
                a.push("--force-with-lease");
            }
            a.extend([o.remote.as_str(), spec.as_str()]);
            line(&a)
        }
        Op::Commit { amend, .. } => {
            let mut a = vec!["commit", "--cleanup=strip", "-F", "<message>"];
            if *amend {
                a.push("--amend");
            }
            line(&a)
        }
        Op::StashPush {
            message,
            untracked,
            staged_only,
        } => {
            let mut a = vec!["stash", "push"];
            if *untracked {
                a.push("--include-untracked");
            }
            if *staged_only {
                a.push("--staged");
            }
            if !message.trim().is_empty() {
                a.extend(["-m", message.as_str()]);
            }
            line(&a)
        }
        Op::StashApply { stash, pop } => line(&[
            "stash",
            if *pop { "pop" } else { "apply" },
            &stash.refname(),
        ]),
        Op::StashDrop(s) => line(&["stash", "drop", &s.refname()]),
        Op::Checkout(b) => line(&["switch", b]),
        Op::CreateBranch {
            name,
            start,
            checkout: true,
        } => line(&["switch", "-c", name, start]),
        Op::CreateBranch { name, start, .. } => line(&["branch", name, start]),
        Op::DeleteBranch { name, force } => {
            line(&["branch", if *force { "-D" } else { "-d" }, name])
        }
        Op::Merge { rev, .. } => line(&["merge", "--no-edit", rev]),
        Op::Rebase { onto } => line(&["rebase", onto]),
        Op::Reset { rev, mode } => {
            let flag = match mode {
                ResetMode::Soft => "--soft",
                ResetMode::Mixed => "--mixed",
                ResetMode::Hard => "--hard",
                ResetMode::Keep => "--keep",
            };
            line(&["reset", flag, rev])
        }
        Op::DeleteTag(n) => line(&["tag", "-d", n]),
        Op::SecretCheck(c) => command_for(&c.op),
        _ => None,
    }
}

/// What a git command does, in a sentence.
pub fn explain(cmd: &str) -> Option<String> {
    let mut words = cmd.split_whitespace().skip(1);
    // Skip global options such as `-c core.editor=true`.
    let mut sub = words.next()?;
    while sub == "-c" {
        words.next();
        sub = words.next()?;
    }
    let rest: Vec<&str> = words.collect();
    let has = |f: &str| rest.contains(&f);
    let first = rest.iter().copied().find(|w| !w.starts_with('-'));
    Some(match sub {
        "fetch" => t!(
            "Downloads new commits, branches and tags from the remote. Your branches and files do not change."
        ),
        "pull" if has("--rebase") => {
            t!("Fetches the upstream branch and replays your local commits on top of it.")
        }
        "pull" => t!("Fetches the upstream branch and merges it into the current branch."),
        "push" if has("--delete") => t!("Deletes a branch or tag on the remote."),
        "push" if has("--force-with-lease") => t!(
            "Uploads your commits and replaces the remote branch, unless someone else pushed to it meanwhile."
        ),
        "push" => t!("Uploads your commits to the remote branch."),
        "commit" if has("--amend") => {
            t!("Replaces the last commit with a new one that also has the staged changes.")
        }
        "commit" => t!("Records the staged changes as a new commit."),
        "add" => t!("Stages changes: they go into the next commit."),
        "reset" if has("--hard") => {
            t!("Moves the current branch to another commit and throws away all local changes.")
        }
        "reset" if has("--soft") => {
            t!("Moves the current branch to another commit; the changes stay staged.")
        }
        "reset" if has("--keep") => t!(
            "Moves the current branch to another commit, keeping local changes that do not conflict."
        ),
        "reset" if has("--") || !rest.iter().any(|w| !w.starts_with('-')) => {
            t!("Unstages changes: they stay in the files but leave the next commit.")
        }
        "reset" => t!("Moves the current branch to another commit; the changes stay in the files."),
        "switch" | "checkout" if has("-c") || has("-b") => {
            t!("Creates a branch and switches to it.")
        }
        "switch" | "checkout" if has("--") => {
            t!("Replaces files with their last committed version.")
        }
        "switch" | "checkout" => t!("Switches to another branch or commit, updating the files."),
        "branch" if has("-d") || has("-D") => {
            t!("Deletes a branch (the commits stay reachable from the reflog for a while).")
        }
        "branch" if has("-m") => t!("Renames a branch."),
        "branch" if has("-f") => t!("Moves a branch to another commit."),
        "branch" => t!("Creates a branch without switching to it."),
        "merge" if has("--abort") => {
            t!("Stops the merge and puts everything back as it was before it.")
        }
        "merge" => t!("Joins another branch's history into the current branch."),
        "rebase" if has("--abort") => {
            t!("Stops the rebase and puts the branch back as it was before it.")
        }
        "rebase" if has("--continue") => {
            t!("Goes on with the rebase after conflicts are resolved.")
        }
        "rebase" => {
            t!("Replays the current branch's commits on top of another commit, rewriting them.")
        }
        "cherry-pick" => t!("Copies a commit onto the current branch."),
        "revert" => t!("Makes a new commit that undoes the changes of an earlier one."),
        "stash" => match first {
            Some("pop") => t!("Applies the latest stash to the files and deletes it."),
            Some("apply") => t!("Applies a stash to the files and keeps it."),
            Some("drop") => t!("Deletes a stash."),
            Some("store") => t!("Puts a commit back in the stash list."),
            _ => t!("Puts local changes aside in a stash and cleans the working tree."),
        },
        "tag" if has("-d") => t!("Deletes a tag."),
        "tag" => t!("Gives a commit a permanent name, such as a version number."),
        "clean" => t!("Deletes untracked files."),
        "apply" => t!("Applies a patch to the files or the index (staging part of a file)."),
        "rm" => t!("Removes files from git (with --cached: keeps them on disk)."),
        "remote" => t!("Manages the remotes: the other repositories you fetch from and push to."),
        "ls-remote" => t!("Lists the branches and tags of a remote, to test the connection."),
        "update-ref" => t!("Low-level: points a ref (branch, tag, restore point) at a commit."),
        "symbolic-ref" => t!("Low-level: sets the branch HEAD points to."),
        "read-tree" => t!("Low-level: loads a saved tree into the index (and the files, with -u)."),
        "write-tree" => t!("Low-level: saves the index as a tree object."),
        "commit-tree" => t!("Low-level: creates a commit object from a tree."),
        "hash-object" | "mktree" => t!("Low-level: stores data in the object database."),
        "rev-parse" => t!("Looks up a commit id or a path inside the repository."),
        "submodule" => t!("Updates the repositories nested in this one."),
        "lfs" => t!("Git LFS: large files kept outside the repository."),
        "worktree" => t!("Manages extra working folders of the same repository."),
        "config" => t!("Reads or changes a git setting."),
        "bisect" => t!("Finds the commit that introduced a bug by testing commits in between."),
        "am" => t!("Applies patches from mail files as commits."),
        "format-patch" => t!("Writes commits as patch files."),
        "restore" => t!("Restores files from the index or a commit."),
        "status" => t!("Shows which files changed."),
        "diff" => t!("Shows the changes between two versions."),
        "log" => t!("Lists commits."),
        "show" => t!("Shows a commit or a file at a commit."),
        "gc" => t!("Cleans up and compresses the repository."),
        _ => return None,
    })
}

#[derive(Default)]
pub struct LearningPanel {
    /// Closed by the user while learning mode is on.
    pub hidden: bool,
}

/// Draws the panel of `repo`'s latest operations.
pub fn show(
    ctx: &egui::Context,
    panel: &mut LearningPanel,
    repo: Option<&Path>,
    cmds: &mut Commands,
) {
    if panel.hidden {
        return;
    }
    let mut open = true;
    let screen = ctx.content_rect();
    egui::Window::new(t!("Learning mode"))
        .id(egui::Id::unique("learning-window"))
        .open(&mut open)
        .collapsible(true)
        .resizable(true)
        .default_size([440.0, 320.0])
        .pivot(egui::Align2::LEFT_BOTTOM)
        .default_pos([screen.left() + 244.0, screen.bottom() - 44.0])
        .show(ctx, |ui| {
            let weak = ui.visuals().weak_text_color();
            ui.horizontal(|ui| {
                ui.label(RichText::new(icons::LIGHTBULB).color(theme::accent(ui)));
                ui.label(
                    RichText::new(t!("The git commands behind each action, as they run."))
                        .size(12.0)
                        .color(weak),
                );
            });
            ui.separator();
            let ops: Vec<_> = crate::ui::activity::visible(
                &jobs::operations(),
                crate::ui::activity::Filter::User,
                repo,
            )
            .into_iter()
            .filter(|o| !o.commands().is_empty() || o.running())
            .take(8)
            .collect();
            if ops.is_empty() {
                crate::ui::empty_state::show(
                    ui,
                    icons::TERMINAL_WINDOW,
                    &t!("Nothing run yet"),
                    &t!(
                        "Fetch, commit, check out a branch… the git commands it uses show up here."
                    ),
                    |_| {},
                );
                return;
            }
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for op in ops {
                        operation(ui, &op, cmds);
                        ui.add_space(theme::SPACE_M);
                    }
                });
        });
    if !open {
        panel.hidden = true;
    }
}

fn operation(ui: &mut egui::Ui, op: &jobs::Operation, cmds: &mut Commands) {
    let v = ui.visuals().clone();
    ui.horizontal(|ui| {
        match op.outcome() {
            None => {
                ui.spinner();
            }
            Some((_, Outcome::Succeeded)) => {
                ui.label(RichText::new(icons::CHECK_CIRCLE).color(crate::util::added_color(ui)));
            }
            Some((_, Outcome::Failed(_))) => {
                ui.label(RichText::new(icons::WARNING_CIRCLE).color(v.error_fg_color));
            }
            Some((_, Outcome::Cancelled)) => {
                ui.label(RichText::new(icons::X_CIRCLE).color(v.weak_text_color()));
            }
        }
        ui.label(theme::strong(&op.label).color(v.strong_text_color()));
    });
    let code = crate::util::code_font(12.0);
    for c in op.commands() {
        theme::card(ui)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                // The button first, on the right; the text takes what is left.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    let copy = ui
                        .add(
                            egui::Button::new(icons::COPY)
                                .small()
                                .frame_when_inactive(false),
                        )
                        .on_hover_text(t!("Copy"));
                    if copy.clicked() {
                        cmds.push(Command::Copy(c.command.clone()));
                    }
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        let cmd = RichText::new(format!("$ {}", c.command)).font(code.clone());
                        ui.add(egui::Label::new(cmd).wrap());
                        if let Some(e) = explain(&c.command) {
                            let e = RichText::new(e).size(11.5).color(v.weak_text_color());
                            ui.add(egui::Label::new(e).wrap());
                        }
                    });
                });
            });
    }
}
