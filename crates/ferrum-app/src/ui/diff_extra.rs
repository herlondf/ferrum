//! Picks how a file's change is shown: text, image, binary sizes, Git LFS
//! pointer, submodule commits or a "load diff" button; and the large diff
//! window opened with Space.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Id};
use ferrum_core::diff::{self, LfsPointer, SubmoduleCommits};
use ferrum_core::{DiffSource, FileDiff, Oid, Repo, short};

use crate::tab::LineSelection;
use crate::ui::diff_view::{self, DiffAction, DiffMode, LARGE_SUFFIX};
use crate::ui::{diff_prefs, image_diff};
use crate::util::format_time;

/// Shows the change of one file, whatever its kind.
#[allow(clippy::too_many_arguments)]
pub fn show_file(
    ui: &mut egui::Ui,
    id: &str,
    repo: &Repo,
    source: DiffSource,
    file: &FileDiff,
    mode: DiffMode,
    line_staging: bool,
    mut selection: Option<&mut LineSelection>,
) -> Option<DiffAction> {
    let ctx = ui.ctx().clone();
    let mut action = content(
        ui,
        id,
        repo,
        source,
        file,
        mode,
        line_staging,
        selection.as_deref_mut(),
    );

    // Space opens the large window when no text field takes the key.
    let typing = ctx.text_edit_focused();
    let open = diff_view::large_open(&ctx, id);
    if !typing && !open && crate::keymap::pressed(&ctx, crate::keymap::Action::LargeView) {
        diff_view::set_large(&ctx, id, true);
    }
    if diff_view::large_open(&ctx, id) {
        let large_id = format!("{id}{LARGE_SUFFIX}");
        let screen = ctx.content_rect();
        let resp = egui::Modal::new(Id::unique(("diff-large-window", id))).show(&ctx, |ui| {
            ui.set_width(screen.width() * 0.92);
            ui.set_height(screen.height() * 0.88);
            let mut close = false;
            ui.horizontal(|ui| {
                ui.strong(&file.path);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.button(t!("Close")).clicked();
                    ui.weak(t!("Space or Esc closes this window"));
                });
            });
            ui.separator();
            let a = content(
                ui,
                &large_id,
                repo,
                source,
                file,
                mode,
                line_staging,
                selection,
            );
            diff_view::mark_active(ui.ctx(), &large_id);
            (a, close)
        });
        let (a, close) = resp.inner;
        if a.is_some() {
            action = a;
        }
        let space = !ctx.text_edit_focused()
            && crate::keymap::pressed(&ctx, crate::keymap::Action::LargeView);
        if close || space || resp.should_close() {
            diff_view::set_large(&ctx, id, false);
        }
    }
    action
}

#[allow(clippy::too_many_arguments)]
fn content(
    ui: &mut egui::Ui,
    id: &str,
    repo: &Repo,
    source: DiffSource,
    file: &FileDiff,
    mode: DiffMode,
    line_staging: bool,
    selection: Option<&mut LineSelection>,
) -> Option<DiffAction> {
    if let Some(size) = file.skipped {
        return large_untracked(ui, size);
    }
    if diff::is_submodule(file) {
        submodule(ui, repo, file);
        return None;
    }
    let (old_ptr, new_ptr) = diff::lfs_pointers(file);
    if old_ptr.is_some() || new_ptr.is_some() {
        lfs(ui, repo, file, old_ptr, new_ptr);
        return None;
    }
    let is_svg = file.path.to_ascii_lowercase().ends_with(".svg");
    if is_svg {
        let mut prefs = diff_prefs::get(ui.ctx());
        let before = prefs.svg_as_image;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut prefs.svg_as_image, false, t!("Text"));
            ui.selectable_value(&mut prefs.svg_as_image, true, t!("Image"));
        });
        if prefs.svg_as_image != before {
            diff_prefs::set(ui.ctx(), prefs.clone());
        }
        if prefs.svg_as_image {
            image_diff::show(ui, repo, file, source);
            return None;
        }
    } else if file.binary && image_diff::is_image(&file.path) {
        image_diff::show(ui, repo, file, source);
        return None;
    }
    if file.binary {
        binary(ui, repo, file, source);
        return None;
    }
    let structural = crate::ui::structural_view::Source::new(repo, source, file);
    diff_view::show_with(
        ui,
        id,
        file,
        mode,
        line_staging,
        selection,
        structural.as_ref(),
    )
}

fn large_untracked(ui: &mut egui::Ui, size: u64) -> Option<DiffAction> {
    let mut action = None;
    ui.add_space(8.0);
    ui.label(t!(
        "This untracked file is large ({}), so its diff was not loaded.",
        format_size(size)
    ));
    if ui.button(t!("Load diff")).clicked() {
        action = Some(DiffAction::LoadFull);
    }
    action
}

pub fn format_size(bytes: u64) -> String {
    match bytes {
        b if b < 1024 => format!("{b} B"),
        b if b < 1024 * 1024 => format!("{:.1} KB", b as f64 / 1024.0),
        b if b < 1024 * 1024 * 1024 => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
        b => format!("{:.2} GB", b as f64 / (1024.0 * 1024.0 * 1024.0)),
    }
}

fn exact_bytes(bytes: u64) -> String {
    t!("{} bytes", bytes)
}

/// Old and new size of a binary file, with the difference.
fn binary(ui: &mut egui::Ui, repo: &Repo, file: &FileDiff, source: DiffSource) {
    let key = Id::unique((
        "binary-sizes",
        &file.path,
        file.old_id,
        file.new_id,
        format!("{source:?}"),
    ));
    // Working-copy sizes change without new ids; re-read them each second.
    let now = ui.input(|i| i.time);
    let cached = ui
        .data(|d| d.get_temp::<(f64, Option<u64>, Option<u64>)>(key))
        .filter(|(t, _, _)| now - t < 1.0);
    let (old, new) = match cached {
        Some((_, o, n)) => (o, n),
        None => {
            let (o, n) = repo.diff_sizes(file, source);
            ui.data_mut(|d| d.insert_temp(key, (now, o, n)));
            (o, n)
        }
    };
    ui.add_space(8.0);
    ui.label(t!("Binary file."));
    let side = |ui: &mut egui::Ui, label: String, size: Option<u64>| {
        ui.horizontal(|ui| {
            ui.weak(label);
            match size {
                Some(s) => {
                    ui.label(format_size(s)).on_hover_text(exact_bytes(s));
                }
                None => {
                    ui.weak("—");
                }
            }
        });
    };
    side(ui, t!("Before"), old);
    side(ui, t!("After"), new);
    if let (Some(o), Some(n)) = (old, new) {
        let delta = n as i64 - o as i64;
        let sign = if delta >= 0 { "+" } else { "−" };
        ui.label(t!(
            "Size change: {}{}",
            sign,
            format_size(delta.unsigned_abs())
        ))
        .on_hover_text(t!("{} bytes -> {} bytes", o, n));
    }
}

// ----- Submodules -----

type SubResult = Arc<Result<SubmoduleCommits, String>>;

fn submodule(ui: &mut egui::Ui, repo: &Repo, file: &FileDiff) {
    let (old, new) = diff::submodule_ids(file);
    let fmt = |id: Option<Oid>| id.map(short).unwrap_or_else(|| "—".into());
    ui.add_space(4.0);
    ui.label(t!("Submodule {}: {} -> {}", file.path, fmt(old), fmt(new)));
    let key = Id::unique(("submodule-commits", &file.path, old, new));
    let result = match ui.data(|d| d.get_temp::<SubResult>(key)) {
        Some(r) => r,
        None => {
            let r = Arc::new(
                repo.submodule_commits(&file.path, old, new)
                    .map_err(|e| e.to_string()),
            );
            ui.data_mut(|d| d.insert_temp(key, r.clone()));
            r
        }
    };
    match &*result {
        Err(e) => {
            ui.colored_label(ui.visuals().warn_fg_color, e);
        }
        Ok(c) => {
            egui::ScrollArea::vertical()
                .id_salt("submodule-commits")
                .auto_shrink(false)
                .show(ui, |ui| {
                    if c.added.is_empty() && c.removed.is_empty() {
                        ui.weak(t!("No commits between the two revisions."));
                    }
                    let list =
                        |ui: &mut egui::Ui, title: String, commits: &[ferrum_core::CommitInfo]| {
                            if commits.is_empty() {
                                return;
                            }
                            ui.add_space(6.0);
                            ui.strong(title);
                            egui::Grid::new(ui.next_auto_id())
                                .num_columns(4)
                                .spacing([12.0, 3.0])
                                .striped(true)
                                .show(ui, |ui| {
                                    for c in commits {
                                        ui.monospace(c.short_id());
                                        ui.label(&c.summary);
                                        ui.weak(&c.author);
                                        ui.weak(format_time(c.time, c.offset_minutes));
                                        ui.end_row();
                                    }
                                });
                        };
                    list(ui, t!("Commits added ({})", c.added.len()), &c.added);
                    list(ui, t!("Commits removed ({})", c.removed.len()), &c.removed);
                    if c.truncated {
                        ui.weak(t!("Only the first commits are listed."));
                    }
                });
        }
    }
}

// ----- Git LFS -----

/// Content of one LFS object: `None` while it downloads.
type LfsSlot = Arc<Mutex<Option<Result<Arc<Vec<u8>>, String>>>>;

/// Content fetched with `git lfs smudge`, per pointer oid.
#[derive(Clone, Default)]
struct LfsCache {
    available: Option<bool>,
    content: HashMap<String, LfsSlot>,
}

fn lfs_cache_id() -> Id {
    Id::unique("lfs-cache")
}

/// Pointer details with download / preview / save of the content. Public so
/// other views showing a file at a revision (file tree) can reuse it.
pub fn lfs(
    ui: &mut egui::Ui,
    repo: &Repo,
    file: &FileDiff,
    old: Option<LfsPointer>,
    new: Option<LfsPointer>,
) {
    let git = repo.git();
    let mut cache = ui
        .data(|d| d.get_temp::<LfsCache>(lfs_cache_id()))
        .unwrap_or_default();
    let available = *cache
        .available
        .get_or_insert_with(|| diff::lfs_available(&git));
    ui.add_space(4.0);
    ui.strong(t!("Git LFS file"));
    egui::Grid::new("lfs-pointers")
        .num_columns(3)
        .spacing([12.0, 3.0])
        .show(ui, |ui| {
            for (label, ptr) in [(t!("Before"), &old), (t!("After"), &new)] {
                ui.weak(label);
                match ptr {
                    Some(p) => {
                        ui.label(format_size(p.size))
                            .on_hover_text(exact_bytes(p.size));
                        ui.monospace(&p.oid);
                    }
                    None => {
                        ui.weak("—");
                        ui.label("");
                    }
                }
                ui.end_row();
            }
        });
    if !available {
        ui.weak(t!("Install Git LFS to download and preview the content."));
        ui.data_mut(|d| d.insert_temp(lfs_cache_id(), cache));
        return;
    }
    let slot = |cache: &LfsCache, p: &Option<LfsPointer>| {
        p.as_ref().and_then(|p| cache.content.get(&p.oid).cloned())
    };
    let loaded = [slot(&cache, &old), slot(&cache, &new)];
    if loaded.iter().all(Option::is_none) {
        if ui
            .button(t!("Download / preview"))
            .on_hover_text(t!("Fetch the content with git lfs (may download)"))
            .clicked()
        {
            for p in [&old, &new].into_iter().flatten() {
                let cell: Arc<Mutex<Option<_>>> = Arc::default();
                cache.content.insert(p.oid.clone(), cell.clone());
                let (git, path, p) = (git.clone(), file.path.clone(), p.clone());
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let r = diff::lfs_smudge(&git, &path, &p)
                        .map(Arc::new)
                        .map_err(|e| e.to_string());
                    *cell.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
                    ctx.request_repaint();
                });
            }
        }
        ui.data_mut(|d| d.insert_temp(lfs_cache_id(), cache));
        return;
    }
    let get = |s: &Option<LfsSlot>| {
        s.as_ref()
            .map(|c| c.lock().unwrap_or_else(|e| e.into_inner()).clone())
    };
    let (before, after) = (get(&loaded[0]), get(&loaded[1]));
    ui.data_mut(|d| d.insert_temp(lfs_cache_id(), cache));
    if matches!(before, Some(None)) || matches!(after, Some(None)) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.weak(t!("Downloading…"));
        });
        return;
    }
    let bytes = |r: Option<Option<Result<Arc<Vec<u8>>, String>>>, ui: &mut egui::Ui| match r {
        Some(Some(Ok(b))) => Some(b),
        Some(Some(Err(e))) => {
            ui.colored_label(ui.visuals().error_fg_color, t!("git lfs failed: {}", e));
            None
        }
        _ => None,
    };
    let (before, after) = (bytes(before, ui), bytes(after, ui));
    if let Some(b) = after.as_ref().or(before.as_ref()) {
        save_button(ui, &file.path, b);
    }
    ui.separator();
    if image_diff::is_image(&file.path) {
        image_diff::show_bytes(
            ui,
            &file.path,
            before
                .as_deref()
                .map(|b| (old.as_ref().map(|p| p.oid.as_str()).unwrap_or(""), &b[..])),
            after
                .as_deref()
                .map(|b| (new.as_ref().map(|p| p.oid.as_str()).unwrap_or(""), &b[..])),
        );
        return;
    }
    // Text content: show the new version (or the old one, if deleted).
    let shown = after.or(before);
    let Some(b) = shown else { return };
    let text = (!b.contains(&0)).then(|| String::from_utf8_lossy(&b[..b.len().min(256 * 1024)]));
    match text {
        Some(t) => {
            egui::ScrollArea::both()
                .id_salt("lfs-preview")
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(t).monospace()).extend());
                });
        }
        None => {
            ui.label(t!("Binary content ({}).", format_size(b.len() as u64)))
                .on_hover_text(exact_bytes(b.len() as u64));
        }
    }
}

/// Saves downloaded LFS content to a file chosen by the user.
fn save_button(ui: &mut egui::Ui, path: &str, bytes: &[u8]) {
    let id = Id::unique(("lfs-save-error", path));
    if ui
        .button(t!("Save as…"))
        .on_hover_text(t!("Save the content to a file"))
        .clicked()
    {
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(dest) = rfd::FileDialog::new().set_file_name(name).save_file() {
            let err = std::fs::write(&dest, bytes).err().map(|e| e.to_string());
            ui.data_mut(|d| d.insert_temp(id, err));
        }
    }
    if let Some(Some(e)) = ui.data(|d| d.get_temp::<Option<String>>(id)) {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
}
