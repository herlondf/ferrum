//! Dialogs for merge, rebase, cherry-pick, revert, reword and the
//! local-changes prompt before a checkout. Each shows a conflict preview.

use eframe::egui::{self, RichText};
use ferrum_core::integrate::{self, LocalChanges, MergeOptions, PickOptions, RebaseOptions};
use ferrum_core::ops::MergeMode;
use ferrum_core::rebase::{self, Quick};
use ferrum_core::{BranchKind, Oid};

use crate::command::{Command, Commands, Op};
use crate::flow::{self, FlowOp, PreviewSlot};
use crate::tab::RepoTab;
use crate::ui::dialogs::{Dialog, buttons};

pub enum FlowDialog {
    Merge {
        rev: String,
        label: String,
        mode: MergeMode,
        no_commit: bool,
        allow_unrelated: bool,
        preview: PreviewSlot,
    },
    Rebase {
        /// Branch, remote branch, tag or commit.
        onto: String,
        autostash: bool,
        update_refs: bool,
        preview: PreviewSlot,
    },
    CherryPick {
        /// Oldest first.
        ids: Vec<Oid>,
        label: String,
        /// Number of parents of a single merge commit (0 otherwise).
        parents: usize,
        mainline: u32,
        record_origin: bool,
        commit: bool,
        preview: PreviewSlot,
    },
    Revert {
        id: Oid,
        label: String,
        parents: usize,
        mainline: u32,
        commit: bool,
        preview: PreviewSlot,
    },
    Reword {
        id: Oid,
        label: String,
        message: String,
    },
    LocalChanges {
        op: Box<Op>,
        local: Vec<String>,
        /// Local changes the checkout would overwrite.
        touched: Vec<String>,
        mode: LocalChanges,
    },
}

fn commit_label(tab: &RepoTab, id: Oid) -> String {
    match tab.repo.find_commit_info(id) {
        Ok(c) => format!("{} {}", c.short_id(), c.summary),
        Err(_) => ferrum_core::short(id),
    }
}

pub fn merge(rev: &str, label: &str) -> Dialog {
    Dialog::Flow(FlowDialog::Merge {
        rev: rev.into(),
        label: label.into(),
        mode: crate::prefs::default_merge(),
        no_commit: false,
        allow_unrelated: false,
        preview: PreviewSlot::default(),
    })
}

pub fn rebase(tab: &RepoTab, onto: &str) -> Dialog {
    Dialog::Flow(FlowDialog::Rebase {
        onto: onto.into(),
        // Without it git refuses to start with local changes.
        autostash: !tab.status.unstaged.is_empty() || !tab.status.staged.is_empty(),
        update_refs: false,
        preview: PreviewSlot::default(),
    })
}

/// Cherry-pick of `ids`, in any order: they are applied oldest first.
pub fn cherry_pick(tab: &RepoTab, ids: &[Oid]) -> Dialog {
    let mut ids = ids.to_vec();
    // The history lists newest first.
    ids.sort_by_key(|id| std::cmp::Reverse(tab.index.get(id).copied().unwrap_or(0)));
    let parents = match ids.as_slice() {
        [one] => tab
            .repo
            .find_commit_info(*one)
            .map(|c| c.parents.len())
            .unwrap_or(0),
        _ => 0,
    };
    let label = match ids.as_slice() {
        [one] => commit_label(tab, *one),
        many => t!("{} commits", many.len()),
    };
    Dialog::Flow(FlowDialog::CherryPick {
        ids,
        label,
        parents: if parents > 1 { parents } else { 0 },
        mainline: 1,
        record_origin: false,
        commit: true,
        preview: PreviewSlot::default(),
    })
}

pub fn revert(tab: &RepoTab, id: Oid) -> Dialog {
    let parents = tab
        .repo
        .find_commit_info(id)
        .map(|c| c.parents.len())
        .unwrap_or(0);
    Dialog::Flow(FlowDialog::Revert {
        id,
        label: commit_label(tab, id),
        parents: if parents > 1 { parents } else { 0 },
        mainline: 1,
        commit: true,
        preview: PreviewSlot::default(),
    })
}

pub fn reword(tab: &RepoTab, id: Oid) -> Dialog {
    let message = tab
        .repo
        .commit_details(id)
        .map(|d| d.message.trim_end().to_owned())
        .unwrap_or_default();
    Dialog::Flow(FlowDialog::Reword {
        id,
        label: commit_label(tab, id),
        message,
    })
}

pub fn local_changes(op: Op, local: Vec<String>, touched: Vec<String>) -> Dialog {
    Dialog::Flow(FlowDialog::LocalChanges {
        op: Box::new(op),
        local,
        mode: LocalChanges::Stash,
        touched,
    })
}

/// A quick interactive-rebase action on `targets`, as an `Op`, or an error to show.
pub fn quick_op(tab: &RepoTab, targets: &[Oid], action: Quick) -> Result<Op, String> {
    let label = match (&action, targets.len()) {
        (Quick::Squash, 1) => t!("Squash {} into its parent", ferrum_core::short(targets[0])),
        (Quick::Squash, n) => t!("Squash {} commits", n),
        (Quick::Fixup, _) => t!("Fix up {} into its parent", ferrum_core::short(targets[0])),
        (Quick::Edit, _) => t!("Edit {}", ferrum_core::short(targets[0])),
        (Quick::Drop, 1) => t!("Drop {}", ferrum_core::short(targets[0])),
        (Quick::Drop, n) => t!("Drop {} commits", n),
        (Quick::Reword(_), _) => t!("Reword {}", ferrum_core::short(targets[0])),
    };
    let (base, steps) =
        rebase::quick_plan(&tab.repo, targets, &action).map_err(|e| plan_error(&e))?;
    Ok(Op::Flow(FlowOp::Plan {
        label,
        base,
        steps,
        opts: Default::default(),
    }))
}

/// Errors of `rebase::quick_plan`, translated.
fn plan_error(e: &str) -> String {
    match e {
        "No commit selected." => t!("No commit selected."),
        "Merge commits cannot be changed this way." => {
            t!("Merge commits cannot be changed this way.")
        }
        "There is no commit yet." => t!("There is no commit yet."),
        "The commit is not on the current branch." => {
            t!("The commit is not on the current branch.")
        }
        "The commits are not on one line of history." => {
            t!("The commits are not on one line of history.")
        }
        "The first commit has no parent to meld into." => {
            t!("The first commit has no parent to meld into.")
        }
        "There are merge commits after it: use the interactive rebase instead." => {
            t!("There are merge commits after it: use the interactive rebase instead.")
        }
        other => other.to_owned(),
    }
}

/// Draws the dialog. Returns `false` when it should close.
pub fn show(
    ui: &mut egui::Ui,
    dialog: &mut FlowDialog,
    tab: Option<&RepoTab>,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    let Some(tab) = tab else { return false };
    let ctx = ui.ctx().clone();
    let path = tab.path().to_path_buf();
    let current = tab.head.clone().unwrap_or_else(|| "HEAD".into());
    let close = |r: Option<bool>| r.is_none();
    match dialog {
        FlowDialog::Merge {
            rev,
            label,
            mode,
            no_commit,
            allow_unrelated,
            preview,
        } => {
            ui.heading(t!("Merge {} into {}", label, current));
            ui.radio_value(
                mode,
                MergeMode::Default,
                t!("Default (fast-forward when possible)"),
            );
            ui.radio_value(
                mode,
                MergeMode::NoFastForward,
                t!("Always create a merge commit (--no-ff)"),
            );
            ui.radio_value(mode, MergeMode::FastForwardOnly, t!("Fast-forward only"));
            ui.radio_value(
                mode,
                MergeMode::Squash,
                t!("Squash into staged changes (commit afterwards)"),
            );
            ui.add_space(4.0);
            ui.add_enabled_ui(
                !matches!(mode, MergeMode::Squash | MergeMode::FastForwardOnly),
                |ui| {
                    ui.checkbox(no_commit, t!("Don't commit (--no-commit)"))
                        .on_hover_text(t!(
                            "Merge the changes but stop before committing, to review or edit them"
                        ));
                },
            );
            ui.checkbox(allow_unrelated, t!("Allow unrelated histories"))
                .on_hover_text(t!("Merge a branch that shares no commit with this one"));
            ui.add_space(4.0);
            let (r, unrelated) = (rev.clone(), *allow_unrelated);
            let p = preview.get(&ctx, &path, format!("{r} {unrelated}"), move |g| {
                integrate::preview_merge(g, &r, unrelated)
            });
            flow::preview_ui(ui, p.as_ref(), Some(tab));
            let r = buttons(ui, &t!("Merge"), true, false, enter);
            if r == Some(true) {
                let no_commit =
                    *no_commit && !matches!(mode, MergeMode::Squash | MergeMode::FastForwardOnly);
                cmds.run(Op::Flow(FlowOp::Merge {
                    rev: rev.clone(),
                    opts: MergeOptions {
                        mode: *mode,
                        no_commit,
                        allow_unrelated: *allow_unrelated,
                    },
                }));
            }
            close(r)
        }
        FlowDialog::Rebase {
            onto,
            autostash,
            update_refs,
            preview,
        } => {
            ui.heading(t!("Rebase {}", current));
            ui.horizontal(|ui| {
                ui.label(t!("Onto"));
                onto_combo(ui, tab, onto);
            });
            let target = tab.repo.resolve(onto).ok();
            if let Some(id) = target {
                ui.weak(commit_label(tab, id));
            }
            ui.add_space(4.0);
            ui.checkbox(
                autostash,
                t!("Stash and reapply local changes (--autostash)"),
            );
            let deps = dependent_branches(tab, target);
            ui.add_enabled_ui(!deps.is_empty(), |ui| {
                ui.checkbox(update_refs, t!("Update dependent branches (--update-refs)"))
                    .on_hover_text(t!(
                        "Move other branches that point at the rebased commits along with them"
                    ));
            });
            if !deps.is_empty() {
                ui.weak(t!("Dependent branches: {}", deps.join(", ")));
            }
            ui.add_space(4.0);
            let o = onto.clone();
            let p = preview.get(&ctx, &path, o.clone(), move |g| {
                integrate::preview_rebase(g, &o)
            });
            flow::preview_ui(ui, p.as_ref(), Some(tab));
            let r = buttons(ui, &t!("Rebase"), target.is_some(), false, enter);
            if r == Some(true) {
                cmds.run(Op::Flow(FlowOp::Rebase {
                    onto: onto.clone(),
                    opts: RebaseOptions {
                        autostash: *autostash,
                        update_refs: *update_refs && !deps.is_empty(),
                    },
                }));
            }
            close(r)
        }
        FlowDialog::CherryPick {
            ids,
            label,
            parents,
            mainline,
            record_origin,
            commit,
            preview,
        } => {
            ui.heading(t!("Cherry-pick onto {}", current));
            ui.label(label.as_str());
            if ids.len() > 1 {
                egui::ScrollArea::vertical()
                    .max_height(140.0)
                    .show(ui, |ui| {
                        for id in ids.iter() {
                            ui.weak(commit_label(tab, *id));
                        }
                    });
            }
            parent_choice(ui, tab, ids.first().copied(), *parents, mainline);
            ui.checkbox(commit, t!("Commit the changes"));
            ui.add_enabled_ui(*commit, |ui| {
                ui.checkbox(record_origin, t!("Append the origin to the message (-x)"))
                    .on_hover_text(t!(
                        "Adds \"(cherry picked from commit …)\" to the commit message"
                    ));
            });
            ui.add_space(4.0);
            let m = (*parents > 0).then_some(*mainline);
            let list = ids.clone();
            let key = format!("{list:?} {m:?}");
            let p = preview.get(&ctx, &path, key, move |g| {
                integrate::preview_cherry_pick(g, &list, m)
            });
            flow::preview_ui(ui, p.as_ref(), Some(tab));
            let r = buttons(ui, &t!("Cherry-pick"), true, false, enter);
            if r == Some(true) {
                cmds.run(Op::Flow(FlowOp::CherryPick {
                    ids: ids.clone(),
                    opts: PickOptions {
                        mainline: m,
                        record_origin: *record_origin && *commit,
                        commit: *commit,
                    },
                }));
            }
            close(r)
        }
        FlowDialog::Revert {
            id,
            label,
            parents,
            mainline,
            commit,
            preview,
        } => {
            ui.heading(t!("Revert on {}", current));
            ui.label(label.as_str());
            parent_choice(ui, tab, Some(*id), *parents, mainline);
            ui.checkbox(commit, t!("Commit the changes"));
            ui.add_space(4.0);
            let m = (*parents > 0).then_some(*mainline);
            let c = *id;
            let p = preview.get(&ctx, &path, format!("{m:?}"), move |g| {
                integrate::preview_revert(g, c, m)
            });
            flow::preview_ui(ui, p.as_ref(), Some(tab));
            let r = buttons(ui, &t!("Revert"), true, false, enter);
            if r == Some(true) {
                cmds.run(Op::Flow(FlowOp::Revert {
                    id: *id,
                    mainline: m,
                    commit: *commit,
                }));
            }
            close(r)
        }
        FlowDialog::Reword { id, label, message } => {
            ui.heading(t!("Reword {}", label));
            ui.add(
                egui::TextEdit::multiline(message)
                    .desired_width(f32::INFINITY)
                    .desired_rows(6)
                    .hint_text(t!("Commit message")),
            );
            ui.weak(t!("The commits after it will be rewritten."));
            let ok = !message.trim().is_empty();
            let r = buttons(ui, &t!("Reword"), ok, false, false);
            if r == Some(true) {
                match quick_op(tab, &[*id], Quick::Reword(message.trim_end().to_owned())) {
                    Ok(op) => cmds.run(op),
                    Err(e) => cmds.push(Command::Error(e)),
                }
            }
            close(r)
        }
        FlowDialog::LocalChanges {
            op,
            local,
            touched,
            mode,
        } => {
            ui.heading(op.label());
            if touched.is_empty() {
                ui.label(if local.len() == 1 {
                    t!("There are local changes in 1 file.")
                } else {
                    t!("There are local changes in {} files.", local.len())
                });
            } else {
                ui.label(t!("These local changes would be overwritten:"));
                egui::ScrollArea::vertical()
                    .max_height(120.0)
                    .show(ui, |ui| {
                        for p in touched.iter() {
                            ui.label(RichText::new(p).monospace());
                        }
                    });
            }
            ui.add_space(4.0);
            ui.radio_value(
                mode,
                LocalChanges::Stash,
                t!("Stash and reapply them afterwards"),
            );
            ui.radio_value(mode, LocalChanges::Discard, t!("Discard them"));
            ui.add_enabled_ui(touched.is_empty(), |ui| {
                ui.radio_value(mode, LocalChanges::Keep, t!("Keep them as they are"));
            });
            if *mode == LocalChanges::Discard {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    t!("Uncommitted changes will be lost and cannot be undone."),
                );
            }
            let danger = *mode == LocalChanges::Discard;
            let r = buttons(ui, &t!("Continue"), true, danger, enter && !danger);
            if r == Some(true) {
                // Wrapped even to keep the changes, so it is not asked again.
                cmds.run(Op::Flow(FlowOp::WithLocalChanges {
                    mode: *mode,
                    op: op.clone(),
                }));
            }
            close(r)
        }
    }
}

/// Branches, remote branches and tags to rebase onto.
fn onto_combo(ui: &mut egui::Ui, tab: &RepoTab, onto: &mut String) {
    egui::ComboBox::from_id_salt("rebase-onto")
        .selected_text(flow::short_rev(onto).to_owned())
        .width(300.0)
        .height(360.0)
        .show_ui(ui, |ui| {
            let head = tab.head.as_deref();
            for kind in [BranchKind::Local, BranchKind::Remote] {
                for b in tab.branches.iter().filter(|b| b.kind == kind) {
                    if Some(b.name.as_str()) != head || kind == BranchKind::Remote {
                        let icon = if kind == BranchKind::Local {
                            "🌿"
                        } else {
                            "☁"
                        };
                        ui.selectable_value(onto, b.name.clone(), format!("{icon} {}", b.name));
                    }
                }
                ui.separator();
            }
            for t in &tab.tags {
                ui.selectable_value(onto, t.name.clone(), format!("🏷 {}", t.name));
            }
        });
}

/// Local branches (not the current one) on commits a rebase onto `onto` would replay.
fn dependent_branches(tab: &RepoTab, onto: Option<Oid>) -> Vec<String> {
    let (Some(head), Some(onto)) = (tab.head_oid, onto) else {
        return Vec::new();
    };
    tab.branches
        .iter()
        .filter(|b| b.kind == BranchKind::Local && !b.is_head)
        .filter(|b| {
            b.target.is_some_and(|t| {
                t != head && tab.repo.is_ancestor(t, head) && !tab.repo.is_ancestor(t, onto)
            })
        })
        .map(|b| b.name.clone())
        .collect()
}

/// The parent a merge commit is compared with (`-m`).
fn parent_choice(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    id: Option<Oid>,
    parents: usize,
    mainline: &mut u32,
) {
    if parents == 0 {
        return;
    }
    let ids = id
        .and_then(|i| tab.repo.find_commit_info(i).ok())
        .map(|c| c.parents)
        .unwrap_or_default();
    let name = |n: u32| {
        ids.get(n as usize - 1)
            .map(|p| t!("Parent {}: {}", n, commit_label(tab, *p)))
            .unwrap_or_else(|| t!("Parent {}", n))
    };
    ui.horizontal(|ui| {
        ui.label(t!("Mainline")).on_hover_text(t!(
            "This is a merge commit: changes are taken relative to this parent"
        ));
        egui::ComboBox::from_id_salt("flow-mainline")
            .selected_text(name(*mainline))
            .width(320.0)
            .show_ui(ui, |ui| {
                for n in 1..=parents as u32 {
                    ui.selectable_value(mainline, n, name(n));
                }
            });
    });
}
