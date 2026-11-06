//! Lists of changed files (as a folder tree or a flat list) and the File
//! Tree tab: browsing a repository at a revision, viewing a file (text with
//! syntax colors, images, Git LFS pointers) and saving it.

use std::collections::{BTreeMap, HashMap, HashSet};

use eframe::egui::{self, Color32};
use ferrum_core::browse::{EntryKind, TreeEntry};
use ferrum_core::diff::{self, parse_lfs_pointer};
use ferrum_core::{ChangeKind, DiffLine, DiffSource, FileDiff, Hunk, LineKind, Oid, Repo, short};

use crate::command::{Command, Commands};
use crate::ui::diff_text::{self, LineStyle, SideHighlighter};
use crate::ui::{diff_extra, diff_view, file_actions, image_diff};
use crate::util::{change_label, code_font, code_row_height};

/// Files above this size are not shown (they can still be saved).
const MAX_VIEW_BYTES: usize = 20 * 1024 * 1024;
const INDENT: f32 = 14.0;

// ----- Changed files -----

/// Folders closed in a changed-files tree.
#[derive(Default)]
pub struct ListState {
    pub collapsed: HashSet<String>,
}

/// Folders of a changed-files tree; files are indices into the list.
#[derive(Default)]
struct Node {
    dirs: BTreeMap<String, Node>,
    files: Vec<usize>,
}

impl Node {
    fn build(files: &[FileDiff]) -> Self {
        let mut root = Node::default();
        for (i, f) in files.iter().enumerate() {
            let mut node = &mut root;
            let mut parts: Vec<&str> = f.path.split('/').collect();
            parts.pop();
            for p in parts {
                node = node.dirs.entry(p.to_owned()).or_default();
            }
            node.files.push(i);
        }
        root
    }

    fn all_dirs(&self, prefix: &str, out: &mut Vec<String>) {
        for (name, child) in &self.dirs {
            let dir = format!("{prefix}{name}");
            child.all_dirs(&format!("{dir}/"), out);
            out.push(dir);
        }
    }
}

/// The count of files, "Tree" / "List" and, for the tree, Expand / Collapse all.
pub fn list_toolbar(ui: &mut egui::Ui, files: &[FileDiff], as_tree: &mut bool, st: &mut ListState) {
    ui.horizontal(|ui| {
        ui.weak(if files.len() == 1 {
            t!("1 file changed")
        } else {
            t!("{} files changed", files.len())
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if *as_tree {
                if ui
                    .small_button("⏵")
                    .on_hover_text(t!("Collapse all"))
                    .clicked()
                {
                    let mut dirs = Vec::new();
                    Node::build(files).all_dirs("", &mut dirs);
                    st.collapsed = dirs.into_iter().collect();
                }
                if ui
                    .small_button("⏷")
                    .on_hover_text(t!("Expand all"))
                    .clicked()
                {
                    st.collapsed.clear();
                }
            }
            ui.selectable_value(as_tree, true, t!("Tree"));
            ui.selectable_value(as_tree, false, t!("List"));
        });
    });
}

/// The changed files; returns the one clicked. `menu` fills a file's context menu.
pub fn changed_files(
    ui: &mut egui::Ui,
    files: &[FileDiff],
    selected: Option<usize>,
    as_tree: bool,
    st: &mut ListState,
    menu: &mut dyn FnMut(&mut egui::Ui, &FileDiff),
) -> Option<usize> {
    let mut clicked = None;
    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
        if as_tree {
            let root = Node::build(files);
            tree_rows(ui, files, &root, "", 0, selected, st, menu, &mut clicked);
        } else {
            for (i, f) in files.iter().enumerate() {
                if file_row(ui, f, &f.path, 0, selected == Some(i), menu) {
                    clicked = Some(i);
                }
            }
        }
    });
    clicked
}

#[allow(clippy::too_many_arguments)]
fn tree_rows(
    ui: &mut egui::Ui,
    files: &[FileDiff],
    node: &Node,
    prefix: &str,
    depth: usize,
    selected: Option<usize>,
    st: &mut ListState,
    menu: &mut dyn FnMut(&mut egui::Ui, &FileDiff),
    clicked: &mut Option<usize>,
) {
    for (name, child) in &node.dirs {
        // A folder holding just one folder is shown as one row.
        let mut label = name.clone();
        let mut child = child;
        while child.files.is_empty() && child.dirs.len() == 1 {
            let (n, c) = child.dirs.iter().next().expect("one folder");
            label = format!("{label}/{n}");
            child = c;
        }
        let dir = format!("{prefix}{label}");
        let open = !st.collapsed.contains(&dir);
        if folder_row(ui, &label, depth, open).clicked() {
            if open {
                st.collapsed.insert(dir.clone());
            } else {
                st.collapsed.remove(&dir);
            }
        }
        if open {
            let p = format!("{dir}/");
            tree_rows(ui, files, child, &p, depth + 1, selected, st, menu, clicked);
        }
    }
    for &i in &node.files {
        let f = &files[i];
        let name = f.path.rsplit('/').next().unwrap_or(&f.path);
        if file_row(ui, f, name, depth, selected == Some(i), menu) {
            *clicked = Some(i);
        }
    }
}

fn folder_row(ui: &mut egui::Ui, label: &str, depth: usize, open: bool) -> egui::Response {
    let arrow = if open { "⏷" } else { "⏵" };
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * INDENT);
        ui.add(egui::Button::selectable(false, format!("{arrow} 📁 {label}")).truncate())
    })
    .inner
}

fn file_row(
    ui: &mut egui::Ui,
    f: &FileDiff,
    shown: &str,
    depth: usize,
    selected: bool,
    menu: &mut dyn FnMut(&mut egui::Ui, &FileDiff),
) -> bool {
    let job = change_label(ui, f.kind, shown, f.old_path.as_deref());
    let resp = ui
        .horizontal(|ui| {
            ui.add_space(depth as f32 * INDENT);
            ui.add(egui::Button::selectable(selected, job).truncate())
        })
        .inner;
    let clicked = resp.clicked();
    resp.on_hover_text(&f.path).context_menu(|ui| menu(ui, f));
    clicked
}

// ----- File Tree tab -----

/// Browsing state of the File Tree tab.
#[derive(Default)]
pub struct RevTree {
    commit: Option<Oid>,
    dirs: HashMap<String, Result<Vec<TreeEntry>, String>>,
    pub open: HashSet<String>,
    pub selected: Option<TreeEntry>,
    viewer: Option<Viewer>,
}

struct Viewer {
    entry: TreeEntry,
    content: Content,
}

enum Content {
    Text {
        lines: Vec<String>,
        highlight: Option<(bool, SideHighlighter)>,
    },
    Image(Result<(egui::TextureHandle, [u32; 2]), String>),
    /// A Git LFS pointer, shown through the diff view's LFS preview.
    Lfs(Box<FileDiff>),
    Binary(usize),
    TooLarge(usize),
    Submodule,
    Error(String),
}

impl RevTree {
    fn reset_for(&mut self, commit: Oid) {
        if self.commit != Some(commit) {
            self.commit = Some(commit);
            self.dirs.clear();
            // Open folders and the selected path stay: the same file at
            // another revision is usually what one wants to see next.
            self.viewer = None;
        }
    }

    fn entries(&mut self, repo: &Repo, commit: Oid, dir: &str) -> Result<Vec<TreeEntry>, String> {
        self.dirs
            .entry(dir.to_owned())
            .or_insert_with(|| repo.tree_dir(commit, dir).map_err(|e| e.to_string()))
            .clone()
    }
}

/// The File Tree tab: folders on the left, the selected file on the right.
pub fn show_tree(
    ui: &mut egui::Ui,
    salt: &str,
    repo: &Repo,
    commit: Oid,
    st: &mut RevTree,
    cmds: &mut Commands,
) {
    st.reset_for(commit);
    egui::Panel::left(egui::Id::unique(("file-tree-list", salt)))
        .resizable(true)
        .default_size(300.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.weak(t!("Files at {}", short(commit)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("⏵")
                        .on_hover_text(t!("Collapse all"))
                        .clicked()
                    {
                        st.open.clear();
                    }
                });
            });
            egui::ScrollArea::vertical()
                .id_salt(("file-tree-scroll", salt))
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                        dir_rows(ui, repo, commit, st, "", 0, cmds);
                    });
                });
        });
    egui::CentralPanel::default_margins().show(ui, |ui| viewer(ui, salt, repo, commit, st, cmds));
}

fn dir_rows(
    ui: &mut egui::Ui,
    repo: &Repo,
    commit: Oid,
    st: &mut RevTree,
    dir: &str,
    depth: usize,
    cmds: &mut Commands,
) {
    let entries = match st.entries(repo, commit, dir) {
        Ok(e) => e,
        Err(e) => {
            ui.colored_label(ui.visuals().error_fg_color, e);
            return;
        }
    };
    if entries.is_empty() && dir.is_empty() {
        ui.weak(t!("No files."));
    }
    for e in entries {
        if e.kind == EntryKind::Dir {
            let open = st.open.contains(&e.path);
            let resp = folder_row(ui, &e.name, depth, open);
            if resp.clicked() {
                if open {
                    st.open.remove(&e.path);
                } else {
                    st.open.insert(e.path.clone());
                }
            }
            resp.context_menu(|ui| {
                if ui.button(t!("Copy path")).clicked() {
                    cmds.push(Command::Copy(e.path.clone()));
                    ui.close();
                }
                file_actions::workdir_menu(ui, repo.path(), &e.path, cmds);
            });
            if open {
                dir_rows(ui, repo, commit, st, &e.path, depth + 1, cmds);
            }
            continue;
        }
        let icon = match e.kind {
            EntryKind::Submodule => "📦",
            _ => "📄",
        };
        let selected = st.selected.as_ref().is_some_and(|s| s.path == e.path);
        let resp = ui
            .horizontal(|ui| {
                ui.add_space(depth as f32 * INDENT);
                ui.add(egui::Button::selectable(selected, format!("{icon} {}", e.name)).truncate())
            })
            .inner;
        if resp.clicked() {
            st.selected = Some(e.clone());
            st.viewer = None;
        }
        resp.context_menu(|ui| entry_menu(ui, repo, &e, cmds));
    }
}

fn entry_menu(ui: &mut egui::Ui, repo: &Repo, e: &TreeEntry, cmds: &mut Commands) {
    if ui.button(t!("Copy path")).clicked() {
        cmds.push(Command::Copy(e.path.clone()));
        ui.close();
    }
    if e.kind != EntryKind::Submodule && ui.button(t!("Save file…")).clicked() {
        ui.close();
        save_entry(repo, e, cmds);
    }
    ui.separator();
    if ui.button(t!("File history")).clicked() {
        cmds.push(Command::FileHistory(e.path.clone()));
        ui.close();
    }
    if ui.button(t!("Blame")).clicked() {
        cmds.push(Command::Blame(e.path.clone()));
        ui.close();
    }
    ui.separator();
    file_actions::workdir_menu(ui, repo.path(), &e.path, cmds);
}

/// Saves a file of the revision; LFS files are saved with their real content.
fn save_entry(repo: &Repo, e: &TreeEntry, cmds: &mut Commands) {
    let (id, path) = (e.id, e.path.clone());
    let git = repo.git();
    file_actions::save_as(
        &e.name,
        || {
            let bytes = repo.blob(id).map_err(|e| e.to_string())?;
            let pointer = std::str::from_utf8(&bytes).ok().and_then(parse_lfs_pointer);
            match pointer {
                Some(p) if diff::lfs_available(&git) => {
                    diff::lfs_smudge(&git, &path, &p).map_err(|e| e.to_string())
                }
                _ => Ok(bytes),
            }
        },
        cmds,
    );
}

fn load(ctx: &egui::Context, repo: &Repo, e: &TreeEntry) -> Content {
    if e.kind == EntryKind::Submodule {
        return Content::Submodule;
    }
    let size = repo.blob_size(e.id).unwrap_or(0);
    if size > MAX_VIEW_BYTES {
        return Content::TooLarge(size);
    }
    let bytes = match repo.blob(e.id) {
        Ok(b) => b,
        Err(err) => return Content::Error(err.to_string()),
    };
    if let Some(p) = std::str::from_utf8(&bytes).ok().and_then(parse_lfs_pointer) {
        return Content::Lfs(Box::new(pointer_diff(e, &p.text)));
    }
    if image_diff::is_image(&e.path) {
        let decoded = image_diff::decode(&bytes, 4096, &e.path).map(|(img, w, h)| {
            let tex = ctx.load_texture(
                format!("file-tree:{}", e.id),
                img,
                egui::TextureOptions::LINEAR,
            );
            (tex, [w, h])
        });
        return Content::Image(decoded);
    }
    if bytes[..bytes.len().min(8000)].contains(&0) {
        return Content::Binary(bytes.len());
    }
    let text = String::from_utf8_lossy(&bytes);
    let lines = text
        .lines()
        .map(|l| l.trim_end_matches('\r').to_owned())
        .collect();
    Content::Text {
        lines,
        highlight: None,
    }
}

/// A pointer file as an added file, so the diff view's LFS preview shows it.
fn pointer_diff(e: &TreeEntry, text: &str) -> FileDiff {
    let lines: Vec<DiffLine> = text
        .lines()
        .enumerate()
        .map(|(i, l)| DiffLine {
            kind: LineKind::Added,
            text: l.to_owned(),
            old_no: None,
            new_no: Some(i as u32 + 1),
            no_newline: false,
        })
        .collect();
    FileDiff {
        path: e.path.clone(),
        old_path: None,
        kind: ChangeKind::Added,
        binary: false,
        old_id: None,
        new_id: Some(e.id),
        hunks: vec![Hunk {
            header: String::new(),
            old_start: 0,
            old_lines: 0,
            new_start: 1,
            new_lines: lines.len() as u32,
            lines,
        }],
        additions: 0,
        deletions: 0,
        old_mode: 0,
        new_mode: 0o100644,
        skipped: None,
    }
}

fn viewer(
    ui: &mut egui::Ui,
    salt: &str,
    repo: &Repo,
    commit: Oid,
    st: &mut RevTree,
    cmds: &mut Commands,
) {
    let Some(entry) = st.selected.clone() else {
        ui.centered_and_justified(|ui| ui.weak(t!("Select a file to see its content.")));
        return;
    };
    if st
        .viewer
        .as_ref()
        .is_none_or(|v| v.entry.path != entry.path)
    {
        // The selected path may not exist at this revision.
        let at_rev = repo
            .tree_dir(commit, entry.path.rsplit_once('/').map_or("", |(d, _)| d))
            .ok()
            .and_then(|list| list.into_iter().find(|e| e.path == entry.path));
        let content = match &at_rev {
            Some(e) => load(ui.ctx(), repo, e),
            None => Content::Error(t!("{} does not exist in this commit.", entry.path)),
        };
        let entry = at_rev.unwrap_or(entry);
        st.selected = Some(entry.clone());
        st.viewer = Some(Viewer { entry, content });
    }
    let Some(v) = &mut st.viewer else { return };
    let e = v.entry.clone();
    ui.horizontal(|ui| {
        ui.strong(&e.path);
        if let Content::Text { lines, .. } = &v.content {
            ui.weak(t!("{} lines", lines.len()));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if e.kind != EntryKind::Submodule && ui.button(t!("Save file…")).clicked() {
                save_entry(repo, &e, cmds);
            }
            ui.menu_button("☰", |ui| entry_menu(ui, repo, &e, cmds));
        });
    });
    ui.separator();
    match &mut v.content {
        Content::Text { lines, highlight } => text_view(ui, salt, &e.path, lines, highlight),
        Content::Image(Ok((tex, size))) => {
            ui.weak(format!("{} × {}", size[0], size[1]));
            egui::ScrollArea::both()
                .id_salt(("file-tree-image", salt))
                .show(ui, |ui| {
                    let avail = ui.available_size();
                    let s = tex.size_vec2();
                    let scale = (avail.x / s.x).min(avail.y / s.y).min(1.0).max(0.05);
                    ui.image((tex.id(), s * scale));
                });
        }
        Content::Image(Err(err)) | Content::Error(err) => {
            ui.colored_label(ui.visuals().error_fg_color, err.as_str());
        }
        Content::Lfs(file) => {
            diff_extra::show_file(
                ui,
                &format!("file-tree-lfs-{salt}"),
                repo,
                DiffSource::Commit(commit),
                file,
                diff_view::DiffMode::ReadOnly,
                false,
                None,
            );
        }
        Content::Binary(n) => {
            ui.weak(t!("Binary file ({}).", diff_extra::format_size(*n as u64)));
        }
        Content::TooLarge(n) => {
            ui.weak(t!(
                "This file is too large to show ({}). Save it to open it in another program.",
                diff_extra::format_size(*n as u64)
            ));
        }
        Content::Submodule => {
            ui.weak(t!("Submodule at commit {}", e.id.to_string()));
        }
    }
}

/// A read-only text file with line numbers and syntax colors.
fn text_view(
    ui: &mut egui::Ui,
    salt: &str,
    path: &str,
    lines: &[String],
    highlight: &mut Option<(bool, SideHighlighter)>,
) {
    let dark = ui.visuals().dark_mode;
    let syntax = crate::ui::diff_prefs::get(ui.ctx()).syntax;
    if !syntax {
        *highlight = None;
    } else if highlight.as_ref().is_none_or(|(d, _)| *d != dark) {
        *highlight = diff_text::syntax_for(path).map(|s| (dark, SideHighlighter::new(s, dark)));
    }
    let font = code_font(12.0);
    let row_h = code_row_height(16.0);
    let digits = lines.len().max(1).to_string().len();
    let num_w = ui.fonts_mut(|f| f.glyph_width(&font, '0')) * (digits as f32 + 1.5);
    let color = ui.visuals().text_color();
    let weak = ui.visuals().weak_text_color();
    let tab_width = crate::prefs::tab_width();
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::both()
        .id_salt(("file-tree-text", salt, path))
        .auto_shrink(false)
        .show_rows(ui, row_h, lines.len(), |ui, range| {
            if let Some((_, hl)) = highlight
                && !hl.advance(|i| lines[i].as_str(), range.end)
            {
                ui.ctx().request_repaint();
            }
            for i in range {
                let spans = highlight.as_ref().and_then(|(_, h)| h.done.get(i));
                let style = LineStyle {
                    font: font.clone(),
                    color,
                    weak,
                    syntax: spans,
                    words: &[],
                    word_bg: Color32::TRANSPARENT,
                    found: &[],
                    found_bg: Color32::TRANSPARENT,
                    current: None,
                    current_bg: Color32::TRANSPARENT,
                    tab_width,
                    show_whitespace: false,
                };
                ui.horizontal(|ui| {
                    ui.set_height(row_h);
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(num_w, row_h), egui::Sense::hover());
                    ui.painter().text(
                        rect.right_center() - egui::vec2(8.0, 0.0),
                        egui::Align2::RIGHT_CENTER,
                        (i + 1).to_string(),
                        font.clone(),
                        weak,
                    );
                    ui.add(egui::Label::new(diff_text::layout_line(&lines[i], &style)).extend());
                });
            }
        });
}
