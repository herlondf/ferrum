//! The Repository Manager: the start screen and the "new tab" (Ctrl+T).
//! Known repositories in groups, workspaces and source folders.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Key, RichText};

use crate::command::{Command, Commands};
use crate::external::Launch;
use crate::repos::{Library, RepoCmd, SourceFolder};
use crate::settings::Settings;
use crate::ui::dialogs::Dialog;

#[derive(Default)]
pub struct Manager {
    filter: String,
    pub selected: BTreeSet<PathBuf>,
    anchor: Option<PathBuf>,
    /// Inline rename (F2): the repository and the name being typed.
    renaming: Option<(PathBuf, String)>,
    focus_rename: bool,
    new_folder: String,
    new_workspace: String,
    /// Rows in the order shown, for Shift+click and the arrow keys.
    order: Vec<PathBuf>,
}

/// Keyboard and context-menu actions on the selection.
enum Action {
    Open(Vec<PathBuf>),
    Remove(Vec<PathBuf>),
    Rename(PathBuf),
}

/// Draws the manager. `open` are the paths of open tabs; `scanning` is true
/// while the source folders are being scanned.
pub fn show(
    ui: &mut egui::Ui,
    m: &mut Manager,
    settings: &mut Settings,
    open: &[PathBuf],
    scanning: bool,
    cmds: &mut Commands,
) {
    import_recent(settings);
    header(ui, m, settings, cmds);
    ui.separator();
    let mut action = None;
    egui::Panel::left("manager-side")
        .resizable(true)
        .default_size(260.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                workspaces(ui, m, &mut settings.library, cmds);
                ui.add_space(12.0);
                sources(ui, &mut settings.library, scanning, cmds);
            });
        });
    egui::CentralPanel::default_margins().show(ui, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                let recent = settings.recent.clone();
                action = repo_list(ui, m, &mut settings.library, open, &recent, cmds);
            });
    });
    if action.is_none() {
        action = keys(ui, m);
    }
    match action {
        Some(Action::Open(paths)) => {
            for p in paths {
                cmds.push(Command::OpenRepo(p));
            }
        }
        Some(Action::Remove(paths)) => {
            settings.library.remove(&paths);
            m.selected.clear();
        }
        Some(Action::Rename(p)) => {
            let name = settings
                .library
                .custom_name(&p)
                .unwrap_or_default()
                .to_owned();
            m.renaming = Some((p, name));
            m.focus_rename = true;
        }
        None => {}
    }
}

/// Copies the recent list in once, so the manager is not empty after an update.
fn import_recent(settings: &mut Settings) {
    if !settings.library.imported_recent {
        settings.library.imported_recent = true;
        for p in settings.recent.clone().into_iter().rev() {
            settings.library.remember(&p);
        }
    }
}

fn header(ui: &mut egui::Ui, m: &mut Manager, settings: &Settings, cmds: &mut Commands) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.heading(RichText::new("Ferrum").strong());
        ui.weak(t!("A fast Git client"));
        ui.add_space(16.0);
        if ui.button(t!("📂 Open…")).clicked() {
            cmds.push(Command::OpenRepoDialog);
        }
        if ui.button(t!("⬇ Clone…")).clicked() {
            cmds.push(Command::Dialog(Dialog::clone_repo(settings)));
        }
        if ui.button(t!("✚ New…")).clicked() {
            cmds.push(Command::Dialog(Dialog::Init {
                path: String::new(),
            }));
        }
        if ui
            .button(t!("🔍 Open Quickly…"))
            .on_hover_text("Ctrl+Shift+O")
            .clicked()
        {
            cmds.push(Command::Repos(RepoCmd::OpenQuickly));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut m.filter)
                    .hint_text(t!("Filter repositories"))
                    .desired_width(220.0),
            );
        });
    });
    ui.weak(t!(
        "Drop a folder on the window to open it. Enter opens, F2 renames, Delete removes from the list."
    ));
}

fn matches(lib: &Library, filter: &str, p: &Path) -> bool {
    let f = filter.trim().to_lowercase();
    f.is_empty()
        || p.to_string_lossy().to_lowercase().contains(&f)
        || lib
            .custom_name(p)
            .is_some_and(|n| n.to_lowercase().contains(&f))
}

/// A path without the trailing separator git adds to working folders.
pub fn shown_path(p: &Path) -> String {
    let text = p.display().to_string();
    match text.trim_end_matches(['/', '\\']) {
        "" => text,
        t => t.to_owned(),
    }
}

pub fn display_name(lib: &Library, p: &Path) -> String {
    lib.custom_name(p)
        .map(str::to_owned)
        .or_else(|| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| p.display().to_string())
}

fn repo_list(
    ui: &mut egui::Ui,
    m: &mut Manager,
    lib: &mut Library,
    open: &[PathBuf],
    recent: &[PathBuf],
    cmds: &mut Commands,
) -> Option<Action> {
    let mut action = None;
    m.order.clear();
    let missing = lib.missing();
    if !missing.is_empty() {
        ui.horizontal(|ui| {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                t!("⚠ {} repositories no longer exist.", missing.len()),
            );
            if ui.button(t!("Remove missing")).clicked() {
                action = Some(Action::Remove(missing.clone()));
            }
        });
        ui.add_space(4.0);
    }
    if lib.repos.is_empty() && lib.found.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.weak(t!(
                "No repositories yet. Open, clone or create one, or add a source folder to scan."
            ));
        });
    }
    // The last few opened repositories, for one click.
    let recent: Vec<&PathBuf> = recent.iter().filter(|p| p.exists()).take(5).collect();
    if m.filter.trim().is_empty() && !recent.is_empty() {
        ui.strong(t!("Recent repositories"));
        ui.horizontal_wrapped(|ui| {
            for p in recent {
                if ui
                    .button(display_name(lib, p))
                    .on_hover_text(shown_path(p))
                    .clicked()
                {
                    cmds.push(Command::OpenRepo(p.clone()));
                }
            }
        });
        ui.add_space(8.0);
    }
    let mut groups: Vec<String> = vec![String::new()];
    groups.extend(lib.folder_names());
    let folder_names = lib.folder_names();
    let workspaces = lib.workspace_names();
    for g in groups {
        let paths: Vec<PathBuf> = lib
            .repos
            .iter()
            .filter(|r| r.folder == g && matches(lib, &m.filter, &r.path))
            .map(|r| r.path.clone())
            .collect();
        let mut body = |ui: &mut egui::Ui, m: &mut Manager, lib: &mut Library| {
            for p in &paths {
                let menu = RowMenu {
                    folders: &folder_names,
                    workspaces: &workspaces,
                };
                if let Some(a) = row(ui, m, lib, p, open.contains(p), &menu, cmds) {
                    action = Some(a);
                }
            }
        };
        if g.is_empty() {
            if !paths.is_empty() {
                ui.strong(t!("Repositories"));
            }
            body(ui, m, lib);
        } else {
            let id = ui.make_persistent_id(("group", &g));
            let resp = egui::CollapsingHeader::new(RichText::new(format!("📁 {g}")).strong())
                .id_salt(id)
                .default_open(true)
                .show(ui, |ui| {
                    body(ui, m, lib);
                    if paths.is_empty() {
                        ui.weak(t!("Empty. Move repositories here from their menu."));
                    }
                });
            resp.header_response.context_menu(|ui| {
                let mut name = g.clone();
                ui.label(t!("Rename group"));
                if ui.text_edit_singleline(&mut name).changed() && !name.trim().is_empty() {
                    lib.rename_folder(&g, &name);
                }
                if ui.button(t!("Delete group")).clicked() {
                    lib.delete_folder(&g);
                    ui.close();
                }
            });
        }
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut m.new_folder)
                .hint_text(t!("New group"))
                .desired_width(160.0),
        );
        let name = m.new_folder.trim().to_owned();
        if ui
            .add_enabled(!name.is_empty(), egui::Button::new(t!("Add group")))
            .clicked()
        {
            if !lib.folders.contains(&name) {
                lib.folders.push(name);
            }
            m.new_folder.clear();
        }
    });

    let found: Vec<PathBuf> = lib
        .unlisted_found()
        .into_iter()
        .filter(|p| matches(lib, &m.filter, p))
        .collect();
    if !found.is_empty() {
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.strong(t!("Found in source folders ({})", found.len()));
            if ui.small_button(t!("Add all to the list")).clicked() {
                for p in &found {
                    lib.remember(p);
                }
            }
        });
        for p in &found {
            ui.horizontal(|ui| {
                let resp = ui
                    .add(egui::Button::selectable(false, display_name(lib, p)))
                    .on_hover_text(t!("Double-click to open"));
                ui.weak(shown_path(p));
                if resp.double_clicked() {
                    cmds.push(Command::OpenRepo(p.clone()));
                }
                if ui.small_button(t!("Add to the list")).clicked() {
                    lib.remember(p);
                }
            });
        }
    }
    action
}

struct RowMenu<'a> {
    folders: &'a [String],
    workspaces: &'a [String],
}

fn row(
    ui: &mut egui::Ui,
    m: &mut Manager,
    lib: &mut Library,
    p: &Path,
    is_open: bool,
    menu: &RowMenu,
    cmds: &mut Commands,
) -> Option<Action> {
    m.order.push(p.to_path_buf());
    let mut action = None;
    let exists = p.exists();
    ui.horizontal(|ui| {
        if let Some((rp, name)) = &mut m.renaming
            && rp == p
        {
            let resp = ui.add(
                egui::TextEdit::singleline(name)
                    .hint_text(display_name(lib, p))
                    .desired_width(220.0),
            );
            if m.focus_rename {
                resp.request_focus();
                m.focus_rename = false;
            }
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                m.renaming = None;
            } else if resp.lost_focus() {
                lib.entry(p).name = name.trim().to_owned();
                // The Enter that ends the rename does not also open the repository.
                ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter));
                m.renaming = None;
            }
            return;
        }
        let selected = m.selected.contains(p);
        let mut text = RichText::new(display_name(lib, p)).strong();
        if let Some(c) = lib.color(p) {
            text = text.color(crate::ui::tabs::color32(c));
        }
        let resp = ui.add(egui::Button::selectable(selected, text));
        ui.weak(shown_path(p));
        if !exists {
            ui.colored_label(ui.visuals().warn_fg_color, t!("⚠ missing"));
        }
        if is_open {
            ui.weak(t!("(open)"));
        }
        if resp.clicked() {
            click(ui, m, p);
        }
        if resp.double_clicked() && exists {
            action = Some(Action::Open(vec![p.to_path_buf()]));
        }
        if resp.secondary_clicked() && !m.selected.contains(p) {
            m.selected = BTreeSet::from([p.to_path_buf()]);
            m.anchor = Some(p.to_path_buf());
        }
        resp.context_menu(|ui| {
            let sel: Vec<PathBuf> = m.selected.iter().cloned().collect();
            if ui.button(t!("Open")).clicked() {
                action = Some(Action::Open(sel.clone()));
                ui.close();
            }
            if sel.len() == 1 && ui.button(t!("Rename (F2)")).clicked() {
                action = Some(Action::Rename(p.to_path_buf()));
                ui.close();
            }
            ui.menu_button(t!("Move to group"), |ui| {
                if ui.button(t!("No group")).clicked() {
                    for s in &sel {
                        lib.entry(s).folder.clear();
                    }
                    ui.close();
                }
                for f in menu.folders {
                    if ui.button(f).clicked() {
                        for s in &sel {
                            lib.entry(s).folder = f.clone();
                        }
                        ui.close();
                    }
                }
            });
            if !menu.workspaces.is_empty() {
                ui.menu_button(t!("Add to workspace"), |ui| {
                    for w in menu.workspaces {
                        if w != &lib.workspace && ui.button(w).clicked() {
                            for s in &sel {
                                lib.add_to_workspace(w, s);
                            }
                            ui.close();
                        }
                    }
                });
            }
            ui.separator();
            if exists {
                if ui.button(t!("Open in terminal")).clicked() {
                    cmds.push(Command::External(Launch::Terminal { dir: p.into() }));
                    ui.close();
                }
                if ui.button(t!("Show in file manager")).clicked() {
                    cmds.push(Command::External(Launch::Reveal { path: p.into() }));
                    ui.close();
                }
            }
            if ui.button(t!("Copy path")).clicked() {
                cmds.push(Command::Copy(p.display().to_string()));
                ui.close();
            }
            ui.separator();
            if ui.button(t!("Remove from list (Delete)")).clicked() {
                action = Some(Action::Remove(sel.clone()));
                ui.close();
            }
        });
    });
    action
}

/// Click, Ctrl+click (toggle) and Shift+click (range).
fn click(ui: &egui::Ui, m: &mut Manager, p: &Path) {
    let mods = ui.input(|i| i.modifiers);
    if mods.shift
        && let Some(a) = &m.anchor
        && let (Some(x), Some(y)) = (
            m.order.iter().position(|o| o == a),
            m.order.iter().position(|o| o == p),
        )
    {
        let (lo, hi) = (x.min(y), x.max(y));
        // Rows after this one are not in `order` yet: the range stops here.
        m.selected = m.order[lo..=hi].iter().cloned().collect();
        return;
    }
    if mods.command {
        if !m.selected.remove(p) {
            m.selected.insert(p.to_path_buf());
        }
    } else {
        m.selected = BTreeSet::from([p.to_path_buf()]);
    }
    m.anchor = Some(p.to_path_buf());
}

/// Enter, Delete, F2, arrows and Ctrl+A, while no text field has the focus.
fn keys(ui: &egui::Ui, m: &mut Manager) -> Option<Action> {
    let ctx = ui.ctx();
    if m.renaming.is_some() || ctx.egui_wants_keyboard_input() || egui::Popup::is_any_open(ctx) {
        return None;
    }
    let (enter, delete, f2, up, down, all) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, Key::Enter),
            i.consume_key(egui::Modifiers::NONE, Key::Delete),
            i.consume_key(egui::Modifiers::NONE, Key::F2),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            i.consume_key(egui::Modifiers::COMMAND, Key::A),
        )
    });
    let sel: Vec<PathBuf> = m.selected.iter().cloned().collect();
    if all {
        m.selected = m.order.iter().cloned().collect();
    }
    if up || down {
        let cur = m
            .anchor
            .as_ref()
            .and_then(|a| m.order.iter().position(|o| o == a));
        let next = match cur {
            Some(i) if up => i.saturating_sub(1),
            Some(i) => (i + 1).min(m.order.len().saturating_sub(1)),
            None => 0,
        };
        if let Some(p) = m.order.get(next).cloned() {
            m.selected = BTreeSet::from([p.clone()]);
            m.anchor = Some(p);
        }
    }
    if sel.is_empty() {
        return None;
    }
    if enter {
        let existing = sel.into_iter().filter(|p| p.exists()).collect();
        return Some(Action::Open(existing));
    }
    if delete {
        return Some(Action::Remove(sel));
    }
    if f2 && sel.len() == 1 {
        return Some(Action::Rename(sel[0].clone()));
    }
    None
}

fn workspaces(ui: &mut egui::Ui, m: &mut Manager, lib: &mut Library, cmds: &mut Commands) {
    ui.strong(t!("Workspaces"));
    ui.weak(t!(
        "Each workspace remembers its own tabs. Nest them with /."
    ));
    let active = lib.workspace.clone();
    let default = ui.add(egui::Button::selectable(
        active.is_empty(),
        t!("Default workspace"),
    ));
    if default.clicked() {
        cmds.push(Command::Repos(RepoCmd::SwitchWorkspace(String::new())));
    }
    for name in lib.workspace_names() {
        let depth = name.matches('/').count();
        let leaf = name.rsplit('/').next().unwrap_or(&name).to_owned();
        let resp = ui
            .horizontal(|ui| {
                ui.add_space(depth as f32 * 14.0);
                ui.add(egui::Button::selectable(name == active, leaf))
            })
            .inner
            .on_hover_text(&name);
        if resp.clicked() {
            cmds.push(Command::Repos(RepoCmd::SwitchWorkspace(name.clone())));
        }
        resp.context_menu(|ui| {
            let mut new = name.clone();
            ui.label(t!("Rename workspace"));
            let r = ui.text_edit_singleline(&mut new);
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                lib.rename_workspace(&name, &new);
                ui.close();
            }
            if name != active && ui.button(t!("Delete workspace")).clicked() {
                lib.delete_workspace(&name);
                ui.close();
            }
        });
    }
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut m.new_workspace)
                .hint_text(t!("New workspace"))
                .desired_width(150.0),
        );
        let name = crate::repos::clean_workspace_name(&m.new_workspace);
        if ui
            .add_enabled(!name.is_empty(), egui::Button::new(t!("Add")))
            .clicked()
        {
            lib.add_workspace(&name);
            m.new_workspace.clear();
        }
    });
}

fn sources(ui: &mut egui::Ui, lib: &mut Library, scanning: bool, cmds: &mut Commands) {
    ui.strong(t!("Source folders"));
    ui.weak(t!(
        "Folders where your repositories live. Ferrum lists every repository found in them."
    ));
    let mut remove = None;
    let mut changed = false;
    for (i, s) in lib.sources.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(s.path.display().to_string());
            if ui.small_button("×").on_hover_text(t!("Remove")).clicked() {
                remove = Some(i);
            }
        });
        ui.horizontal(|ui| {
            ui.weak(t!("Depth"));
            let r = ui
                .add(egui::DragValue::new(&mut s.depth).range(1..=6))
                .on_hover_text(t!("How many folders deep to look"));
            changed |= r.drag_stopped() || r.lost_focus();
        });
    }
    if let Some(i) = remove {
        lib.sources.remove(i);
        changed = true;
    }
    ui.horizontal(|ui| {
        if ui.button(t!("+ Add folder…")).clicked()
            && let Some(p) = rfd::FileDialog::new().pick_folder()
        {
            lib.sources.push(SourceFolder {
                path: p,
                ..Default::default()
            });
            changed = true;
        }
        if scanning {
            ui.spinner();
            ui.weak(t!("Scanning…"));
        } else if !lib.sources.is_empty()
            && ui
                .button(t!("↻ Rescan"))
                .on_hover_text(t!("Look again for new repositories"))
                .clicked()
        {
            changed = true;
        }
    });
    if changed {
        cmds.push(Command::Repos(RepoCmd::Scan));
    }
}

/// Entries of the toolbar's Repository menu.
pub fn repository_menu(ui: &mut egui::Ui, has_tabs: bool, cmds: &mut Commands) {
    let mut item = |ui: &mut egui::Ui, text: String, cmd: RepoCmd| {
        if ui.button(text).clicked() {
            cmds.push(Command::Repos(cmd));
            ui.close();
        }
    };
    item(
        ui,
        t!("Open Quickly…  (Ctrl+Shift+O)"),
        RepoCmd::OpenQuickly,
    );
    item(ui, t!("Repository Manager  (Ctrl+T)"), RepoCmd::NewTab);
    if has_tabs {
        item(ui, t!("Repository settings…"), RepoCmd::Settings);
    }
}

type Make = Box<dyn Fn() -> Command>;

/// Command palette entries: Open Quickly, tabs and "Workspace: X".
pub fn palette_entries(settings: &Settings, has_tab: bool) -> Vec<(String, &'static str, Make)> {
    let repos = |c: fn() -> RepoCmd| -> Make { Box::new(move || Command::Repos(c())) };
    let mut list: Vec<(String, &'static str, Make)> = vec![
        (
            t!("Open Quickly…"),
            "Ctrl+Shift+O",
            repos(|| RepoCmd::OpenQuickly),
        ),
        (
            t!("Repository Manager"),
            "Ctrl+T",
            repos(|| RepoCmd::NewTab),
        ),
        (
            t!("Reopen closed tab"),
            "Ctrl+Shift+T",
            repos(|| RepoCmd::ReopenClosed),
        ),
        (t!("Rescan source folders"), "", repos(|| RepoCmd::Scan)),
    ];
    if has_tab {
        list.push((t!("Repository settings…"), "", repos(|| RepoCmd::Settings)));
        list.push((
            t!("Show repository overview"),
            "",
            Box::new(|| Command::SetView(crate::tab::View::Overview)),
        ));
    }
    let lib = &settings.library;
    if !lib.workspace.is_empty() {
        list.push((
            t!("Workspace: {}", t!("Default workspace")),
            "",
            Box::new(|| Command::Repos(RepoCmd::SwitchWorkspace(String::new()))),
        ));
    }
    for name in lib.workspace_names() {
        if name != lib.workspace {
            let n = name.clone();
            list.push((
                t!("Workspace: {}", name),
                "",
                Box::new(move || Command::Repos(RepoCmd::SwitchWorkspace(n.clone()))),
            ));
        }
    }
    list
}
