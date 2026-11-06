//! Modal dialogs.

use std::path::PathBuf;

use eframe::egui::{self, RichText};
use ferrum_core::ops::{PushOptions, ResetMode};
use ferrum_core::{BranchInfo, BranchKind, Identity};

use crate::command::{Command, Commands, Op};
use crate::settings::{Profile, Settings};
use crate::tab::RepoTab;
use crate::ui::forge as forge_ui;
use crate::util::valid_ref_name;

pub enum Dialog {
    Confirm {
        title: String,
        text: String,
        op: Box<Op>,
        danger: bool,
    },
    CreateBranch {
        start: String,
        start_label: String,
        name: String,
        checkout: bool,
    },
    RenameBranch {
        old: String,
        new: String,
    },
    DeleteBranch {
        name: String,
        force: bool,
        remote: Option<(String, String)>,
        delete_remote: bool,
    },
    CheckoutRemote {
        remote_branch: String,
        local: String,
    },
    CreateTag {
        rev: String,
        label: String,
        name: String,
        message: String,
        push_to: Option<String>,
    },
    DeleteTag {
        name: String,
        remote: Option<String>,
    },
    Reset {
        rev: String,
        label: String,
        branch: String,
        mode: ResetMode,
    },
    Push {
        remote: String,
        branch: String,
        set_upstream: bool,
        force: bool,
        tags: bool,
    },
    Pull,
    Stash {
        message: String,
        untracked: bool,
        staged_only: bool,
    },
    AddRemote {
        name: String,
        url: String,
    },
    Clone {
        url: String,
        parent: String,
        name: String,
    },
    Init {
        path: String,
    },
    WorktreeAdd {
        path: String,
        rev: String,
        new_branch: String,
    },
    Identity {
        name: String,
        email: String,
        signing_key: String,
        sign: bool,
        ssh_key: String,
    },
    Forge(forge_ui::ForgeDialog),
    /// Merge, rebase, cherry-pick, revert, reword and local-changes prompts.
    Flow(crate::ui::flow_dialogs::FlowDialog),
}

impl Dialog {
    pub fn confirm(title: &str, text: &str, op: Op, danger: bool) -> Self {
        Dialog::Confirm {
            title: title.into(),
            text: text.into(),
            op: Box::new(op),
            danger,
        }
    }

    pub fn create_branch(start: &str, label: &str) -> Self {
        Dialog::CreateBranch {
            start: start.into(),
            start_label: label.into(),
            name: String::new(),
            checkout: true,
        }
    }

    pub fn create_tag(rev: &str, label: &str) -> Self {
        Dialog::CreateTag {
            rev: rev.into(),
            label: label.into(),
            name: String::new(),
            message: String::new(),
            push_to: None,
        }
    }

    pub fn merge(rev: &str, label: &str) -> Self {
        crate::ui::flow_dialogs::merge(rev, label)
    }

    pub fn reset(rev: &str, label: &str, branch: &str, mode: ResetMode) -> Self {
        Dialog::Reset {
            rev: rev.into(),
            label: label.into(),
            branch: branch.into(),
            mode,
        }
    }

    pub fn checkout_remote(remote_branch: &str, local: &str) -> Self {
        Dialog::CheckoutRemote {
            remote_branch: remote_branch.into(),
            local: local.into(),
        }
    }

    pub fn delete_branch(b: &BranchInfo) -> Self {
        let remote = b
            .upstream
            .as_deref()
            .and_then(|u| u.split_once('/'))
            .map(|(r, n)| (r.to_owned(), n.to_owned()));
        Dialog::DeleteBranch {
            name: b.name.clone(),
            force: false,
            remote,
            delete_remote: false,
        }
    }

    pub fn push(tab: &RepoTab, branch: &str) -> Self {
        let info = tab
            .branches
            .iter()
            .find(|b| b.kind == BranchKind::Local && b.name == branch);
        let upstream_remote = info
            .and_then(|b| b.upstream.as_deref())
            .and_then(|u| u.split_once('/'))
            .map(|(r, _)| r.to_owned());
        Dialog::Push {
            remote: upstream_remote
                .clone()
                .or_else(|| tab.default_remote())
                .unwrap_or_default(),
            branch: branch.into(),
            set_upstream: upstream_remote.is_none(),
            force: false,
            tags: false,
        }
    }

    pub fn stash() -> Self {
        Dialog::Stash {
            message: String::new(),
            untracked: true,
            staged_only: false,
        }
    }

    pub fn identity(id: &Identity) -> Self {
        let key_from_ssh = id
            .ssh_command
            .as_deref()
            .and_then(|c| c.split('"').nth(1))
            .unwrap_or_default()
            .to_owned();
        Dialog::Identity {
            name: id.name.clone().unwrap_or_default(),
            email: id.email.clone().unwrap_or_default(),
            signing_key: id.signing_key.clone().unwrap_or_default(),
            sign: id.gpg_sign,
            ssh_key: key_from_ssh,
        }
    }

    pub fn worktree_add(tab: &RepoTab) -> Self {
        let parent = tab
            .path()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let path = parent
            .join(format!("{}-worktree", tab.name))
            .to_string_lossy()
            .into_owned();
        Dialog::WorktreeAdd {
            path,
            rev: tab.head.clone().unwrap_or_else(|| "HEAD".into()),
            new_branch: String::new(),
        }
    }

    pub fn clone_repo(settings: &Settings) -> Self {
        let parent = if settings.clone_dir.is_empty() {
            dirs_home()
                .map(|h| h.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            settings.clone_dir.clone()
        };
        Dialog::Clone {
            url: String::new(),
            parent,
            name: String::new(),
        }
    }
}

pub fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Name of the folder `git clone` would create for a URL.
pub fn clone_dir_name(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/').trim_end_matches(".git");
    trimmed
        .rsplit(['/', ':'])
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// Shows the dialog. Returns `false` when it should close.
pub fn show(
    ctx: &egui::Context,
    dialog: &mut Dialog,
    tab: Option<&RepoTab>,
    settings: &mut Settings,
    cmds: &mut Commands,
) -> bool {
    let mut open = true;
    let modal = egui::Modal::new(egui::Id::unique("ferrum-dialog")).show(ctx, |ui| {
        ui.set_width(460.0);
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
        open = body(ui, dialog, tab, settings, cmds, enter);
    });
    if modal.should_close() {
        return false;
    }
    open
}

/// Draws OK/Cancel buttons. Returns Some(true) for OK, Some(false) for Cancel.
pub(crate) fn buttons(
    ui: &mut egui::Ui,
    ok: &str,
    enabled: bool,
    danger: bool,
    enter: bool,
) -> Option<bool> {
    ui.add_space(8.0);
    let mut result = None;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let text = if danger {
                RichText::new(ok).color(ui.visuals().error_fg_color)
            } else {
                RichText::new(ok)
            };
            if ui.add_enabled(enabled, egui::Button::new(text)).clicked() || (enter && enabled) {
                result = Some(true);
            }
            if ui.button(t!("Cancel")).clicked() {
                result = Some(false);
            }
        });
    });
    result
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) -> egui::Response {
    ui.label(label);
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(f32::INFINITY),
    )
}

fn ref_name_warning(ui: &mut egui::Ui, name: &str) {
    if !name.is_empty() && !valid_ref_name(name) {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            t!("Names cannot contain spaces or characters like ~ ^ : ? * [ \\ or \"..\"."),
        );
    }
}

fn body(
    ui: &mut egui::Ui,
    dialog: &mut Dialog,
    tab: Option<&RepoTab>,
    settings: &mut Settings,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    let mut run = |op: Op| cmds.push(Command::Run(op));
    match dialog {
        Dialog::Confirm {
            title,
            text,
            op,
            danger,
        } => {
            ui.heading(title.as_str());
            ui.label(text.as_str());
            let ok = if *danger { t!("Yes, do it") } else { t!("OK") };
            match buttons(ui, &ok, true, *danger, enter && !*danger) {
                Some(true) => {
                    run((**op).clone());
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        Dialog::CreateBranch {
            start,
            start_label,
            name,
            checkout,
        } => {
            ui.heading(t!("New branch"));
            ui.label(t!("Starting at {}", start_label));
            field(ui, &t!("Name"), name, "feature/my-change").request_focus();
            // Spaces become the replacement chosen in Settings > Git.
            if name.contains(' ') {
                *name = settings.prefs.branch_name(name);
            }
            ref_name_warning(ui, name);
            ui.checkbox(checkout, t!("Check out after creating"));
            let ok = valid_ref_name(name);
            match buttons(ui, &t!("Create branch"), ok, false, enter) {
                Some(true) => {
                    run(Op::CreateBranch {
                        name: name.trim().into(),
                        start: start.clone(),
                        checkout: *checkout,
                    });
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        Dialog::RenameBranch { old, new } => {
            ui.heading(t!("Rename {}", old));
            field(ui, &t!("New name"), new, "").request_focus();
            ref_name_warning(ui, new);
            let ok = valid_ref_name(new) && new.trim() != old;
            match buttons(ui, &t!("Rename"), ok, false, enter) {
                Some(true) => {
                    run(Op::RenameBranch {
                        old: old.clone(),
                        new: new.trim().into(),
                    });
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        Dialog::DeleteBranch {
            name,
            force,
            remote,
            delete_remote,
        } => {
            ui.heading(t!("Delete {}", name));
            ui.checkbox(force, t!("Delete even if it is not merged"))
                .on_hover_text(t!(
                    "Commits only on this branch may be lost (you can still undo right after)"
                ));
            if let Some((r, n)) = remote {
                ui.checkbox(delete_remote, t!("Also delete {} from {}", n, r));
            }
            match buttons(ui, &t!("Delete"), true, true, false) {
                Some(true) => {
                    let mut ops = vec![Op::DeleteBranch {
                        name: name.clone(),
                        force: *force,
                    }];
                    if *delete_remote && let Some((r, n)) = remote {
                        ops.push(Op::DeleteRemoteRef {
                            remote: r.clone(),
                            name: n.clone(),
                        });
                    }
                    for op in ops {
                        run(op);
                    }
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        Dialog::CheckoutRemote {
            remote_branch,
            local,
        } => {
            ui.heading(t!("Check out {}", remote_branch));
            ui.label(t!("A local branch tracking it will be created."));
            field(ui, &t!("Local branch name"), local, "").request_focus();
            ref_name_warning(ui, local);
            let exists = tab.is_some_and(|t| {
                t.branches
                    .iter()
                    .any(|b| b.kind == BranchKind::Local && b.name == local.trim())
            });
            if exists {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    t!("A local branch with this name already exists."),
                );
            }
            match buttons(
                ui,
                &t!("Check out"),
                valid_ref_name(local) && !exists,
                false,
                enter,
            ) {
                Some(true) => {
                    run(Op::CheckoutRemote {
                        remote_branch: remote_branch.clone(),
                        local: local.trim().into(),
                    });
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        Dialog::CreateTag {
            rev: _,
            label,
            name,
            message,
            push_to,
        } => {
            ui.heading(t!("New tag"));
            ui.label(t!("At {}", label));
            field(ui, &t!("Name"), name, "v1.0.0").request_focus();
            ref_name_warning(ui, name);
            ui.label(t!("Message (optional — makes an annotated tag)"));
            ui.add(
                egui::TextEdit::multiline(message)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
            if let Some(t) = tab
                && !t.remotes.is_empty()
            {
                let mut push = push_to.is_some();
                ui.horizontal(|ui| {
                    ui.checkbox(&mut push, t!("Push to"));
                    let mut remote = push_to
                        .clone()
                        .or_else(|| t.default_remote())
                        .unwrap_or_default();
                    egui::ComboBox::from_id_salt("tag-remote")
                        .selected_text(&remote)
                        .show_ui(ui, |ui| {
                            for r in &t.remotes {
                                ui.selectable_value(&mut remote, r.name.clone(), &r.name);
                            }
                        });
                    *push_to = push.then_some(remote);
                });
            }
            match buttons(ui, &t!("Create tag"), valid_ref_name(name), false, false) {
                Some(true) => {
                    let Dialog::CreateTag {
                        rev,
                        name,
                        message,
                        push_to,
                        ..
                    } = dialog
                    else {
                        unreachable!()
                    };
                    let msg = Some(message.clone()).filter(|m| !m.trim().is_empty());
                    run(Op::CreateTag {
                        name: name.trim().into(),
                        rev: rev.clone(),
                        message: msg,
                    });
                    if let Some(remote) = push_to {
                        run(Op::PushTag {
                            remote: remote.clone(),
                            tag: name.trim().into(),
                        });
                    }
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        Dialog::DeleteTag { name, remote } => {
            ui.heading(t!("Delete tag {}", name));
            if let Some(t) = tab
                && !t.remotes.is_empty()
            {
                let mut also = remote.is_some();
                ui.horizontal(|ui| {
                    ui.checkbox(&mut also, t!("Also delete from"));
                    let mut r = remote
                        .clone()
                        .or_else(|| t.default_remote())
                        .unwrap_or_default();
                    egui::ComboBox::from_id_salt("tag-del-remote")
                        .selected_text(&r)
                        .show_ui(ui, |ui| {
                            for rem in &t.remotes {
                                ui.selectable_value(&mut r, rem.name.clone(), &rem.name);
                            }
                        });
                    *remote = also.then_some(r);
                });
            }
            match buttons(ui, &t!("Delete"), true, true, false) {
                Some(true) => {
                    run(Op::DeleteTag(name.clone()));
                    if let Some(r) = remote {
                        run(Op::DeleteRemoteRef {
                            remote: r.clone(),
                            name: format!("refs/tags/{name}"),
                        });
                    }
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        Dialog::Reset {
            rev,
            label,
            branch,
            mode,
        } => {
            ui.heading(t!("Reset {} to {}", branch, label));
            ui.radio_value(mode, ResetMode::Soft, t!("Soft — keep all changes staged"));
            ui.radio_value(
                mode,
                ResetMode::Mixed,
                t!("Mixed — keep changes in the working copy"),
            );
            ui.radio_value(
                mode,
                ResetMode::Hard,
                t!("Hard — discard all local changes"),
            );
            if *mode == ResetMode::Hard {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    t!("Uncommitted changes will be lost and cannot be undone."),
                );
            }
            match buttons(ui, &t!("Reset"), true, *mode == ResetMode::Hard, false) {
                Some(true) => {
                    run(Op::Reset {
                        rev: rev.clone(),
                        mode: *mode,
                    });
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        Dialog::Push {
            remote,
            branch,
            set_upstream,
            force,
            tags,
        } => {
            ui.heading(t!("Push"));
            let Some(t) = tab else { return false };
            if t.remotes.is_empty() {
                ui.label(t!("This repository has no remotes. Add one first (right-click Remotes in the sidebar)."));
                return buttons(ui, &t!("OK"), true, false, enter).is_none();
            }
            egui::Grid::new("push-grid")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label(t!("Branch"));
                    egui::ComboBox::from_id_salt("push-branch")
                        .selected_text(branch.as_str())
                        .show_ui(ui, |ui| {
                            for b in t.branches.iter().filter(|b| b.kind == BranchKind::Local) {
                                ui.selectable_value(branch, b.name.clone(), &b.name);
                            }
                        });
                    ui.end_row();
                    ui.label(t!("To remote"));
                    egui::ComboBox::from_id_salt("push-remote")
                        .selected_text(remote.as_str())
                        .show_ui(ui, |ui| {
                            for r in &t.remotes {
                                ui.selectable_value(remote, r.name.clone(), &r.name);
                            }
                        });
                    ui.end_row();
                });
            ui.checkbox(set_upstream, t!("Track this remote branch (set upstream)"));
            ui.checkbox(tags, t!("Also push annotated tags"));
            ui.checkbox(force, t!("Force push (with lease)"))
                .on_hover_text(t!("Overwrites the remote branch, unless someone else pushed to it since your last fetch"));
            if *force {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    t!("Force pushing rewrites history on the remote."),
                );
            }
            match buttons(
                ui,
                &t!("Push"),
                !remote.is_empty() && !branch.is_empty(),
                *force,
                enter,
            ) {
                Some(true) => {
                    run(Op::Push(PushOptions {
                        remote: remote.clone(),
                        branch: branch.clone(),
                        set_upstream: *set_upstream,
                        force_with_lease: *force,
                        tags: *tags,
                    }));
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        Dialog::Pull => {
            ui.heading(t!("Pull"));
            let branch = tab.and_then(|t| t.current_branch().cloned());
            match branch.as_ref().and_then(|b| b.upstream.clone()) {
                Some(u) => ui.label(t!(
                    "Pull {} into {}",
                    u,
                    branch.as_ref().map_or("", |b| b.name.as_str())
                )),
                None => ui.colored_label(
                    ui.visuals().warn_fg_color,
                    t!("The current branch has no upstream; git will use its defaults."),
                ),
            };
            for choice in [
                crate::settings::PullChoice::Default,
                crate::settings::PullChoice::Merge,
                crate::settings::PullChoice::Rebase,
                crate::settings::PullChoice::FastForwardOnly,
            ] {
                use crate::settings::PullChoice;
                let label = match choice {
                    PullChoice::Default => t!("Default (git config)"),
                    PullChoice::Merge => t!("Merge"),
                    PullChoice::Rebase => t!("Rebase"),
                    PullChoice::FastForwardOnly => t!("Fast-forward only"),
                };
                ui.radio_value(&mut settings.pull, choice, label);
            }
            match buttons(ui, &t!("Pull"), true, false, enter) {
                Some(true) => {
                    run(Op::Pull(settings.pull.mode()));
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        Dialog::Stash {
            message,
            untracked,
            staged_only,
        } => {
            ui.heading(t!("Stash changes"));
            field(
                ui,
                &t!("Message (optional)"),
                message,
                &t!("work in progress"),
            )
            .request_focus();
            ui.checkbox(untracked, t!("Include new (untracked) files"));
            ui.checkbox(staged_only, t!("Only staged changes"));
            match buttons(ui, &t!("Stash"), true, false, enter) {
                Some(true) => {
                    run(Op::StashPush {
                        message: message.clone(),
                        untracked: *untracked && !*staged_only,
                        staged_only: *staged_only,
                    });
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        Dialog::AddRemote { name, url } => {
            ui.heading(t!("Add remote"));
            field(ui, &t!("Name"), name, "origin").request_focus();
            field(ui, "URL", url, "git@github.com:user/repo.git");
            let ok = valid_ref_name(name) && !url.trim().is_empty();
            match buttons(ui, &t!("Add remote"), ok, false, enter) {
                Some(true) => {
                    run(Op::AddRemote {
                        name: name.trim().into(),
                        url: url.trim().into(),
                    });
                    run(Op::Fetch {
                        remote: Some(name.trim().into()),
                    });
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        Dialog::Clone { url, parent, name } => {
            ui.heading(t!("Clone repository"));
            let before = clone_dir_name(url);
            if field(
                ui,
                &t!("Repository URL"),
                url,
                "https://github.com/user/repo.git",
            )
            .changed()
                && (name.is_empty() || *name == before)
            {
                *name = clone_dir_name(url);
            }
            ui.label(t!("Into folder"));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(parent).desired_width(340.0));
                if ui.button(t!("Browse…")).clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_folder()
                {
                    *parent = p.to_string_lossy().into_owned();
                }
            });
            field(ui, &t!("Folder name"), name, "");
            let target = PathBuf::from(parent.as_str()).join(name.trim());
            let exists = !name.trim().is_empty() && target.exists();
            if exists {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    t!("That folder already exists."),
                );
            }
            let ok = !url.trim().is_empty()
                && !name.trim().is_empty()
                && !parent.trim().is_empty()
                && !exists;
            match buttons(ui, &t!("Clone"), ok, false, enter) {
                Some(true) => {
                    settings.clone_dir = parent.clone();
                    cmds.push(Command::Clone {
                        url: url.trim().into(),
                        parent: PathBuf::from(parent.as_str()),
                        name: name.trim().into(),
                    });
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        Dialog::Init { path } => {
            ui.heading(t!("New repository"));
            ui.label(t!("Folder"));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(path).desired_width(340.0));
                if ui.button(t!("Browse…")).clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_folder()
                {
                    *path = p.to_string_lossy().into_owned();
                }
            });
            ui.weak(t!("The folder is created if it does not exist."));
            match buttons(ui, &t!("Create"), !path.trim().is_empty(), false, enter) {
                Some(true) => {
                    cmds.push(Command::Init(path.trim().into()));
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        Dialog::WorktreeAdd {
            path,
            rev,
            new_branch,
        } => {
            ui.heading(t!("Add worktree"));
            ui.label(t!("Folder"));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(path).desired_width(340.0));
                if ui.button(t!("Browse…")).clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_folder()
                {
                    *path = p.to_string_lossy().into_owned();
                }
            });
            field(ui, &t!("Start from (branch, tag or commit)"), rev, "main");
            field(
                ui,
                &t!("New branch (optional)"),
                new_branch,
                &t!("leave empty to check out the branch above"),
            );
            if !new_branch.is_empty() {
                ref_name_warning(ui, new_branch);
            }
            let ok = !path.trim().is_empty()
                && !rev.trim().is_empty()
                && (new_branch.trim().is_empty() || valid_ref_name(new_branch));
            match buttons(ui, &t!("Add worktree"), ok, false, enter) {
                Some(true) => {
                    let nb = Some(new_branch.trim().to_owned()).filter(|b| !b.is_empty());
                    run(Op::WorktreeAdd {
                        path: path.trim().into(),
                        rev: rev.trim().into(),
                        new_branch: nb,
                    });
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        Dialog::Identity {
            name,
            email,
            signing_key,
            sign,
            ssh_key,
        } => {
            ui.heading(t!("Repository identity"));
            ui.label(t!("Saved in this repository only (git config --local)."));
            if !settings.profiles.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(t!("Use profile"));
                    egui::ComboBox::from_id_salt("identity-profile")
                        .selected_text(t!("Choose…"))
                        .show_ui(ui, |ui| {
                            for p in &settings.profiles {
                                if ui
                                    .selectable_label(
                                        false,
                                        format!("{} — {} <{}>", p.label, p.name, p.email),
                                    )
                                    .clicked()
                                {
                                    *name = p.name.clone();
                                    *email = p.email.clone();
                                    *signing_key = p.signing_key.clone();
                                    *sign = p.sign_commits;
                                    *ssh_key = p.ssh_key.clone();
                                }
                            }
                        });
                });
            }
            field(ui, &t!("Name"), name, &t!("Your Name"));
            field(ui, &t!("Email"), email, "you@example.com");
            field(
                ui,
                &t!("SSH key for remotes (optional)"),
                ssh_key,
                "~/.ssh/id_ed25519_work",
            );
            field(
                ui,
                &t!("Signing key (optional)"),
                signing_key,
                &t!("GPG key id or ~/.ssh/id_ed25519.pub"),
            );
            ui.checkbox(sign, t!("Sign commits"));
            let profile = Profile {
                label: String::new(),
                name: name.clone(),
                email: email.clone(),
                signing_key: signing_key.clone(),
                sign_commits: *sign,
                ssh_key: ssh_key.clone(),
            };
            let known = settings
                .profiles
                .iter()
                .any(|p| p.name == profile.name && p.email == profile.email);
            if !known && !name.trim().is_empty() && ui.small_button(t!("Save as profile")).clicked()
            {
                let mut p = profile.clone();
                p.label = email.split('@').nth(1).unwrap_or("profile").to_owned();
                settings.profiles.push(p);
            }
            let ok = !name.trim().is_empty() && email.contains('@');
            match buttons(ui, &t!("Apply"), ok, false, enter) {
                Some(true) => {
                    run(Op::SetIdentity(profile.identity()));
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        Dialog::Forge(d) => forge_ui::show_dialog(ui, d, tab, settings, cmds),
        Dialog::Flow(d) => crate::ui::flow_dialogs::show(ui, d, tab, cmds, enter),
    }
}

#[cfg(test)]
mod tests {
    use super::clone_dir_name;

    #[test]
    fn derives_clone_folder() {
        assert_eq!(clone_dir_name("https://github.com/a/ferrum.git"), "ferrum");
        assert_eq!(clone_dir_name("git@github.com:a/ferrum.git"), "ferrum");
        assert_eq!(clone_dir_name("https://gitlab.com/g/sub/repo/"), "repo");
    }
}
