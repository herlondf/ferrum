//! Merge, rebase, cherry-pick and revert with options, conflict previews,
//! resolution helpers and the local-changes prompt before a checkout.

use std::path::Path;
use std::sync::{Arc, Mutex};

use eframe::egui;
use ferrum_core::cli::Git;
use ferrum_core::integrate::{
    self, LocalChanges, MergeOptions, PickOptions, Preview, RebaseOptions,
};
use ferrum_core::ops::{self, Side};
use ferrum_core::rebase::{self, Step};
use ferrum_core::{ChangeKind, Oid, short};

use crate::command::{Command, Op};
use crate::tab::RepoTab;

/// Operations of this module, run through `Op::Flow`.
#[derive(Debug, Clone)]
pub enum FlowOp {
    Merge {
        rev: String,
        opts: MergeOptions,
    },
    Rebase {
        onto: String,
        opts: RebaseOptions,
    },
    /// Oldest first.
    CherryPick {
        ids: Vec<Oid>,
        opts: PickOptions,
    },
    Revert {
        id: Oid,
        mainline: Option<u32>,
        commit: bool,
    },
    /// An interactive rebase with options (planner or quick action).
    Plan {
        label: String,
        base: Option<Oid>,
        steps: Vec<Step>,
        opts: rebase::Options,
    },
    /// Bring back the conflict of files marked resolved.
    Unresolve(Vec<String>),
    ResolveMany {
        paths: Vec<String>,
        side: Side,
    },
    /// Pick a side of a submodule conflict (2 = ours, 3 = theirs).
    ResolveGitlink {
        path: String,
        stage: u8,
    },
    /// A checkout (or branch creation) after stashing or discarding local changes.
    WithLocalChanges {
        mode: LocalChanges,
        op: Box<Op>,
    },
}

impl FlowOp {
    pub fn label(&self) -> String {
        match self {
            FlowOp::Merge { rev, .. } => t!("Merge {}", short_rev(rev)),
            FlowOp::Rebase { onto, .. } => t!("Rebase onto {}", short_rev(onto)),
            FlowOp::CherryPick { ids, .. } if ids.len() == 1 => {
                t!("Cherry-pick {}", short(ids[0]))
            }
            FlowOp::CherryPick { ids, .. } => t!("Cherry-pick {} commits", ids.len()),
            FlowOp::Revert { id, .. } => t!("Revert {}", short(*id)),
            FlowOp::Plan { label, .. } => label.clone(),
            FlowOp::Unresolve(p) if p.len() == 1 => t!("Undo resolution of {}", p[0]),
            FlowOp::Unresolve(p) => t!("Undo resolution of {} files", p.len()),
            FlowOp::ResolveMany { paths, side } => {
                if *side == Side::Ours {
                    t!("Resolve {} files with ours", paths.len())
                } else {
                    t!("Resolve {} files with theirs", paths.len())
                }
            }
            FlowOp::ResolveGitlink { path, .. } => t!("Resolve {}", path),
            FlowOp::WithLocalChanges { op, .. } => op.label(),
        }
    }

    /// Only the working copy changes.
    pub fn status_only(&self) -> bool {
        matches!(
            self,
            FlowOp::Unresolve(_) | FlowOp::ResolveMany { .. } | FlowOp::ResolveGitlink { .. }
        )
    }

    /// Moves HEAD: undone by resetting back.
    pub fn rewrites_head(&self) -> bool {
        matches!(
            self,
            FlowOp::Merge { .. }
                | FlowOp::Rebase { .. }
                | FlowOp::CherryPick { .. }
                | FlowOp::Revert { .. }
                | FlowOp::Plan { .. }
        )
    }

    pub fn run(&self, git: &Git, progress: &dyn Fn(&str)) -> ferrum_core::Result<()> {
        match self {
            FlowOp::Merge { rev, opts } => integrate::merge(git, rev, opts),
            FlowOp::Rebase { onto, opts } => integrate::rebase(git, onto, opts),
            FlowOp::CherryPick { ids, opts } => integrate::cherry_pick(git, ids, opts),
            FlowOp::Revert {
                id,
                mainline,
                commit,
            } => integrate::revert(git, *id, *mainline, *commit),
            FlowOp::Plan {
                base, steps, opts, ..
            } => rebase::run_with(git, *base, steps, opts),
            FlowOp::Unresolve(paths) => integrate::unresolve(git, paths),
            FlowOp::ResolveMany { paths, side } => paths
                .iter()
                .try_for_each(|p| ops::resolve_with(git, p, *side)),
            FlowOp::ResolveGitlink { path, stage } => integrate::resolve_gitlink(git, path, *stage),
            FlowOp::WithLocalChanges { mode, op } => {
                integrate::with_local_changes(git, *mode, || op.run(git, progress))
            }
        }
    }
}

/// Shortens full hashes; leaves names alone.
pub fn short_rev(rev: &str) -> &str {
    if rev.len() == 40 && rev.chars().all(|c| c.is_ascii_hexdigit()) {
        &rev[..7]
    } else {
        rev
    }
}

/// A conflict check running on a background thread, restarted when its key changes.
#[derive(Default)]
pub struct PreviewSlot {
    key: Option<String>,
    result: Arc<Mutex<Option<Preview>>>,
}

impl PreviewSlot {
    /// The result for `key`, starting the check when needed.
    pub fn get(
        &mut self,
        ctx: &egui::Context,
        repo: &Path,
        key: String,
        check: impl FnOnce(&Git) -> Preview + Send + 'static,
    ) -> Option<Preview> {
        if self.key.as_ref() != Some(&key) {
            self.key = Some(key);
            let slot = Arc::new(Mutex::new(None));
            self.result = slot.clone();
            let git = Git::new(repo);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let p = check(&git);
                if let Ok(mut s) = slot.lock() {
                    *s = Some(p);
                }
                ctx.request_repaint();
            });
        }
        self.result.lock().ok().and_then(|s| s.clone())
    }
}

/// "✔ Applies cleanly" / "⚠ Will conflict in: a.txt, b.txt".
pub fn preview_ui(ui: &mut egui::Ui, preview: Option<&Preview>, tab: Option<&RepoTab>) {
    match preview {
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(t!("Checking for conflicts…"));
            });
        }
        Some(Preview::Clean) => {
            ui.colored_label(
                crate::util::added_color(ui),
                t!("✔ Applies cleanly, without conflicts"),
            );
        }
        Some(Preview::UpToDate) => {
            ui.weak(t!("✔ Already up to date: nothing to do"));
        }
        Some(Preview::Conflicts { files, at }) => {
            let mut list: Vec<&str> = files.iter().take(6).map(String::as_str).collect();
            let more = files.len().saturating_sub(list.len());
            let more_text = t!("{} more", more);
            if more > 0 {
                list.push(&more_text);
            }
            let text = match at {
                Some(id) => {
                    let summary = tab
                        .and_then(|t| t.repo.find_commit_info(*id).ok())
                        .map(|c| format!("{} {}", c.short_id(), c.summary))
                        .unwrap_or_else(|| short(*id));
                    t!("⚠ Will conflict at {} in: {}", summary, list.join(", "))
                }
                None => t!("⚠ Will conflict in: {}", list.join(", ")),
            };
            ui.add(
                egui::Label::new(egui::RichText::new(text).color(ui.visuals().warn_fg_color))
                    .wrap(),
            );
        }
        Some(Preview::Unavailable(e)) => {
            ui.weak(t!(
                "Conflicts could not be checked in advance (needs git 2.38 or newer)."
            ))
            .on_hover_text(e);
        }
    }
}

/// Dev aid for `FERRUM_START`: `flow:merge=REV`, `flow:rebase=REV`,
/// `flow:pick=REV,REV`, `flow:revert=REV`, `flow:reword=REV`,
/// `flow:select=REV,REV` (several commits selected in the history).
pub fn dev_steps(arg: &str, tab: Option<&RepoTab>) -> Vec<Command> {
    use crate::ui::{dialogs::Dialog, flow_dialogs};
    let Some(t) = tab else {
        return Vec::new();
    };
    let (what, revs) = arg.split_once('=').unwrap_or((arg, "HEAD"));
    let ids: Vec<Oid> = revs
        .split(',')
        .filter_map(|r| t.repo.resolve(r.trim()).ok())
        .collect();
    let Some(&first) = ids.first() else {
        eprintln!("FERRUM_START: unknown revision {revs}");
        return Vec::new();
    };
    let dialog = match what {
        "merge" => Dialog::merge(revs, revs),
        "rebase" => flow_dialogs::rebase(t, revs),
        "pick" => flow_dialogs::cherry_pick(t, &ids),
        "revert" => flow_dialogs::revert(t, first),
        "reword" => flow_dialogs::reword(t, first),
        "select" => {
            return vec![crate::ui::history_options::cmd(move |t, _| {
                t.history.selected = Some(first);
                t.history.ext.selected_many = ids;
            })];
        }
        _ => {
            eprintln!("FERRUM_START: unknown flow step {what}");
            return Vec::new();
        }
    };
    vec![Command::Dialog(dialog)]
}

/// Where a checkout-like operation goes, to compare with local changes.
fn checkout_target(op: &Op) -> Option<&str> {
    match op {
        Op::Checkout(b) => Some(b),
        Op::CheckoutRemote { remote_branch, .. } => Some(remote_branch),
        Op::CheckoutDetached(r) => Some(r),
        Op::CreateBranch {
            start,
            checkout: true,
            ..
        } => Some(start),
        _ => None,
    }
}

/// Locally changed tracked files.
fn local_paths(tab: &RepoTab) -> Vec<String> {
    let mut v: Vec<String> = tab
        .status
        .staged
        .iter()
        .chain(&tab.status.unstaged)
        .filter(|f| f.kind != ChangeKind::Untracked)
        .map(|f| f.path.clone())
        .collect();
    v.sort();
    v.dedup();
    v
}

/// Runs a checkout, or first asks what to do with local changes: always
/// when Settings ask for it, otherwise when git would refuse because the
/// checkout touches locally changed files.
pub fn checkout(tab: &RepoTab, op: Op, ask_always: bool) -> Command {
    let Some(target) = checkout_target(&op) else {
        return Command::Run(op);
    };
    let local = local_paths(tab);
    let same_commit = tab.repo.resolve(target).ok() == tab.head_oid;
    if local.is_empty() || same_commit {
        return Command::Run(op);
    }
    let touched: Vec<String> = match integrate::changed_between(tab.path(), "HEAD", target) {
        Ok(changed) => local
            .iter()
            .filter(|p| changed.binary_search(p).is_ok())
            .cloned()
            .collect(),
        Err(_) => Vec::new(),
    };
    if touched.is_empty() && !ask_always {
        return Command::Run(op);
    }
    Command::Dialog(crate::ui::flow_dialogs::local_changes(op, local, touched))
}
