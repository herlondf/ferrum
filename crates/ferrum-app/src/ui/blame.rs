//! File history (commits that touched a file or folder, with their diff),
//! blame (see `blame_view`) and the history of a line range (`file_lines`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use eframe::egui::{self, Align2, FontId, Rect, Sense, pos2, vec2};
use ferrum_core::diff::DiffConfig;
use ferrum_core::{BranchKind, DiffSource, FileCommit, FileDiff, Oid, Repo, short};

use crate::command::{Command, Commands, Op};
use crate::external::{self, Launch};
use crate::tab::{RepoTab, View};
use crate::ui::dialogs::Dialog;
use crate::ui::{blame_view, diff_extra, diff_view, file_lines};
use crate::util::format_date;

const HISTORY_ROW: f32 = 42.0;

/// Data loaded on a background thread, with its own repository handle.
#[derive(Default)]
pub(crate) enum Load<T> {
    #[default]
    Idle,
    Pending(Receiver<Result<T, String>>),
    Done(Result<T, String>),
}

impl<T: Send + 'static> Load<T> {
    pub(crate) fn start(
        &mut self,
        repo: &Repo,
        work: impl FnOnce(&Repo) -> Result<T, String> + Send + 'static,
    ) {
        let path = repo.path().to_path_buf();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = Repo::open(&path)
                .map_err(|e| e.to_string())
                .and_then(|r| work(&r));
            let _ = tx.send(result);
        });
        *self = Load::Pending(rx);
    }

    /// The result once loaded; keeps the UI repainting while waiting.
    pub(crate) fn poll(&mut self, ctx: &egui::Context) -> Option<&Result<T, String>> {
        if let Load::Pending(rx) = self {
            match rx.try_recv() {
                Ok(r) => *self = Load::Done(r),
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(50))
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    *self = Load::Done(Err(t!("Loading stopped unexpectedly.")))
                }
            }
        }
        match self {
            Load::Done(r) => Some(r),
            _ => None,
        }
    }

    pub(crate) fn ok(&self) -> Option<&T> {
        match self {
            Load::Done(Ok(v)) => Some(v),
            _ => None,
        }
    }

    pub(crate) fn is_idle(&self) -> bool {
        matches!(self, Load::Idle)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    History,
    Blame,
    /// History of a line range.
    Lines,
}

/// What a diff in the history shows: one file between two commits.
#[derive(Clone, PartialEq)]
pub(crate) struct DiffKey {
    pub old: Option<Oid>,
    pub new: Oid,
    pub path: String,
    pub old_path: Option<String>,
    cfg: DiffConfig,
}

pub struct FileView {
    /// Unique per view, for widget ids and keyboard routing.
    pub(crate) id: u64,
    pub path: String,
    pub is_folder: bool,
    pub mode: Mode,
    /// Branch or ref the history and blame are relative to (`None`: HEAD).
    pub base: Option<String>,
    pub(crate) history: Load<Vec<FileCommit>>,
    history_base: Option<Option<String>>,
    pub(crate) selected: Option<usize>,
    /// A second entry picked with Ctrl+click: the diff compares the two.
    pub(crate) other: Option<usize>,
    diff: Load<Option<FileDiff>>,
    pub(crate) diff_key: Option<DiffKey>,
    /// Folders: the files the selected commits changed, and the one shown.
    files: Load<Vec<FileDiff>>,
    files_key: Option<(Option<Oid>, Oid)>,
    file_sel: Option<usize>,
    pub(crate) blame: blame_view::BlameState,
    pub(crate) lines: Option<file_lines::LinesState>,
}

/// What a view asks its owner to do after drawing.
#[derive(Default, PartialEq, Eq)]
pub(crate) struct Outcome {
    /// Move the view to a separate window.
    pub detach: bool,
    /// Close the view (its window).
    pub close: bool,
}

/// Where a view is drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    Main,
    Window,
}

impl FileView {
    pub fn new(repo: &Repo, path: String, mode: Mode) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = path.trim_end_matches('/').to_owned();
        let is_folder = repo.is_folder("HEAD", &path);
        let mode = if is_folder && mode == Mode::Blame {
            Mode::History
        } else {
            mode
        };
        FileView {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            blame: blame_view::BlameState::new(&path),
            path,
            is_folder,
            mode,
            base: None,
            history: Load::Idle,
            history_base: None,
            selected: None,
            other: None,
            diff: Load::Idle,
            diff_key: None,
            files: Load::Idle,
            files_key: None,
            file_sel: None,
            lines: None,
        }
    }

    /// The revision history and blame start from.
    pub(crate) fn base_rev(&self) -> String {
        self.base.clone().unwrap_or_else(|| "HEAD".into())
    }

    /// Id of the diff view inside this view.
    pub(crate) fn diff_id(&self, place: Place) -> String {
        match place {
            Place::Main => "file-history-diff".into(),
            Place::Window => format!("file-history-diff-w{}", self.id),
        }
    }

    pub(crate) fn entries(&self) -> &[FileCommit] {
        self.history.ok().map_or(&[], Vec::as_slice)
    }

    /// Selects the history entry of a commit; returns whether it is listed.
    pub(crate) fn select_commit(&mut self, id: Oid) -> bool {
        let Some(i) = self.entries().iter().position(|e| e.commit.id == id) else {
            return false;
        };
        self.selected = Some(i);
        self.other = None;
        self.mode = Mode::History;
        true
    }

    fn reload(&mut self) {
        self.history = Load::Idle;
        self.history_base = None;
        self.diff = Load::Idle;
        self.diff_key = None;
        self.files = Load::Idle;
        self.files_key = None;
        self.blame.reload();
        if let Some(l) = &mut self.lines {
            l.reload();
        }
    }
}

pub fn open(tab: &mut RepoTab, path: String, blame: bool) {
    open_mode(tab, path, if blame { Mode::Blame } else { Mode::History });
}

pub fn open_mode(tab: &mut RepoTab, path: String, mode: Mode) {
    let path = path.trim_end_matches('/').to_owned();
    match &mut tab.file_view {
        Some(v) if v.path == path => {
            if !(v.is_folder && mode == Mode::Blame) {
                v.mode = mode;
            }
        }
        _ => tab.file_view = Some(FileView::new(&tab.repo, path, mode)),
    }
    tab.view = View::File;
}

/// The file view of the main window.
pub fn show(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    let Some(mut view) = tab.file_view.take() else {
        return;
    };
    let out = show_view(ui, tab, &mut view, cmds, Place::Main);
    tab.file_view = Some(view);
    if out.detach {
        // After the frame: the window starts drawing on the next one.
        cmds.push(crate::ui::history_options::cmd(|t, _| {
            if let Some(v) = t.file_view.take() {
                crate::ui::file_window::detach(t, v);
                t.view = View::History;
            }
        }));
    }
}

/// Draws a file view (in the main window or a separate one).
pub(crate) fn show_view(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    view: &mut FileView,
    cmds: &mut Commands,
    place: Place,
) -> Outcome {
    let mut out = Outcome::default();
    // The history is needed by every mode (the blame shows it as a timeline).
    if view.history_base.as_ref() != Some(&view.base) {
        view.history_base = Some(view.base.clone());
        view.selected = None;
        view.other = None;
        let (path, rev) = (view.path.clone(), view.base_rev());
        view.history.start(&tab.repo, move |r| {
            r.file_history_at(&path, &rev).map_err(|e| e.to_string())
        });
    }
    view.history.poll(ui.ctx());

    toolbar(ui, tab, view, cmds, place, &mut out);
    ui.separator();

    match view.mode {
        Mode::Blame => blame_view::show(ui, tab, view, cmds),
        Mode::Lines => match &mut view.lines {
            Some(l) => {
                let id = format!("line-history-diff-{}", view.id);
                file_lines::show(ui, &tab.repo, l, cmds, &id);
            }
            None => view.mode = Mode::History,
        },
        Mode::History => history_view(ui, tab, view, cmds, place),
    }
    out
}

fn toolbar(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    view: &mut FileView,
    cmds: &mut Commands,
    place: Place,
    out: &mut Outcome,
) {
    ui.horizontal(|ui| {
        if place == Place::Main
            && ui
                .button(t!("⬅ Back"))
                .on_hover_text(t!("Back to the history"))
                .clicked()
        {
            cmds.push(Command::SetView(View::History));
        }
        if place == Place::Main {
            ui.separator();
        }
        if ui
            .add(egui::Button::selectable(
                view.mode == Mode::History,
                t!("History"),
            ))
            .clicked()
        {
            view.mode = Mode::History;
        }
        let blame = ui.add_enabled(
            !view.is_folder,
            egui::Button::selectable(view.mode == Mode::Blame, "Blame"),
        );
        if blame.clicked() {
            view.mode = Mode::Blame;
        }
        blame.on_disabled_hover_text(t!("Folders have no blame."));
        if let Some(l) = &view.lines
            && ui
                .add(egui::Button::selectable(
                    view.mode == Mode::Lines,
                    file_lines::label(l),
                ))
                .on_hover_text(t!("History of the selected lines"))
                .clicked()
        {
            view.mode = Mode::Lines;
        }
        ui.separator();
        let icon = if view.is_folder { "📁 " } else { "" };
        ui.add(
            egui::Label::new(egui::RichText::new(format!("{icon}{}", view.path)).strong())
                .truncate(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            match place {
                Place::Main => {
                    if ui
                        .button(t!("Open in a window"))
                        .on_hover_text(t!("Show this history in a separate window"))
                        .clicked()
                    {
                        out.detach = true;
                    }
                }
                Place::Window => {
                    if ui.button(t!("Close")).clicked() {
                        out.close = true;
                    }
                }
            }
            if ui.button("↻").on_hover_text(t!("Reload")).clicked() {
                view.reload();
            }
            if ui.small_button(t!("Copy path")).clicked() {
                cmds.push(Command::Copy(view.path.clone()));
            }
            base_menu(ui, tab, view);
        });
    });
}

/// The branch the history is relative to.
fn base_menu(ui: &mut egui::Ui, tab: &RepoTab, view: &mut FileView) {
    let current = view.base.clone().unwrap_or_else(|| "HEAD".into());
    let mut chosen = None;
    egui::ComboBox::from_id_salt(("file-history-base", view.id))
        .selected_text(&current)
        .width(160.0)
        .height(400.0)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(view.base.is_none(), t!("HEAD (current branch)"))
                .clicked()
            {
                chosen = Some(None);
            }
            for kind in [BranchKind::Local, BranchKind::Remote] {
                for b in tab.branches.iter().filter(|b| b.kind == kind) {
                    let on = view.base.as_deref() == Some(b.name.as_str());
                    if ui.selectable_label(on, &b.name).clicked() {
                        chosen = Some(Some(b.name.clone()));
                    }
                }
            }
            for t in &tab.tags {
                let on = view.base.as_deref() == Some(t.name.as_str());
                if ui.selectable_label(on, format!("🏷 {}", t.name)).clicked() {
                    chosen = Some(Some(t.name.clone()));
                }
            }
        })
        .response
        .on_hover_text(t!("Branch the history is shown for"));
    if let Some(base) = chosen
        && base != view.base
    {
        view.base = base;
        view.blame.reset_to_base(&view.path);
    }
}

pub(crate) fn loading_or_error<T>(ui: &mut egui::Ui, result: Option<&Result<T, String>>) -> bool {
    match result {
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(t!("Loading…"));
            });
            true
        }
        Some(Err(e)) => {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| ui.colored_label(ui.visuals().error_fg_color, e));
            true
        }
        Some(Ok(_)) => false,
    }
}

// ---- History ----

/// The two sides the diff compares: (old commit and path, new commit and path).
type Sides = (Option<(Oid, String)>, (Oid, String));

/// What the selection compares: one commit with its parent, or two commits.
fn sides(entries: &[FileCommit], selected: usize, other: Option<usize>) -> Option<Sides> {
    let e = entries.get(selected)?;
    match other.and_then(|o| entries.get(o).map(|x| (o, x))) {
        Some((o, x)) if o != selected => {
            // Older entries come later in the list.
            let (newer, older) = if o > selected { (e, x) } else { (x, e) };
            Some((
                Some((older.commit.id, older.path.clone())),
                (newer.commit.id, newer.path.clone()),
            ))
        }
        _ => {
            let old_path = e.old_path.clone().unwrap_or_else(|| e.path.clone());
            let old = e.commit.parents.first().map(|p| (*p, old_path));
            Some((old, (e.commit.id, e.path.clone())))
        }
    }
}

fn history_view(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    view: &mut FileView,
    cmds: &mut Commands,
    place: Place,
) {
    if loading_or_error(ui, view.history.poll(ui.ctx())) {
        return;
    }
    // Moved out while drawing, so the view can change.
    let history = std::mem::take(&mut view.history);
    if let Some(entries) = history.ok() {
        history_body(ui, tab, view, entries, cmds, place);
    }
    view.history = history;
}

fn history_body(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    view: &mut FileView,
    entries: &[FileCommit],
    cmds: &mut Commands,
    place: Place,
) {
    let ctx = ui.ctx().clone();
    if entries.is_empty() {
        ui.weak(if view.is_folder {
            t!("No commits touched this folder.")
        } else {
            t!("No commits touched this file.")
        });
        return;
    }
    let selected = *view.selected.get_or_insert(0);

    let mut clicked = None;
    egui::Panel::left(("file-history-list", view.id))
        .resizable(true)
        .default_size(380.0)
        .show(ui, |ui| {
            ui.weak(if entries.len() == 1 {
                t!("1 commit")
            } else {
                t!("{} commits", entries.len())
            });
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical()
                .id_salt(("file-history-rows", view.id))
                .auto_shrink(false)
                .show_rows(ui, HISTORY_ROW, entries.len(), |ui, range| {
                    for i in range {
                        let e = &entries[i];
                        let (rect, resp) = ui.allocate_exact_size(
                            vec2(ui.available_width(), HISTORY_ROW),
                            Sense::click(),
                        );
                        let state = if i == selected {
                            RowState::Selected
                        } else if view.other == Some(i) {
                            RowState::Other
                        } else {
                            RowState::None
                        };
                        history_row(ui, rect, e, &view.path, state, resp.hovered());
                        if resp.clicked() {
                            let ctrl = ui.input(|i| i.modifiers.command);
                            clicked = Some((i, ctrl));
                        }
                        resp.context_menu(|ui| {
                            entry_menu(ui, tab, view, entries, i, cmds);
                        });
                    }
                });
        });
    match clicked {
        // Ctrl+click picks a second commit to compare with.
        Some((i, true)) if i != selected => {
            view.other = if view.other == Some(i) { None } else { Some(i) };
        }
        Some((i, _)) => {
            view.selected = Some(i);
            view.other = None;
        }
        None => {}
    }

    egui::CentralPanel::default_margins().show(ui, |ui| {
        let selected = view.selected.unwrap_or(0);
        let Some((old, new)) = sides(entries, selected, view.other) else {
            return;
        };
        let e = &entries[selected];
        if let Some(o) = view.other.and_then(|o| entries.get(o)) {
            let (a, b) = match &old {
                Some((id, _)) => (short(*id), short(new.0)),
                None => (String::new(), short(new.0)),
            };
            ui.horizontal(|ui| {
                ui.strong(t!("Comparing {} with {}", a, b));
                ui.weak(t!("(Ctrl+click a commit to change the comparison)"));
            });
            ui.weak(format!("{} · {}", o.commit.summary, e.commit.summary));
        } else {
            ui.horizontal(|ui| {
                ui.strong(&e.commit.summary);
                ui.weak(e.commit.short_id());
            });
            if let Some(old) = &e.old_path {
                ui.weak(t!("Renamed from {}", old));
            } else if e.path != view.path {
                ui.weak(t!("Named {} in this commit", e.path));
            }
        }
        let cfg = tab.diff_options.config;

        if view.is_folder {
            let key = (old.as_ref().map(|o| o.0), new.0);
            if view.files_key != Some(key) {
                view.files_key = Some(key);
                view.file_sel = None;
                let folder = view.path.clone();
                view.files.start(&tab.repo, move |r| {
                    r.changed_files_between(key.0, key.1, &folder)
                        .map_err(|e| e.to_string())
                });
            }
            if loading_or_error(ui, view.files.poll(&ctx)) {
                return;
            }
            let files = view.files.ok().cloned().unwrap_or_default();
            if files.is_empty() {
                ui.weak(t!("No files in this folder changed."));
                return;
            }
            let sel = *view.file_sel.get_or_insert(0);
            egui::ScrollArea::vertical()
                .id_salt(("folder-files", view.id))
                .max_height((ui.available_height() * 0.3).max(60.0))
                .show(ui, |ui| {
                    for (i, f) in files.iter().enumerate() {
                        let label = format!("{}  {}", f.kind.letter(), f.path);
                        if ui.selectable_label(i == sel, label).clicked() {
                            view.file_sel = Some(i);
                        }
                    }
                });
            ui.separator();
            let Some(f) = files.get(view.file_sel.unwrap_or(0)) else {
                return;
            };
            let key = DiffKey {
                old: key.0,
                new: key.1,
                path: f.path.clone(),
                old_path: f.old_path.clone(),
                cfg,
            };
            diff_actions(ui, tab, &key, None, cmds);
            show_diff(ui, tab, view, key, place);
        } else {
            let key = DiffKey {
                old: old.as_ref().map(|o| o.0),
                new: new.0,
                path: new.1.clone(),
                old_path: old.as_ref().map(|o| o.1.clone()).filter(|p| *p != new.1),
                cfg,
            };
            diff_actions(ui, tab, &key, Some(view.id), cmds);
            show_diff(ui, tab, view, key, place);
        }
    });
}

/// Loads (when it changed) and shows the diff of `key`.
fn show_diff(ui: &mut egui::Ui, tab: &RepoTab, view: &mut FileView, key: DiffKey, place: Place) {
    if view.diff_key.as_ref() != Some(&key) {
        view.diff_key = Some(key.clone());
        view.diff.start(&tab.repo, move |r| {
            r.file_diff_between(
                key.old,
                key.new,
                &key.path,
                key.old_path.as_deref(),
                &key.cfg,
            )
            .map_err(|e| e.to_string())
        });
    }
    if loading_or_error(ui, view.diff.poll(ui.ctx())) {
        return;
    }
    let Some(file) = view.diff.ok() else {
        return;
    };
    match file {
        None if view.other.is_some() => {
            ui.weak(t!("No differences in this file between these commits."));
        }
        None => {
            let note = t!("This commit does not change the file (for example, a merge).");
            ui.weak(note);
        }
        Some(f) => {
            let new = view.diff_key.as_ref().map_or(key.new, |k| k.new);
            diff_extra::show_file(
                ui,
                &view.diff_id(place),
                &tab.repo,
                DiffSource::Commit(new),
                f,
                diff_view::DiffMode::ReadOnly,
                false,
                None,
            );
        }
    }
}

/// Buttons above a diff of the history. `view`: the file view to blame in
/// (`None`: open the file's own view).
fn diff_actions(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    key: &DiffKey,
    view: Option<u64>,
    cmds: &mut Commands,
) {
    ui.horizontal(|ui| {
        if ui
            .small_button(t!("Open in diff tool"))
            .on_hover_text(t!("Compare the two versions in the external diff tool"))
            .clicked()
        {
            match external_diff(&tab.repo, key) {
                Ok(l) => cmds.push(Command::External(l)),
                Err(e) => cmds.push(Command::Error(e)),
            }
        }
        if ui
            .small_button("Blame")
            .on_hover_text(t!("Blame of the file at the newer revision"))
            .clicked()
        {
            let pos = blame_view::Pos {
                rev: Some(key.new),
                path: key.path.clone(),
                line: 0,
            };
            let blame = move |v: &mut FileView| {
                v.mode = Mode::Blame;
                v.blame.navigate(pos);
            };
            cmds.push(match view {
                Some(id) => crate::ui::file_window::with_view(id, blame),
                None => {
                    let path = key.path.clone();
                    crate::ui::history_options::cmd(move |t, _| {
                        open_mode(t, path, Mode::Blame);
                        if let Some(v) = &mut t.file_view {
                            blame(v);
                        }
                    })
                }
            });
        }
    });
}

/// Writes both versions to temporary files for the diff tool.
fn external_diff(repo: &Repo, key: &DiffKey) -> Result<Launch, String> {
    let write = |id: Option<Oid>, path: &str| -> Result<std::path::PathBuf, String> {
        let data = match id {
            Some(id) => repo
                .file_at(id, path)
                .map_err(|e| e.to_string())?
                .unwrap_or_default(),
            None => Vec::new(),
        };
        let tag = id.map_or_else(|| "empty".into(), short);
        external::temp_file(&temp_name(path, &tag), &data)
    };
    let old_path = key.old_path.as_deref().unwrap_or(&key.path);
    Ok(Launch::Diff {
        left: write(key.old, old_path)?,
        right: write(Some(key.new), &key.path)?,
        cwd: repo.path().to_path_buf(),
    })
}

/// `dir/main.rs` at `abc1234` -> `main.abc1234.rs`.
fn temp_name(path: &str, tag: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem}.{tag}.{ext}"),
        _ => format!("{name}.{tag}"),
    }
}

fn entry_menu(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    view: &FileView,
    entries: &[FileCommit],
    i: usize,
    cmds: &mut Commands,
) {
    let e = &entries[i];
    let selected = view.selected.unwrap_or(0);
    if i != selected && ui.button(t!("Compare with the selected commit")).clicked() {
        let vid = view.id;
        cmds.push(crate::ui::file_window::with_view(vid, move |v| {
            v.other = Some(i)
        }));
        ui.close();
    }
    if !view.is_folder {
        if ui.button(t!("Blame at this revision")).clicked() {
            let (vid, rev, path) = (view.id, e.commit.id, e.path.clone());
            cmds.push(crate::ui::file_window::with_view(vid, move |v| {
                v.mode = Mode::Blame;
                v.blame.navigate(blame_view::Pos {
                    rev: Some(rev),
                    path,
                    line: 0,
                });
            }));
            ui.close();
        }
        let same_name = e.path == view.path;
        let reset = ui
            .add_enabled(
                same_name,
                egui::Button::new(t!("Reset file to this revision…")),
            )
            .on_disabled_hover_text(t!("The file had another name in this commit."));
        if reset.clicked() {
            cmds.push(Command::Dialog(Dialog::confirm(
                &t!("Reset file"),
                &t!(
                    "Replace {} with its version from {}? Uncommitted changes in this file will be lost.",
                    view.path,
                    e.commit.short_id()
                ),
                Op::ResetFileTo {
                    path: view.path.clone(),
                    rev: e.commit.id.to_string(),
                },
                true,
            )));
            ui.close();
        }
        if ui.button(t!("Open in diff tool")).clicked() {
            if let Some((old, new)) = sides(entries, i, None) {
                let key = DiffKey {
                    old: old.as_ref().map(|o| o.0),
                    new: new.0,
                    old_path: old.map(|o| o.1).filter(|p| *p != new.1),
                    path: new.1,
                    cfg: tab.diff_options.config,
                };
                match external_diff(&tab.repo, &key) {
                    Ok(l) => cmds.push(Command::External(l)),
                    Err(err) => cmds.push(Command::Error(err)),
                }
            }
            ui.close();
        }
    }
    ui.separator();
    if ui.button(t!("Show in history")).clicked() {
        cmds.push(Command::SelectCommit(e.commit.id));
        ui.close();
    }
    if ui.button(t!("Copy hash")).clicked() {
        cmds.push(Command::Copy(e.commit.id.to_string()));
        ui.close();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowState {
    None,
    Selected,
    /// Picked with Ctrl+click for comparing.
    Other,
}

fn history_row(
    ui: &egui::Ui,
    rect: Rect,
    e: &FileCommit,
    current_path: &str,
    state: RowState,
    hovered: bool,
) {
    let visuals = ui.visuals();
    let painter = ui.painter();
    let selected = state == RowState::Selected;
    let fill = match state {
        RowState::Selected => Some(visuals.selection.bg_fill),
        RowState::Other => Some(visuals.selection.bg_fill.gamma_multiply(0.45)),
        RowState::None if hovered => Some(visuals.widgets.hovered.weak_bg_fill),
        RowState::None => None,
    };
    if let Some(fill) = fill {
        painter.rect_filled(rect, 0.0, fill);
    }
    let (text, weak) = if selected {
        let c = visuals.selection.stroke.color;
        (c, c)
    } else {
        (visuals.text_color(), visuals.weak_text_color())
    };
    let inner = rect.shrink2(vec2(8.0, 0.0));
    let clip = painter.with_clip_rect(inner);
    let top = rect.top() + 13.0;
    let bottom = rect.bottom() - 12.0;
    let hash_w = 60.0;
    clip.with_clip_rect(Rect::from_x_y_ranges(
        inner.left()..=inner.right() - hash_w,
        inner.y_range(),
    ))
    .text(
        pos2(inner.left(), top),
        Align2::LEFT_CENTER,
        &e.commit.summary,
        FontId::proportional(13.0),
        text,
    );
    clip.text(
        pos2(inner.right(), top),
        Align2::RIGHT_CENTER,
        e.commit.short_id(),
        FontId::monospace(11.5),
        weak,
    );
    let mut meta = format!(
        "{} · {}",
        e.commit.author,
        format_date(e.commit.time, e.commit.offset_minutes)
    );
    if e.path != current_path {
        meta.push_str(&format!(" · {}", e.path));
    }
    clip.text(
        pos2(inner.left(), bottom),
        Align2::LEFT_CENTER,
        meta,
        FontId::proportional(11.5),
        weak,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrum_core::CommitInfo;

    fn entry(n: u8, path: &str, old: Option<&str>) -> FileCommit {
        FileCommit {
            commit: CommitInfo {
                id: Oid::from_bytes(&[n; 20]).unwrap(),
                summary: String::new(),
                author: String::new(),
                email: String::new(),
                time: 0,
                offset_minutes: 0,
                parents: vec![Oid::from_bytes(&[n + 100; 20]).unwrap()],
            },
            path: path.into(),
            old_path: old.map(Into::into),
        }
    }

    #[test]
    fn picks_the_sides_to_compare() {
        let list = [
            entry(1, "b", Some("a")),
            entry(2, "a", None),
            entry(3, "a", None),
        ];
        let id = |n: u8| Oid::from_bytes(&[n; 20]).unwrap();
        // One commit: its parent, under the old name.
        let (old, new) = sides(&list, 0, None).unwrap();
        assert_eq!(old, Some((id(101), "a".into())));
        assert_eq!(new, (id(1), "b".into()));
        // Two commits: older to newer, whichever was picked first.
        for (a, b) in [(0, 2), (2, 0)] {
            let (old, new) = sides(&list, a, Some(b)).unwrap();
            assert_eq!(old, Some((id(3), "a".into())));
            assert_eq!(new, (id(1), "b".into()));
        }
    }

    #[test]
    fn names_temporary_files() {
        assert_eq!(temp_name("src/main.rs", "abc1234"), "main.abc1234.rs");
        assert_eq!(temp_name("Makefile", "abc1234"), "Makefile.abc1234");
        assert_eq!(temp_name(".gitignore", "x"), ".gitignore.x");
    }
}
