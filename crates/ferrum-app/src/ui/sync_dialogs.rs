//! Dialogs for fetch, pull, push, tracking branches and remotes.

use std::collections::BTreeSet;
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use eframe::egui::{self, RichText};
use ferrum_core::cli::Git;
use ferrum_core::ops::{MergeMode, ResetMode};
use ferrum_core::sync::{PullOptions, PushRequest};
use ferrum_core::{BranchInfo, BranchKind};

use crate::command::{Command, Commands, Op};
use crate::settings::{PullChoice, Settings};
use crate::tab::RepoTab;
use crate::ui::dialogs::{buttons, field, ref_name_warning};
use crate::ui::filter_combo::filter_combo;
use crate::util::valid_ref_name;

#[derive(Clone)]
pub enum SyncDialog {
    Fetch(FetchForm),
    Pull(PullForm),
    Push(PushForm),
    PushMany(PushManyForm),
    PushTags(PushTagsForm),
    Track(TrackForm),
    CheckoutTracked(CheckoutForm),
    EditRemote(RemoteForm),
}

#[derive(Clone)]
pub struct FetchForm {
    remote: String,
    /// Opened from a remote's menu: fetch that one.
    only_remote: bool,
    /// `None` until the remembered options are read.
    opts: Option<(bool, bool, bool)>,
}

#[derive(Clone)]
pub struct PullForm {
    remote: String,
    branch: String,
    autostash: Option<bool>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TagChoice {
    None,
    Annotated,
    All,
}

pub struct PushForm {
    branch: String,
    remote: String,
    remote_branch: String,
    set_upstream: bool,
    force: bool,
    tags: Option<TagChoice>,
    check_submodules: bool,
    unpushed: Option<Vec<(String, usize)>>,
    /// Looks for unpushed submodule commits, started when first shown.
    checking: Option<Receiver<Vec<(String, usize)>>>,
}

impl Clone for PushForm {
    fn clone(&self) -> Self {
        Self {
            branch: self.branch.clone(),
            remote: self.remote.clone(),
            remote_branch: self.remote_branch.clone(),
            set_upstream: self.set_upstream,
            force: self.force,
            tags: self.tags,
            check_submodules: self.check_submodules,
            unpushed: None,
            checking: None,
        }
    }
}

#[derive(Clone)]
pub struct PushManyForm {
    branches: Vec<String>,
    remote: String,
    atomic: bool,
    force: bool,
    set_upstream: bool,
}

#[derive(Clone)]
pub struct PushTagsForm {
    remote: String,
    selected: BTreeSet<String>,
    all: bool,
    filter: String,
}

#[derive(Clone)]
pub struct TrackForm {
    branch: String,
    upstream: String,
    /// Pull (current branch) or fast-forward (other branch) right after.
    update: bool,
    autostash: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CheckoutChoice {
    FastForward,
    Merge,
    Rebase,
    Reset,
    Only,
}

#[derive(Clone)]
pub struct CheckoutForm {
    remote_branch: String,
    local: String,
    /// The local branch only lacks commits of the remote branch.
    can_ff: bool,
    choice: CheckoutChoice,
}

#[derive(Clone)]
pub struct RemoteForm {
    old: String,
    name: String,
    url: String,
    /// When the connection test started, and what it tests.
    tested: Option<(Instant, String)>,
}

// ----- Constructors -----

pub fn fetch(tab: &RepoTab, remote: Option<&str>) -> SyncDialog {
    SyncDialog::Fetch(FetchForm {
        remote: remote
            .map(str::to_owned)
            .or_else(|| tab.default_remote())
            .unwrap_or_default(),
        only_remote: remote.is_some(),
        opts: None,
    })
}

pub fn pull(tab: &RepoTab) -> SyncDialog {
    let up = tab
        .current_branch()
        .and_then(|b| b.upstream.as_deref())
        .and_then(|u| u.split_once('/'))
        .map(|(r, b)| (r.to_owned(), b.to_owned()));
    let (remote, branch) = up.unwrap_or_else(|| {
        (
            tab.default_remote().unwrap_or_default(),
            tab.head.clone().unwrap_or_default(),
        )
    });
    SyncDialog::Pull(PullForm {
        remote,
        branch,
        autostash: None,
    })
}

/// The upstream (remote, branch name) of a local branch.
fn upstream_of(tab: &RepoTab, branch: &str) -> Option<(String, String)> {
    tab.branches
        .iter()
        .find(|b| b.kind == BranchKind::Local && b.name == branch)
        .and_then(|b| b.upstream.as_deref())
        .and_then(|u| u.split_once('/'))
        .map(|(r, b)| (r.to_owned(), b.to_owned()))
}

pub fn push(tab: &RepoTab, branch: &str) -> SyncDialog {
    let up = upstream_of(tab, branch);
    let remote = up
        .as_ref()
        .map(|u| u.0.clone())
        .or_else(|| tab.default_remote())
        .unwrap_or_default();
    let mut form = PushForm {
        branch: branch.into(),
        remote,
        remote_branch: String::new(),
        set_upstream: up.is_none(),
        force: false,
        tags: None,
        check_submodules: !tab.submodules.is_empty(),
        unpushed: None,
        checking: None,
    };
    form.retarget(tab);
    SyncDialog::Push(form)
}

pub fn push_many(tab: &RepoTab, branches: Vec<String>) -> SyncDialog {
    let remote = branches
        .iter()
        .find_map(|b| upstream_of(tab, b))
        .map(|u| u.0)
        .or_else(|| tab.default_remote())
        .unwrap_or_default();
    let set_upstream = branches.iter().any(|b| upstream_of(tab, b).is_none());
    SyncDialog::PushMany(PushManyForm {
        branches,
        remote,
        atomic: true,
        force: false,
        set_upstream,
    })
}

pub fn push_tags(tab: &RepoTab, tags: &[String]) -> SyncDialog {
    SyncDialog::PushTags(PushTagsForm {
        remote: tab.default_remote().unwrap_or_default(),
        selected: tags.iter().cloned().collect(),
        all: false,
        filter: String::new(),
    })
}

pub fn track(tab: &RepoTab, branch: &BranchInfo) -> SyncDialog {
    let upstream = branch.upstream.clone().unwrap_or_else(|| {
        let r = tab.default_remote().unwrap_or_default();
        format!("{r}/{}", branch.name)
    });
    SyncDialog::Track(TrackForm {
        branch: branch.name.clone(),
        upstream,
        update: false,
        autostash: !tab.status.is_empty(),
    })
}

pub fn edit_remote(name: &str, url: &str) -> SyncDialog {
    SyncDialog::EditRemote(RemoteForm {
        old: name.into(),
        name: name.into(),
        url: url.into(),
        tested: None,
    })
}

/// What checking out a remote branch does: switch to its local branch, ask
/// how to bring a local branch that is behind or has diverged up to date,
/// or create a new local branch.
pub fn checkout_remote(tab: &RepoTab, remote: &BranchInfo) -> Command {
    let short = remote.split_remote().map_or(remote.name.as_str(), |r| r.1);
    let local = tab
        .branches
        .iter()
        .filter(|l| l.kind == BranchKind::Local)
        .find(|l| l.upstream.as_deref() == Some(&remote.name))
        .or_else(|| {
            tab.branches
                .iter()
                .find(|l| l.kind == BranchKind::Local && l.name == short)
        });
    let Some(l) = local else {
        let d = crate::ui::dialogs::Dialog::checkout_remote(&remote.name, short);
        return Command::Dialog(d);
    };
    let (Some(lt), Some(rt)) = (l.target, remote.target) else {
        return Command::Run(Op::Checkout(l.name.clone()));
    };
    // Up to date, or only ahead: nothing to bring in.
    if tab.repo.is_ancestor(rt, lt) {
        return Command::Run(Op::Checkout(l.name.clone()));
    }
    let can_ff = tab.repo.is_ancestor(lt, rt);
    Command::Dialog(crate::ui::dialogs::Dialog::Sync(
        SyncDialog::CheckoutTracked(CheckoutForm {
            remote_branch: remote.name.clone(),
            local: l.name.clone(),
            can_ff,
            choice: if can_ff {
                CheckoutChoice::FastForward
            } else {
                CheckoutChoice::Merge
            },
        }),
    ))
}

// ----- Helpers -----

/// Branch names on `remote`, without the `remote/` prefix.
fn remote_branches(tab: &RepoTab, remote: &str) -> Vec<String> {
    let prefix = format!("{remote}/");
    tab.branches
        .iter()
        .filter(|b| b.kind == BranchKind::Remote)
        .filter_map(|b| b.name.strip_prefix(&prefix))
        .filter(|n| *n != "HEAD")
        .map(str::to_owned)
        .collect()
}

fn local_branches(tab: &RepoTab) -> Vec<String> {
    tab.branches
        .iter()
        .filter(|b| b.kind == BranchKind::Local)
        .map(|b| b.name.clone())
        .collect()
}

fn remote_combo(ui: &mut egui::Ui, id: &str, tab: &RepoTab, remote: &mut String) -> bool {
    let before = remote.clone();
    egui::ComboBox::from_id_salt(id)
        .selected_text(remote.as_str())
        .show_ui(ui, |ui| {
            for r in &tab.remotes {
                ui.selectable_value(remote, r.name.clone(), &r.name);
            }
        });
    *remote != before
}

/// While another operation runs: what it is, and a button to stop it.
fn running_note(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    let Some(job) = &tab.job else { return };
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(t!("Running: {}", job.label));
        if job.cancellable()
            && !job.cancel_requested()
            && ui
                .small_button(t!("✖ Cancel it"))
                .on_hover_text(t!("Cancel {}", job.label))
                .clicked()
        {
            cmds.push(Command::CancelJob);
        }
    });
    ui.weak(t!("This one starts when it finishes."));
    ui.add_space(4.0);
}

fn no_remotes(ui: &mut egui::Ui, enter: bool) -> bool {
    ui.label(t!(
        "This repository has no remotes. Add one first (right-click Remotes in the sidebar)."
    ));
    buttons(ui, &t!("OK"), true, false, enter).is_none()
}

fn pull_mode(ui: &mut egui::Ui, settings: &mut Settings) {
    for choice in [
        PullChoice::Default,
        PullChoice::Merge,
        PullChoice::Rebase,
        PullChoice::FastForwardOnly,
    ] {
        ui.radio_value(&mut settings.pull, choice, choice.label());
    }
}

// ----- Drawing -----

/// Draws a dialog. Returns `false` when it should close.
pub fn show(
    ui: &mut egui::Ui,
    dialog: &mut SyncDialog,
    tab: Option<&RepoTab>,
    settings: &mut Settings,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    let Some(tab) = tab else { return false };
    match dialog {
        SyncDialog::Fetch(f) => fetch_ui(ui, f, tab, settings, cmds, enter),
        SyncDialog::Pull(f) => pull_ui(ui, f, tab, settings, cmds, enter),
        SyncDialog::Push(f) => push_ui(ui, f, tab, settings, cmds, enter),
        SyncDialog::PushMany(f) => push_many_ui(ui, f, tab, cmds, enter),
        SyncDialog::PushTags(f) => push_tags_ui(ui, f, tab, cmds, enter),
        SyncDialog::Track(f) => track_ui(ui, f, tab, settings, cmds, enter),
        SyncDialog::CheckoutTracked(f) => checkout_ui(ui, f, cmds, enter),
        SyncDialog::EditRemote(f) => remote_ui(ui, f, tab, settings, cmds, enter),
    }
}

fn fetch_ui(
    ui: &mut egui::Ui,
    f: &mut FetchForm,
    tab: &RepoTab,
    settings: &mut Settings,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    ui.heading(t!("Fetch"));
    if tab.remotes.is_empty() {
        return no_remotes(ui, enter);
    }
    running_note(ui, tab, cmds);
    let s = &mut settings.sync;
    let (all, prune, tags) =
        f.opts
            .get_or_insert((s.fetch_all && !f.only_remote, s.fetch_prune, s.fetch_tags));
    ui.add_enabled_ui(!*all, |ui| {
        ui.horizontal(|ui| {
            ui.label(t!("Remote"));
            remote_combo(ui, "fetch-remote", tab, &mut f.remote);
        });
    });
    ui.checkbox(all, t!("Fetch all remotes"));
    ui.checkbox(prune, t!("Prune remote branches that were deleted"))
        .on_hover_text("--prune");
    ui.checkbox(tags, t!("Always fetch all tags"))
        .on_hover_text("--tags");
    match buttons(ui, &t!("Fetch"), *all || !f.remote.is_empty(), false, enter) {
        Some(true) => {
            (s.fetch_prune, s.fetch_tags) = (*prune, *tags);
            if !f.only_remote {
                s.fetch_all = *all;
            }
            let remotes = if *all {
                Vec::new()
            } else {
                vec![f.remote.clone()]
            };
            cmds.run(Op::FetchWith(s.fetch_options(remotes)));
            false
        }
        Some(false) => false,
        None => true,
    }
}

fn pull_ui(
    ui: &mut egui::Ui,
    f: &mut PullForm,
    tab: &RepoTab,
    settings: &mut Settings,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    ui.heading(t!("Pull"));
    if tab.remotes.is_empty() {
        return no_remotes(ui, enter);
    }
    running_note(ui, tab, cmds);
    let current = tab.current_branch().cloned();
    let upstream = current.as_ref().and_then(|b| b.upstream.clone());
    egui::Grid::new("pull-grid")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label(t!("Remote"));
            if remote_combo(ui, "pull-remote", tab, &mut f.remote) {
                let list = remote_branches(tab, &f.remote);
                if !list.contains(&f.branch) {
                    f.branch = list.first().cloned().unwrap_or_default();
                }
            }
            ui.end_row();
            ui.label(t!("Branch"));
            let list = remote_branches(tab, &f.remote);
            filter_combo(ui, "pull-branch", &mut f.branch, &list, 260.0, false);
            ui.end_row();
            ui.label(t!("Into"));
            ui.label(format!(
                "🌿 {}",
                current.as_ref().map_or("HEAD", |b| b.name.as_str())
            ));
            ui.end_row();
        });
    if upstream.is_none() {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            t!("The current branch has no upstream; git will use its defaults."),
        );
    }
    ui.add_space(4.0);
    pull_mode(ui, settings);
    let autostash = f.autostash.get_or_insert(settings.sync.pull_autostash);
    ui.checkbox(autostash, t!("Stash and reapply local changes"))
        .on_hover_text("--autostash");
    let picked = format!("{}/{}", f.remote, f.branch);
    let ok = !f.remote.is_empty() && !f.branch.is_empty();
    match buttons(ui, &t!("Pull"), ok, false, enter) {
        Some(true) => {
            settings.sync.pull_autostash = *autostash;
            // The upstream itself: let git pick it, as a plain pull does.
            let explicit = upstream.as_deref() != Some(picked.as_str());
            cmds.run(Op::PullWith(PullOptions {
                mode: settings.pull.mode(),
                autostash: *autostash,
                remote: explicit.then(|| f.remote.clone()),
                branch: explicit.then(|| f.branch.clone()),
            }));
            false
        }
        Some(false) => false,
        None => true,
    }
}

impl PushForm {
    /// Picks the remote branch for the chosen branch and remote.
    fn retarget(&mut self, tab: &RepoTab) {
        self.remote_branch = match upstream_of(tab, &self.branch) {
            Some((r, b)) if r == self.remote => b,
            _ => self.branch.clone(),
        };
        self.set_upstream = upstream_of(tab, &self.branch).is_none();
    }

    fn poll_submodules(&mut self, ui: &egui::Ui, tab: &RepoTab) {
        if self.unpushed.is_none() && self.checking.is_none() && !tab.submodules.is_empty() {
            let (tx, rx) = mpsc::channel();
            let git = Git::new(tab.path());
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let _ = tx.send(ferrum_core::sync::unpushed_submodules(&git).unwrap_or_default());
                ctx.request_repaint();
            });
            self.checking = Some(rx);
        }
        let result = self.checking.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(list) = result {
            self.unpushed = Some(list);
            self.checking = None;
        }
    }
}

fn push_ui(
    ui: &mut egui::Ui,
    f: &mut PushForm,
    tab: &RepoTab,
    settings: &mut Settings,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    ui.heading(t!("Push"));
    if tab.remotes.is_empty() {
        return no_remotes(ui, enter);
    }
    running_note(ui, tab, cmds);
    f.poll_submodules(ui, tab);
    let mut tags = f.tags.unwrap_or(if settings.sync.push_follow_tags {
        TagChoice::Annotated
    } else {
        TagChoice::None
    });
    let mut retarget = false;
    let on_remote = remote_branches(tab, &f.remote);
    egui::Grid::new("push-grid")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label(t!("Branch"));
            retarget |= filter_combo(
                ui,
                "push-branch",
                &mut f.branch,
                &local_branches(tab),
                260.0,
                false,
            );
            ui.end_row();
            ui.label(t!("To remote"));
            retarget |= remote_combo(ui, "push-remote", tab, &mut f.remote);
            ui.end_row();
            ui.label(t!("Remote branch"));
            filter_combo(
                ui,
                "push-remote-branch",
                &mut f.remote_branch,
                &on_remote,
                260.0,
                true,
            );
            ui.end_row();
        });
    if retarget {
        f.retarget(tab);
    }
    let rb = f.remote_branch.trim();
    if !rb.is_empty() && !on_remote.iter().any(|b| b == rb) {
        ui.weak(t!("{}/{} will be created.", f.remote, rb));
    }
    ref_name_warning(ui, rb);
    ui.checkbox(
        &mut f.set_upstream,
        t!("Track this remote branch (set upstream)"),
    );
    ui.horizontal(|ui| {
        ui.label(t!("Tags"));
        ui.radio_value(&mut tags, TagChoice::None, t!("No tags"));
        ui.radio_value(
            &mut tags,
            TagChoice::Annotated,
            t!("Annotated, with the branch"),
        )
        .on_hover_text("--follow-tags");
        ui.radio_value(&mut tags, TagChoice::All, t!("All tags"))
            .on_hover_text("--tags");
    });
    f.tags = Some(tags);
    if !tab.tags.is_empty() && ui.small_button(t!("Choose tags to push…")).clicked() {
        cmds.push(Command::Dialog(crate::ui::dialogs::Dialog::Sync(
            push_tags(tab, &[]),
        )));
        return false;
    }
    ui.checkbox(&mut f.force, t!("Force push (with lease)"))
        .on_hover_text(t!(
            "Overwrites the remote branch, unless someone else pushed to it since your last fetch"
        ));
    if f.force {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            t!("Force pushing rewrites history on the remote."),
        );
    }
    if !tab.submodules.is_empty() {
        ui.checkbox(
            &mut f.check_submodules,
            t!("Refuse to push if submodule commits are not pushed"),
        )
        .on_hover_text("--recurse-submodules=check");
    }
    if let Some(list) = f.unpushed.as_ref().filter(|l| !l.is_empty()) {
        let names: Vec<String> = list
            .iter()
            .map(|(p, n)| t!("{} ({} commits)", p, n))
            .collect();
        ui.colored_label(
            ui.visuals().warn_fg_color,
            t!(
                "⚠ Submodules with commits that are not pushed yet: {}. Push them first, or others cannot check out this commit.",
                names.join(", ")
            ),
        );
    }
    let ok = !f.remote.is_empty() && !f.branch.is_empty() && valid_ref_name(rb);
    match buttons(ui, &t!("Push"), ok, f.force, enter) {
        Some(true) => {
            settings.sync.push_follow_tags = tags == TagChoice::Annotated;
            cmds.run(Op::PushRefs(PushRequest {
                remote: f.remote.clone(),
                branches: vec![(f.branch.clone(), rb.to_owned())],
                all_tags: tags == TagChoice::All,
                follow_tags: tags == TagChoice::Annotated,
                set_upstream: f.set_upstream,
                force_with_lease: f.force,
                check_submodules: f.check_submodules && !tab.submodules.is_empty(),
                ..Default::default()
            }));
            false
        }
        Some(false) => false,
        None => true,
    }
}

/// Where each branch goes on `remote`: its upstream there, or the same name.
fn targets(tab: &RepoTab, branches: &[String], remote: &str) -> Vec<(String, String)> {
    branches
        .iter()
        .map(|b| {
            let to = match upstream_of(tab, b) {
                Some((r, n)) if r == remote => n,
                _ => b.clone(),
            };
            (b.clone(), to)
        })
        .collect()
}

fn push_many_ui(
    ui: &mut egui::Ui,
    f: &mut PushManyForm,
    tab: &RepoTab,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    ui.heading(t!("Push {} branches", f.branches.len()));
    if tab.remotes.is_empty() {
        return no_remotes(ui, enter);
    }
    running_note(ui, tab, cmds);
    ui.horizontal(|ui| {
        ui.label(t!("To remote"));
        remote_combo(ui, "push-many-remote", tab, &mut f.remote);
    });
    let pairs = targets(tab, &f.branches, &f.remote);
    egui::ScrollArea::vertical()
        .max_height(180.0)
        .show(ui, |ui| {
            for (local, to) in &pairs {
                ui.label(format!("🌿 {local}  ➡  {}/{to}", f.remote));
            }
        });
    ui.checkbox(&mut f.atomic, t!("All or nothing (atomic)"))
        .on_hover_text(t!("If one branch is rejected, none is updated (--atomic)"));
    ui.checkbox(
        &mut f.set_upstream,
        t!("Track the remote branches (set upstream)"),
    );
    ui.checkbox(&mut f.force, t!("Force push (with lease)"));
    match buttons(ui, &t!("Push"), !f.remote.is_empty(), f.force, enter) {
        Some(true) => {
            cmds.run(Op::PushRefs(PushRequest {
                remote: f.remote.clone(),
                branches: pairs,
                atomic: f.atomic,
                set_upstream: f.set_upstream,
                force_with_lease: f.force,
                ..Default::default()
            }));
            false
        }
        Some(false) => false,
        None => true,
    }
}

fn push_tags_ui(
    ui: &mut egui::Ui,
    f: &mut PushTagsForm,
    tab: &RepoTab,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    ui.heading(t!("Push tags"));
    if tab.remotes.is_empty() {
        return no_remotes(ui, enter);
    }
    running_note(ui, tab, cmds);
    ui.horizontal(|ui| {
        ui.label(t!("To remote"));
        remote_combo(ui, "push-tags-remote", tab, &mut f.remote);
    });
    ui.checkbox(&mut f.all, t!("All tags"));
    ui.add_enabled_ui(!f.all, |ui| {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut f.filter)
                    .hint_text(t!("Filter…"))
                    .desired_width(220.0),
            );
            let shown: Vec<&String> = tab
                .tags
                .iter()
                .map(|t| &t.name)
                .filter(|n| crate::ui::filter_combo::matches(n, &f.filter))
                .collect();
            if ui.small_button(t!("Select all")).clicked() {
                f.selected.extend(shown.iter().map(|s| (*s).clone()));
            }
            if ui.small_button(t!("Select none")).clicked() {
                f.selected.clear();
            }
        });
        egui::ScrollArea::vertical()
            .max_height(220.0)
            .show(ui, |ui| {
                for tag in &tab.tags {
                    if !crate::ui::filter_combo::matches(&tag.name, &f.filter) {
                        continue;
                    }
                    let mut on = f.selected.contains(&tag.name);
                    if ui.checkbox(&mut on, format!("🏷 {}", tag.name)).changed() {
                        if on {
                            f.selected.insert(tag.name.clone());
                        } else {
                            f.selected.remove(&tag.name);
                        }
                    }
                }
            });
    });
    let n = if f.all {
        tab.tags.len()
    } else {
        f.selected.len()
    };
    let ok = !f.remote.is_empty() && n > 0;
    match buttons(ui, &t!("Push {} tags", n), ok, false, enter) {
        Some(true) => {
            cmds.run(Op::PushRefs(PushRequest {
                remote: f.remote.clone(),
                tags: if f.all {
                    Vec::new()
                } else {
                    f.selected.iter().cloned().collect()
                },
                all_tags: f.all,
                ..Default::default()
            }));
            false
        }
        Some(false) => false,
        None => true,
    }
}

fn track_ui(
    ui: &mut egui::Ui,
    f: &mut TrackForm,
    tab: &RepoTab,
    settings: &mut Settings,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    ui.heading(t!("Tracking branch of {}", f.branch));
    running_note(ui, tab, cmds);
    let all: Vec<String> = tab
        .branches
        .iter()
        .filter(|b| b.kind == BranchKind::Remote && !b.name.ends_with("/HEAD"))
        .map(|b| b.name.clone())
        .collect();
    if all.is_empty() {
        ui.label(t!("There are no remote branches. Fetch first."));
        return buttons(ui, &t!("OK"), true, false, enter).is_none();
    }
    ui.horizontal(|ui| {
        ui.label(t!("Track"));
        filter_combo(ui, "track-upstream", &mut f.upstream, &all, 280.0, false);
    });
    let current = tab.head.as_deref() == Some(f.branch.as_str());
    let local = tab.repo.resolve(&format!("refs/heads/{}", f.branch)).ok();
    let remote = tab
        .branches
        .iter()
        .find(|b| b.kind == BranchKind::Remote && b.name == f.upstream)
        .and_then(|b| b.target);
    let can_ff =
        matches!((local, remote), (Some(l), Some(r)) if l != r && tab.repo.is_ancestor(l, r));
    if current {
        ui.checkbox(&mut f.update, t!("Pull from it now"));
        ui.add_enabled_ui(f.update && !tab.status.is_empty(), |ui| {
            ui.checkbox(&mut f.autostash, t!("Stash and reapply local changes"))
                .on_hover_text("--autostash");
        });
    } else if can_ff {
        ui.checkbox(&mut f.update, t!("Fast-forward {} to it", f.branch));
    }
    let ok = all.contains(&f.upstream);
    match buttons(ui, &t!("Track"), ok, false, enter) {
        Some(true) => {
            let mut ops = vec![Op::SetUpstream {
                branch: f.branch.clone(),
                upstream: f.upstream.clone(),
            }];
            if f.update && current {
                ops.push(Op::PullWith(PullOptions {
                    mode: settings.pull.mode(),
                    autostash: f.autostash && !tab.status.is_empty(),
                    ..Default::default()
                }));
            } else if f.update && can_ff {
                ops.push(Op::FastForward(vec![(
                    f.branch.clone(),
                    f.upstream.clone(),
                )]));
            }
            let label = t!("Track {} from {}", f.upstream, f.branch);
            cmds.run(Op::Sequence(label, ops));
            false
        }
        Some(false) => false,
        None => true,
    }
}

fn checkout_ui(ui: &mut egui::Ui, f: &mut CheckoutForm, cmds: &mut Commands, enter: bool) -> bool {
    ui.heading(t!("Check out {}", f.remote_branch));
    if f.can_ff {
        ui.label(t!(
            "The local branch {} is behind {}.",
            f.local,
            f.remote_branch
        ));
    } else {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            t!(
                "The local branch {} and {} have diverged: it cannot be fast-forwarded.",
                f.local,
                f.remote_branch
            ),
        );
    }
    ui.add_space(4.0);
    let c = &mut f.choice;
    if f.can_ff {
        ui.radio_value(
            c,
            CheckoutChoice::FastForward,
            t!("Check out and fast-forward"),
        );
    }
    ui.radio_value(
        c,
        CheckoutChoice::Merge,
        t!("Check out and merge {}", f.remote_branch),
    );
    ui.radio_value(
        c,
        CheckoutChoice::Rebase,
        t!("Check out and rebase onto {}", f.remote_branch),
    );
    ui.radio_value(
        c,
        CheckoutChoice::Reset,
        t!("Check out and reset to {}", f.remote_branch),
    );
    if *c == CheckoutChoice::Reset && !f.can_ff {
        ui.colored_label(
            ui.visuals().error_fg_color,
            t!("Commits only on the local branch will no longer be on it (undo can bring them back)."),
        );
    }
    ui.radio_value(c, CheckoutChoice::Only, t!("Only check out {}", f.local));
    let danger = *c == CheckoutChoice::Reset && !f.can_ff;
    match buttons(ui, &t!("Check out"), true, danger, enter && !danger) {
        Some(true) => {
            let remote = f.remote_branch.clone();
            let mut ops = vec![Op::Checkout(f.local.clone())];
            ops.extend(match f.choice {
                CheckoutChoice::FastForward => Some(Op::Merge {
                    rev: remote,
                    mode: MergeMode::FastForwardOnly,
                }),
                CheckoutChoice::Merge => Some(Op::Merge {
                    rev: remote,
                    mode: MergeMode::Default,
                }),
                CheckoutChoice::Rebase => Some(Op::Rebase { onto: remote }),
                CheckoutChoice::Reset => Some(Op::Reset {
                    rev: remote,
                    mode: ResetMode::Keep,
                }),
                CheckoutChoice::Only => None,
            });
            cmds.run(Op::Sequence(t!("Check out {}", f.local), ops));
            false
        }
        Some(false) => false,
        None => true,
    }
}

fn remote_ui(
    ui: &mut egui::Ui,
    f: &mut RemoteForm,
    tab: &RepoTab,
    settings: &mut Settings,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    ui.heading(t!("Edit remote {}", f.old));
    field(ui, &t!("Name"), &mut f.name, "origin");
    ref_name_warning(ui, &f.name);
    let taken = f.name.trim() != f.old && tab.remotes.iter().any(|r| r.name == f.name.trim());
    if taken {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            t!("A remote with this name already exists."),
        );
    }
    field(ui, "URL", &mut f.url, "git@github.com:user/repo.git");
    ui.horizontal(|ui| {
        let testing = tab.job.as_ref().is_some_and(|j| {
            f.tested
                .as_ref()
                .is_some_and(|(_, url)| j.label == t!("Test connection to {}", url))
        });
        if testing {
            ui.spinner();
            ui.label(t!("Testing…"));
            if ui.small_button(t!("✖ Cancel it")).clicked() {
                cmds.push(Command::CancelJob);
            }
        } else if ui
            .add_enabled(
                !f.url.trim().is_empty() && !tab.busy(),
                egui::Button::new(t!("Test connection")),
            )
            .on_hover_text("git ls-remote --heads")
            .clicked()
        {
            let url = f.url.trim().to_owned();
            f.tested = Some((Instant::now(), url.clone()));
            cmds.run(Op::TestRemote(url));
        }
        if ui.button(t!("Copy URL")).clicked() {
            cmds.push(Command::Copy(f.url.trim().to_owned()));
        }
    });
    if let (Some((at, _)), Some(m)) = (&f.tested, &tab.message)
        && m.at >= *at
        && tab.job.is_none()
    {
        let color = if m.error {
            ui.visuals().error_fg_color
        } else {
            ui.visuals().text_color()
        };
        ui.label(RichText::new(&m.text).color(color));
    }
    let ok = valid_ref_name(&f.name) && !f.url.trim().is_empty() && !taken;
    match buttons(ui, &t!("Save"), ok, false, enter) {
        Some(true) => {
            let new = f.name.trim().to_owned();
            if new != f.old && settings.sync.auto_fetch_off(tab, &f.old) {
                settings.sync.set_auto_fetch(tab, &f.old, true);
                settings.sync.set_auto_fetch(tab, &new, false);
            }
            cmds.run(Op::EditRemote {
                old: f.old.clone(),
                new,
                url: f.url.trim().to_owned(),
            });
            false
        }
        Some(false) => false,
        None => true,
    }
}
