//! Rebase, cherry-pick and checkout actions of the commit list: selecting
//! several commits (Ctrl/Shift+click), the menu for them, quick
//! interactive-rebase actions and checking out a label by double-clicking it.

use eframe::egui;
use ferrum_core::rebase::Quick;
use ferrum_core::{BranchKind, CommitInfo, Oid, RefKind};

use crate::command::{Command, Commands, Op};
use crate::tab::RepoTab;
use crate::ui::dialogs::Dialog;
use crate::ui::flow_dialogs;
use crate::ui::history_options::cmd;

/// Handles Ctrl+click (add or remove) and Shift+click (range from the
/// selected commit). Returns false for a plain click, which clears the set.
pub fn click(tab: &mut RepoTab, id: Oid, mods: egui::Modifiers) -> bool {
    if mods.shift {
        let Some(anchor) = tab.history.selected else {
            return false;
        };
        let rows: Vec<usize> = match &tab.history.visible {
            Some(v) => v.clone(),
            None => (0..tab.commits.len()).collect(),
        };
        let pos = |x: Oid| {
            tab.index
                .get(&x)
                .and_then(|i| rows.iter().position(|r| r == i))
        };
        let (Some(a), Some(b)) = (pos(anchor), pos(id)) else {
            return false;
        };
        let (lo, hi) = (a.min(b), a.max(b));
        tab.history.ext.selected_many = rows[lo..=hi].iter().map(|&i| tab.commits[i].id).collect();
        true
    } else if mods.command {
        let many = &mut tab.history.ext.selected_many;
        if many.is_empty()
            && let Some(s) = tab.history.selected
        {
            many.push(s);
        }
        match many.iter().position(|x| *x == id) {
            Some(p) => {
                many.remove(p);
            }
            None => many.push(id),
        }
        true
    } else {
        tab.history.ext.selected_many.clear();
        false
    }
}

/// Whether the row is part of a multiple selection.
pub fn is_picked(tab: &RepoTab, id: Oid) -> bool {
    tab.history.ext.selected_many.len() > 1 && tab.history.ext.selected_many.contains(&id)
}

fn in_head(tab: &RepoTab, id: Oid) -> bool {
    tab.head_oid.is_some_and(|h| tab.repo.is_ancestor(id, h))
}

/// Confirms and runs a quick rebase action, or shows why it cannot run.
fn quick(tab: &RepoTab, ids: &[Oid], action: Quick, cmds: &mut Commands) {
    match flow_dialogs::quick_op(tab, ids, action) {
        Ok(op) => {
            let branch = tab.head.clone().unwrap_or_else(|| "HEAD".into());
            let text = t!(
                "{}? The commits of {} will be rewritten.",
                op.label(),
                branch
            );
            cmds.push(Command::Dialog(Dialog::confirm(
                &t!("Rewrite history"),
                &text,
                op,
                false,
            )));
        }
        Err(e) => cmds.push(Command::Error(e)),
    }
}

/// Menu for several selected commits. Returns false (drawing nothing) when
/// `id` is not part of a multiple selection.
pub fn multi_menu(ui: &mut egui::Ui, tab: &RepoTab, id: Oid, cmds: &mut Commands) -> bool {
    if !is_picked(tab, id) {
        return false;
    }
    let many = tab.history.ext.selected_many.clone();
    let n = many.len();
    ui.set_min_width(240.0);
    ui.weak(t!("{} commits selected", n));
    ui.separator();
    ui.add_enabled_ui(!tab.busy(), |ui| {
        if ui.button(t!("Cherry-pick {} commits…", n)).clicked() {
            cmds.push(Command::Dialog(flow_dialogs::cherry_pick(tab, &many)));
            ui.close();
        }
        let on_branch = tab.head.is_some() && many.iter().all(|c| in_head(tab, *c));
        ui.add_enabled_ui(on_branch, |ui| {
            if ui
                .button(t!("Squash {} commits into one…", n))
                .on_disabled_hover_text(t!("Only commits of the current branch can be squashed"))
                .clicked()
            {
                quick(tab, &many, Quick::Squash, cmds);
                ui.close();
            }
            if ui
                .button(t!("Drop {} commits…", n))
                .on_disabled_hover_text(t!("Only commits of the current branch can be dropped"))
                .clicked()
            {
                quick(tab, &many, Quick::Drop, cmds);
                ui.close();
            }
        });
        crate::ui::patches::multi_menu(ui, tab, &many, cmds);
    });
    ui.separator();
    if ui.button(t!("Clear selection")).clicked() {
        cmds.push(cmd(|t, _| t.history.ext.selected_many.clear()));
        ui.close();
    }
    true
}

/// "Interactive rebase ▸" with the planner and quick actions, for commits of
/// the current branch.
pub fn rebase_menu(ui: &mut egui::Ui, tab: &RepoTab, c: &CommitInfo, cmds: &mut Commands) {
    let id = c.id;
    if tab.head.is_none() || !in_head(tab, id) {
        return;
    }
    let short = c.short_id();
    ui.menu_button(t!("Interactive rebase"), |ui| {
        ui.set_min_width(240.0);
        if tab.head_oid != Some(id)
            && ui
                .button(t!("Interactive rebase after {}…", short))
                .clicked()
        {
            cmds.push(Command::PlanRebase(Some(id)));
            ui.close();
        }
        if c.parents.is_empty() {
            if ui
                .button(t!("Interactive rebase from the first commit…"))
                .clicked()
            {
                cmds.push(Command::PlanRebase(None));
                ui.close();
            }
        } else if c.parents.len() == 1
            && ui
                .button(t!("Interactive rebase from {}…", short))
                .clicked()
        {
            cmds.push(Command::PlanRebase(c.parents.first().copied()));
            ui.close();
        }
        ui.separator();
        ui.weak(t!("Quick actions"));
        let plain = c.parents.len() <= 1;
        ui.add_enabled_ui(plain, |ui| {
            if ui.button(t!("Reword…")).clicked() {
                cmds.push(Command::Dialog(flow_dialogs::reword(tab, id)));
                ui.close();
            }
            if ui
                .button(t!("Edit…"))
                .on_hover_text(t!("Stop the rebase at this commit so you can amend it"))
                .clicked()
            {
                quick(tab, &[id], Quick::Edit, cmds);
                ui.close();
            }
            let has_parent = !c.parents.is_empty();
            ui.add_enabled_ui(has_parent, |ui| {
                if ui
                    .button(t!("Squash into parent…"))
                    .on_hover_text(t!("Meld into the commit before it, keeping both messages"))
                    .clicked()
                {
                    quick(tab, &[id], Quick::Squash, cmds);
                    ui.close();
                }
                if ui
                    .button(t!("Fixup into parent…"))
                    .on_hover_text(t!("Meld into the commit before it, keeping its message"))
                    .clicked()
                {
                    quick(tab, &[id], Quick::Fixup, cmds);
                    ui.close();
                }
            });
            if ui.button(t!("Drop…")).clicked() {
                quick(tab, &[id], Quick::Drop, cmds);
                ui.close();
            }
        });
    });
}

/// Checks out what a double-clicked label names: a local branch, the local
/// branch tracking a remote one (or a new one), or a tag (detached).
pub fn checkout_label(
    tab: &RepoTab,
    kind: RefKind,
    name: &str,
    is_head: bool,
    cmds: &mut Commands,
) {
    if tab.busy() || is_head {
        return;
    }
    match kind {
        RefKind::Local => cmds.run(Op::Checkout(name.to_owned())),
        RefKind::Remote => {
            let local = tab
                .branches
                .iter()
                .find(|b| b.kind == BranchKind::Local && b.upstream.as_deref() == Some(name));
            match local {
                Some(l) if l.is_head => {}
                Some(l) => cmds.run(Op::Checkout(l.name.clone())),
                None => {
                    let local = name.split_once('/').map_or(name, |(_, b)| b);
                    cmds.push(Command::Dialog(Dialog::checkout_remote(name, local)));
                }
            }
        }
        RefKind::Tag => cmds.push(Command::Dialog(Dialog::confirm(
            &t!("Check out commit"),
            &t!("Check out {}? HEAD will be detached: new commits will not belong to any branch until you create one.", name),
            Op::CheckoutDetached(name.to_owned()),
            false,
        ))),
    }
}
