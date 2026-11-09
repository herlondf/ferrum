//! The tab bar (a row on top, or a list on the left) and the tab menu.

use eframe::egui::{self, Color32, RichText};

use crate::command::{Command, Commands};
use crate::external::Launch;
use crate::icons;
use crate::repos::{Library, RepoCmd, TAB_COLORS};
use crate::tab::RepoTab;

/// What the tab bar shows.
pub struct Bar<'a> {
    pub tabs: &'a [RepoTab],
    /// The selected repository tab (`None` while the Repository Manager tab is).
    pub active: Option<usize>,
    /// The Repository Manager tab is open (Ctrl+T).
    pub home: bool,
    /// Tabs shown in windows of their own.
    pub in_window: &'a [std::path::PathBuf],
}

/// Labels of all tabs, with same-named tabs told apart.
pub fn labels(tabs: &[RepoTab], lib: &Library) -> Vec<String> {
    let items: Vec<(String, std::path::PathBuf)> = tabs
        .iter()
        .map(|t| (crate::repos::base_label(lib, t), t.path().to_path_buf()))
        .collect();
    crate::repos::unique_labels(&items)
}

pub fn color32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Draws the tabs in a wrapping row.
pub fn row(ui: &mut egui::Ui, bar: &Bar, lib: &mut Library, cmds: &mut Commands) {
    ui.horizontal_wrapped(|ui| {
        tabs(ui, bar, lib, cmds, false);
    });
}

/// Draws the tabs as a list (Settings: vertical tabs).
pub fn list(ui: &mut egui::Ui, bar: &Bar, lib: &mut Library, cmds: &mut Commands) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
            tabs(ui, bar, lib, cmds, true);
        });
    });
}

fn tabs(ui: &mut egui::Ui, bar: &Bar, lib: &mut Library, cmds: &mut Commands, vertical: bool) {
    let names = labels(bar.tabs, lib);
    for (i, tab) in bar.tabs.iter().enumerate() {
        let mut label = names[i].clone();
        if let Some(b) = &tab.head
            && !vertical
        {
            label.push_str(&format!("  {} {b}", icons::BRANCH));
        }
        if tab.busy() {
            label.push_str(&format!(" {}", icons::HOURGLASS));
        }
        if bar.in_window.iter().any(|p| p == tab.path()) {
            label.push_str(&format!(" {}", icons::ARROW_SQUARE_OUT));
        }
        let mut text = RichText::new(label);
        let dirty = tab.status.total() > 0;
        let selected = bar.active == Some(i);
        if selected {
            text = text.strong();
        }
        let resp = ui.horizontal(|ui| {
            let resp = ui.add(egui::Button::selectable(selected, text));
            if dirty {
                // A dot: uncommitted changes.
                let (rect, r) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(rect.center(), 3.5, ui.visuals().warn_fg_color);
                r.on_hover_text(t!("Uncommitted changes"));
            }
            if ui
                .small_button(icons::CLOSE)
                .on_hover_text(t!("Close (Ctrl+W)"))
                .clicked()
            {
                cmds.push(Command::CloseTab(i));
            }
            resp
        });
        let resp = resp.inner;
        if let Some(c) = lib.color(tab.path()) {
            let r = resp.rect;
            let stripe = if vertical {
                egui::Rect::from_min_max(r.left_top(), egui::pos2(r.left() + 3.0, r.bottom()))
            } else {
                egui::Rect::from_min_max(egui::pos2(r.left(), r.bottom() - 3.0), r.right_bottom())
            };
            ui.painter().rect_filled(stripe, 1.0, color32(c));
        }
        let mut tip = tab.path().display().to_string();
        if let Some(b) = &tab.head {
            tip.push_str(&format!("\n{} {b}", icons::BRANCH));
        }
        if dirty {
            tip.push_str(&format!("\n{}", t!("{} changed files", tab.status.total())));
        }
        let resp = resp.on_hover_text(tip);
        if resp.clicked() {
            cmds.push(Command::SelectTab(i));
        }
        if resp.middle_clicked() {
            cmds.push(Command::CloseTab(i));
        }
        resp.context_menu(|ui| menu(ui, i, tab, &names[i], lib, cmds));
        if !vertical {
            ui.separator();
        }
    }
    if bar.home {
        let selected = bar.active.is_none();
        ui.horizontal(|ui| {
            let resp = ui.add(egui::Button::selectable(
                selected,
                RichText::new(t!("📁 Repositories")).strong(),
            ));
            if resp.clicked() {
                cmds.push(Command::Repos(RepoCmd::NewTab));
            }
            if resp.middle_clicked()
                || ui
                    .small_button(icons::CLOSE)
                    .on_hover_text(t!("Close (Ctrl+W)"))
                    .clicked()
            {
                cmds.push(Command::Repos(RepoCmd::CloseHome));
            }
        });
    } else if ui
        .small_button(icons::ADD)
        .on_hover_text(t!("New tab (Ctrl+T)"))
        .clicked()
    {
        cmds.push(Command::Repos(RepoCmd::NewTab));
    }
}

fn menu(
    ui: &mut egui::Ui,
    i: usize,
    tab: &RepoTab,
    label: &str,
    lib: &mut Library,
    cmds: &mut Commands,
) {
    let path = tab.path().to_path_buf();
    let mut act = |ui: &mut egui::Ui, text: String, cmd: Command| {
        if ui.button(text).clicked() {
            cmds.push(cmd);
            ui.close();
        }
    };
    act(ui, t!("Close"), Command::CloseTab(i));
    act(
        ui,
        t!("Close Others"),
        Command::Repos(RepoCmd::CloseOthers(i)),
    );
    act(ui, t!("Close All"), Command::Repos(RepoCmd::CloseAll));
    ui.separator();
    ui.menu_button(t!("Rename…"), |ui| {
        let entry = lib.entry(&path);
        let resp = ui.add(
            egui::TextEdit::singleline(&mut entry.name)
                .hint_text(label)
                .desired_width(200.0),
        );
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            ui.close();
        }
        if ui.button(t!("Use the folder name")).clicked() {
            entry.name.clear();
            ui.close();
        }
    });
    ui.menu_button(t!("🎨 Color"), |ui| {
        ui.horizontal(|ui| {
            for &c in TAB_COLORS {
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
                ui.painter().rect_filled(rect, 3.0, color32(c));
                if lib.color(&path) == Some(c) {
                    ui.painter().rect_stroke(
                        rect,
                        3.0,
                        egui::Stroke::new(2.0, ui.visuals().strong_text_color()),
                        egui::StrokeKind::Outside,
                    );
                }
                if resp.clicked() {
                    lib.entry(&path).color = Some(c);
                    ui.close();
                }
            }
        });
        if ui.button(t!("No color")).clicked() {
            lib.entry(&path).color = None;
            ui.close();
        }
    });
    ui.separator();
    act(
        ui,
        t!("Open in terminal"),
        Command::External(Launch::Terminal { dir: path.clone() }),
    );
    act(
        ui,
        t!("Show in file manager"),
        Command::External(Launch::Reveal { path: path.clone() }),
    );
    act(
        ui,
        t!("Copy path"),
        Command::Copy(path.display().to_string()),
    );
    act(
        ui,
        t!("Open in new window"),
        Command::Repos(RepoCmd::OpenInWindow(i)),
    );
    act(
        ui,
        t!("Repository settings…"),
        Command::Repos(RepoCmd::Settings),
    );
    ui.separator();
    ui.menu_button(t!("Move to workspace"), |ui| {
        for name in lib.workspace_names() {
            if name != lib.workspace && ui.button(&name).clicked() {
                cmds.push(Command::Repos(RepoCmd::MoveToWorkspace(i, name)));
                ui.close();
            }
        }
        if !lib.workspace.is_empty() && ui.button(t!("Default workspace")).clicked() {
            cmds.push(Command::Repos(RepoCmd::MoveToWorkspace(i, String::new())));
            ui.close();
        }
        let id = ui.scope_id().with("new-ws");
        let mut name: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
        let resp = ui.add(
            egui::TextEdit::singleline(&mut name)
                .hint_text(t!("New workspace"))
                .desired_width(180.0),
        );
        if resp.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && !crate::repos::clean_workspace_name(&name).is_empty()
        {
            cmds.push(Command::Repos(RepoCmd::MoveToWorkspace(i, name.clone())));
            name.clear();
            ui.close();
        }
        ui.data_mut(|d| d.insert_temp(id, name));
    });
    ui.checkbox(&mut lib.vertical_tabs, t!("Vertical tab list"));
}
