//! Git-flow menu and dialogs: init with prefixes, start and finish of
//! features, releases and hotfixes. The git logic is in `ferrum_core::gitflow`.

use eframe::egui;
use ferrum_core::BranchKind;
use ferrum_core::cli::Git;
use ferrum_core::gitflow::{self, FinishOptions, FlowConfig, Kind};

use crate::command::{Command, Commands, Op};
use crate::tab::RepoTab;
use crate::ui::dialogs::{Dialog, buttons, field, ref_name_warning};
use crate::util::valid_ref_name;

/// Operations of this module, run through `Op::GitFlow`.
#[derive(Debug, Clone)]
pub enum GitFlowOp {
    Init(FlowConfig),
    Deinit,
    Start {
        cfg: FlowConfig,
        kind: Kind,
        name: String,
        base: String,
    },
    Finish {
        cfg: FlowConfig,
        kind: Kind,
        name: String,
        opts: FinishOptions,
    },
}

impl GitFlowOp {
    pub fn label(&self) -> String {
        match self {
            GitFlowOp::Init(_) => t!("Initialize git-flow"),
            GitFlowOp::Deinit => t!("Remove git-flow settings"),
            GitFlowOp::Start { kind, name, .. } => t!("Start {} {}", kind_word(*kind), name),
            GitFlowOp::Finish { kind, name, .. } => t!("Finish {} {}", kind_word(*kind), name),
        }
    }

    pub fn run(&self, git: &Git) -> ferrum_core::Result<()> {
        match self {
            GitFlowOp::Init(cfg) => gitflow::init(git, cfg),
            GitFlowOp::Deinit => gitflow::deinit(git),
            GitFlowOp::Start {
                cfg,
                kind,
                name,
                base,
            } => gitflow::start(git, cfg, *kind, name, Some(base)),
            GitFlowOp::Finish {
                cfg,
                kind,
                name,
                opts,
            } => gitflow::finish(git, cfg, *kind, name, opts),
        }
    }
}

pub fn kind_word(k: Kind) -> String {
    match k {
        Kind::Feature => t!("feature"),
        Kind::Release => t!("release"),
        Kind::Hotfix => t!("hotfix"),
    }
}

pub enum GitFlowDialog {
    Init(FlowConfig),
    Start {
        cfg: FlowConfig,
        kind: Kind,
        name: String,
        base: String,
    },
    Finish {
        cfg: FlowConfig,
        kind: Kind,
        name: String,
        opts: FinishOptions,
        tag: bool,
        message: String,
        push: bool,
        remote: String,
    },
}

fn has_branch(tab: &RepoTab, name: &str) -> bool {
    tab.branches
        .iter()
        .any(|b| b.kind == BranchKind::Local && b.name == name)
}

/// The init dialog, with the saved settings or sensible defaults.
pub fn init(tab: &RepoTab) -> Dialog {
    let cfg = gitflow::config(&tab.repo).unwrap_or_else(|| {
        let master = ["main", "master"]
            .into_iter()
            .find(|b| has_branch(tab, b))
            .map(str::to_owned)
            .or_else(|| tab.head.clone())
            .unwrap_or_else(|| "main".into());
        FlowConfig {
            master,
            ..Default::default()
        }
    });
    Dialog::GitFlow(GitFlowDialog::Init(cfg))
}

pub fn start(cfg: &FlowConfig, kind: Kind) -> Dialog {
    Dialog::GitFlow(GitFlowDialog::Start {
        cfg: cfg.clone(),
        kind,
        name: String::new(),
        base: kind.base(cfg).to_owned(),
    })
}

pub fn finish(tab: &RepoTab, cfg: &FlowConfig, kind: Kind, name: &str) -> Dialog {
    let remote = tab.default_remote().unwrap_or_default();
    Dialog::GitFlow(GitFlowDialog::Finish {
        cfg: cfg.clone(),
        kind,
        name: name.to_owned(),
        opts: FinishOptions::default(),
        tag: kind.is_release(),
        message: t!("Version {}", name),
        push: false,
        remote,
    })
}

/// Branches of the flow that can be finished.
fn flow_branches<'a>(tab: &'a RepoTab, cfg: &FlowConfig) -> Vec<(Kind, &'a str)> {
    tab.branches
        .iter()
        .filter(|b| b.kind == BranchKind::Local)
        .filter_map(|b| cfg.kind_of(&b.name))
        .collect()
}

/// The "Git-flow" submenu of the toolbar's ☰ menu.
pub fn menu(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    ui.menu_button(t!("Git-flow"), |ui| {
        ui.set_min_width(220.0);
        let Some(cfg) = gitflow::config(&tab.repo) else {
            if ui.button(t!("Initialize git-flow…")).clicked() {
                cmds.push(Command::Dialog(init(tab)));
                ui.close();
            }
            return;
        };
        for kind in Kind::ALL {
            let text = match kind {
                Kind::Feature => t!("Start feature…"),
                Kind::Release => t!("Start release…"),
                Kind::Hotfix => t!("Start hotfix…"),
            };
            if ui.button(text).clicked() {
                cmds.push(Command::Dialog(start(&cfg, kind)));
                ui.close();
            }
        }
        let open = flow_branches(tab, &cfg);
        if !open.is_empty() {
            ui.separator();
            for (kind, name) in open {
                if ui
                    .button(t!("Finish {} {}…", kind_word(kind), name))
                    .clicked()
                {
                    cmds.push(Command::Dialog(finish(tab, &cfg, kind, name)));
                    ui.close();
                }
            }
        }
        ui.separator();
        if ui.button(t!("Git-flow settings…")).clicked() {
            cmds.push(Command::Dialog(init(tab)));
            ui.close();
        }
        if ui.button(t!("Remove git-flow settings…")).clicked() {
            cmds.push(Command::Dialog(Dialog::confirm(
                &t!("Remove git-flow settings"),
                &t!("Remove the git-flow settings of this repository? Branches are kept."),
                Op::GitFlow(GitFlowOp::Deinit),
                false,
            )));
            ui.close();
        }
    });
}

/// "Finish feature x…" in the context menu of a flow branch in the sidebar.
pub fn branch_menu(ui: &mut egui::Ui, tab: &RepoTab, branch: &str, cmds: &mut Commands) {
    let Some(cfg) = gitflow::config(&tab.repo) else {
        return;
    };
    let Some((kind, name)) = cfg.kind_of(branch) else {
        return;
    };
    ui.separator();
    if ui
        .button(t!("Git-flow: finish {} {}…", kind_word(kind), name))
        .clicked()
    {
        cmds.push(Command::Dialog(finish(tab, &cfg, kind, name)));
        ui.close();
    }
}

fn base_items(tab: &RepoTab) -> Vec<String> {
    let mut v: Vec<String> = tab.branches.iter().map(|b| b.name.clone()).collect();
    v.extend(tab.tags.iter().map(|t| t.name.clone()));
    v
}

pub fn show_dialog(
    ui: &mut egui::Ui,
    d: &mut GitFlowDialog,
    tab: Option<&RepoTab>,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    let Some(tab) = tab else { return false };
    match d {
        GitFlowDialog::Init(cfg) => {
            ui.heading(t!("Initialize git-flow"));
            ui.weak(t!(
                "Settings are stored in the repository's git config, compatible with git-flow AVH."
            ));
            ui.add_space(4.0);
            let first = field(ui, &t!("Production branch"), &mut cfg.master, "main");
            if ui.memory(|m| m.focused().is_none()) {
                first.request_focus();
            }
            field(ui, &t!("Development branch"), &mut cfg.develop, "develop");
            egui::Grid::new("gitflow-prefixes")
                .num_columns(2)
                .show(ui, |ui| {
                    for (label, value) in [
                        (t!("Feature prefix"), &mut cfg.feature),
                        (t!("Release prefix"), &mut cfg.release),
                        (t!("Hotfix prefix"), &mut cfg.hotfix),
                        (t!("Support prefix"), &mut cfg.support),
                        (t!("Version tag prefix"), &mut cfg.version_tag),
                    ] {
                        ui.label(label);
                        ui.add(egui::TextEdit::singleline(value).desired_width(200.0));
                        ui.end_row();
                    }
                });
            let names_ok = valid_ref_name(cfg.master.trim())
                && valid_ref_name(cfg.develop.trim())
                && cfg.master.trim() != cfg.develop.trim();
            if !has_branch(tab, cfg.master.trim()) && tab.head_oid.is_some() {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    t!("The branch {} does not exist.", cfg.master.trim()),
                );
            } else if !has_branch(tab, cfg.develop.trim()) {
                ui.weak(t!(
                    "{} will be created from {}.",
                    cfg.develop.trim(),
                    cfg.master.trim()
                ));
            }
            let ok = names_ok && (has_branch(tab, cfg.master.trim()) || tab.head_oid.is_none());
            let r = buttons(ui, &t!("Initialize"), ok, false, enter);
            if r == Some(true) {
                let mut c = cfg.clone();
                c.master = c.master.trim().into();
                c.develop = c.develop.trim().into();
                cmds.run(Op::GitFlow(GitFlowOp::Init(c)));
            }
            r.is_none()
        }
        GitFlowDialog::Start {
            cfg,
            kind,
            name,
            base,
        } => {
            ui.heading(match kind {
                Kind::Feature => t!("Start feature"),
                Kind::Release => t!("Start release"),
                Kind::Hotfix => t!("Start hotfix"),
            });
            let prefix = kind.prefix(cfg).to_owned();
            let hint = if kind.is_release() {
                "1.2.0"
            } else {
                "my-feature"
            };
            field(ui, &t!("Name"), name, hint).request_focus();
            let full = format!("{prefix}{}", name.trim());
            if !name.trim().is_empty() {
                ui.weak(t!("Branch: {}", full));
            }
            ref_name_warning(ui, &full);
            let exists = has_branch(tab, &full);
            if exists {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    t!("A local branch with this name already exists."),
                );
            }
            ui.horizontal(|ui| {
                ui.label(t!("Start from"));
                crate::ui::filter_combo::filter_combo(
                    ui,
                    "gitflow-base",
                    base,
                    &base_items(tab),
                    260.0,
                    true,
                );
            });
            let ok = !name.trim().is_empty() && valid_ref_name(&full) && !exists;
            let r = buttons(ui, &t!("Start"), ok, false, enter);
            if r == Some(true) {
                cmds.run(Op::GitFlow(GitFlowOp::Start {
                    cfg: cfg.clone(),
                    kind: *kind,
                    name: name.trim().into(),
                    base: base.trim().into(),
                }));
            }
            r.is_none()
        }
        GitFlowDialog::Finish {
            cfg,
            kind,
            name,
            opts,
            tag,
            message,
            push,
            remote,
        } => {
            let branch = format!("{}{name}", kind.prefix(cfg));
            ui.heading(t!("Finish {} {}", kind_word(*kind), name));
            if kind.is_release() {
                ui.label(t!("Merges {} into {}.", branch, cfg.master));
            } else {
                ui.label(t!("Merges {} into {}.", branch, cfg.develop));
            }
            ui.add_space(4.0);
            ui.checkbox(
                &mut opts.no_ff,
                t!("Always create a merge commit (--no-ff)"),
            );
            if kind.is_release() {
                let tag_name = gitflow::tag_name(cfg, name);
                ui.checkbox(tag, t!("Create tag {}", tag_name));
                if *tag {
                    ui.add(
                        egui::TextEdit::multiline(message)
                            .hint_text(t!("Tag message"))
                            .desired_rows(2)
                            .desired_width(f32::INFINITY),
                    );
                }
                ui.checkbox(&mut opts.back_merge, t!("Merge back into {}", cfg.develop));
            }
            ui.checkbox(&mut opts.delete_branch, t!("Delete branch {}", branch));
            ui.add_enabled_ui(!tab.remotes.is_empty(), |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(push, t!("Push to"));
                    egui::ComboBox::from_id_salt("gitflow-remote")
                        .selected_text(remote.as_str())
                        .show_ui(ui, |ui| {
                            for r in &tab.remotes {
                                ui.selectable_value(remote, r.name.clone(), &r.name);
                            }
                        });
                });
            });
            if !tab.status.is_empty() {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    t!("You have local changes: commit or stash them before finishing."),
                );
            }
            let r = buttons(ui, &t!("Finish"), true, false, false);
            let _ = enter;
            if r == Some(true) {
                let mut o = opts.clone();
                o.tag = (kind.is_release() && *tag).then(|| message.trim().to_owned());
                o.push = (*push && !remote.is_empty()).then(|| remote.clone());
                if !kind.is_release() {
                    o.back_merge = false;
                }
                cmds.run(Op::GitFlow(GitFlowOp::Finish {
                    cfg: cfg.clone(),
                    kind: *kind,
                    name: name.clone(),
                    opts: o,
                }));
            }
            r.is_none()
        }
    }
}

/// Dev aid for `FERRUM_START`: `gitflow:init`, `gitflow:start=feature`,
/// `gitflow:finish=BRANCH`.
pub fn dev_step(arg: &str, tab: Option<&RepoTab>) -> Vec<Command> {
    let Some(t) = tab else { return Vec::new() };
    let cfg = gitflow::config(&t.repo);
    let d = match (arg.split_once('='), cfg) {
        (Some(("start", k)), Some(cfg)) => {
            let kind = match k {
                "release" => Kind::Release,
                "hotfix" => Kind::Hotfix,
                _ => Kind::Feature,
            };
            start(&cfg, kind)
        }
        (Some(("finish", b)), Some(cfg)) => match cfg.kind_of(b) {
            Some((kind, name)) => finish(t, &cfg, kind, name),
            None => return Vec::new(),
        },
        _ => init(t),
    };
    vec![Command::Dialog(d)]
}
