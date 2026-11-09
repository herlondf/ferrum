//! "Absorb into commits": a preview of which commit each hunk of the local
//! changes goes to (worked out on a background thread), then fixup commits
//! and an optional autosquash rebase, all undoable.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};
use ferrum_core::absorb::{self, AbsorbHunk, Plan, Stay};
use ferrum_core::{Oid, Repo, short};

use crate::command::{Commands, Op};
use crate::icons;
use crate::stack_ops::{self, StackOp};
use crate::tab::RepoTab;
use crate::theme;
use crate::ui::dialogs::{Dialog, buttons};
use crate::ui::stacks::{StackDialog, backup_note, note};

type Slot = Arc<Mutex<Option<Result<Plan, String>>>>;

#[derive(Clone)]
pub struct AbsorbForm {
    repo: PathBuf,
    plan: Slot,
    started: bool,
    autosquash: bool,
}

/// The dialog; the plan is worked out when it first shows.
pub fn open(tab: &RepoTab) -> Dialog {
    Dialog::Stack(StackDialog::Absorb(AbsorbForm {
        repo: tab.path().to_path_buf(),
        plan: Arc::default(),
        started: false,
        autosquash: true,
    }))
}

fn start(f: &mut AbsorbForm, ctx: &egui::Context) {
    f.started = true;
    let (slot, path, ctx) = (f.plan.clone(), f.repo.clone(), ctx.clone());
    std::thread::spawn(move || {
        let plan = Repo::open(&path)
            .and_then(|r| absorb::plan(&r))
            .map_err(|e| e.to_string());
        if let Ok(mut s) = slot.lock() {
            *s = Some(plan);
        }
        ctx.request_repaint();
    });
}

pub fn reason(s: Stay) -> String {
    match s {
        Stay::NewFile => t!("new file: no commit touched it yet"),
        Stay::WholeFile => t!("deleted, renamed, binary or mode change"),
        Stay::NoOwner => t!("new lines next to lines of other commits"),
        Stay::NotOnBranch => t!("the lines come from a commit that is not on this branch"),
        Stay::SeveralCommits => t!("the lines come from several commits"),
    }
}

pub fn show(
    ui: &mut egui::Ui,
    f: &mut AbsorbForm,
    tab: &RepoTab,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    ui.set_width(620.0);
    if !f.started {
        start(f, ui.ctx());
    }
    ui.heading(t!("Absorb local changes into commits"));
    let plan = f.plan.lock().ok().and_then(|p| p.clone());
    let plan = match plan {
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(t!("Finding the commit that last touched each change…"));
            });
            return buttons(ui, &t!("Absorb"), false, false, false).is_none();
        }
        Some(Err(e)) => {
            note(ui, icons::WARNING, &e, Some(ui.visuals().error_fg_color));
            return buttons(ui, &t!("Absorb"), false, false, false).is_none();
        }
        Some(Ok(p)) => p,
    };
    let branch = tab.head.clone().unwrap_or_else(|| "HEAD".into());
    let intro = if plan.staged {
        t!(
            "Each staged change goes into the commit of {} that last touched its lines, as a fixup commit.",
            branch
        )
    } else {
        t!(
            "Each change goes into the commit of {} that last touched its lines, as a fixup commit.",
            branch
        )
    };
    ui.add(egui::Label::new(intro).wrap());
    ui.add_space(theme::SPACE_S);
    if plan.hunks.is_empty() {
        note(
            ui,
            icons::INFO,
            &t!("There are no local changes to absorb."),
            None,
        );
        return buttons(ui, &t!("Absorb"), false, false, false).is_none();
    }
    let targets = plan.targets();
    egui::ScrollArea::vertical()
        .max_height(340.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for id in targets.iter().rev() {
                group(ui, &plan, Some(*id));
            }
            if plan.hunks.iter().any(|h| h.target.is_err()) {
                group(ui, &plan, None);
            }
        });
    ui.add_space(theme::SPACE_S);
    let n = plan.absorbed();
    if n == 0 {
        note(
            ui,
            icons::INFO,
            &t!("None of the changes can be absorbed: they stay as local changes."),
            None,
        );
    } else {
        ui.checkbox(
            &mut f.autosquash,
            t!("Meld the fixup commits in right away (autosquash rebase)"),
        )
        .on_hover_text(t!(
            "Otherwise the fixup! commits stay on top, to squash later with an interactive rebase"
        ));
        let in_range: Vec<Oid> = plan.commits.iter().map(|c| c.id).collect();
        let branches = stack_ops::branches_at(tab, &in_range);
        if f.autosquash && branches.len() > 1 {
            let others = branches[1..].join(", ");
            let text = if ferrum_core::stack::git_supports_update_refs() {
                t!("Branches on these commits move along: {}", others)
            } else {
                t!(
                    "This git is older than 2.38: {} will not move along.",
                    others
                )
            };
            note(ui, icons::STACK, &text, None);
        }
        if f.autosquash {
            note(
                ui,
                icons::INFO,
                &t!("If the rebase stops for a conflict, resolve it and continue, or abort it."),
                None,
            );
        }
        backup_note(ui);
    }
    let detached = tab.head.is_none();
    if detached {
        note(
            ui,
            icons::WARNING,
            &t!("Check out a branch first."),
            Some(ui.visuals().warn_fg_color),
        );
    }
    let ok = n > 0 && !detached && !tab.busy();
    let label = t!("Absorb {}", stack_ops::hunks(n));
    let r = buttons(ui, &label, ok, false, enter && ok);
    if r == Some(true) {
        let in_range: Vec<Oid> = plan.commits.iter().map(|c| c.id).collect();
        let update_refs = f.autosquash && ferrum_core::stack::git_supports_update_refs();
        cmds.run(Op::Stack(StackOp::Absorb {
            opts: absorb::Options {
                autosquash: f.autosquash,
                update_refs,
            },
            backup: stack_ops::backup_id(tab, "absorb"),
            branches: stack_ops::branches_at(tab, &in_range),
            plan: Box::new(plan),
        }));
    }
    r.is_none()
}

/// The hunks going to `target` (or staying, with `None`).
fn group(ui: &mut egui::Ui, plan: &Plan, target: Option<Oid>) {
    let hunks: Vec<&AbsorbHunk> = plan
        .hunks
        .iter()
        .filter(|h| h.target.ok() == target)
        .collect();
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::SPACE_S;
            match target.and_then(|id| plan.commit(id)) {
                Some(c) => {
                    ui.label(RichText::new(icons::GIT_COMMIT).color(theme::accent(ui)));
                    ui.label(
                        RichText::new(short(c.id))
                            .font(crate::util::code_font(12.0))
                            .weak(),
                    );
                    ui.add(egui::Label::new(theme::strong(&c.summary)).truncate());
                }
                None => {
                    let weak = ui.visuals().weak_text_color();
                    ui.label(RichText::new(icons::FILE_DASHED).color(weak));
                    ui.label(theme::strong(t!("Stays as a local change")));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(stack_ops::hunks(hunks.len()));
            });
        });
        for h in hunks {
            hunk(ui, h);
        }
    });
    ui.add_space(theme::SPACE_XS);
}

fn hunk(ui: &mut egui::Ui, h: &AbsorbHunk) {
    let weak = ui.visuals().weak_text_color();
    ui.horizontal(|ui| {
        ui.add_space(theme::SPACE_L);
        let place = if h.old_lines == 0 && h.new_lines == 0 {
            h.path.clone()
        } else if h.old_lines == 0 {
            t!("{}, after line {}", h.path, h.old_start)
        } else if h.old_lines == 1 {
            t!("{}, line {}", h.path, h.old_start)
        } else {
            t!(
                "{}, lines {}–{}",
                h.path,
                h.old_start,
                h.old_start + h.old_lines - 1
            )
        };
        ui.label(RichText::new(place).size(12.0));
        if let Err(s) = h.target {
            ui.label(
                RichText::new(format!("· {}", reason(s)))
                    .size(12.0)
                    .color(weak),
            );
        }
    });
    let font = crate::util::code_font(11.5);
    let lines: Vec<(char, String)> = h
        .removed_text()
        .into_iter()
        .map(|l| ('−', l))
        .chain(h.added_text().into_iter().map(|l| ('+', l)))
        .collect();
    let more = lines.len().saturating_sub(4);
    for (sign, text) in lines.into_iter().take(4) {
        let color = if sign == '+' {
            crate::util::added_color(ui)
        } else {
            crate::util::removed_color(ui)
        };
        ui.horizontal(|ui| {
            ui.add_space(theme::SPACE_L);
            let shown: String = text.chars().take(90).collect();
            ui.add(
                egui::Label::new(
                    RichText::new(format!("{sign} {}", crate::util::expand_tabs(&shown)))
                        .font(font.clone())
                        .color(color),
                )
                .truncate(),
            );
        });
    }
    if more > 0 {
        ui.horizontal(|ui| {
            ui.add_space(theme::SPACE_L);
            ui.label(
                RichText::new(t!("{} more lines", more))
                    .size(11.5)
                    .color(weak),
            );
        });
    }
}
