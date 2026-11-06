//! Git LFS: the toolbar menu, tracked patterns, status and locks dialogs,
//! and lock/unlock in file menus.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use eframe::egui::{self, RichText};
use ferrum_core::cli::Git;
use ferrum_core::lfs::{self, Lock, Unlock};

use crate::command::{Command, Commands, Op};
use crate::repo_parts::PartOp;
use crate::tab::RepoTab;
use crate::ui::dialogs::Dialog;

static AVAILABLE: OnceLock<bool> = OnceLock::new();
static CHECKING: AtomicBool = AtomicBool::new(false);

/// Whether `git lfs` is installed: checked once, in the background.
/// `None` until the check finishes.
pub fn available(ctx: &egui::Context) -> Option<bool> {
    if let Some(a) = AVAILABLE.get() {
        return Some(*a);
    }
    if !CHECKING.swap(true, Ordering::SeqCst) {
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = AVAILABLE.set(lfs::available(&Git::new(std::env::temp_dir())));
            ctx.request_repaint();
        });
    }
    None
}

fn part(op: PartOp) -> Command {
    Command::Run(Op::Part(op))
}

/// The "Git LFS" submenu of the toolbar.
pub fn menu(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    ui.menu_button("Git LFS", |ui| {
        ui.set_min_width(200.0);
        match available(ui.ctx()) {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak(t!("Checking…"));
                });
                return;
            }
            Some(false) => {
                ui.weak(t!("Git LFS is not installed"));
                return;
            }
            Some(true) => {}
        }
        ui.add_enabled_ui(!tab.busy(), |ui| {
            if ui.button(t!("Tracked patterns…")).clicked() {
                cmds.push(Command::Dialog(Dialog::Lfs(LfsDialog::track(tab))));
                ui.close();
            }
            if ui.button(t!("Status…")).clicked() {
                cmds.push(Command::Dialog(Dialog::Lfs(LfsDialog::status(
                    tab,
                    ui.ctx(),
                ))));
                ui.close();
            }
            if ui.button(t!("Locks…")).clicked() {
                cmds.push(Command::Dialog(Dialog::Lfs(LfsDialog::locks(
                    tab,
                    ui.ctx(),
                ))));
                ui.close();
            }
            ui.separator();
            for (action, tip) in [
                ("fetch", t!("Download the LFS files of the current branch")),
                ("pull", t!("Download and check out the LFS files")),
                (
                    "prune",
                    t!("Delete old local LFS files that are not needed"),
                ),
            ] {
                let resp = ui.button(format!("Git LFS {action}")).on_hover_text(tip);
                if resp.clicked() {
                    cmds.run(Op::Lfs(action));
                    ui.close();
                }
            }
            ui.separator();
            if ui
                .button(t!("Install (git lfs install)"))
                .on_hover_text(t!("Sets up the LFS filters and this repository's hooks"))
                .clicked()
            {
                cmds.push(part(PartOp::LfsInstall));
                ui.close();
            }
        });
    });
}

/// Lock / unlock entries for a file menu (only for files handled by LFS).
pub fn file_menu(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands, paths: &[String]) {
    if !tab.uses_lfs || available(ui.ctx()) != Some(true) {
        return;
    }
    let lfs: Vec<String> = paths
        .iter()
        .filter(|p| tab.repo.is_lfs_tracked(p))
        .cloned()
        .collect();
    if lfs.is_empty() {
        return;
    }
    ui.separator();
    ui.add_enabled_ui(!tab.busy(), |ui| {
        if ui.button(t!("🔒 Lock (Git LFS)")).clicked() {
            cmds.push(part(PartOp::LfsLock(lfs.clone())));
            ui.close();
        }
        if ui.button(t!("Unlock (Git LFS)")).clicked() {
            let which = lfs.iter().cloned().map(Unlock::Path).collect();
            cmds.push(part(PartOp::LfsUnlock {
                which,
                force: false,
            }));
            ui.close();
        }
    });
}

// ----- Dialogs -----

/// Result of a background read, filled in by its thread.
type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

fn load<T: Send + 'static>(
    ctx: &egui::Context,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Slot<T> {
    let slot: Slot<T> = Arc::default();
    let (out, ctx) = (slot.clone(), ctx.clone());
    std::thread::spawn(move || {
        let r = work();
        *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
        ctx.request_repaint();
    });
    slot
}

pub enum LfsDialog {
    Track {
        patterns: Vec<String>,
        new: String,
        /// A track/untrack job was running: reread .gitattributes when it ends.
        waiting: bool,
    },
    Status(Slot<(String, String)>),
    Locks {
        list: Slot<Vec<Lock>>,
        filter: String,
        selected: BTreeSet<String>,
        force: bool,
        waiting: bool,
    },
}

impl LfsDialog {
    pub fn track(tab: &RepoTab) -> Self {
        LfsDialog::Track {
            patterns: lfs::tracked_patterns(tab.path()),
            new: String::new(),
            waiting: false,
        }
    }

    pub fn status(tab: &RepoTab, ctx: &egui::Context) -> Self {
        let git = tab.git.clone();
        LfsDialog::Status(load(ctx, move || {
            lfs::status(&git).map_err(|e| e.to_string())
        }))
    }

    pub fn locks(tab: &RepoTab, ctx: &egui::Context) -> Self {
        LfsDialog::Locks {
            list: load_locks(tab, ctx),
            filter: String::new(),
            selected: BTreeSet::new(),
            force: false,
            waiting: false,
        }
    }
}

fn load_locks(tab: &RepoTab, ctx: &egui::Context) -> Slot<Vec<Lock>> {
    let git = tab.git.clone();
    load(ctx, move || lfs::locks(&git).map_err(|e| e.to_string()))
}

/// Draws an LFS dialog. Returns `false` when it should close.
pub fn show_dialog(
    ui: &mut egui::Ui,
    d: &mut LfsDialog,
    tab: Option<&RepoTab>,
    cmds: &mut Commands,
) -> bool {
    let Some(tab) = tab else { return false };
    if available(ui.ctx()) == Some(false) {
        ui.heading("Git LFS");
        ui.label(t!("Git LFS is not installed"));
        return !ui.button(t!("Close")).clicked();
    }
    match d {
        LfsDialog::Track {
            patterns,
            new,
            waiting,
        } => track_dialog(ui, tab, cmds, patterns, new, waiting),
        LfsDialog::Status(slot) => {
            ui.heading(t!("Git LFS status"));
            match &*slot.lock().unwrap_or_else(|e| e.into_inner()) {
                None => {
                    ui.spinner();
                }
                Some(Err(e)) => {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                Some(Ok((status, files))) => {
                    egui::ScrollArea::vertical()
                        .max_height(380.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new(status.trim()).monospace());
                            ui.add_space(6.0);
                            ui.strong(t!("Files in LFS"));
                            if files.trim().is_empty() {
                                ui.weak(t!("None"));
                            } else {
                                ui.label(RichText::new(files.trim()).monospace());
                            }
                        });
                }
            }
            ui.add_space(8.0);
            !ui.button(t!("Close")).clicked()
        }
        LfsDialog::Locks {
            list,
            filter,
            selected,
            force,
            waiting,
        } => {
            if *waiting && !tab.busy() {
                *waiting = false;
                *list = load_locks(tab, ui.ctx());
                selected.clear();
            }
            locks_dialog(ui, tab, cmds, list, filter, selected, force, waiting)
        }
    }
}

fn track_dialog(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    cmds: &mut Commands,
    patterns: &mut Vec<String>,
    new: &mut String,
    waiting: &mut bool,
) -> bool {
    if *waiting && !tab.busy() {
        *waiting = false;
        *patterns = lfs::tracked_patterns(tab.path());
    }
    ui.heading(t!("Git LFS: tracked patterns"));
    ui.weak(t!(
        "Files matching these patterns are stored in LFS. They are saved in .gitattributes: commit it to share them."
    ));
    ui.add_space(6.0);
    ui.add_enabled_ui(!tab.busy(), |ui| {
        if patterns.is_empty() {
            ui.weak(t!("No patterns yet."));
        }
        for p in patterns.iter() {
            ui.horizontal(|ui| {
                ui.monospace(p);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(t!("Untrack")).clicked() {
                        cmds.push(part(PartOp::LfsUntrack(p.clone())));
                        *waiting = true;
                    }
                });
            });
        }
        ui.separator();
        ui.horizontal(|ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(new)
                    .hint_text("*.psd")
                    .desired_width(300.0),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let ok = !new.trim().is_empty();
            if (ui
                .add_enabled(ok, egui::Button::new(t!("Track with LFS")))
                .clicked()
                || enter)
                && ok
            {
                cmds.push(part(PartOp::LfsTrack(new.trim().to_owned())));
                new.clear();
                *waiting = true;
            }
        });
    });
    if tab.busy() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.weak(t!("Working…"));
        });
    }
    ui.add_space(8.0);
    !ui.button(t!("Close")).clicked()
}

#[allow(clippy::too_many_arguments)]
fn locks_dialog(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    cmds: &mut Commands,
    list: &mut Slot<Vec<Lock>>,
    filter: &mut String,
    selected: &mut BTreeSet<String>,
    force: &mut bool,
    waiting: &mut bool,
) -> bool {
    ui.set_width(620.0);
    ui.heading(t!("Git LFS locks"));
    let mut reload = false;
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(filter)
                .hint_text(t!("Filter by path or owner"))
                .desired_width(320.0),
        );
        if ui.button(t!("Reload")).clicked() {
            reload = true;
        }
    });
    let state = list.lock().unwrap_or_else(|e| e.into_inner()).clone();
    match state {
        None => {
            ui.spinner();
        }
        Some(Err(e)) => {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        Some(Ok(locks)) => {
            let f = filter.trim().to_lowercase();
            let shown: Vec<&Lock> = locks
                .iter()
                .filter(|l| {
                    f.is_empty()
                        || l.path.to_lowercase().contains(&f)
                        || l.owner.to_lowercase().contains(&f)
                })
                .collect();
            if shown.is_empty() {
                ui.weak(t!("No locks."));
            }
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    egui::Grid::new("lfs-locks")
                        .num_columns(4)
                        .striped(true)
                        .spacing([12.0, 4.0])
                        .show(ui, |ui| {
                            for l in &shown {
                                let mut on = selected.contains(&l.id);
                                if ui.checkbox(&mut on, "").changed() {
                                    if on {
                                        selected.insert(l.id.clone());
                                    } else {
                                        selected.remove(&l.id);
                                    }
                                }
                                ui.monospace(&l.path);
                                ui.label(&l.owner);
                                ui.weak(&l.locked_at);
                                ui.end_row();
                            }
                        });
                });
            ui.horizontal(|ui| {
                if ui.small_button(t!("Select all")).clicked() {
                    selected.extend(shown.iter().map(|l| l.id.clone()));
                }
                if ui.small_button(t!("Select none")).clicked() {
                    selected.clear();
                }
            });
        }
    }
    ui.separator();
    ui.checkbox(force, t!("Force (also release locks held by other people)"));
    let mut close = false;
    ui.horizontal(|ui| {
        let n = selected.len();
        let can = n > 0 && !tab.busy();
        if ui
            .add_enabled(can, egui::Button::new(t!("Unlock {} selected", n)))
            .clicked()
        {
            let which = selected.iter().cloned().map(Unlock::Id).collect();
            cmds.push(part(PartOp::LfsUnlock {
                which,
                force: *force,
            }));
            *waiting = true;
        }
        if tab.busy() {
            ui.spinner();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            close = ui.button(t!("Close")).clicked();
        });
    });
    if reload {
        *list = load_locks(tab, ui.ctx());
        selected.clear();
    }
    !close
}
