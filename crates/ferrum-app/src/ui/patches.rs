//! Patches: saving or copying commits, the working copy or a stash as a
//! patch, and applying patch files (picked, dropped on the window or pasted)
//! after a preview. Patches of selected local-change files are in
//! `changes_tools`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};
use ferrum_core::cli::Git;
use ferrum_core::patch::{self, Applies, Check, Mode, Source};
use ferrum_core::{Oid, StashInfo, short};

use crate::command::{Command, Commands, Op};
use crate::jobs::{JobOutput, JobResult, Refresh};
use crate::tab::{RepoTab, View};
use crate::ui::dialogs::{Dialog, buttons};

/// Operations of this module, run through `Op::Patch`.
#[derive(Debug, Clone)]
pub enum PatchOp {
    Save {
        source: Source,
        path: PathBuf,
    },
    /// Puts the patch on the clipboard.
    Copy(Source),
    Apply {
        text: String,
        mode: Mode,
        three_way: bool,
    },
}

fn source_label(s: &Source) -> String {
    match s {
        Source::Commits(ids) if ids.len() == 1 => short(ids[0]),
        Source::Commits(ids) => t!("{} commits", ids.len()),
        Source::Range { from, to } => format!("{}..{}", short(*from), short(*to)),
        Source::WorkingCopy => t!("local changes"),
        Source::Stash(s) => s.refname(),
    }
}

impl PatchOp {
    pub fn label(&self) -> String {
        match self {
            PatchOp::Save { source, .. } => t!("Save {} as a patch", source_label(source)),
            PatchOp::Copy(source) => t!("Copy {} as a patch", source_label(source)),
            PatchOp::Apply {
                mode: Mode::Commit, ..
            } => t!("Apply patches (git am)"),
            PatchOp::Apply { .. } => t!("Apply patch"),
        }
    }

    pub fn refresh(&self) -> Refresh {
        match self {
            PatchOp::Save { .. } | PatchOp::Copy(_) => Refresh::None,
            PatchOp::Apply {
                mode: Mode::Commit, ..
            } => Refresh::All,
            PatchOp::Apply { .. } => Refresh::Status,
        }
    }

    pub fn execute(&self, git: &Git) -> JobResult {
        match self {
            PatchOp::Copy(source) => patch::create(git, source)
                .map(JobOutput::Clipboard)
                .map_err(|e| e.to_string()),
            _ => self
                .run(git)
                .map(|()| JobOutput::Done)
                .map_err(|e| e.to_string()),
        }
    }

    pub fn run(&self, git: &Git) -> ferrum_core::Result<()> {
        match self {
            PatchOp::Save { source, path } => {
                std::fs::write(path, patch::create(git, source)?)?;
                Ok(())
            }
            PatchOp::Copy(source) => patch::create(git, source).map(drop),
            PatchOp::Apply {
                text,
                mode,
                three_way,
            } => patch::apply(git, text, *mode, *three_way),
        }
    }
}

fn save_dialog(name: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_file_name(name)
        .add_filter("Patch", &["patch", "diff"])
        .save_file()
}

fn save(source: Source, name: &str, cmds: &mut Commands) {
    if let Some(path) = save_dialog(name) {
        cmds.run(Op::Patch(PatchOp::Save { source, path }));
    }
}

/// Commits in the order `git am` must apply them: oldest first.
fn oldest_first(tab: &RepoTab, ids: &[Oid]) -> Vec<Oid> {
    let mut ids = ids.to_vec();
    ids.sort_by_key(|id| std::cmp::Reverse(tab.index.get(id).copied().unwrap_or(0)));
    ids
}

// ----- Menus -----

/// Entries of a commit's context menu.
pub fn commit_menu(ui: &mut egui::Ui, id: Oid, cmds: &mut Commands) {
    let s = short(id);
    if ui.button(t!("Save {} as patch…", s)).clicked() {
        save(Source::Commits(vec![id]), &format!("{s}.patch"), cmds);
        ui.close();
    }
    if ui.button(t!("Copy {} as patch", s)).clicked() {
        cmds.run(Op::Patch(PatchOp::Copy(Source::Commits(vec![id]))));
        ui.close();
    }
}

/// Entries for several selected commits.
pub fn multi_menu(ui: &mut egui::Ui, tab: &RepoTab, many: &[Oid], cmds: &mut Commands) {
    let ids = oldest_first(tab, many);
    let n = ids.len();
    ui.separator();
    if ui.button(t!("Save {} commits as one patch…", n)).clicked() {
        save(Source::Commits(ids.clone()), "commits.patch", cmds);
        ui.close();
    }
    if ui.button(t!("Copy {} commits as patch", n)).clicked() {
        cmds.run(Op::Patch(PatchOp::Copy(Source::Commits(ids.clone()))));
        ui.close();
    }
    // Two commits: everything between them.
    if let [old, new] = ids.as_slice()
        && tab.repo.is_ancestor(*old, *new)
    {
        let (old, new) = (*old, *new);
        if ui
            .button(t!("Save range {}..{} as patch…", short(old), short(new)))
            .on_hover_text(t!("All commits after the older one, up to the newer one"))
            .clicked()
        {
            save(
                Source::Range { from: old, to: new },
                &format!("{}..{}.patch", short(old), short(new)),
                cmds,
            );
            ui.close();
        }
    }
    crate::ui::bisect::multi_menu(ui, tab, many, cmds);
}

/// "Copy as patch" for a stash ("Save as patch…" is in the stash menu).
pub fn stash_menu(ui: &mut egui::Ui, s: &StashInfo, cmds: &mut Commands) {
    if ui.button(t!("Copy as patch")).clicked() {
        cmds.run(Op::Patch(PatchOp::Copy(Source::Stash(s.clone()))));
        ui.close();
    }
}

/// The "Patches" submenu of the toolbar's ☰ menu.
pub fn menu(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    ui.menu_button(t!("Patches"), |ui| {
        ui.set_min_width(240.0);
        if ui.button(t!("Apply patch from file…")).clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Patch", &["patch", "diff", "eml", "mbox"])
                .pick_file()
            {
                cmds.push(open_file(&path));
            }
            ui.close();
        }
        if ui
            .button(t!("Apply patch from clipboard  (Ctrl+V)"))
            .on_hover_text(t!("Press Ctrl+V in the history or local changes"))
            .clicked()
        {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            ui.close();
        }
        ui.separator();
        let clean = tab.status.is_empty();
        ui.add_enabled_ui(!clean, |ui| {
            if ui.button(t!("Save local changes as patch…")).clicked() {
                save(Source::WorkingCopy, "changes.patch", cmds);
                ui.close();
            }
            if ui.button(t!("Copy local changes as patch")).clicked() {
                cmds.run(Op::Patch(PatchOp::Copy(Source::WorkingCopy)));
                ui.close();
            }
        });
    });
}

// ----- Opening patches -----

/// Reads a patch file and opens the apply dialog.
pub fn open_file(path: &Path) -> Command {
    match std::fs::read(path) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if patch::looks_like_patch(&text) {
                Command::Dialog(apply_dialog(text, name))
            } else {
                Command::Error(t!("{} is not a patch.", name))
            }
        }
        Err(e) => Command::Error(t!("Could not read {}: {}", path.display(), e)),
    }
}

/// A patch file name (dropped files that are not patches are left to others).
pub fn is_patch_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["patch", "diff", "eml"].contains(&e.to_ascii_lowercase().as_str()))
}

/// Patch files dropped on the window open the apply dialog.
pub fn dropped_files(ctx: &egui::Context, cmds: &mut Commands) {
    let files = ctx.input(|i| i.raw.dropped_files.clone());
    for f in files {
        let p = f.path();
        if is_patch_file(p) {
            cmds.push(open_file(p));
            return;
        }
    }
}

/// Ctrl+V in the history or local changes with a patch on the clipboard.
pub fn paste(ctx: &egui::Context, tab: &RepoTab, cmds: &mut Commands) {
    if !matches!(tab.view, View::History | View::Changes)
        || tab.pane.pane != crate::ui::focus::Pane::Commits
        || ctx.memory(|m| m.focused().is_some())
        || egui::Popup::is_any_open(ctx)
    {
        return;
    }
    let text = ctx.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Paste(t) if patch::looks_like_patch(t) => Some(t.clone()),
            _ => None,
        })
    });
    if let Some(text) = text {
        cmds.push(Command::Dialog(apply_dialog(text, t!("Clipboard"))));
    }
}

// ----- Apply dialog -----

pub struct PatchDialog {
    text: String,
    origin: String,
    mode: Mode,
    three_way: bool,
    /// Picked by the user; otherwise follows the check result.
    three_way_set: bool,
    check: Option<Arc<Mutex<Option<Check>>>>,
}

pub fn apply_dialog(text: String, origin: String) -> Dialog {
    let mode = if patch::is_mailbox(&text) {
        Mode::Commit
    } else {
        Mode::WorkingCopy
    };
    Dialog::Patch(PatchDialog {
        text,
        origin,
        mode,
        three_way: false,
        three_way_set: false,
        check: None,
    })
}

fn checked(d: &mut PatchDialog, ctx: &egui::Context, repo: &Path) -> Option<Check> {
    let slot = d.check.get_or_insert_with(|| {
        let slot = Arc::new(Mutex::new(None));
        let (out, git, text, ctx) = (slot.clone(), Git::new(repo), d.text.clone(), ctx.clone());
        std::thread::spawn(move || {
            let c = patch::check(&git, &text);
            if let Ok(mut s) = out.lock() {
                *s = Some(c);
            }
            ctx.request_repaint();
        });
        slot
    });
    slot.lock().ok().and_then(|s| s.clone())
}

pub fn show_dialog(
    ui: &mut egui::Ui,
    d: &mut PatchDialog,
    tab: Option<&RepoTab>,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    let Some(tab) = tab else { return false };
    ui.set_width(540.0);
    ui.heading(t!("Apply patch"));
    ui.weak(&d.origin);
    let check = checked(d, ui.ctx(), tab.path());
    let Some(c) = check else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.weak(t!("Checking the patch…"));
        });
        return buttons(ui, &t!("Apply"), false, false, false).is_none();
    };
    if !d.three_way_set {
        d.three_way = c.applies == Applies::ThreeWay;
    }
    if !c.subjects.is_empty() {
        ui.add_space(4.0);
        ui.strong(if c.subjects.len() == 1 {
            t!("1 commit")
        } else {
            t!("{} commits", c.subjects.len())
        });
        for s in c.subjects.iter().take(8) {
            ui.label(format!("• {s}"));
        }
        if c.subjects.len() > 8 {
            ui.weak(t!("{} more", c.subjects.len() - 8));
        }
    }
    ui.add_space(4.0);
    ui.strong(t!("{} files", c.files.len()));
    egui::ScrollArea::vertical()
        .max_height(160.0)
        .id_salt("patch-files")
        .show(ui, |ui| {
            let added = crate::util::added_color(ui);
            let removed = ui.visuals().error_fg_color;
            for f in &c.files {
                ui.horizontal(|ui| {
                    match (f.added, f.removed) {
                        (Some(a), Some(r)) => {
                            ui.colored_label(added, format!("+{a}"));
                            ui.colored_label(removed, format!("-{r}"));
                        }
                        _ => {
                            ui.weak(t!("binary"));
                        }
                    }
                    ui.label(RichText::new(&f.path).font(crate::util::code_font(12.0)));
                });
            }
        });
    ui.add_space(4.0);
    let warn = ui.visuals().warn_fg_color;
    match &c.applies {
        Applies::Clean => {
            ui.colored_label(
                crate::util::added_color(ui),
                t!("✔ Applies cleanly, without conflicts"),
            );
        }
        Applies::AlreadyApplied => {
            ui.colored_label(warn, t!("⚠ Its changes are already in the working copy."));
        }
        Applies::ThreeWay => {
            ui.colored_label(
                warn,
                t!("⚠ Applies only with a 3-way merge, which may leave conflicts."),
            );
        }
        Applies::No(e) => {
            ui.colored_label(ui.visuals().error_fg_color, t!("✖ Does not apply here."))
                .on_hover_text(e);
        }
    }
    ui.add_space(4.0);
    ui.add_enabled_ui(c.mailbox, |ui| {
        ui.radio_value(&mut d.mode, Mode::Commit, t!("Commit the patches (git am)"))
            .on_disabled_hover_text(t!(
                "Only patches with commit messages (from format-patch) can be committed"
            ));
    });
    ui.radio_value(
        &mut d.mode,
        Mode::WorkingCopy,
        t!("Change the files only (git apply)"),
    );
    ui.radio_value(
        &mut d.mode,
        Mode::Index,
        t!("Change and stage the files (git apply --index)"),
    );
    if ui
        .checkbox(
            &mut d.three_way,
            t!("Use a 3-way merge when it does not fit (--3way)"),
        )
        .changed()
    {
        d.three_way_set = true;
    }
    let ok = !matches!(c.applies, Applies::No(_));
    let r = buttons(ui, &t!("Apply"), ok, false, enter);
    if r == Some(true) {
        cmds.run(Op::Patch(PatchOp::Apply {
            text: d.text.clone(),
            mode: d.mode,
            three_way: d.three_way,
        }));
    }
    r.is_none()
}

/// "Applying patch 2 of 3: subject" while `git am` is stopped.
pub fn am_progress(git_dir: &Path) -> Option<(usize, usize, String)> {
    let dir = git_dir.join("rebase-apply");
    let read = |f: &str| std::fs::read_to_string(dir.join(f)).ok();
    let num = |f: &str| read(f)?.trim().parse::<usize>().ok();
    let (next, last) = (num("next")?, num("last")?);
    let subject = read("final-commit")
        .and_then(|m| m.lines().next().map(str::to_owned))
        .unwrap_or_default();
    Some((next, last, subject))
}

/// The banner text of a stopped `git am`.
pub fn banner_text(ui: &mut egui::Ui, tab: &RepoTab) {
    ui.label(t!("Applying patches"));
    if let Some((n, total, subject)) = am_progress(tab.repo.git_dir()) {
        ui.weak(t!("({} of {})", n, total));
        if !subject.is_empty() {
            ui.strong(subject);
        }
    }
}

/// Dev aid for `FERRUM_START`: `patch:FILE` opens the apply dialog.
pub fn dev_step(arg: &str, tab: Option<&RepoTab>) -> Vec<Command> {
    if tab.is_none() {
        return Vec::new();
    }
    vec![open_file(Path::new(arg))]
}
