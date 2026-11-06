//! Modal dialogs.

use std::path::PathBuf;

use eframe::egui::{self, RichText};
use ferrum_core::ops::ResetMode;
use ferrum_core::{BranchKind, Identity};

use crate::command::{Command, Commands, Op};
use crate::settings::{Profile, Settings};
use crate::tab::RepoTab;
use crate::ui::forge as forge_ui;
use crate::ui::sync_dialogs;
use crate::util::valid_ref_name;

pub enum Dialog {
    Confirm {
        title: String,
        text: String,
        op: Box<Op>,
        danger: bool,
    },
    CheckoutRemote {
        remote_branch: String,
        local: String,
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
    /// Fetch, pull, push, tracking and remotes (see `sync_dialogs`).
    Sync(sync_dialogs::SyncDialog),
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
    /// Branch, tag and stash dialogs (`ui::ref_dialogs`).
    Refs(super::ref_dialogs::RefDialog),
    Lfs(crate::ui::lfs::LfsDialog),
    Submodule(crate::ui::submodules::SubmoduleDialog),
    Worktree(crate::ui::worktrees::WorktreeDialog),
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
        super::ref_dialogs::new_branch(start, label, "")
    }

    pub fn create_tag(rev: &str, label: &str) -> Self {
        super::ref_dialogs::new_tag(rev, label)
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

    pub fn push(tab: &RepoTab, branch: &str) -> Self {
        Dialog::Sync(sync_dialogs::push(tab, branch))
    }

    pub fn pull(tab: &RepoTab) -> Self {
        Dialog::Sync(sync_dialogs::pull(tab))
    }

    pub fn stash() -> Self {
        super::ref_dialogs::stash()
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
        // Enter in an open drop-down (e.g. its filter field) is not "OK".
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift)
            && !egui::Popup::is_any_open(ctx);
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

pub(crate) fn field(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    hint: &str,
) -> egui::Response {
    ui.label(label);
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(f32::INFINITY),
    )
}

pub(crate) fn ref_name_warning(ui: &mut egui::Ui, name: &str) {
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
        Dialog::Sync(d) => sync_dialogs::show(ui, d, tab, settings, cmds, enter),
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
            if crate::ui::clone_repos::picker(ui, settings, url)
                && (name.is_empty() || *name == before)
            {
                *name = clone_dir_name(url);
            }
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
        Dialog::Refs(d) => super::ref_dialogs::show(ui, d, tab, settings, cmds, enter),
        Dialog::Lfs(d) => crate::ui::lfs::show_dialog(ui, d, tab, cmds),
        Dialog::Submodule(d) => crate::ui::submodules::show_dialog(ui, d, tab, cmds, enter),
        Dialog::Worktree(d) => crate::ui::worktrees::show_dialog(ui, d, tab, cmds, enter),
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
