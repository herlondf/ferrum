//! Stacked branches: the sidebar section, the stack marks in the graph, the
//! branch menu entries and the dialogs (restack, push the stack, chained
//! pull requests, a new branch in the stack). Absorb and moving hunks have
//! their dialogs here too (see `ui::absorb`, `ui::move_hunks`).

use eframe::egui::{self, Color32, RichText, pos2, vec2};
use ferrum_core::forge::chain::ChainEntry;
use ferrum_core::stack::{self, Insert, Stack};
use ferrum_core::sync::PushRequest;
use ferrum_core::{BranchKind, ChangeKind, Oid};

use crate::command::{Command, Commands, Op};
use crate::icons;
use crate::settings::Settings;
use crate::stack_ops::{self, StackOp};
use crate::tab::RepoTab;
use crate::theme;
use crate::ui::dialogs::{Dialog, buttons};
use crate::ui::sidebar::{children, header, leaf};
use crate::ui::sidebar_model::{NavAction, SidebarState};

/// The dialogs of this module.
#[derive(Clone)]
pub enum StackDialog {
    Restack(RestackForm),
    Push(PushForm),
    Requests(Box<RequestsForm>),
    AddBranch(AddForm),
    Absorb(crate::ui::absorb::AbsorbForm),
    Move(Box<crate::ui::move_hunks::MoveForm>),
}

#[derive(Clone)]
pub struct RestackForm {
    stack: Stack,
}

#[derive(Clone)]
pub struct PushForm {
    stack: Stack,
    remote: String,
    remotes: Vec<String>,
    force: bool,
}

#[derive(Clone)]
pub struct RequestsForm {
    stack: Stack,
    /// Read once: the service does not change under an open dialog.
    hosted: Option<ferrum_core::forge::HostedRepo>,
    titles: Vec<String>,
    body: String,
    draft: bool,
    push_first: bool,
}

#[derive(Clone)]
pub struct AddForm {
    at: Insert,
    name: String,
    checkout: bool,
}

pub fn restack(stack: &Stack) -> Dialog {
    Dialog::Stack(StackDialog::Restack(RestackForm {
        stack: stack.clone(),
    }))
}

pub fn push(tab: &RepoTab, stack: &Stack) -> Dialog {
    let remotes: Vec<String> = tab.remotes.iter().map(|r| r.name.clone()).collect();
    Dialog::Stack(StackDialog::Push(PushForm {
        stack: stack.clone(),
        remote: tab.default_remote().unwrap_or_default(),
        remotes,
        force: true,
    }))
}

pub fn requests(tab: &RepoTab, stack: &Stack) -> Dialog {
    let titles = stack
        .branches
        .iter()
        .map(|b| {
            tab.repo
                .find_commit_info(b.tip)
                .map(|c| c.summary)
                .unwrap_or_else(|_| b.name.clone())
        })
        .collect();
    Dialog::Stack(StackDialog::Requests(Box::new(RequestsForm {
        stack: stack.clone(),
        titles,
        body: String::new(),
        draft: false,
        push_first: stack.branches.iter().any(|b| needs_push(tab, &b.name)),
        hosted: tab.hosted.clone(),
    })))
}

pub fn add_branch(at: Insert) -> Dialog {
    Dialog::Stack(StackDialog::AddBranch(AddForm {
        at,
        name: String::new(),
        checkout: true,
    }))
}

fn needs_push(tab: &RepoTab, name: &str) -> bool {
    tab.branches
        .iter()
        .find(|b| b.kind == BranchKind::Local && b.name == name)
        .is_some_and(|b| b.upstream.is_none() || b.ahead_behind.is_some_and(|(a, _)| a > 0))
}

/// Local changes to tracked files.
pub fn has_local_changes(tab: &RepoTab) -> bool {
    !tab.status.staged.is_empty()
        || tab
            .status
            .unstaged
            .iter()
            .any(|f| f.kind != ChangeKind::Untracked)
}

/// Shows a dialog of this module. Returns `false` when it should close.
pub fn show(
    ui: &mut egui::Ui,
    d: &mut StackDialog,
    tab: Option<&RepoTab>,
    settings: &Settings,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    let Some(tab) = tab else { return false };
    match d {
        StackDialog::Restack(f) => restack_ui(ui, f, tab, cmds, enter),
        StackDialog::Push(f) => push_ui(ui, f, tab, cmds, enter),
        StackDialog::Requests(f) => requests_ui(ui, f, tab, settings, cmds),
        StackDialog::AddBranch(f) => add_ui(ui, f, tab, cmds, enter),
        StackDialog::Absorb(f) => crate::ui::absorb::show(ui, f, tab, cmds, enter),
        StackDialog::Move(f) => crate::ui::move_hunks::show(ui, f, tab, cmds, enter),
    }
}

/// A line with an icon in the weak color, wrapped.
pub fn note(ui: &mut egui::Ui, icon: &str, text: &str, color: Option<Color32>) {
    let color = color.unwrap_or_else(|| ui.visuals().weak_text_color());
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SPACE_S + 2.0;
        ui.label(RichText::new(icon).color(color));
        ui.add(egui::Label::new(RichText::new(text).color(color).size(12.5)).wrap());
    });
}

/// "Saved first as a backup; Undo restores it."
pub fn backup_note(ui: &mut egui::Ui) {
    note(
        ui,
        icons::CLOCK_COUNTER_CLOCKWISE,
        &t!(
            "The branches are saved first as a backup (refs/ferrum/backup/…): Undo puts them back."
        ),
        None,
    );
}

/// A row: branch icon, name, then weak details on the right.
fn branch_line(ui: &mut egui::Ui, icon: &str, name: &str, detail: &str, warn: bool) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SPACE_S;
        let weak = ui.visuals().weak_text_color();
        ui.label(RichText::new(icon).color(theme::accent(ui)));
        ui.label(theme::strong(name));
        let color = if warn {
            ui.visuals().warn_fg_color
        } else {
            weak
        };
        ui.label(RichText::new(detail).color(color).size(12.0));
    });
}

fn restack_ui(
    ui: &mut egui::Ui,
    f: &mut RestackForm,
    tab: &RepoTab,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    let s = &f.stack;
    ui.heading(t!("Restack {}", s.top().name));
    let steps = stack::restack_preview(s);
    if steps.is_empty() {
        ui.add_space(theme::SPACE_S);
        note(
            ui,
            icons::CHECK_CIRCLE,
            &t!(
                "Every branch is based on the latest commit of the one below it: nothing to restack."
            ),
            Some(crate::util::added_color(ui)),
        );
    } else {
        ui.label(t!(
            "These branches are rebased onto the branch below them, bottom first:"
        ));
        ui.add_space(theme::SPACE_S);
        theme::card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            for step in steps.iter().rev() {
                let detail = t!("{} onto {}", stack_ops::commits(step.commits), step.onto);
                branch_line(ui, icons::BRANCH, &step.branch, &detail, false);
            }
            branch_line(ui, icons::GIT_COMMIT, &s.trunk, &t!("base"), false);
        });
    }
    ui.add_space(theme::SPACE_S);
    let update_refs = stack::git_supports_update_refs();
    if update_refs {
        note(
            ui,
            icons::INFO,
            &t!(
                "One rebase moves the whole stack (--update-refs). If it stops for a conflict, resolve it and continue, or abort to leave everything as it was."
            ),
            None,
        );
    } else {
        note(
            ui,
            icons::WARNING,
            &t!(
                "This git is older than 2.38, so the branches are rebased one at a time. After a conflict, continue the rebase and restack again."
            ),
            Some(ui.visuals().warn_fg_color),
        );
    }
    backup_note(ui);
    let top = s.top().name.as_str();
    let blocked = has_local_changes(tab) && (tab.head.as_deref() != Some(top) || !update_refs);
    if blocked {
        note(
            ui,
            icons::WARNING,
            &t!("Commit or stash the local changes first."),
            Some(ui.visuals().warn_fg_color),
        );
    }
    let ok = !steps.is_empty() && !blocked && !tab.busy();
    let r = buttons(ui, &t!("Restack"), ok, false, enter);
    if r == Some(true) {
        cmds.run(Op::Stack(StackOp::Restack {
            stack: s.clone(),
            backup: stack_ops::backup_id(tab, "restack"),
            return_to: tab.head.clone(),
            update_refs,
        }));
    }
    r.is_none()
}

/// The push of every branch of `stack`.
pub fn push_op(stack: &Stack, remote: &str, force: bool) -> Op {
    Op::PushRefs(PushRequest {
        remote: remote.to_owned(),
        branches: stack
            .branches
            .iter()
            .map(|b| (b.name.clone(), b.name.clone()))
            .collect(),
        set_upstream: true,
        force_with_lease: force,
        ..Default::default()
    })
}

fn push_ui(
    ui: &mut egui::Ui,
    f: &mut PushForm,
    tab: &RepoTab,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    ui.heading(t!("Push stack {}", f.stack.top().name));
    if f.remotes.is_empty() {
        note(
            ui,
            icons::WARNING,
            &t!("This repository has no remote."),
            Some(ui.visuals().warn_fg_color),
        );
    } else {
        ui.horizontal(|ui| {
            ui.label(t!("To"));
            egui::ComboBox::from_id_salt("stack-push-remote")
                .selected_text(&f.remote)
                .show_ui(ui, |ui| {
                    for r in &f.remotes {
                        ui.selectable_value(&mut f.remote, r.clone(), r);
                    }
                });
        });
    }
    ui.add_space(theme::SPACE_S);
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        for b in f.stack.branches.iter().rev() {
            let info = tab
                .branches
                .iter()
                .find(|x| x.kind == BranchKind::Local && x.name == b.name);
            let detail = match info.map(|i| (&i.upstream, i.ahead_behind)) {
                Some((None, _)) => t!("not pushed yet"),
                Some((Some(_), Some((0, 0)))) => t!("up to date"),
                Some((Some(_), Some((a, 0)))) => t!("{} to push", stack_ops::commits(a)),
                Some((Some(_), Some((a, b)))) => t!("{} ahead, {} behind", a, b),
                _ => String::new(),
            };
            branch_line(ui, icons::BRANCH, &b.name, &detail, false);
        }
    });
    ui.add_space(theme::SPACE_S);
    ui.checkbox(
        &mut f.force,
        t!("Force with lease (needed after a restack)"),
    )
    .on_hover_text(t!(
        "Overwrites the remote branches only if nobody else pushed to them since they were fetched"
    ));
    let ok = !f.remote.is_empty() && !tab.busy();
    let r = buttons(ui, &t!("Push"), ok, false, enter);
    if r == Some(true) {
        cmds.run(push_op(&f.stack, &f.remote, f.force));
    }
    r.is_none()
}

fn requests_ui(
    ui: &mut egui::Ui,
    f: &mut RequestsForm,
    tab: &RepoTab,
    settings: &Settings,
    cmds: &mut Commands,
) -> bool {
    ui.set_width(560.0);
    let Some(hosted) = f.hosted.clone() else {
        ui.heading(t!("Pull requests for the stack"));
        ui.label(t!(
            "No remote of this repository is on a hosting service Ferrum knows."
        ));
        return buttons(ui, &t!("OK"), false, false, false).is_none();
    };
    let noun = hosted.kind.request_name();
    ui.heading(t!("Create {}s for stack {}", noun, f.stack.top().name));
    ui.label(t!(
        "One {} per branch, each based on the branch below it. Every description links the others.",
        noun
    ));
    ui.add_space(theme::SPACE_S);
    egui::ScrollArea::vertical()
        .max_height(260.0)
        .show(ui, |ui| {
            for (i, b) in f.stack.branches.iter().enumerate().rev() {
                theme::card(ui).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = theme::SPACE_S;
                        ui.label(RichText::new(icons::GIT_PULL_REQUEST).color(theme::accent(ui)));
                        ui.label(theme::strong(&b.name));
                        ui.weak(icons::ARROW_RIGHT);
                        ui.weak(&b.parent);
                    });
                    ui.add(
                        egui::TextEdit::singleline(&mut f.titles[i])
                            .hint_text(t!("Title"))
                            .desired_width(f32::INFINITY),
                    );
                });
                ui.add_space(theme::SPACE_XS);
            }
        });
    ui.label(t!("Description (added to each)"));
    ui.add(
        egui::TextEdit::multiline(&mut f.body)
            .desired_rows(2)
            .desired_width(f32::INFINITY),
    );
    ui.horizontal(|ui| {
        ui.checkbox(&mut f.draft, t!("Drafts"));
        ui.checkbox(&mut f.push_first, t!("Push the stack first"));
    });
    let token = settings.token_for(hosted.kind, &hosted.host);
    if token.is_none() {
        ui.horizontal(|ui| {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                t!("Log in to {} first.", hosted.host),
            );
            if ui.button(t!("Log in…")).clicked() {
                cmds.push(Command::Dialog(Dialog::Forge(crate::ui::forge::login(
                    hosted.kind,
                    &hosted.host,
                ))));
            }
        });
    }
    let titled = f.titles.iter().all(|t| !t.trim().is_empty());
    let ok = titled && token.is_some() && !tab.busy();
    let r = buttons(ui, &t!("Create {}s", noun), ok, false, false);
    if r == Some(true)
        && let Some(token) = token
    {
        if f.push_first {
            let remote = crate::hosted::hosted_remote_name(tab).unwrap_or_else(|| "origin".into());
            cmds.run(push_op(&f.stack, &remote, true));
        }
        let entries = f
            .stack
            .branches
            .iter()
            .zip(&f.titles)
            .map(|(b, title)| ChainEntry {
                branch: b.name.clone(),
                target: b.parent.clone(),
                title: title.trim().to_owned(),
                body: f.body.clone(),
                draft: f.draft,
            })
            .collect();
        cmds.run(Op::Stack(StackOp::CreateRequests {
            hosted,
            token,
            entries,
        }));
    }
    r.is_none()
}

fn add_ui(
    ui: &mut egui::Ui,
    f: &mut AddForm,
    tab: &RepoTab,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    let (title, where_) = match &f.at {
        Insert::Above(b) => (
            t!("New branch on top of {}", b),
            t!(
                "It starts at the last commit of {} and is stacked on it.",
                b
            ),
        ),
        Insert::Below(b) => {
            let parent = tab
                .stacks
                .iter()
                .flat_map(|s| s.branches.iter())
                .find(|x| &x.name == b)
                .map(|x| x.parent.clone())
                .unwrap_or_default();
            (
                t!("New branch below {}", b),
                t!(
                    "It goes between {} and {}. Commit on it, then restack to move {} on top.",
                    parent,
                    b,
                    b
                ),
            )
        }
    };
    ui.heading(title);
    ui.label(where_);
    ui.add_space(theme::SPACE_S);
    let resp = crate::ui::dialogs::field(ui, &t!("Name"), &mut f.name, "feature/part-2");
    if resp.gained_focus() || f.name.is_empty() {
        resp.request_focus();
    }
    let name = f.name.trim();
    crate::ui::dialogs::ref_name_warning(ui, name);
    let exists = tab
        .branches
        .iter()
        .any(|b| b.kind == BranchKind::Local && b.name == name);
    if exists {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            t!("A branch named {} already exists.", name),
        );
    }
    let blocked = f.checkout && has_local_changes(tab);
    ui.checkbox(&mut f.checkout, t!("Check it out"));
    if blocked {
        note(
            ui,
            icons::INFO,
            &t!("The local changes come along to the new branch."),
            None,
        );
    }
    let ok = !name.is_empty() && crate::util::valid_ref_name(name) && !exists && !tab.busy();
    let r = buttons(ui, &t!("Create branch"), ok, false, enter);
    if r == Some(true) {
        cmds.run(Op::Stack(StackOp::AddBranch {
            name: name.to_owned(),
            at: f.at.clone(),
            checkout: f.checkout,
        }));
    }
    r.is_none()
}

// ----- Sidebar -----

pub fn section(ui: &mut egui::Ui, tab: &RepoTab, st: &mut SidebarState, cmds: &mut Commands) {
    if tab.stacks.is_empty() {
        return;
    }
    let shown: Vec<&Stack> = tab
        .stacks
        .iter()
        .filter(|s| s.branches.iter().any(|b| st.matches(&b.name)))
        .collect();
    let h = header(
        ui,
        st,
        "sec:stacks",
        true,
        RichText::new(t!("Stacks")),
        Some(shown.len()),
        None,
    );
    h.resp.on_hover_text(t!(
        "Branches based on each other: restack, push or open pull requests for all of them at once"
    ));
    if !h.open {
        return;
    }
    children(ui, st, "sec:stacks", |ui, st| {
        if shown.is_empty() {
            ui.weak(t!("No matches."));
        }
        for s in shown {
            stack_rows(ui, tab, st, s, cmds);
        }
    });
}

fn stack_rows(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    st: &mut SidebarState,
    s: &Stack,
    cmds: &mut Commands,
) {
    let key = format!("stack:{}", s.top().name);
    let label = format!("{}  {}", icons::STACK, s.top().name);
    let h = header(
        ui,
        st,
        &key,
        true,
        RichText::new(label),
        Some(s.branches.len()),
        None,
    );
    h.resp
        .on_hover_text(t!(
            "Stack of {} on {}",
            stack_ops::branches(s.branches.len()),
            s.trunk
        ))
        .context_menu(|ui| stack_menu(ui, tab, s, cmds));
    if !h.open {
        return;
    }
    children(ui, st, &key, |ui, st| {
        if s.needs_restack() {
            let text = icons::tinted(
                icons::ARROWS_CLOCKWISE,
                ui.visuals().warn_fg_color,
                t!("Restack needed…"),
            );
            let b = egui::Button::new(text).frame_when_inactive(false);
            if ui
                .add_enabled(!tab.busy(), b)
                .on_hover_text(t!("A branch below has new commits"))
                .clicked()
            {
                cmds.push(Command::Dialog(restack(s)));
            }
        }
        for b in s.branches.iter().rev() {
            let head = tab.head.as_deref() == Some(b.name.as_str());
            let text = if head {
                RichText::new(format!("{} {}", icons::CHECK, b.name)).strong()
            } else {
                RichText::new(format!("{} {}", icons::BRANCH, b.name))
            };
            let mut right = b.commits.to_string();
            if b.behind_parent {
                right = format!("{} {right}", icons::WARNING);
            }
            let right = if b.behind_parent {
                RichText::new(right).color(ui.visuals().warn_fg_color)
            } else {
                RichText::new(right).weak()
            };
            let selected = tab.history.selected == Some(b.tip);
            let button = egui::Button::selectable(selected, text).right_text(right);
            let row_key = format!("{key}/{}", b.name);
            let (resp, action) = leaf(ui, st, &row_key, button);
            let tip = if b.behind_parent {
                t!(
                    "{} is not based on the latest commit of {}: restack it",
                    b.name,
                    b.parent
                )
            } else {
                t!("{} on {}", stack_ops::commits(b.commits), b.parent)
            };
            let resp = resp.on_hover_text(tip);
            if resp.clicked() || action == Some(NavAction::Select) {
                cmds.push(Command::SelectCommit(b.tip));
            }
            if (resp.double_clicked() || action == Some(NavAction::Activate))
                && !head
                && !tab.busy()
            {
                cmds.run(Op::Checkout(b.name.clone()));
            }
            resp.context_menu(|ui| {
                if !head && ui.button(t!("Check out {}", b.name)).clicked() {
                    cmds.run(Op::Checkout(b.name.clone()));
                    ui.close();
                }
                branch_actions(ui, tab, s, &b.name, cmds);
                ui.separator();
                stack_menu(ui, tab, s, cmds);
            });
        }
        let base = egui::Button::selectable(
            false,
            RichText::new(format!("{} {}", icons::GIT_COMMIT, s.trunk)).weak(),
        );
        let (resp, _) = leaf(ui, st, &format!("{key}/base"), base);
        if resp.on_hover_text(t!("The base of the stack")).clicked()
            && let Ok(id) = tab.repo.resolve(&format!("refs/heads/{}", s.trunk))
        {
            cmds.push(Command::SelectCommit(id));
        }
    });
}

fn item(ui: &mut egui::Ui, icon: &str, label: String, enabled: bool) -> bool {
    ui.add_enabled(enabled, egui::Button::new(icons::text(icon, label)))
        .clicked()
}

/// Actions on the whole stack.
fn stack_menu(ui: &mut egui::Ui, tab: &RepoTab, s: &Stack, cmds: &mut Commands) {
    let idle = !tab.busy();
    if item(ui, icons::ARROWS_CLOCKWISE, t!("Restack…"), idle) {
        cmds.push(Command::Dialog(restack(s)));
        ui.close();
    }
    if item(
        ui,
        icons::PUSH,
        t!("Push stack…"),
        idle && !tab.remotes.is_empty(),
    ) {
        cmds.push(Command::Dialog(push(tab, s)));
        ui.close();
    }
    if let Some(h) = &tab.hosted {
        let label = t!("Create {}s for the stack…", h.kind.request_name());
        if item(ui, icons::GIT_PULL_REQUEST, label, idle) {
            cmds.push(Command::Dialog(requests(tab, s)));
            ui.close();
        }
    }
    let top = s.top().name.clone();
    if item(ui, icons::PLUS, t!("New branch on top of {}…", top), idle) {
        cmds.push(Command::Dialog(add_branch(Insert::Above(top))));
        ui.close();
    }
}

/// Entries for one branch of a stack.
fn branch_actions(ui: &mut egui::Ui, tab: &RepoTab, _s: &Stack, name: &str, cmds: &mut Commands) {
    let idle = !tab.busy();
    if item(ui, icons::PLUS, t!("New branch on top of {}…", name), idle) {
        cmds.push(Command::Dialog(add_branch(Insert::Above(name.to_owned()))));
        ui.close();
    }
    if item(
        ui,
        icons::ARROW_ELBOW_DOWN_LEFT,
        t!("Insert a branch below {}…", name),
        idle,
    ) {
        cmds.push(Command::Dialog(add_branch(Insert::Below(name.to_owned()))));
        ui.close();
    }
}

/// Stack entries for a local branch's context menu in the sidebar.
pub fn branch_menu(ui: &mut egui::Ui, tab: &RepoTab, name: &str, cmds: &mut Commands) {
    ui.separator();
    match tab.stacks.iter().find(|s| s.contains(name)) {
        Some(s) => {
            ui.menu_button(icons::text(icons::STACK, t!("Stack")), |ui| {
                branch_actions(ui, tab, s, name, cmds);
                ui.separator();
                stack_menu(ui, tab, s, cmds);
            });
        }
        None => {
            if item(
                ui,
                icons::STACK,
                t!("Stack a new branch on top…"),
                !tab.busy(),
            ) {
                cmds.push(Command::Dialog(add_branch(Insert::Above(name.to_owned()))));
                ui.close();
            }
        }
    }
}

/// Entries of the main menu's Repository menu.
pub fn repository_menu(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    ui.separator();
    let idle = !tab.busy();
    let dirty = !tab.status.is_empty();
    if item(
        ui,
        icons::ARROWS_IN_SIMPLE,
        t!("Absorb local changes into commits…"),
        idle && dirty,
    ) {
        cmds.push(Command::Dialog(crate::ui::absorb::open(tab)));
        ui.close();
    }
    if let Some(head) = tab.head.clone() {
        match stack::stack_of(&tab.stacks, &head) {
            Some(s) => {
                if item(
                    ui,
                    icons::ARROWS_CLOCKWISE,
                    t!("Restack {}…", s.top().name),
                    idle,
                ) {
                    cmds.push(Command::Dialog(restack(s)));
                    ui.close();
                }
            }
            None => {
                if item(ui, icons::STACK, t!("Stack a new branch on top…"), idle) {
                    cmds.push(Command::Dialog(add_branch(Insert::Above(head))));
                    ui.close();
                }
            }
        }
    }
}

/// Palette entries.
pub fn palette_entries(tab: &RepoTab) -> Vec<crate::ui::palette::Entry> {
    use crate::ui::palette::{Entry, Group};
    let mut out = Vec::new();
    if !tab.status.is_empty() {
        let d = crate::ui::absorb::open(tab);
        if let Dialog::Stack(sd) = d {
            out.push(
                Entry::new(
                    Group::Commands,
                    t!("Absorb local changes into commits…"),
                    "",
                    move || Command::Dialog(Dialog::Stack(sd.clone())),
                )
                .icon(icons::ARROWS_IN_SIMPLE)
                .keywords("absorb fixup autosquash"),
            );
        }
    }
    if let Some(head) = tab.head.clone() {
        if let Some(s) = stack::stack_of(&tab.stacks, &head) {
            let s1 = s.clone();
            out.push(
                Entry::new(
                    Group::Commands,
                    t!("Restack {}…", s.top().name),
                    "",
                    move || Command::Dialog(restack(&s1)),
                )
                .icon(icons::ARROWS_CLOCKWISE)
                .keywords("restack stack rebase update-refs"),
            );
            if let Dialog::Stack(p) = push(tab, s) {
                out.push(
                    Entry::new(Group::Commands, t!("Push stack…"), "", move || {
                        Command::Dialog(Dialog::Stack(p.clone()))
                    })
                    .icon(icons::PUSH),
                );
            }
            if let Some(h) = &tab.hosted
                && let Dialog::Stack(r) = requests(tab, s)
            {
                out.push(
                    Entry::new(
                        Group::Commands,
                        t!("Create {}s for the stack…", h.kind.request_name()),
                        "",
                        move || Command::Dialog(Dialog::Stack(r.clone())),
                    )
                    .icon(icons::GIT_PULL_REQUEST)
                    .keywords("stack chained pull requests merge requests pr mr"),
                );
            }
        }
        out.push(
            Entry::new(
                Group::Commands,
                t!("Stack a new branch on top…"),
                "",
                move || Command::Dialog(add_branch(Insert::Above(head.clone()))),
            )
            .icon(icons::STACK)
            .keywords("stack stacked branch new"),
        );
    }
    out
}

// ----- Graph -----

/// A small "stack 2/3" mark after the labels of a commit that is the tip of
/// a stack branch. Returns the x where the next item goes.
pub fn paint_graph_mark(
    ui: &egui::Ui,
    painter: &egui::Painter,
    pos: egui::Pos2,
    tab: &RepoTab,
    id: Oid,
) -> f32 {
    let Some((s, at)) = tab.stacks.iter().find_map(|s| {
        s.branches
            .iter()
            .position(|b| b.tip == id)
            .map(|i| (s, i + 1))
    }) else {
        return pos.x;
    };
    let v = ui.visuals();
    let warn = s.branches[at - 1].behind_parent;
    let color = if warn {
        v.warn_fg_color
    } else {
        v.weak_text_color()
    };
    let font = egui::FontId::proportional(11.0);
    let text = format!("{} {at}/{}", icons::STACK, s.branches.len());
    let galley = painter.layout_no_wrap(text, font, color);
    let size = vec2(galley.size().x + 10.0, 16.0);
    let rect = egui::Rect::from_min_size(pos2(pos.x, pos.y - size.y / 2.0), size);
    painter.rect_stroke(
        rect,
        theme::RADIUS,
        egui::Stroke::new(1.0, color.gamma_multiply(0.6)),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        pos2(rect.left() + 5.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    rect.right() + 4.0
}

/// `FERRUM_START` steps: `stack:restack`, `stack:push`, `stack:prs`,
/// `stack:add=BRANCH`, `stack:insert=BRANCH` (for the stack of HEAD, else
/// the first one), `stack:absorb`, `stack:move=PATH[>REV]` (see
/// `move_hunks::dev_step`).
pub fn dev_step(arg: &str, tab: Option<&RepoTab>) -> Vec<Command> {
    let Some(t) = tab else { return Vec::new() };
    let (what, rest) = arg.split_once('=').unwrap_or((arg, ""));
    let current = t
        .head
        .as_deref()
        .and_then(|h| stack::stack_of(&t.stacks, h))
        .or(t.stacks.first());
    let dialog = match (what, current) {
        ("restack", Some(s)) => restack(s),
        ("push", Some(s)) => push(t, s),
        ("prs", Some(s)) => requests(t, s),
        ("add", _) => add_branch(Insert::Above(rest.to_owned())),
        ("insert", _) => add_branch(Insert::Below(rest.to_owned())),
        ("absorb", _) => crate::ui::absorb::open(t),
        ("move", _) => return crate::ui::move_hunks::dev_step(rest, t),
        _ => {
            eprintln!("FERRUM_START: no stack for stack:{arg}");
            return Vec::new();
        }
    };
    vec![Command::Dialog(dialog)]
}
