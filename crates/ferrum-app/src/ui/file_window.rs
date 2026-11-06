//! File views in separate windows (native windows where the platform
//! supports them, else floating windows inside the main one), plus the
//! per-tab state of the file tools.

use eframe::egui;

use crate::command::{Command, Commands};
use crate::tab::RepoTab;
use crate::ui::blame::{self, DiffKey, FileView, Place};
use crate::ui::{file_finder, file_lines};

/// Per-tab state of the file history tools.
#[derive(Default)]
pub struct FileTools {
    /// Views shown in their own window.
    pub windows: Vec<FileView>,
    pub finder: Option<file_finder::Finder>,
    /// Loads the tracked files (`git ls-files`) for the finder.
    pub(crate) tracked: blame::Load<Vec<String>>,
    /// The finder's last list of files and folders, kept between uses.
    pub(crate) candidates: Vec<String>,
}

/// Moves a view to its own window.
pub fn detach(tab: &mut RepoTab, view: FileView) {
    tab.file_tools.windows.push(view);
}

/// A command that changes the file view `id` (in the main window or a
/// separate one) of the active tab.
pub fn with_view(id: u64, f: impl FnOnce(&mut FileView) + 'static) -> Command {
    crate::ui::history_options::cmd(move |tab, _| {
        let main = tab.file_view.as_mut().filter(|v| v.id == id);
        if let Some(v) = main.or_else(|| tab.file_tools.windows.iter_mut().find(|v| v.id == id)) {
            f(v);
        }
    })
}

/// What the file view whose diff has the id `diff_id` compares.
pub(crate) fn diff_key_of<'a>(tab: &'a RepoTab, diff_id: &str) -> Option<&'a DiffKey> {
    let main = tab
        .file_view
        .iter()
        .find(|v| v.diff_id(Place::Main) == diff_id);
    let window = || {
        tab.file_tools
            .windows
            .iter()
            .find(|v| v.diff_id(Place::Window) == diff_id)
    };
    main.or_else(window)?.diff_key.as_ref()
}

/// Draws the file tools of every tab: separate windows, the file finder of
/// the active tab and line history requests from diff views. Called once
/// per frame after the main view.
pub fn show_all(ctx: &egui::Context, tabs: &mut [RepoTab], active: usize, cmds: &mut Commands) {
    if let Some(tab) = tabs.get_mut(active) {
        file_lines::take_request(ctx, tab);
        file_finder::show(ctx, tab, cmds);
    }
    for (i, tab) in tabs.iter_mut().enumerate() {
        let mut windows = std::mem::take(&mut tab.file_tools.windows);
        let mut own = Commands::default();
        windows.retain_mut(|view| show_window(ctx, tab, view, &mut own));
        // Views added meanwhile (none expected) are kept too.
        windows.append(&mut tab.file_tools.windows);
        tab.file_tools.windows = windows;
        let list = own.take();
        if !list.is_empty() && i != active {
            // Commands act on the active tab: switch to the window's tab first.
            cmds.push(Command::SelectTab(i));
        }
        for c in list {
            cmds.push(c);
        }
    }
}

/// Shows one window; returns whether it stays open.
fn show_window(
    ctx: &egui::Context,
    tab: &RepoTab,
    view: &mut FileView,
    cmds: &mut Commands,
) -> bool {
    let id = egui::ViewportId::from_hash_of(("ferrum-file-window", view.id));
    let title = format!("{} — {}", view.path, tab.name);
    let builder = egui::ViewportBuilder::default()
        .with_title(title)
        .with_inner_size([1000.0, 700.0])
        .with_min_inner_size([480.0, 320.0]);
    ctx.show_viewport_immediate(id, builder, |ui, class| {
        let out = if class == egui::ViewportClass::EmbeddedWindow {
            // No native windows here: a floating window in the main one.
            ui.set_min_size(egui::vec2(820.0, 520.0));
            blame::show_view(ui, tab, view, cmds, Place::Window)
        } else {
            egui::CentralPanel::default_margins()
                .show(ui, |ui| {
                    blame::show_view(ui, tab, view, cmds, Place::Window)
                })
                .inner
        };
        let closed = ui.input(|i| i.viewport().close_requested());
        !(out.close || closed)
    })
}

/// `FERRUM_START` steps: `lines:PATH:A-B` (history of lines A to B),
/// `finder:TEXT` (File history… with TEXT typed), `file-window` (moves the
/// file view to a separate window).
pub fn dev_step(step: &str, arg: &str) -> Command {
    let (step, arg) = (step.to_owned(), arg.to_owned());
    crate::ui::history_options::cmd(move |tab, _| match step.as_str() {
        "lines" => {
            let (path, range) = arg.rsplit_once(':').unwrap_or((&arg, "1-1"));
            let (a, b) = range.split_once('-').unwrap_or((range, range));
            let (a, b) = (a.parse().unwrap_or(1), b.parse().unwrap_or(1));
            blame::open_mode(tab, path.to_owned(), blame::Mode::Lines);
            if let Some(v) = &mut tab.file_view {
                v.lines = Some(file_lines::LinesState::new(
                    path.into(),
                    a,
                    b,
                    "HEAD".into(),
                ));
                v.mode = blame::Mode::Lines;
            }
        }
        "finder" => {
            file_finder::open(tab);
            if let Some(f) = &mut tab.file_tools.finder {
                f.set_query(&arg);
            }
        }
        "file-window" => {
            if let Some(v) = tab.file_view.take() {
                detach(tab, v);
                tab.view = crate::tab::View::History;
            }
        }
        _ => eprintln!("FERRUM_START: unknown step {step}"),
    })
}
