//! The bar shown while a merge, rebase, cherry-pick or revert is stopped:
//! what is going on (with links to the commits involved) and Continue,
//! Skip and Abort.

use eframe::egui::{self, RichText};
use ferrum_core::{Oid, RefKind, RepoState};

use crate::command::{Command, Commands, Op};
use crate::flow::FlowOp;
use crate::tab::RepoTab;
use crate::ui::conflicts;
use crate::ui::dialogs::Dialog;

/// A branch or tag name for a commit, or its short hash.
pub fn name_of(tab: &RepoTab, id: Oid) -> String {
    let labels = tab.labels.get(&id);
    let pick = |kind: RefKind| {
        labels.and_then(|ls| ls.iter().find(|l| l.kind == kind).map(|l| l.name.clone()))
    };
    pick(RefKind::Local)
        .or_else(|| pick(RefKind::Remote))
        .or_else(|| pick(RefKind::Tag))
        .unwrap_or_else(|| ferrum_core::short(id))
}

/// "abc1234 Subject" of a commit.
fn commit_text(tab: &RepoTab, id: Oid) -> String {
    match tab.repo.find_commit_info(id) {
        Ok(c) => {
            let mut s: String = c.summary.chars().take(50).collect();
            if c.summary.chars().count() > 50 {
                s.push('…');
            }
            format!("{} {}", c.short_id(), s)
        }
        Err(_) => ferrum_core::short(id),
    }
}

fn link(ui: &mut egui::Ui, tab: &RepoTab, text: String, id: Oid, cmds: &mut Commands) {
    if ui
        .link(RichText::new(text).strong())
        .on_hover_text(t!("Show this commit"))
        .clicked()
    {
        cmds.push(if tab.index.contains_key(&id) {
            Command::SelectCommit(id)
        } else {
            Command::ShowCommit(id)
        });
    }
}

pub fn show(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    if tab.state == RepoState::Clean {
        tab.conflicts.resolved.clear();
        return;
    }
    if tab.state == RepoState::Bisect {
        return crate::ui::bisect::banner(ui, tab, cmds);
    }
    let info = conflicts::in_progress(tab);
    let tab = &*tab;
    let conflicts = tab.status.conflicted.len();
    let head = tab.head.clone().unwrap_or_else(|| "HEAD".into());
    let fill = ui.visuals().warn_fg_color.gamma_multiply(0.25);
    egui::Frame::new()
        .fill(fill)
        .inner_margin(6.0)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong("⚠");
                match tab.state {
                    RepoState::Merge => {
                        ui.label(t!("Merging"));
                        match (info.merge_heads.first(), &info.merge_name) {
                            (Some(id), name) => {
                                let text = name.clone().unwrap_or_else(|| name_of(tab, *id));
                                link(ui, tab, text, *id, cmds);
                            }
                            (None, Some(name)) => {
                                ui.strong(name);
                            }
                            (None, None) => {}
                        }
                        ui.label(t!("into"));
                        ui.strong(&head);
                    }
                    RepoState::Rebase => {
                        ui.label(t!("Rebasing"));
                        ui.strong(info.head_name.clone().unwrap_or_else(|| head.clone()));
                        if let Some(onto) = info.onto {
                            ui.label(t!("onto"));
                            link(ui, tab, name_of(tab, onto), onto, cmds);
                        }
                        if let Some((done, total)) = info.step {
                            ui.weak(t!("({} of {})", done, total));
                            ui.add(
                                egui::ProgressBar::new(done as f32 / total.max(1) as f32)
                                    .desired_width(80.0),
                            );
                        }
                        if let Some(cur) = info.current {
                            ui.label(t!("stopped at"));
                            link(ui, tab, commit_text(tab, cur), cur, cmds);
                        }
                    }
                    RepoState::CherryPick | RepoState::Revert => {
                        ui.label(if tab.state == RepoState::CherryPick {
                            t!("Cherry-picking")
                        } else {
                            t!("Reverting")
                        });
                        if let Some(cur) = info.current {
                            link(ui, tab, commit_text(tab, cur), cur, cmds);
                        }
                        ui.label(t!("on"));
                        ui.strong(&head);
                    }
                    RepoState::Am => crate::ui::patches::banner_text(ui, tab),
                    s => {
                        ui.strong(crate::util::state_label(s));
                    }
                }
                ui.label("—");
                if conflicts == 1 {
                    ui.label(t!("fix 1 conflict and then continue."));
                } else if conflicts > 1 {
                    ui.label(t!("fix {} conflicts and then continue.", conflicts));
                } else if tab.state != RepoState::Bisect {
                    ui.label(t!("no conflicts left: continue when ready."));
                }
                buttons(ui, tab, cmds, conflicts);
            });
        });
}

fn buttons(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands, conflicts: usize) {
    ui.add_enabled_ui(!tab.busy(), |ui| {
        if conflicts > 0 && ui.button(t!("Resolve conflicts")).clicked() {
            cmds.push(Command::ResolveConflict(
                tab.status.conflicted[0].path.clone(),
            ));
        }
        let can_continue = conflicts == 0 && tab.state != RepoState::Bisect;
        if ui
            .add_enabled(can_continue, egui::Button::new(t!("Continue")))
            .clicked()
        {
            cmds.run(Op::Continue(tab.state));
        }
        if matches!(
            tab.state,
            RepoState::Rebase | RepoState::CherryPick | RepoState::Revert | RepoState::Am
        ) && ui
            .button(t!("Skip"))
            .on_hover_text(t!("Leave out the commit being applied"))
            .clicked()
        {
            cmds.run(Op::Skip(tab.state));
        }
        if ui.button(t!("Abort")).clicked() {
            let name = crate::util::state_name(tab.state);
            // One line, so the translation tests find the key.
            #[rustfmt::skip]
            let text = t!("Abort the {} in progress? Changes made by it will be thrown away.", name);
            cmds.push(Command::Dialog(Dialog::confirm(
                &t!("Abort"),
                &text,
                Op::Abort(tab.state),
                true,
            )));
        }
        let resolved = &tab.conflicts.resolved;
        if !resolved.is_empty() {
            ui.menu_button(t!("Resolved ({}) ⏷", resolved.len()), |ui| {
                ui.weak(t!("Bring back the conflict of a file marked resolved:"));
                for p in resolved {
                    if ui.button(t!("Undo {}", p)).clicked() {
                        cmds.run(Op::Flow(FlowOp::Unresolve(vec![p.clone()])));
                        ui.close();
                    }
                }
                if resolved.len() > 1 && ui.button(t!("Undo all")).clicked() {
                    cmds.run(Op::Flow(FlowOp::Unresolve(resolved.clone())));
                    ui.close();
                }
            });
        }
    });
}
