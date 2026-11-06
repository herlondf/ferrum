//! Left sidebar: views, branches, remotes, tags, stashes, submodules, worktrees.

use std::collections::BTreeMap;

use eframe::egui::{self, RichText};
use ferrum_core::{BranchInfo, BranchKind};

use crate::command::{Command, Commands, Op};
use crate::tab::{RepoTab, View};
use crate::ui::dialogs::Dialog;
use crate::ui::forge as forge_ui;

pub fn show(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            views(ui, tab, cmds);
            ui.separator();
            local_branches(ui, tab, cmds);
            remotes(ui, tab, cmds);
            tags(ui, tab, cmds);
            stashes(ui, tab, cmds);
            submodules(ui, tab, cmds);
            worktrees(ui, tab, cmds);
        });
}

fn views(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    let changes = tab.status.total();
    let label = if changes > 0 {
        t!("📝 Local Changes ({})", changes)
    } else {
        t!("📝 Local Changes")
    };
    let is_changes = matches!(tab.view, View::Changes | View::Conflicts);
    if ui
        .add(egui::Button::selectable(is_changes, label).truncate())
        .clicked()
    {
        cmds.push(Command::SetView(View::Changes));
    }
    if ui
        .add(egui::Button::selectable(
            tab.view == View::History,
            t!("🕓 All Commits"),
        ))
        .clicked()
    {
        cmds.push(Command::SetView(View::History));
    }
    if let Some(h) = &tab.hosted {
        let label = format!("🔀 {}s", capitalize(h.kind.request_name()));
        if ui
            .add(egui::Button::selectable(
                tab.view == View::PullRequests,
                label,
            ))
            .clicked()
        {
            cmds.push(Command::SetView(View::PullRequests));
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

/// Groups branch names by their first folder (`feature/x` -> `feature`).
fn grouped<'a>(
    branches: impl Iterator<Item = &'a BranchInfo>,
    strip: usize,
) -> BTreeMap<Option<String>, Vec<&'a BranchInfo>> {
    let mut map: BTreeMap<Option<String>, Vec<&BranchInfo>> = BTreeMap::new();
    for b in branches {
        let name = &b.name[strip..];
        let folder = name.split_once('/').map(|(f, _)| f.to_owned());
        map.entry(folder).or_default().push(b);
    }
    map
}

fn local_branches(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    let header = egui::CollapsingHeader::new(RichText::new(t!("Branches")).strong())
        .id_salt("branches")
        .default_open(true);
    header.show(ui, |ui| {
        let groups = grouped(
            tab.branches.iter().filter(|b| b.kind == BranchKind::Local),
            0,
        );
        if groups.is_empty() {
            ui.weak(t!("No branches yet."));
        }
        for (folder, list) in groups {
            match folder {
                None => list.iter().for_each(|b| branch_row(ui, tab, b, 0, cmds)),
                Some(f) => {
                    egui::CollapsingHeader::new(format!("📁 {f}"))
                        .id_salt(("local-folder", &f))
                        .default_open(true)
                        .show(ui, |ui| {
                            for b in list {
                                branch_row(ui, tab, b, f.len() + 1, cmds);
                            }
                        });
                }
            }
        }
    });
}

fn branch_row(ui: &mut egui::Ui, tab: &RepoTab, b: &BranchInfo, strip: usize, cmds: &mut Commands) {
    let short = &b.name[strip..];
    let mut text = if b.is_head {
        format!("✔ {short}")
    } else {
        short.to_owned()
    };
    if let Some((ahead, behind)) = b.ahead_behind {
        if ahead > 0 {
            text.push_str(&format!("  ⬆{ahead}"));
        }
        if behind > 0 {
            text.push_str(&format!("  ⬇{behind}"));
        }
    }
    let rich = if b.is_head {
        RichText::new(text).strong()
    } else {
        RichText::new(text)
    };
    let selected =
        b.target.is_some() && tab.history.selected == b.target && tab.view == View::History;
    let resp = ui.add(egui::Button::selectable(selected, rich).truncate());
    let tracking = match (&b.upstream, b.ahead_behind) {
        (Some(u), Some((a, bh))) => t!("tracks {} ({} ahead, {} behind)", u, a, bh),
        (Some(u), None) => t!("tracks {}", u),
        _ => t!("no upstream branch"),
    };
    let resp = resp.on_hover_text(t!("{}\n{}\nDouble-click to check out", b.name, tracking));
    if resp.clicked()
        && let Some(t) = b.target
    {
        cmds.push(Command::SelectCommit(t));
    }
    if resp.double_clicked() && !b.is_head && !tab.busy() {
        cmds.run(Op::Checkout(b.name.clone()));
    }
    resp.context_menu(|ui| local_branch_menu(ui, tab, b, cmds));
}

fn local_branch_menu(ui: &mut egui::Ui, tab: &RepoTab, b: &BranchInfo, cmds: &mut Commands) {
    ui.set_min_width(220.0);
    let name = b.name.clone();
    let current = tab.head.clone();
    ui.add_enabled_ui(!tab.busy(), |ui| {
        if !b.is_head && ui.button(t!("Check out {}", name)).clicked() {
            cmds.run(Op::Checkout(name.clone()));
            ui.close();
        }
        if b.is_head && ui.button(t!("Pull")).clicked() {
            cmds.push(Command::Dialog(Dialog::Pull));
            ui.close();
        }
        if ui.button(t!("Push {}…", name)).clicked() {
            cmds.push(Command::Dialog(Dialog::push(tab, &name)));
            ui.close();
        }
        if let Some(cur) = current.as_ref().filter(|c| **c != name) {
            ui.separator();
            if ui.button(t!("Merge {} into {}…", name, cur)).clicked() {
                cmds.push(Command::Dialog(Dialog::merge(&name, &name)));
                ui.close();
            }
            if ui.button(t!("Rebase {} onto {}…", cur, name)).clicked() {
                #[rustfmt::skip]
                let text = t!("Rebase {} onto {}? The commits of {} will be rewritten.", cur, name, cur);
                cmds.push(Command::Dialog(Dialog::confirm(
                    &t!("Rebase"),
                    &text,
                    Op::Rebase { onto: name.clone() },
                    false,
                )));
                ui.close();
            }
        }
        ui.separator();
        if ui.button(t!("New branch from {}…", name)).clicked() {
            cmds.push(Command::Dialog(Dialog::create_branch(&name, &name)));
            ui.close();
        }
        if ui.button(t!("Rename…")).clicked() {
            cmds.push(Command::Dialog(Dialog::RenameBranch {
                old: name.clone(),
                new: name.clone(),
            }));
            ui.close();
        }
        if !b.is_head && ui.button(t!("Delete…")).clicked() {
            cmds.push(Command::Dialog(Dialog::delete_branch(b)));
            ui.close();
        }
        let remote_branches: Vec<&BranchInfo> = tab
            .branches
            .iter()
            .filter(|r| r.kind == BranchKind::Remote)
            .collect();
        if !remote_branches.is_empty() {
            ui.menu_button(t!("Track remote branch"), |ui| {
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        for r in remote_branches {
                            let checked = b.upstream.as_deref() == Some(&r.name);
                            if ui.selectable_label(checked, &r.name).clicked() {
                                cmds.run(Op::SetUpstream {
                                    branch: name.clone(),
                                    upstream: r.name.clone(),
                                });
                                ui.close();
                            }
                        }
                    });
            });
        }
    });
    if let Some(h) = &tab.hosted {
        ui.separator();
        if ui.button(t!("Create {}…", h.kind.request_name())).clicked() {
            cmds.push(Command::Dialog(Dialog::Forge(forge_ui::new_pull_request(
                tab, &name,
            ))));
            ui.close();
        }
    }
    ui.separator();
    if ui.button(t!("Copy name")).clicked() {
        cmds.push(Command::Copy(name));
        ui.close();
    }
}

fn remotes(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    let header = egui::CollapsingHeader::new(RichText::new(t!("Remotes")).strong())
        .id_salt("remotes")
        .default_open(true);
    let resp = header.show(ui, |ui| {
        if tab.remotes.is_empty() {
            ui.weak(t!("No remotes."));
        }
        for remote in &tab.remotes {
            let prefix = format!("{}/", remote.name);
            let list: Vec<&BranchInfo> =
                tab.branches.iter().filter(|b| b.kind == BranchKind::Remote && b.name.starts_with(&prefix)).collect();
            let h = egui::CollapsingHeader::new(format!("☁ {}", remote.name)).id_salt(("remote", &remote.name)).default_open(true);
            let r = h.show(ui, |ui| {
                for (folder, group) in grouped(list.into_iter(), prefix.len()) {
                    match folder {
                        None => group.iter().for_each(|b| remote_branch_row(ui, tab, b, prefix.len(), cmds)),
                        Some(f) => {
                            egui::CollapsingHeader::new(format!("📁 {f}")).id_salt(("remote-folder", &remote.name, &f)).show(ui, |ui| {
                                for b in group {
                                    remote_branch_row(ui, tab, b, prefix.len() + f.len() + 1, cmds);
                                }
                            });
                        }
                    }
                }
            });
            r.header_response.on_hover_text(&remote.url).context_menu(|ui| {
                ui.add_enabled_ui(!tab.busy(), |ui| {
                    if ui.button(t!("Fetch {}", remote.name)).clicked() {
                        cmds.run(Op::Fetch { remote: Some(remote.name.clone()) });
                        ui.close();
                    }
                    if ui.button(t!("Remove remote…")).clicked() {
                        cmds.push(Command::Dialog(Dialog::confirm(
                            &t!("Remove remote"),
                            &t!("Remove the remote {}? Its remote-tracking branches will be deleted locally.", remote.name),
                            Op::RemoveRemote(remote.name.clone()),
                            true,
                        )));
                        ui.close();
                    }
                });
                if ui.button(t!("Copy URL")).clicked() {
                    cmds.push(Command::Copy(remote.url.clone()));
                    ui.close();
                }
            });
        }
    });
    resp.header_response.context_menu(|ui| {
        if ui.button(t!("Add remote…")).clicked() {
            cmds.push(Command::Dialog(Dialog::AddRemote {
                name: String::new(),
                url: String::new(),
            }));
            ui.close();
        }
        if ui
            .add_enabled(!tab.busy(), egui::Button::new(t!("Fetch all")))
            .clicked()
        {
            cmds.run(Op::Fetch { remote: None });
            ui.close();
        }
    });
}

fn remote_branch_row(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    b: &BranchInfo,
    strip: usize,
    cmds: &mut Commands,
) {
    let short = &b.name[strip.min(b.name.len())..];
    let selected =
        b.target.is_some() && tab.history.selected == b.target && tab.view == View::History;
    let resp = ui.add(egui::Button::selectable(selected, short).truncate());
    let local = tab
        .branches
        .iter()
        .find(|l| l.kind == BranchKind::Local && l.upstream.as_deref() == Some(&b.name));
    let resp = resp.on_hover_text(t!("{}\nDouble-click to check out", b.name));
    if resp.clicked()
        && let Some(t) = b.target
    {
        cmds.push(Command::SelectCommit(t));
    }
    let checkout = |cmds: &mut Commands| match local {
        Some(l) => cmds.run(Op::Checkout(l.name.clone())),
        None => {
            let local_name = b.split_remote().map_or(b.name.as_str(), |(_, n)| n);
            cmds.push(Command::Dialog(Dialog::checkout_remote(
                &b.name, local_name,
            )));
        }
    };
    if resp.double_clicked() && !tab.busy() {
        checkout(cmds);
    }
    resp.context_menu(|ui| {
        ui.set_min_width(220.0);
        ui.add_enabled_ui(!tab.busy(), |ui| {
            let label = match local {
                Some(l) => t!("Check out {}", l.name),
                None => t!("Check out as local branch…"),
            };
            if ui.button(label).clicked() {
                checkout(cmds);
                ui.close();
            }
            if let Some(cur) = &tab.head {
                if ui.button(t!("Merge {} into {}…", b.name, cur)).clicked() {
                    cmds.push(Command::Dialog(Dialog::merge(&b.name, &b.name)));
                    ui.close();
                }
                if ui.button(t!("Rebase {} onto {}…", cur, b.name)).clicked() {
                    #[rustfmt::skip]
                    let text = t!("Rebase {} onto {}? The commits of {} will be rewritten.", cur, b.name, cur);
                    cmds.push(Command::Dialog(Dialog::confirm(
                        &t!("Rebase"),
                        &text,
                        Op::Rebase {
                            onto: b.name.clone(),
                        },
                        false,
                    )));
                    ui.close();
                }
            }
            if ui.button(t!("New branch from here…")).clicked() {
                cmds.push(Command::Dialog(Dialog::create_branch(&b.name, &b.name)));
                ui.close();
            }
            if let Some((remote, name)) = b.split_remote()
                && ui.button(t!("Delete from {}…", remote)).clicked()
            {
                #[rustfmt::skip]
                let text = t!("Delete the branch {} from {}? Other people using it will lose it.", name, remote);
                cmds.push(Command::Dialog(Dialog::confirm(
                    &t!("Delete remote branch"),
                    &text,
                    Op::DeleteRemoteRef {
                        remote: remote.to_owned(),
                        name: name.to_owned(),
                    },
                    true,
                )));
                ui.close();
            }
        });
        ui.separator();
        if ui.button(t!("Copy name")).clicked() {
            cmds.push(Command::Copy(b.name.clone()));
            ui.close();
        }
    });
}

fn tags(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    let title = t!("Tags ({})", tab.tags.len());
    egui::CollapsingHeader::new(RichText::new(title).strong())
        .id_salt("tags")
        .show(ui, |ui| {
            for t in &tab.tags {
                let resp =
                    ui.add(egui::Button::selectable(false, format!("🏷 {}", t.name)).truncate());
                if resp.clicked() {
                    cmds.push(Command::SelectCommit(t.target));
                }
                resp.context_menu(|ui| {
                    ui.add_enabled_ui(!tab.busy(), |ui| {
                        if ui.button(t!("Check out (detached)")).clicked() {
                            cmds.run(Op::CheckoutDetached(t.name.clone()));
                            ui.close();
                        }
                        for r in &tab.remotes {
                            if ui.button(t!("Push to {}", r.name)).clicked() {
                                cmds.run(Op::PushTag {
                                    remote: r.name.clone(),
                                    tag: t.name.clone(),
                                });
                                ui.close();
                            }
                        }
                        if ui.button(t!("Delete…")).clicked() {
                            cmds.push(Command::Dialog(Dialog::DeleteTag {
                                name: t.name.clone(),
                                remote: None,
                            }));
                            ui.close();
                        }
                    });
                    if ui.button(t!("Copy name")).clicked() {
                        cmds.push(Command::Copy(t.name.clone()));
                        ui.close();
                    }
                });
            }
        });
}

fn stashes(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    if tab.stashes.is_empty() {
        return;
    }
    let title = t!("Stashes ({})", tab.stashes.len());
    egui::CollapsingHeader::new(RichText::new(title).strong())
        .id_salt("stashes")
        .default_open(true)
        .show(ui, |ui| {
            for s in &tab.stashes {
                let selected = tab
                    .commit_view
                    .as_ref()
                    .is_some_and(|v| v.is_stash && v.details.info.id == s.id);
                let resp = ui.add(
                    egui::Button::selectable(selected, format!("📦 {}", s.message)).truncate(),
                );
                if resp.clicked() {
                    cmds.push(Command::ShowCommit(s.id));
                }
                resp.on_hover_text(s.refname()).context_menu(|ui| {
                    ui.add_enabled_ui(!tab.busy(), |ui| {
                        if ui.button(t!("Apply")).clicked() {
                            cmds.run(Op::StashApply {
                                stash: s.clone(),
                                pop: false,
                            });
                            ui.close();
                        }
                        if ui.button(t!("Pop (apply and delete)")).clicked() {
                            cmds.run(Op::StashApply {
                                stash: s.clone(),
                                pop: true,
                            });
                            ui.close();
                        }
                        if ui.button(t!("Drop…")).clicked() {
                            let (r, m) = (s.refname(), &s.message);
                            let text = t!("Delete {} ({})? This cannot be undone.", r, m);
                            cmds.push(Command::Dialog(Dialog::confirm(
                                &t!("Drop stash"),
                                &text,
                                Op::StashDrop(s.clone()),
                                true,
                            )));
                            ui.close();
                        }
                    });
                });
            }
        });
}

fn submodules(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    if tab.submodules.is_empty() {
        return;
    }
    let title = t!("Submodules ({})", tab.submodules.len());
    let resp = egui::CollapsingHeader::new(RichText::new(title).strong())
        .id_salt("submodules")
        .show(ui, |ui| {
            for s in &tab.submodules {
                let label = if s.initialized {
                    format!("▣ {}", s.path)
                } else {
                    t!("☐ {} (not initialized)", s.path)
                };
                let resp = ui.add(egui::Button::selectable(false, label).truncate());
                let resp = resp.on_hover_text(s.url.clone().unwrap_or_default());
                if resp.double_clicked() && s.initialized {
                    cmds.push(Command::OpenRepo(tab.path().join(&s.path)));
                }
                resp.context_menu(|ui| {
                    if s.initialized && ui.button(t!("Open in new tab")).clicked() {
                        cmds.push(Command::OpenRepo(tab.path().join(&s.path)));
                        ui.close();
                    }
                    if ui
                        .add_enabled(!tab.busy(), egui::Button::new(t!("Update all submodules")))
                        .clicked()
                    {
                        cmds.run(Op::SubmoduleUpdate);
                        ui.close();
                    }
                });
            }
        });
    resp.header_response.context_menu(|ui| {
        if ui
            .add_enabled(!tab.busy(), egui::Button::new(t!("Update all submodules")))
            .clicked()
        {
            cmds.run(Op::SubmoduleUpdate);
            ui.close();
        }
    });
}

fn worktrees(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    let title = t!("Worktrees ({})", tab.worktrees.len());
    let resp = egui::CollapsingHeader::new(RichText::new(title).strong()).id_salt("worktrees").show(ui, |ui| {
        for w in &tab.worktrees {
            let name = std::path::Path::new(&w.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(w.path.clone());
            let branch = w.branch.clone().unwrap_or_else(|| t!("detached"));
            let current = std::path::Path::new(&w.path) == tab.path() || tab.path().starts_with(&w.path) && w.is_main;
            let label = format!("{}{name} ({branch})", if current { "✔ " } else { "" });
            let resp = ui.add(egui::Button::selectable(false, label).truncate()).on_hover_text(&w.path);
            if resp.double_clicked() {
                cmds.push(Command::OpenRepo(w.path.clone().into()));
            }
            resp.context_menu(|ui| {
                if ui.button(t!("Open in new tab")).clicked() {
                    cmds.push(Command::OpenRepo(w.path.clone().into()));
                    ui.close();
                }
                if !w.is_main && ui.add_enabled(!tab.busy(), egui::Button::new(t!("Remove…"))).clicked() {
                    cmds.push(Command::Dialog(Dialog::confirm(
                        &t!("Remove worktree"),
                        &t!("Remove the worktree at {}? Uncommitted changes in it will be lost.", w.path),
                        Op::WorktreeRemove { path: w.path.clone(), force: true },
                        true,
                    )));
                    ui.close();
                }
                if ui.button(t!("Copy path")).clicked() {
                    cmds.push(Command::Copy(w.path.clone()));
                    ui.close();
                }
            });
        }
        if ui.small_button(t!("+ Add worktree…")).clicked() {
            cmds.push(Command::Dialog(Dialog::worktree_add(tab)));
        }
    });
    resp.header_response
        .on_hover_text(t!("Extra working folders that share this repository"));
}
