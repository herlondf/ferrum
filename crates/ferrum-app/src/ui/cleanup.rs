//! The cleanup assistant (Insights > Cleanup): the largest files in
//! history with advice (Git LFS or removal), removing paths from history
//! with git filter-repo (backup first, typed confirmation, force-push
//! warning, guided commands when filter-repo is missing), merged branches
//! to delete and `git gc`.

use std::collections::BTreeSet;
use std::sync::mpsc::{self, Receiver};

use eframe::egui::{self, RichText};
use ferrum_core::BranchKind;
use ferrum_core::cleanup::{self, Advice, BigFile, Merged};
use ferrum_core::cli::{CancelToken, Git};

use crate::cleanup_ops::{CleanupOp, size};
use crate::command::{Command, Commands, Op};
use crate::icons;
use crate::tab::RepoTab;
use crate::theme;

/// Files listed.
const LIMIT: usize = 40;

/// What one scan finds.
pub struct Scan {
    pub files: Vec<BigFile>,
    pub filter_repo: bool,
    /// Bytes of the object database.
    pub size: u64,
}

#[derive(Default)]
pub struct CleanupState {
    scan: Option<Result<Scan, String>>,
    scanning: Option<(Receiver<Result<Scan, String>>, CancelToken)>,
    /// HEAD the scan and the merged list belong to.
    scanned_at: Option<Option<ferrum_core::Oid>>,
    picked: BTreeSet<String>,
    rewrite: Option<Rewrite>,
    base: String,
    merged: Option<Result<Vec<Merged>, String>>,
    merged_for: String,
    /// Unchecked merged branches (by full name).
    skip: BTreeSet<String>,
    remote_too: bool,
    prune_confirm: bool,
    aggressive: bool,
    /// Dev aid (`insights:rewrite`): open the rewrite dialog for the largest
    /// file once the scan is done.
    pub demo_rewrite: bool,
}

impl CleanupState {
    #[cfg(test)]
    pub fn scan(&self) -> Option<&Scan> {
        self.scan.as_ref()?.as_ref().ok()
    }

    #[cfg(test)]
    pub fn dialog_open(&self) -> bool {
        self.rewrite.is_some() || self.prune_confirm
    }

    #[cfg(test)]
    pub fn scanning(&self) -> bool {
        self.scanning.is_some()
    }
}

/// The "remove from history" confirmation.
struct Rewrite {
    paths: Vec<String>,
    typed: String,
    bundle: bool,
}

fn start_scan(tab: &mut RepoTab, ctx: &egui::Context) {
    let st = &mut tab.insights.cleanup;
    if let Some((_, c)) = st.scanning.take() {
        c.cancel();
    }
    st.scanned_at = Some(tab.head_oid);
    st.merged = None;
    let cancel = CancelToken::new();
    let git = Git::new(tab.repo.path()).with_cancel(cancel.clone());
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let r = cleanup::largest_files(&git, LIMIT)
            .map(|files| Scan {
                files,
                filter_repo: cleanup::filter_repo_available(&git),
                size: cleanup::object_size(&git).unwrap_or(0),
            })
            .map_err(|e| e.to_string());
        let _ = tx.send(r);
        ctx.request_repaint();
    });
    st.scanning = Some((rx, cancel));
}

fn poll(tab: &mut RepoTab, ctx: &egui::Context) {
    if tab.insights.cleanup.scanned_at != Some(tab.head_oid) {
        start_scan(tab, ctx);
    }
    let st = &mut tab.insights.cleanup;
    if let Some((rx, _)) = &st.scanning
        && let Ok(r) = rx.try_recv()
    {
        if let Ok(s) = &r {
            st.picked.retain(|p| s.files.iter().any(|f| &f.path == p));
            if std::mem::take(&mut st.demo_rewrite)
                && let Some(f) = s.files.iter().find(|f| f.advice() == Advice::Remove)
            {
                st.picked.insert(f.path.clone());
                st.rewrite = Some(Rewrite {
                    paths: vec![f.path.clone()],
                    typed: String::new(),
                    bundle: true,
                });
            }
        }
        st.scan = Some(r);
        st.scanning = None;
    }
    if st.base.is_empty()
        && let Some(b) = cleanup::default_base(&tab.repo)
    {
        st.base = b;
    }
    if st.merged.is_none() || st.merged_for != st.base {
        st.merged_for = st.base.clone();
        st.merged = Some(cleanup::merged_branches(&tab.repo, &st.base).map_err(|e| e.to_string()));
        st.skip.clear();
        // Branches with no commits of their own are often new: unchecked.
        if let Some(Ok(list)) = &st.merged {
            st.skip = list
                .iter()
                .filter(|m| m.at_base)
                .map(Merged::full_name)
                .collect();
        }
    }
}

pub fn show(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    poll(tab, ui.ctx());
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            large_files(ui, tab);
            ui.add_space(theme::SPACE_L);
            merged(ui, tab);
            ui.add_space(theme::SPACE_L);
            maintenance(ui, tab, cmds);
            ui.add_space(theme::SPACE_XL);
        });
    rewrite_dialog(ui.ctx(), tab, cmds);
    prune_dialog(ui.ctx(), tab, cmds);
}

fn card_title(ui: &mut egui::Ui, icon: &str, title: &str, hint: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).size(16.0).color(theme::accent(ui)));
        ui.label(theme::strong(title).size(14.0));
        if !hint.is_empty() {
            ui.weak(hint);
        }
    });
    ui.add_space(theme::SPACE_M);
}

fn advice_badge(ui: &mut egui::Ui, a: Advice) {
    let (text, color) = match a {
        Advice::Lfs => (t!("Use Git LFS"), theme::accent(ui)),
        Advice::Remove => (t!("Remove from history"), ui.visuals().warn_fg_color),
        Advice::Keep => (t!("Fine"), ui.visuals().weak_text_color()),
    };
    let g = ui
        .painter()
        .layout_no_wrap(text, theme::strong_font(10.5), color);
    let (rect, _) = ui.allocate_exact_size(g.size() + egui::vec2(12.0, 4.0), egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        8.0,
        egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 30),
    );
    ui.painter()
        .galley(rect.min + egui::vec2(6.0, 2.0), g, color);
}

fn large_files(ui: &mut egui::Ui, tab: &mut RepoTab) {
    let ctx = ui.ctx().clone();
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        card_title(
            ui,
            icons::PACKAGE,
            &t!("Largest files in history"),
            &t!("every version ever committed, on any branch"),
        );
        let st = &mut tab.insights.cleanup;
        let scan = match &st.scan {
            _ if st.scanning.is_some() => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak(t!("Looking through every object…"));
                    if ui.small_button(t!("Cancel")).clicked()
                        && let Some((_, c)) = &st.scanning
                    {
                        c.cancel();
                    }
                });
                return;
            }
            None => return,
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
                if ui.button(t!("Try again")).clicked() {
                    st.scanned_at = None;
                }
                return;
            }
            Some(Ok(s)) => s,
        };
        if scan.files.is_empty() {
            ui.weak(t!("No files in history yet."));
            return;
        }
        ui.horizontal(|ui| {
            ui.label(t!("Objects on disk: {}", size(scan.size)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(icons::icon_button(icons::REFRESH).frame_when_inactive(false))
                    .on_hover_text(t!("Scan again"))
                    .clicked()
                {
                    st.scanned_at = None;
                    ctx.request_repaint();
                }
            });
        });
        ui.add_space(theme::SPACE_S);
        let path_w = (ui.available_width() - 470.0).max(160.0);
        egui::Grid::new("big-files")
            .num_columns(7)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label("");
                for h in [
                    t!("Path"),
                    t!("Largest"),
                    t!("Versions"),
                    t!("Commits"),
                    t!("In HEAD"),
                    t!("Advice"),
                ] {
                    ui.label(RichText::new(h).small().weak());
                }
                ui.end_row();
                for f in &scan.files {
                    let mut on = st.picked.contains(&f.path);
                    if ui.checkbox(&mut on, "").changed() {
                        if on {
                            st.picked.insert(f.path.clone());
                        } else {
                            st.picked.remove(&f.path);
                        }
                    }
                    ui.horizontal(|ui| {
                        ui.set_width(path_w);
                        ui.add(
                            egui::Label::new(RichText::new(&f.path).font(crate::util::code_font(12.0)))
                                .truncate(),
                        )
                        .on_hover_text(t!("All versions: {}", size(f.total)));
                    });
                    ui.label(theme::strong(size(f.size)));
                    ui.label(f.versions.to_string());
                    ui.label(f.commits.to_string());
                    if f.in_head {
                        ui.label(RichText::new(icons::CHECK).color(crate::util::added_color(ui)));
                    } else {
                        ui.weak(t!("deleted"));
                    }
                    advice_badge(ui, f.advice());
                    ui.end_row();
                }
            });
        ui.add_space(theme::SPACE_M);
        let picked: Vec<String> = st.picked.iter().cloned().collect();
        ui.horizontal(|ui| {
            let n = picked.len();
            let label = icons::text(icons::TRASH, t!("Remove {} from history…", n));
            if ui
                .add_enabled(n > 0, theme::danger(label))
                .on_disabled_hover_text(t!("Check the files to remove first."))
                .clicked()
            {
                st.rewrite = Some(Rewrite {
                    paths: picked.clone(),
                    typed: String::new(),
                    bundle: true,
                });
            }
            if n == 0 {
                ui.weak(t!("Check files to remove them from every commit."));
            }
        });
        // Git LFS for what is still used.
        let lfs: BTreeSet<String> = scan
            .files
            .iter()
            .filter(|f| f.advice() == Advice::Lfs)
            .map(|f| cleanup::lfs_pattern(&f.path))
            .collect();
        if !lfs.is_empty() {
            ui.add_space(theme::SPACE_M);
            ui.horizontal_top(|ui| {
                ui.label(RichText::new(icons::LIGHTBULB).color(theme::accent(ui)));
                ui.vertical(|ui| {
                    ui.label(t!(
                        "Binary files that are still used belong in Git LFS. To move them, rewriting history too (back up first):"
                    ));
                    let list: Vec<String> = lfs.into_iter().collect();
                    let cmd = format!(
                        "git lfs migrate import --everything --include=\"{}\"",
                        list.join(",")
                    );
                    command_box(ui, &cmd);
                });
            });
        }
    });
}

/// A command in a code box with a Copy button.
fn command_box(ui: &mut egui::Ui, text: &str) {
    let v = ui.visuals().clone();
    egui::Frame::new()
        .fill(v.extreme_bg_color)
        .stroke(v.widgets.noninteractive.bg_stroke)
        .corner_radius(theme::RADIUS)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                let w = ui.available_width() - 40.0;
                ui.allocate_ui(egui::vec2(w, 0.0), |ui| {
                    ui.set_width(w);
                    let text = RichText::new(text).font(crate::util::code_font(12.0));
                    ui.add(egui::Label::new(text).wrap());
                });
                if ui
                    .add(icons::icon_button(icons::COPY).frame_when_inactive(false))
                    .on_hover_text(t!("Copy"))
                    .clicked()
                {
                    ui.ctx().copy_text(text.to_owned());
                }
            });
        });
}

fn merged(ui: &mut egui::Ui, tab: &mut RepoTab) {
    let locals: Vec<String> = tab
        .branches
        .iter()
        .filter(|b| b.kind == BranchKind::Local)
        .map(|b| b.name.clone())
        .collect();
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        card_title(
            ui,
            icons::BRANCH,
            &t!("Merged branches"),
            &t!("all their commits are already in the base branch"),
        );
        let st = &mut tab.insights.cleanup;
        ui.horizontal(|ui| {
            ui.label(t!("Base branch"));
            egui::ComboBox::from_id_salt("cleanup-base")
                .selected_text(&st.base)
                .show_ui(ui, |ui| {
                    for b in &locals {
                        ui.selectable_value(&mut st.base, b.clone(), b);
                    }
                });
            ui.checkbox(&mut st.remote_too, t!("Remote branches too"));
        });
        ui.add_space(theme::SPACE_S);
        let list = match &st.merged {
            Some(Ok(l)) => l,
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
                return;
            }
            None => return,
        };
        let shown: Vec<&Merged> = list
            .iter()
            .filter(|m| st.remote_too || m.remote.is_none())
            .collect();
        if shown.is_empty() {
            ui.weak(t!("No merged branches. Nothing to clean up."));
            return;
        }
        let now = crate::util::now_secs();
        for m in &shown {
            let name = m.full_name();
            let mut on = !st.skip.contains(&name);
            ui.horizontal(|ui| {
                if ui.checkbox(&mut on, "").changed() {
                    if on {
                        st.skip.remove(&name);
                    } else {
                        st.skip.insert(name.clone());
                    }
                }
                let icon = if m.remote.is_some() {
                    icons::REMOTE
                } else {
                    icons::BRANCH
                };
                ui.label(icons::text(icon, &name));
                ui.weak(crate::util::relative_time(m.time, now));
                if m.at_base {
                    ui.weak(t!("(no commits of its own)"));
                }
            });
        }
        let n = shown
            .iter()
            .filter(|m| !st.skip.contains(&m.full_name()))
            .count();
        ui.add_space(theme::SPACE_S);
        if ui
            .add_enabled(
                n > 0,
                theme::danger(icons::text(icons::TRASH, t!("Delete {} branches…", n))),
            )
            .clicked()
        {
            st.prune_confirm = true;
        }
    });
}

fn chosen(st: &CleanupState) -> Vec<Merged> {
    match &st.merged {
        Some(Ok(l)) => l
            .iter()
            .filter(|m| st.remote_too || m.remote.is_none())
            .filter(|m| !st.skip.contains(&m.full_name()))
            .cloned()
            .collect(),
        _ => Vec::new(),
    }
}

fn maintenance(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    let busy = tab.busy();
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        card_title(
            ui,
            icons::HARD_DRIVES,
            &t!("Maintenance"),
            &t!("pack objects and drop unreachable ones (git gc)"),
        );
        let st = &mut tab.insights.cleanup;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !busy,
                    theme::secondary(icons::text(icons::CLEANUP, t!("Compact now"))),
                )
                .clicked()
            {
                cmds.run(Op::Cleanup(CleanupOp::Gc {
                    aggressive: st.aggressive,
                }));
                st.scanned_at = None;
            }
            ui.checkbox(&mut st.aggressive, t!("Aggressive (slower, smaller)"));
        });
        ui.weak(t!(
            "Objects still kept by backups, restore points or the reflog are not removed."
        ));
    });
}

fn rewrite_dialog(ctx: &egui::Context, tab: &mut RepoTab, cmds: &mut Commands) {
    let name = tab.name.clone();
    let dirty = !tab.status.staged.is_empty()
        || tab
            .status
            .unstaged
            .iter()
            .any(|f| f.kind != ferrum_core::ChangeKind::Untracked);
    let bundle_path = cleanup::bundle_path(&tab.repo);
    let st = &mut tab.insights.cleanup;
    let filter_repo = st
        .scan
        .as_ref()
        .and_then(|s| s.as_ref().ok())
        .is_some_and(|s| s.filter_repo);
    let Some(rw) = &mut st.rewrite else { return };
    let mut close = false;
    let mut run = false;
    let modal = egui::Modal::new(egui::Id::unique("cleanup-rewrite"))
        .frame(theme::dialog_frame(&ctx.global_style()))
        .show(ctx, |ui| {
            ui.set_width(540.0);
            ui.set_max_width(540.0);
            let warn = ui.visuals().warn_fg_color;
            ui.horizontal(|ui| {
                ui.label(RichText::new(icons::WARNING).size(22.0).color(warn));
                ui.label(theme::strong(t!("Remove from history")).size(16.0));
            });
            ui.add_space(theme::SPACE_M);
            ui.label(t!("These paths are removed from every commit of every local branch and tag:"));
            egui::ScrollArea::vertical()
                .max_height(110.0)
                .show(ui, |ui| {
                    for p in &rw.paths {
                        ui.label(RichText::new(p).font(crate::util::code_font(12.0)));
                    }
                });
            ui.add_space(theme::SPACE_M);
            theme::card(ui).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new(icons::WARNING).color(warn));
                    ui.add(egui::Label::new(t!(
                        "This rewrites history: commit hashes change. The server keeps the old history until you force push every branch and tag (git push --force-with-lease --all, then --tags). Everyone else must clone again or reset their branches, and open pull requests may need to be recreated."
                    )).wrap());
                });
            });
            ui.add_space(theme::SPACE_M);
            if !filter_repo {
                ui.label(theme::strong(t!("git filter-repo is not installed")));
                ui.label(t!(
                    "Install it (pip install git-filter-repo, or your package manager) and scan again, or run these commands in a terminal:"
                ));
                let guided = cleanup::guided_commands(&rw.paths, &bundle_path.to_string_lossy());
                command_box(ui, &guided);
                ui.add_space(theme::SPACE_S);
                ui.horizontal(|ui| {
                    if ui.button(icons::text(icons::TERMINAL, t!("Open terminal"))).clicked() {
                        cmds.push(Command::External(crate::external::Launch::Terminal {
                            dir: tab_dir(&bundle_path),
                        }));
                    }
                    if ui.button(t!("Close")).clicked() {
                        close = true;
                    }
                });
                return;
            }
            ui.label(RichText::new(icons::text(icons::ARCHIVE, t!("Backup first"))).strong());
            ui.label(t!(
                "The branch tips are saved under refs/ferrum/backup/filter-… first; the timeline window (Backups) puts them back. A restore point is taken too."
            ));
            ui.checkbox(
                &mut rw.bundle,
                t!("Also save the whole repository as a bundle file"),
            );
            if rw.bundle {
                ui.weak(bundle_path.display().to_string());
            }
            if dirty {
                ui.add_space(theme::SPACE_S);
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    t!("Commit or stash your local changes before rewriting history."),
                );
            }
            ui.add_space(theme::SPACE_M);
            ui.label(t!("Type the repository name ({}) to confirm:", name));
            ui.add(
                egui::TextEdit::singleline(&mut rw.typed)
                    .hint_text(&name)
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(theme::SPACE_M);
            ui.horizontal(|ui| {
                let ok = rw.typed.trim() == name && !dirty;
                if ui
                    .add_enabled(ok, theme::danger(t!("Rewrite history")))
                    .clicked()
                {
                    run = true;
                }
                if ui.button(t!("Cancel")).clicked() {
                    close = true;
                }
            });
        });
    if modal.should_close() {
        close = true;
    }
    if run {
        let bundle = rw.bundle.then(|| bundle_path.clone());
        cmds.run(Op::Cleanup(CleanupOp::RemovePaths {
            paths: rw.paths.clone(),
            bundle,
        }));
        st.picked.clear();
        close = true;
    }
    if close {
        st.rewrite = None;
    }
}

/// The repository folder from a path in its git folder (for the terminal).
fn tab_dir(bundle: &std::path::Path) -> std::path::PathBuf {
    bundle
        .ancestors()
        .find(|a| a.file_name().is_some_and(|n| n == ".git"))
        .and_then(|g| g.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| bundle.to_path_buf())
}

fn prune_dialog(ctx: &egui::Context, tab: &mut RepoTab, cmds: &mut Commands) {
    let st = &mut tab.insights.cleanup;
    if !st.prune_confirm {
        return;
    }
    let list = chosen(st);
    let remote = list.iter().filter(|m| m.remote.is_some()).count();
    let mut close = false;
    let mut run = false;
    let modal = egui::Modal::new(egui::Id::unique("cleanup-prune"))
        .frame(theme::dialog_frame(&ctx.global_style()))
        .show(ctx, |ui| {
            ui.set_width(460.0);
            ui.set_max_width(460.0);
            ui.label(theme::strong(t!("Delete {} merged branches?", list.len())).size(16.0));
            ui.add_space(theme::SPACE_M);
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .show(ui, |ui| {
                    for m in &list {
                        let icon = if m.remote.is_some() {
                            icons::REMOTE
                        } else {
                            icons::BRANCH
                        };
                        ui.label(icons::text(icon, m.full_name()));
                    }
                });
            if remote > 0 {
                ui.add_space(theme::SPACE_S);
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    t!(
                        "{} remote branches are deleted on the server, for everyone.",
                        remote
                    ),
                );
            }
            ui.weak(t!(
                "Their commits stay in the base branch; a restore point brings the local ones back."
            ));
            ui.add_space(theme::SPACE_M);
            ui.horizontal(|ui| {
                if ui.add(theme::danger(t!("Delete"))).clicked() {
                    run = true;
                }
                if ui.button(t!("Cancel")).clicked() {
                    close = true;
                }
            });
        });
    if modal.should_close() {
        close = true;
    }
    if run {
        cmds.run(Op::Cleanup(CleanupOp::PruneBranches(list)));
        st.merged = None;
        close = true;
    }
    if close {
        st.prune_confirm = false;
    }
}
