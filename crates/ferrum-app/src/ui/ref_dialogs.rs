//! Branch, tag and stash dialogs: quick checkout, new / rename / delete
//! branches, new tag, tag details and the stash options.

use eframe::egui::{self, Key, Modifiers, RichText};
use ferrum_core::refs::TagDetails;
use ferrum_core::{BranchKind, RefKind, RefLabel, StashInfo};
use serde::{Deserialize, Serialize};

use crate::command::{Command, Commands, Op};
use crate::ref_ops::RefOp;
use crate::settings::Settings;
use crate::tab::RepoTab;
use crate::ui::dialogs::{Dialog, buttons, field, ref_name_warning};
use crate::ui::sidebar_model::folder_paths;
use crate::util::valid_ref_name;

/// Remembered choices of these dialogs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RefPrefs {
    /// Folder of the last branch created, e.g. `feature/`.
    pub last_branch_prefix: String,
    /// "Restore the staged state" when applying a stash.
    pub stash_restore_index: bool,
}

/// Checks a branch name with `git check-ref-format --branch`, only when it changes.
#[derive(Default)]
pub struct NameCheck {
    name: String,
    valid: bool,
}

impl NameCheck {
    fn valid(&mut self, tab: Option<&RepoTab>, name: &str) -> bool {
        let name = name.trim();
        if !valid_ref_name(name) {
            return false;
        }
        if self.name != name {
            self.name = name.to_owned();
            self.valid = tab.is_none_or(|t| {
                let git = ferrum_core::cli::Git::new(t.path());
                ferrum_core::refs::check_branch_name(&git, name)
            });
        }
        self.valid
    }
}

pub struct DeleteItem {
    pub name: String,
    /// Remote and branch name there.
    pub upstream: Option<(String, String)>,
    /// Another worktree that has the branch checked out.
    pub worktree: Option<String>,
}

pub enum RefDialog {
    QuickCheckout {
        query: String,
        cursor: usize,
        init: bool,
    },
    NewBranch {
        start: String,
        start_label: String,
        name: String,
        checkout: bool,
        check: NameCheck,
        init: bool,
    },
    Rename {
        old: String,
        new: String,
        remote: Option<(String, String)>,
        rename_remote: bool,
        check: NameCheck,
        init: bool,
    },
    DeleteBranches {
        items: Vec<DeleteItem>,
        force: bool,
        delete_remote: bool,
        remove_worktrees: bool,
    },
    NewTag {
        rev: String,
        label: String,
        name: String,
        message: String,
        sign: bool,
        push_to: Option<String>,
        init: bool,
    },
    TagDetails(Box<Result<TagDetails, String>>),
    Stash {
        message: String,
        untracked: bool,
        staged_only: bool,
        snapshot: bool,
        /// Stash only the checked files.
        pick: bool,
        files: Vec<(String, bool)>,
        init: bool,
    },
    ApplyStash {
        stash: StashInfo,
        pop: bool,
        index: Option<bool>,
    },
    RenameStash {
        stash: StashInfo,
        message: String,
        init: bool,
    },
    DropStashes(Vec<(StashInfo, bool)>),
}

// ----- Constructors -----

pub fn quick_checkout() -> Dialog {
    Dialog::Refs(RefDialog::QuickCheckout {
        query: String::new(),
        cursor: 0,
        init: true,
    })
}

/// New branch dialog; an empty `name` is filled with the last used prefix.
pub fn new_branch(start: &str, label: &str, name: &str) -> Dialog {
    Dialog::Refs(RefDialog::NewBranch {
        start: start.into(),
        start_label: label.into(),
        name: name.into(),
        checkout: true,
        check: NameCheck::default(),
        init: true,
    })
}

pub fn rename_branch(tab: &RepoTab, name: &str) -> Dialog {
    let remote = tab
        .branches
        .iter()
        .find(|b| b.kind == BranchKind::Local && b.name == name)
        .and_then(|b| b.upstream.as_deref())
        .and_then(|u| u.split_once('/'))
        .map(|(r, n)| (r.to_owned(), n.to_owned()));
    Dialog::Refs(RefDialog::Rename {
        old: name.into(),
        new: name.into(),
        remote,
        rename_remote: false,
        check: NameCheck::default(),
        init: true,
    })
}

pub fn delete_branches(tab: &RepoTab, names: &[String]) -> Dialog {
    let items = names
        .iter()
        .map(|n| {
            let b = tab
                .branches
                .iter()
                .find(|b| b.kind == BranchKind::Local && &b.name == n);
            DeleteItem {
                name: n.clone(),
                upstream: b
                    .and_then(|b| b.upstream.as_deref())
                    .and_then(|u| u.split_once('/'))
                    .map(|(r, n)| (r.to_owned(), n.to_owned())),
                worktree: tab
                    .worktrees
                    .iter()
                    .find(|w| w.branch.as_deref() == Some(n.as_str()) && !w.is_main)
                    .map(|w| w.path.clone()),
            }
        })
        .collect();
    Dialog::Refs(RefDialog::DeleteBranches {
        items,
        force: false,
        delete_remote: false,
        remove_worktrees: false,
    })
}

pub fn new_tag(rev: &str, label: &str) -> Dialog {
    Dialog::Refs(RefDialog::NewTag {
        rev: rev.into(),
        label: label.into(),
        name: String::new(),
        message: String::new(),
        sign: false,
        push_to: None,
        init: true,
    })
}

pub fn tag_details(tab: &RepoTab, name: &str) -> Dialog {
    let d = ferrum_core::refs::tag_details(&tab.repo, name).map_err(|e| e.to_string());
    Dialog::Refs(RefDialog::TagDetails(Box::new(d)))
}

pub fn stash() -> Dialog {
    Dialog::Refs(RefDialog::Stash {
        message: String::new(),
        untracked: true,
        staged_only: false,
        snapshot: false,
        pick: false,
        files: Vec::new(),
        init: true,
    })
}

/// Stash everything, new files included, without asking.
pub fn quick_stash() -> Op {
    Op::StashPush {
        message: String::new(),
        untracked: true,
        staged_only: false,
    }
}

pub fn apply_stash(stash: &StashInfo, pop: bool) -> Dialog {
    Dialog::Refs(RefDialog::ApplyStash {
        stash: stash.clone(),
        pop,
        index: None,
    })
}

pub fn rename_stash(stash: &StashInfo) -> Dialog {
    Dialog::Refs(RefDialog::RenameStash {
        stash: stash.clone(),
        message: stash.message.clone(),
        init: true,
    })
}

pub fn drop_stashes(tab: &RepoTab) -> Dialog {
    Dialog::Refs(RefDialog::DropStashes(
        tab.stashes.iter().map(|s| (s.clone(), false)).collect(),
    ))
}

// ----- Shortcuts and menus -----

/// Ctrl+B quick checkout, Ctrl+Shift+B new branch, Ctrl+Shift+G new tag,
/// F2 renames and Delete deletes the ref under the sidebar cursor.
pub fn shortcuts(ctx: &egui::Context, tab: &RepoTab, cmds: &mut Commands) {
    use crate::keymap::{Action, pressed};
    let head = tab.head.clone().unwrap_or_else(|| "HEAD".into());
    if pressed(ctx, Action::NewBranch) {
        cmds.push(Command::Dialog(new_branch(&head, &head, "")));
    }
    if pressed(ctx, Action::QuickCheckout) {
        cmds.push(Command::Dialog(quick_checkout()));
    }
    if pressed(ctx, Action::NewTag) {
        cmds.push(Command::Dialog(crate::ui::shortcut_actions::new_tag(tab)));
    }
    let typing = ctx.memory(|m| m.focused().is_some());
    if !typing
        && pressed(ctx, Action::DeleteRef)
        && let Some(d) = crate::ui::shortcut_actions::delete_ref(tab)
    {
        cmds.push(Command::Dialog(d));
    }
    if !typing
        && let Some(name) = tab
            .sidebar
            .cursor
            .as_deref()
            .and_then(|c| c.strip_prefix("ref:refs/heads/"))
        && tab
            .branches
            .iter()
            .any(|b| b.kind == BranchKind::Local && b.name == name)
        && crate::keymap::pressed(ctx, crate::keymap::Action::Rename)
    {
        cmds.push(Command::Dialog(rename_branch(tab, name)));
    }
}

/// Entries of a stash's menu (sidebar and toolbar).
pub fn stash_menu(ui: &mut egui::Ui, tab: &RepoTab, s: &StashInfo, cmds: &mut Commands) {
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
        if ui.button(t!("Apply with options…")).clicked() {
            cmds.push(Command::Dialog(apply_stash(s, false)));
            ui.close();
        }
        if ui.button(t!("Compare with working copy")).clicked() {
            cmds.push(crate::ui::details::compare_with_workdir(
                s.id,
                Some(s.refname()),
            ));
            ui.close();
        }
        ui.separator();
        if ui.button(t!("Rename…")).clicked() {
            cmds.push(Command::Dialog(rename_stash(s)));
            ui.close();
        }
        if ui.button(t!("Save as patch…")).clicked() {
            let file = format!("stash-{}.patch", s.index);
            if let Some(path) = rfd::FileDialog::new().set_file_name(file).save_file() {
                cmds.run(Op::Refs(RefOp::StashSavePatch {
                    stash: s.clone(),
                    path,
                }));
            }
            ui.close();
        }
        crate::ui::patches::stash_menu(ui, s, cmds);
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
    ui.separator();
    if ui.button(t!("Show changes")).clicked() {
        cmds.push(Command::ShowCommit(s.id));
        ui.close();
    }
}

/// Entries for stashing without the dialog, and for several stashes.
pub fn stash_actions(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    let changes = !tab.status.is_empty();
    ui.add_enabled_ui(!tab.busy(), |ui| {
        if ui
            .add_enabled(changes, egui::Button::new(t!("Stash changes…")))
            .clicked()
        {
            cmds.push(Command::Dialog(stash()));
            ui.close();
        }
        if ui
            .add_enabled(changes, egui::Button::new(t!("Quick stash")))
            .on_hover_text(t!("Stash all changes, new files included, without asking"))
            .clicked()
        {
            cmds.run(quick_stash());
            ui.close();
        }
        if ui
            .add_enabled(changes, egui::Button::new(t!("Save a snapshot")))
            .on_hover_text(t!(
                "Save the changes as a stash and keep them in the working copy"
            ))
            .clicked()
        {
            cmds.run(Op::Refs(RefOp::StashSnapshot(String::new())));
            ui.close();
        }
        if tab.stashes.len() > 1 && ui.button(t!("Drop several stashes…")).clicked() {
            cmds.push(Command::Dialog(drop_stashes(tab)));
            ui.close();
        }
    });
}

/// The toolbar's stash dropdown: stash actions, then every stash.
pub fn stashes_dropdown(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    ui.set_min_width(240.0);
    stash_actions(ui, tab, cmds);
    ui.separator();
    if tab.stashes.is_empty() {
        ui.weak(t!("No stashes."));
        return;
    }
    egui::ScrollArea::vertical()
        .max_height(360.0)
        .show(ui, |ui| {
            for s in &tab.stashes {
                ui.menu_button(format!("📦 {}", s.message), |ui| {
                    stash_menu(ui, tab, s, cmds)
                });
            }
        });
}

/// Extra entries of the commit menu for its branch and tag labels.
pub fn label_menu(ui: &mut egui::Ui, tab: &RepoTab, labels: &[RefLabel], cmds: &mut Commands) {
    let names: Vec<&RefLabel> = labels.iter().filter(|l| l.kind != RefKind::Tag).collect();
    match names.as_slice() {
        [] => {}
        [one] => {
            if ui.button(t!("Copy branch name")).clicked() {
                cmds.push(Command::Copy(one.name.clone()));
                ui.close();
            }
        }
        many => {
            ui.menu_button(t!("Copy branch name"), |ui| {
                for l in many {
                    if ui.button(&l.name).clicked() {
                        cmds.push(Command::Copy(l.name.clone()));
                        ui.close();
                    }
                }
            });
        }
    }
    for l in labels.iter().filter(|l| l.kind == RefKind::Tag) {
        if ui.button(t!("Tag {} details…", l.name)).clicked() {
            cmds.push(Command::Dialog(tag_details(tab, &l.name)));
            ui.close();
        }
    }
}

// ----- Drawing -----

/// Draws the dialog. Returns `false` when it should close.
pub fn show(
    ui: &mut egui::Ui,
    d: &mut RefDialog,
    tab: Option<&RepoTab>,
    settings: &mut Settings,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    match d {
        RefDialog::QuickCheckout {
            query,
            cursor,
            init,
        } => quick_checkout_ui(ui, query, cursor, init, tab, cmds, enter),
        RefDialog::NewBranch {
            start,
            start_label,
            name,
            checkout,
            check,
            init,
        } => {
            ui.heading(t!("New branch"));
            ui.label(t!("Starting at {}", start_label));
            if *init && name.is_empty() {
                name.clone_from(&settings.refs.last_branch_prefix);
            }
            let ok = branch_name_field(ui, tab, settings, name, check, None, init);
            ui.checkbox(checkout, t!("Check out after creating"));
            match buttons(ui, &t!("Create branch"), ok, false, enter) {
                Some(true) => {
                    let name = name.trim().to_owned();
                    settings.refs.last_branch_prefix = name
                        .rsplit_once('/')
                        .map(|(p, _)| format!("{p}/"))
                        .unwrap_or_default();
                    cmds.run(Op::CreateBranch {
                        name,
                        start: start.clone(),
                        checkout: *checkout,
                    });
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        RefDialog::Rename {
            old,
            new,
            remote,
            rename_remote,
            check,
            init,
        } => {
            ui.heading(t!("Rename {}", old));
            let mut ok = branch_name_field(ui, tab, settings, new, check, Some(old), init);
            ok &= new.trim() != old;
            if let Some((r, n)) = remote {
                ui.checkbox(rename_remote, t!("Also rename {} on {}", n, r))
                    .on_hover_text(t!(
                        "Pushes the new name, deletes the old one and tracks the new one"
                    ));
            }
            match buttons(ui, &t!("Rename"), ok, false, enter) {
                Some(true) => {
                    let new = new.trim().to_owned();
                    match remote.as_ref().filter(|_| *rename_remote) {
                        Some(r) => cmds.run(Op::Refs(RefOp::RenameBranch {
                            old: old.clone(),
                            new,
                            remote: Some(r.clone()),
                        })),
                        // Without the remote it stays a plain rename, which can be undone.
                        None => cmds.run(Op::RenameBranch {
                            old: old.clone(),
                            new,
                        }),
                    }
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        RefDialog::DeleteBranches {
            items,
            force,
            delete_remote,
            remove_worktrees,
        } => delete_ui(ui, items, force, delete_remote, remove_worktrees, cmds),
        RefDialog::NewTag {
            rev,
            label,
            name,
            message,
            sign,
            push_to,
            init,
        } => {
            ui.heading(t!("New tag"));
            ui.label(t!("At {}", label));
            let resp = field(ui, &t!("Name"), name, "v1.0.0");
            if std::mem::take(init) {
                resp.request_focus();
            }
            ref_name_warning(ui, name);
            let exists = tab.is_some_and(|t| t.tags.iter().any(|x| x.name == name.trim()));
            if exists {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    t!("A tag with this name already exists."),
                );
            }
            ui.label(t!("Message (optional — makes an annotated tag)"));
            ui.add(
                egui::TextEdit::multiline(message)
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );
            let key = tab.and_then(|t| t.identity.signing_key.clone());
            if let Some(key) = key {
                ui.checkbox(sign, t!("Sign the tag (-s)"))
                    .on_hover_text(t!("With the signing key {}", key));
            }
            if let Some(t) = tab
                && !t.remotes.is_empty()
            {
                remote_choice(ui, t, push_to, &t!("Push to"), "tag-remote");
            }
            let ok = valid_ref_name(name) && !exists;
            match buttons(ui, &t!("Create tag"), ok, false, false) {
                Some(true) => {
                    let name = name.trim().to_owned();
                    let signed = *sign && tab.is_some_and(|t| t.identity.signing_key.is_some());
                    if signed {
                        cmds.run(Op::Refs(RefOp::CreateTag {
                            name: name.clone(),
                            rev: rev.clone(),
                            message: message.clone(),
                            sign: true,
                        }));
                    } else {
                        let msg = Some(message.clone()).filter(|m| !m.trim().is_empty());
                        cmds.run(Op::CreateTag {
                            name: name.clone(),
                            rev: rev.clone(),
                            message: msg,
                        });
                    }
                    if let Some(remote) = push_to {
                        cmds.run(Op::PushTag {
                            remote: remote.clone(),
                            tag: name,
                        });
                    }
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        RefDialog::TagDetails(details) => tag_details_ui(ui, details, tab, cmds),
        RefDialog::Stash {
            message,
            untracked,
            staged_only,
            snapshot,
            pick,
            files,
            init,
        } => {
            ui.heading(t!("Stash changes"));
            let resp = field(
                ui,
                &t!("Message (optional)"),
                message,
                &t!("work in progress"),
            );
            if std::mem::take(init) {
                resp.request_focus();
                if let Some(t) = tab {
                    *files = stash_files(t);
                }
            }
            ui.checkbox(
                snapshot,
                t!("Keep the changes in the working copy (snapshot)"),
            );
            ui.add_enabled_ui(!*snapshot, |ui| {
                ui.checkbox(untracked, t!("Include new (untracked) files"));
                ui.add_enabled_ui(!*pick, |ui| {
                    ui.checkbox(staged_only, t!("Only staged changes"));
                });
                ui.add_enabled_ui(!*staged_only, |ui| {
                    ui.checkbox(pick, t!("Only some files"));
                });
            });
            if *snapshot {
                ui.weak(t!("New (untracked) files are not saved in a snapshot."));
            }
            let picking = *pick && !*snapshot && !*staged_only;
            if picking {
                file_picker(ui, files);
            }
            let chosen: Vec<String> = files
                .iter()
                .filter(|(_, on)| *on)
                .map(|(p, _)| p.clone())
                .collect();
            let ok = !picking || !chosen.is_empty();
            match buttons(ui, &t!("Stash"), ok, false, enter) {
                Some(true) => {
                    let op = if *snapshot {
                        Op::Refs(RefOp::StashSnapshot(message.clone()))
                    } else if picking {
                        Op::Refs(RefOp::StashPaths {
                            message: message.clone(),
                            untracked: *untracked,
                            paths: chosen,
                        })
                    } else {
                        Op::StashPush {
                            message: message.clone(),
                            untracked: *untracked && !*staged_only,
                            staged_only: *staged_only,
                        }
                    };
                    cmds.run(op);
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        RefDialog::ApplyStash { stash, pop, index } => {
            ui.heading(t!("Apply {}", stash.refname()));
            ui.label(&stash.message);
            let index = index.get_or_insert(settings.refs.stash_restore_index);
            ui.checkbox(index, t!("Restore the staged state (--index)"))
                .on_hover_text(t!("Files that were staged when stashing are staged again"));
            ui.checkbox(pop, t!("Delete the stash after applying it (pop)"));
            match buttons(ui, &t!("Apply"), true, false, enter) {
                Some(true) => {
                    settings.refs.stash_restore_index = *index;
                    let op = if *index {
                        Op::Refs(RefOp::StashApply {
                            stash: stash.clone(),
                            pop: *pop,
                            index: true,
                        })
                    } else {
                        Op::StashApply {
                            stash: stash.clone(),
                            pop: *pop,
                        }
                    };
                    cmds.run(op);
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        RefDialog::RenameStash {
            stash,
            message,
            init,
        } => {
            ui.heading(t!("Rename {}", stash.refname()));
            let resp = field(ui, &t!("Message"), message, "");
            if std::mem::take(init) {
                resp.request_focus();
            }
            ui.weak(t!("The stash moves to the top of the list."));
            let ok = !message.trim().is_empty() && message.trim() != stash.message;
            match buttons(ui, &t!("Rename"), ok, false, enter) {
                Some(true) => {
                    cmds.run(Op::Refs(RefOp::StashRename {
                        stash: stash.clone(),
                        message: message.trim().to_owned(),
                    }));
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
        RefDialog::DropStashes(list) => {
            ui.heading(t!("Drop stashes"));
            ui.horizontal(|ui| {
                if ui.small_button(t!("All")).clicked() {
                    list.iter_mut().for_each(|(_, on)| *on = true);
                }
                if ui.small_button(t!("None")).clicked() {
                    list.iter_mut().for_each(|(_, on)| *on = false);
                }
            });
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    for (s, on) in list.iter_mut() {
                        ui.checkbox(on, format!("{}  {}", s.refname(), s.message));
                    }
                });
            let chosen: Vec<StashInfo> = list
                .iter()
                .filter(|(_, on)| *on)
                .map(|(s, _)| s.clone())
                .collect();
            if !chosen.is_empty() {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    t!("Dropped stashes cannot be restored."),
                );
            }
            let label = t!("Drop {} stashes", chosen.len());
            match buttons(ui, &label, !chosen.is_empty(), true, false) {
                Some(true) => {
                    cmds.run(Op::Refs(RefOp::StashDrop(chosen)));
                    false
                }
                Some(false) => false,
                None => true,
            }
        }
    }
}

/// A branch name field with folder suggestions and validation. `old` is the
/// branch being renamed. Returns whether the name can be used.
fn branch_name_field(
    ui: &mut egui::Ui,
    tab: Option<&RepoTab>,
    settings: &Settings,
    name: &mut String,
    check: &mut NameCheck,
    old: Option<&str>,
    init: &mut bool,
) -> bool {
    let folders: Vec<String> = tab.map_or_else(Vec::new, |t| {
        folder_paths(
            t.branches
                .iter()
                .filter(|b| b.kind == BranchKind::Local)
                .map(|b| b.name.as_str()),
        )
        .into_iter()
        .collect()
    });
    let typed = name.trim().to_lowercase();
    let current = name.rsplit_once('/').map_or("", |(p, _)| p);
    let suggestions: Vec<&String> = folders
        .iter()
        .filter(|f| f.as_str() != current && f.to_lowercase().starts_with(&typed))
        .take(8)
        .collect();
    let edit_id = ui.make_persistent_id("branch-name");
    // Tab completes the first suggestion.
    let has_focus = ui.memory(|m| m.has_focus(edit_id));
    let mut complete = None;
    if has_focus
        && !suggestions.is_empty()
        && !typed.is_empty()
        && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab))
    {
        complete = Some(suggestions[0].clone());
    }
    ui.label(if old.is_some() {
        t!("New name")
    } else {
        t!("Name")
    });
    let resp = ui.add(
        egui::TextEdit::singleline(name)
            .id(edit_id)
            .hint_text("feature/my-change")
            .desired_width(f32::INFINITY),
    );
    if *init {
        *init = false;
        resp.request_focus();
        move_cursor_to_end(ui.ctx(), edit_id, name);
    }
    if !suggestions.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.weak(t!("Folders:"));
            for f in &suggestions {
                if ui.small_button(format!("📁 {f}/")).clicked() {
                    complete = Some((*f).clone());
                }
            }
        });
    }
    if let Some(f) = complete {
        let leaf = name.rsplit_once('/').map_or("", |(_, l)| l).to_owned();
        // A typed partial folder name is replaced, a leaf after a slash kept.
        *name = if name.contains('/') {
            format!("{f}/{leaf}")
        } else {
            format!("{f}/")
        };
        resp.request_focus();
        move_cursor_to_end(ui.ctx(), edit_id, name);
    }
    // Spaces become the replacement chosen in Settings > Git.
    if name.contains(' ') {
        *name = settings.prefs.branch_name(name);
    }
    let trimmed = name.trim();
    let exists = old != Some(trimmed)
        && tab.is_some_and(|t| {
            t.branches
                .iter()
                .any(|b| b.kind == BranchKind::Local && b.name == trimmed)
        });
    let valid = check.valid(tab, name);
    if exists {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            t!("A branch with this name already exists."),
        );
    } else if !trimmed.is_empty() && valid_ref_name(trimmed) && !valid {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            t!("Git does not accept this branch name."),
        );
    } else {
        ref_name_warning(ui, name);
    }
    valid && !exists
}

fn move_cursor_to_end(ctx: &egui::Context, id: egui::Id, text: &str) {
    if let Some(mut state) = egui::text_edit::TextEditState::load(ctx, id) {
        let end = egui::text::CCursor::new(text.chars().count());
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(end)));
        state.store(ctx, id);
    }
}

/// A "Push to [remote]" (or similar) checkbox with a remote picker.
fn remote_choice(
    ui: &mut egui::Ui,
    t: &RepoTab,
    choice: &mut Option<String>,
    label: &str,
    id: &str,
) {
    let mut on = choice.is_some();
    ui.horizontal(|ui| {
        ui.checkbox(&mut on, label);
        let mut remote = choice
            .clone()
            .or_else(|| t.default_remote())
            .unwrap_or_default();
        egui::ComboBox::from_id_salt(id)
            .selected_text(&remote)
            .show_ui(ui, |ui| {
                for r in &t.remotes {
                    ui.selectable_value(&mut remote, r.name.clone(), &r.name);
                }
            });
        *choice = on.then_some(remote);
    });
}

/// What the quick checkout list offers.
struct Target {
    label: String,
    op: Op,
}

fn checkout_targets(tab: &RepoTab, query: &str) -> Vec<Target> {
    let locals: Vec<&str> = tab
        .branches
        .iter()
        .filter(|b| b.kind == BranchKind::Local)
        .map(|b| b.name.as_str())
        .collect();
    let tracked: Vec<&str> = tab
        .branches
        .iter()
        .filter_map(|b| b.upstream.as_deref())
        .collect();
    let mut all = Vec::new();
    for b in &tab.branches {
        match b.kind {
            BranchKind::Local if !b.is_head => all.push(Target {
                label: b.name.clone(),
                op: Op::Checkout(b.name.clone()),
            }),
            BranchKind::Remote => {
                let Some((_, short)) = b.split_remote() else {
                    continue;
                };
                // Remote branches that have a local branch are listed as that one.
                if short == "HEAD" || locals.contains(&short) || tracked.contains(&b.name.as_str())
                {
                    continue;
                }
                all.push(Target {
                    label: b.name.clone(),
                    op: Op::CheckoutRemote {
                        remote_branch: b.name.clone(),
                        local: short.to_owned(),
                    },
                });
            }
            _ => {}
        }
    }
    let q = query.trim().to_lowercase();
    let mut scored: Vec<(i32, Target)> = all
        .into_iter()
        .filter_map(|t| crate::ui::palette::fuzzy_score(&t.label, &q).map(|s| (s, t)))
        .collect();
    scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    scored.into_iter().map(|(_, t)| t).collect()
}

fn quick_checkout_ui(
    ui: &mut egui::Ui,
    query: &mut String,
    cursor: &mut usize,
    init: &mut bool,
    tab: Option<&RepoTab>,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    ui.heading(t!("Check out branch"));
    let Some(tab) = tab else { return false };
    let (up, down) = ui.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
        )
    });
    let resp = ui.add(
        egui::TextEdit::singleline(query)
            .hint_text(t!("Search branches"))
            .desired_width(f32::INFINITY),
    );
    if std::mem::take(init) {
        resp.request_focus();
    }
    if resp.changed() {
        *cursor = 0;
    }
    let targets = checkout_targets(tab, query);
    if up {
        *cursor = cursor.saturating_sub(1);
    }
    if down && *cursor + 1 < targets.len() {
        *cursor += 1;
    }
    *cursor = (*cursor).min(targets.len().saturating_sub(1));
    let mut chosen = None;
    egui::ScrollArea::vertical()
        .max_height(320.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                if targets.is_empty() {
                    ui.weak(t!("No matches."));
                }
                for (i, t) in targets.iter().enumerate() {
                    let icon = if matches!(t.op, Op::CheckoutRemote { .. }) {
                        "☁"
                    } else {
                        "🌿"
                    };
                    let r = ui.selectable_label(i == *cursor, format!("{icon} {}", t.label));
                    if i == *cursor && (up || down) {
                        r.scroll_to_me(None);
                    }
                    if r.clicked() {
                        chosen = Some(i);
                    }
                }
            });
        });
    if enter && !targets.is_empty() {
        chosen = Some(*cursor);
    }
    if targets
        .iter()
        .any(|t| matches!(t.op, Op::CheckoutRemote { .. }))
    {
        ui.weak(t!("☁ creates a local branch that tracks the remote one."));
    }
    if let Some(i) = chosen {
        if !tab.busy() {
            cmds.run(targets[i].op.clone());
        }
        return false;
    }
    ui.add_space(4.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        !ui.button(t!("Cancel")).clicked()
    })
    .inner
}

fn delete_ui(
    ui: &mut egui::Ui,
    items: &[DeleteItem],
    force: &mut bool,
    delete_remote: &mut bool,
    remove_worktrees: &mut bool,
    cmds: &mut Commands,
) -> bool {
    if let [one] = items {
        ui.heading(t!("Delete {}", one.name));
    } else {
        ui.heading(t!("Delete {} branches", items.len()));
    }
    egui::ScrollArea::vertical()
        .max_height(220.0)
        .show(ui, |ui| {
            for it in items {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&it.name).strong());
                    if let Some((r, n)) = &it.upstream {
                        ui.weak(format!("☁ {r}/{n}"));
                    }
                    if let Some(w) = &it.worktree {
                        ui.weak(t!("checked out in {}", w));
                    }
                });
            }
        });
    ui.add_space(4.0);
    ui.checkbox(force, t!("Delete even if it is not merged"))
        .on_hover_text(t!(
            "Commits only on this branch may be lost (you can still undo right after)"
        ));
    let remotes = items.iter().filter(|i| i.upstream.is_some()).count();
    if remotes > 0 {
        ui.checkbox(
            delete_remote,
            t!("Also delete their remote branches ({})", remotes),
        );
    }
    let worktrees = items.iter().filter(|i| i.worktree.is_some()).count();
    if worktrees > 0 {
        ui.checkbox(
            remove_worktrees,
            t!(
                "Also remove the worktrees that have them checked out ({})",
                worktrees
            ),
        );
        if !*remove_worktrees {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                t!("A branch checked out in another worktree cannot be deleted."),
            );
        }
    }
    match buttons(ui, &t!("Delete"), true, true, false) {
        Some(true) => {
            let mut ops = Vec::new();
            if *remove_worktrees {
                ops.extend(items.iter().filter_map(|i| {
                    i.worktree.as_ref().map(|w| Op::WorktreeRemove {
                        path: w.clone(),
                        force: false,
                    })
                }));
            }
            ops.extend(items.iter().map(|i| Op::DeleteBranch {
                name: i.name.clone(),
                force: *force,
            }));
            if *delete_remote {
                ops.extend(items.iter().filter_map(|i| {
                    i.upstream.as_ref().map(|(r, n)| Op::DeleteRemoteRef {
                        remote: r.clone(),
                        name: n.clone(),
                    })
                }));
            }
            if items.len() == 1 && !*remove_worktrees {
                // Separate jobs, so the branch deletion can be undone.
                ops.into_iter().for_each(|op| cmds.run(op));
            } else {
                let label = t!("Delete {} branches", items.len());
                cmds.run(Op::Sequence(label, ops));
            }
            false
        }
        Some(false) => false,
        None => true,
    }
}

fn tag_details_ui(
    ui: &mut egui::Ui,
    details: &Result<TagDetails, String>,
    tab: Option<&RepoTab>,
    cmds: &mut Commands,
) -> bool {
    let d = match details {
        Ok(d) => d,
        Err(e) => {
            ui.heading(t!("Tag details"));
            ui.colored_label(ui.visuals().error_fg_color, e);
            return buttons(ui, &t!("Close"), true, false, false).is_none();
        }
    };
    ui.heading(format!("🏷 {}", d.name));
    let mut open = true;
    egui::Grid::new("tag-details")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.label(t!("Points to"));
            ui.horizontal(|ui| {
                let short = ferrum_core::short(d.target);
                if ui
                    .link(RichText::new(short).monospace())
                    .on_hover_text(t!("Show in the history"))
                    .clicked()
                {
                    cmds.push(Command::SelectCommit(d.target));
                    open = false;
                }
                ui.label(&d.target_summary);
            });
            ui.end_row();
            ui.label(t!("Type"));
            ui.label(match (d.annotated, d.signed) {
                (true, true) => t!("Annotated, signed"),
                (true, false) => t!("Annotated"),
                _ => t!("Lightweight (no message)"),
            });
            ui.end_row();
            if let Some(name) = &d.tagger {
                ui.label(t!("Tagger"));
                let email = d.email.as_deref().unwrap_or_default();
                ui.label(format!("{name} <{email}>"));
                ui.end_row();
            }
            if let Some(time) = d.time {
                ui.label(t!("Date"));
                ui.label(crate::util::format_date(time, d.offset));
                ui.end_row();
            }
        });
    if let Some(m) = d.message.as_deref().filter(|m| !m.is_empty()) {
        ui.add_space(6.0);
        ui.label(t!("Message"));
        egui::ScrollArea::vertical()
            .max_height(220.0)
            .show(ui, |ui| {
                let mut text = m;
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .desired_width(f32::INFINITY)
                        .font(crate::util::code_font(12.5)),
                );
            });
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button(t!("Copy name")).clicked() {
            cmds.push(Command::Copy(d.name.clone()));
        }
        let busy = tab.is_none_or(|t| t.busy());
        if ui
            .add_enabled(!busy, egui::Button::new(t!("Check out (detached)")))
            .clicked()
        {
            cmds.run(Op::CheckoutDetached(d.name.clone()));
            open = false;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(t!("Close")).clicked() {
                open = false;
            }
        });
    });
    open
}

/// Changed files for the stash file picker, all checked.
fn stash_files(tab: &RepoTab) -> Vec<(String, bool)> {
    let mut paths: Vec<String> = tab
        .status
        .staged
        .iter()
        .chain(&tab.status.unstaged)
        .map(|f| f.path.clone())
        .collect();
    paths.sort();
    paths.dedup();
    paths.into_iter().map(|p| (p, true)).collect()
}

fn file_picker(ui: &mut egui::Ui, files: &mut [(String, bool)]) {
    ui.horizontal(|ui| {
        let n = files.iter().filter(|(_, on)| *on).count();
        ui.label(t!("{} of {} files", n, files.len()));
        if ui.small_button(t!("All")).clicked() {
            files.iter_mut().for_each(|(_, on)| *on = true);
        }
        if ui.small_button(t!("None")).clicked() {
            files.iter_mut().for_each(|(_, on)| *on = false);
        }
    });
    egui::Frame::group(ui.style()).show(ui, |ui| {
        egui::ScrollArea::vertical()
            .max_height(200.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (path, on) in files.iter_mut() {
                    ui.checkbox(on, path.as_str());
                }
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_prefix_defaults() {
        let p = RefPrefs::default();
        assert!(p.last_branch_prefix.is_empty() && !p.stash_restore_index);
        let old: RefPrefs = serde_json::from_str("{}").unwrap();
        assert_eq!(old, p);
    }
}
