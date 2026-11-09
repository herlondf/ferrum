//! Fetch, pull and push from the toolbar (see `toolbar.rs`) and the sidebar menus, and their
//! settings.

use eframe::egui::{self, RichText};
use ferrum_core::sync::PushRequest;
use ferrum_core::{BranchInfo, BranchKind, RemoteInfo};

use crate::command::{Command, Commands, Op};
use crate::prefs::ToolbarButton as Tb;
use crate::settings::Settings;
use crate::sync::SyncSettings;
use crate::tab::RepoTab;
use crate::ui::dialogs::Dialog;
use crate::ui::sync_dialogs as sd;

fn dialog(d: sd::SyncDialog) -> Command {
    Command::Dialog(Dialog::Sync(d))
}

/// Push the current branch to its upstream, or publish it to the default remote.
pub fn quick_push(tab: &RepoTab, s: &SyncSettings) -> Option<Op> {
    let branch = tab.head.clone()?;
    let up = tab
        .current_branch()
        .and_then(|b| b.upstream.as_deref())
        .and_then(|u| u.split_once('/'))
        .map(|(r, b)| (r.to_owned(), b.to_owned()));
    let (remote, to) = match &up {
        Some((r, b)) => (r.clone(), b.clone()),
        None => (tab.default_remote()?, branch.clone()),
    };
    Some(Op::PushRefs(PushRequest {
        remote,
        branches: vec![(branch, to)],
        set_upstream: up.is_none(),
        follow_tags: s.push_follow_tags,
        ..Default::default()
    }))
}

/// What a click on the toolbar's Fetch, Pull or Push button does: `run`
/// right away, or `open` the options dialog.
pub fn toolbar_action(
    tab: &RepoTab,
    settings: &Settings,
    button: Tb,
    run: bool,
    open: bool,
    cmds: &mut Commands,
) {
    match button {
        Tb::Fetch => {
            if run {
                cmds.run(Op::Fetch { remote: None });
            }
            if open {
                cmds.push(dialog(sd::fetch(tab, None)));
            }
        }
        Tb::Pull => {
            if run {
                cmds.run(Op::Pull(settings.pull.mode()));
            }
            if open {
                cmds.push(dialog(sd::pull(tab)));
            }
        }
        Tb::Push => {
            if run && let Some(op) = quick_push(tab, &settings.sync) {
                cmds.run(op);
            }
            if open && let Some(b) = &tab.head {
                cmds.push(dialog(sd::push(tab, b)));
            }
        }
        _ => {}
    }
}

/// Entries of a remote's context menu in the sidebar.
pub fn remote_menu(ui: &mut egui::Ui, tab: &RepoTab, remote: &RemoteInfo, cmds: &mut Commands) {
    let name = remote.name.clone();
    ui.add_enabled_ui(!tab.busy(), |ui| {
        if ui.button(t!("Fetch {}…", name)).clicked() {
            cmds.push(dialog(sd::fetch(tab, Some(&name))));
            ui.close();
        }
        if ui.button(t!("Edit {}…", name)).clicked() {
            cmds.push(dialog(sd::edit_remote(&name, &remote.url)));
            ui.close();
        }
        if ui
            .button(t!("Test connection"))
            .on_hover_text("git ls-remote --heads")
            .clicked()
        {
            cmds.run(Op::TestRemote(name.clone()));
            ui.close();
        }
    });
    let mut on = !tab.sync.auto_off.contains(&name);
    if ui
        .checkbox(&mut on, t!("Fetch {} automatically", name))
        .changed()
    {
        cmds.push(Command::Sync(Box::new(move |tab, s| {
            s.set_auto_fetch(tab, &name, on);
            tab.sync.auto_off = s
                .no_auto_fetch
                .get(&crate::ui::sidebar_model::repo_key(tab.path()))
                .cloned()
                .unwrap_or_default();
        })));
    }
}

/// The upstream to fast-forward a branch to, when it is behind and not ahead.
fn ff_target(b: &BranchInfo) -> Option<String> {
    match (b.ahead_behind, &b.upstream) {
        (Some((0, behind)), Some(u)) if behind > 0 => Some(u.clone()),
        _ => None,
    }
}

/// Entries of a local branch's context menu.
pub fn branch_menu(ui: &mut egui::Ui, tab: &RepoTab, b: &BranchInfo, cmds: &mut Commands) {
    if let Some(up) = ff_target(b)
        && ui
            .button(t!("Fast-forward {} to {}", b.name, up))
            .on_hover_text(t!("Without checking it out"))
            .clicked()
    {
        cmds.run(Op::FastForward(vec![(b.name.clone(), up)]));
        ui.close();
    }
    let has_remote_branches = tab.branches.iter().any(|r| r.kind == BranchKind::Remote);
    if has_remote_branches && ui.button(t!("Change tracking branch…")).clicked() {
        cmds.push(dialog(sd::track(tab, b)));
        ui.close();
    }
}

/// Buttons for the branches picked in the sidebar.
pub fn selection_buttons(ui: &mut egui::Ui, tab: &RepoTab, names: &[String], cmds: &mut Commands) {
    let pairs: Vec<(String, String)> = tab
        .branches
        .iter()
        .filter(|b| b.kind == BranchKind::Local && names.contains(&b.name))
        .filter_map(|b| ff_target(b).map(|u| (b.name.clone(), u)))
        .collect();
    if ui
        .add_enabled(
            !pairs.is_empty() && !tab.busy(),
            egui::Button::new(t!("Fast-forward ({})", pairs.len())).small(),
        )
        .on_hover_text(t!(
            "Fast-forward the selected branches that are behind their upstream"
        ))
        .clicked()
    {
        cmds.run(Op::FastForward(pairs));
    }
    if ui
        .add_enabled(
            !tab.remotes.is_empty() && !tab.busy(),
            egui::Button::new(t!("Push…")).small(),
        )
        .clicked()
    {
        cmds.push(dialog(sd::push_many(tab, names.to_vec())));
    }
}

/// "Push tags…" for the Tags section and tag rows.
pub fn push_tags_button(ui: &mut egui::Ui, tab: &RepoTab, tags: &[String], cmds: &mut Commands) {
    if !tab.remotes.is_empty() && !tab.tags.is_empty() && ui.button(t!("Push tags…")).clicked() {
        cmds.push(dialog(sd::push_tags(tab, tags)));
        ui.close();
    }
}

pub fn settings_section(ui: &mut egui::Ui, s: &mut SyncSettings) {
    ui.add_space(12.0);
    ui.label(
        RichText::new(t!("Fetch, pull and push"))
            .size(16.0)
            .strong(),
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(t!("Fetch open repositories every"));
        ui.add(
            egui::DragValue::new(&mut s.auto_fetch_minutes)
                .range(0..=240)
                .suffix(" min"),
        );
        ui.weak(t!("(0 turns it off)"));
    })
    .response
    .on_hover_text(t!(
        "In the background, with --prune. A remote can be left out from its menu in the sidebar."
    ));
    ui.checkbox(
        &mut s.fetch_prune,
        t!("Prune remote branches that were deleted"),
    );
    ui.checkbox(&mut s.fetch_tags, t!("Always fetch all tags"));
    ui.checkbox(
        &mut s.pull_autostash,
        t!("Stash and reapply local changes when pulling"),
    );
    ui.checkbox(
        &mut s.push_follow_tags,
        t!("Push annotated tags with the branch"),
    );
}
