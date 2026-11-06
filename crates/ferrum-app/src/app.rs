use std::path::PathBuf;

use eframe::egui;
use ferrum_core::{BranchInfo, BranchKind, CommitInfo, Repo};

const COMMIT_LIMIT: usize = 5_000;

/// One open repository (one tab).
struct RepoTab {
    repo: Repo,
    head: Option<String>,
    branches: Vec<BranchInfo>,
    tags: Vec<String>,
    commits: Vec<CommitInfo>,
    selected: Option<usize>,
}

impl RepoTab {
    fn open(path: PathBuf) -> Result<Self, String> {
        let repo = Repo::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut tab = Self {
            repo,
            head: None,
            branches: Vec::new(),
            tags: Vec::new(),
            commits: Vec::new(),
            selected: None,
        };
        tab.refresh()?;
        Ok(tab)
    }

    fn refresh(&mut self) -> Result<(), String> {
        let err = |e: ferrum_core::Error| e.to_string();
        self.head = self.repo.head_branch();
        self.branches = self.repo.branches().map_err(err)?;
        self.tags = self.repo.tags().map_err(err)?;
        self.commits = self.repo.commits(COMMIT_LIMIT).map_err(err)?;
        self.selected = self.selected.filter(|&i| i < self.commits.len());
        Ok(())
    }
}

pub struct FerrumApp {
    tabs: Vec<RepoTab>,
    active: usize,
    error: Option<String>,
}

impl FerrumApp {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        let mut app = Self {
            tabs: Vec::new(),
            active: 0,
            error: None,
        };
        for path in paths {
            app.open(path);
        }
        app
    }

    fn open(&mut self, path: PathBuf) {
        match RepoTab::open(path) {
            Ok(tab) => {
                // Reuse the tab if this repository is already open.
                if let Some(i) = self
                    .tabs
                    .iter()
                    .position(|t| t.repo.path() == tab.repo.path())
                {
                    self.active = i;
                } else {
                    self.tabs.push(tab);
                    self.active = self.tabs.len() - 1;
                }
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("📂 Open").clicked()
                && let Some(path) = rfd::FileDialog::new().pick_folder()
            {
                self.open(path);
            }
            let has_tab = !self.tabs.is_empty();
            if ui
                .add_enabled(has_tab, egui::Button::new("⟳ Refresh"))
                .clicked()
                && let Err(e) = self.tabs[self.active].refresh()
            {
                self.error = Some(e);
            }
            // Planned actions, disabled until implemented.
            for label in ["⬇ Fetch", "⤓ Pull", "⤒ Push", "⑂ Branch", "▤ Stash"] {
                ui.add_enabled(false, egui::Button::new(label));
            }
        });
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        let mut close = None;
        ui.horizontal(|ui| {
            for (i, tab) in self.tabs.iter().enumerate() {
                if ui
                    .selectable_label(i == self.active, tab.repo.name())
                    .clicked()
                {
                    self.active = i;
                }
                if ui.small_button("✕").clicked() {
                    close = Some(i);
                }
                ui.separator();
            }
        });
        if let Some(i) = close {
            self.tabs.remove(i);
            self.active = self.active.min(self.tabs.len().saturating_sub(1));
        }
    }
}

fn sidebar(ui: &mut egui::Ui, tab: &RepoTab) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        let section = |ui: &mut egui::Ui, title: &str, kind: BranchKind| {
            egui::CollapsingHeader::new(title)
                .default_open(true)
                .show(ui, |ui| {
                    for b in tab.branches.iter().filter(|b| b.kind == kind) {
                        let text = egui::RichText::new(&b.name);
                        let text = if b.is_head { text.strong() } else { text };
                        ui.label(text).on_hover_text(
                            b.upstream
                                .as_deref()
                                .map_or("no upstream".into(), |u| format!("tracks {u}")),
                        );
                    }
                });
        };
        section(ui, "Branches", BranchKind::Local);
        section(ui, "Remotes", BranchKind::Remote);
        egui::CollapsingHeader::new("Tags").show(ui, |ui| {
            for t in &tab.tags {
                ui.label(t);
            }
        });
    });
}

fn commit_list(ui: &mut egui::Ui, tab: &mut RepoTab) {
    let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(
        ui,
        row_height,
        tab.commits.len(),
        |ui, range| {
            for i in range {
                let c = &tab.commits[i];
                let selected = tab.selected == Some(i);
                let text = format!(
                    "{}   {}   {}   {}",
                    c.short_id(),
                    c.summary,
                    c.author,
                    format_time(c.time)
                );
                let resp = ui.add_sized(
                    [ui.available_width(), row_height],
                    egui::Button::selectable(selected, text).frame(false),
                );
                if resp.clicked() {
                    tab.selected = Some(i);
                }
            }
        },
    );
}

fn commit_details(ui: &mut egui::Ui, tab: &RepoTab) {
    let Some(c) = tab.selected.and_then(|i| tab.commits.get(i)) else {
        ui.weak("Select a commit to see its details.");
        return;
    };
    egui::Grid::new("commit-details")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.strong("Commit");
            ui.monospace(c.id.to_string());
            ui.end_row();
            ui.strong("Author");
            ui.label(format!("{} <{}>", c.author, c.email));
            ui.end_row();
            ui.strong("Date");
            ui.label(format_time(c.time));
            ui.end_row();
            ui.strong("Parents");
            ui.monospace(
                c.parents
                    .iter()
                    .map(|p| p.to_string()[..7].to_owned())
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            ui.end_row();
        });
    ui.separator();
    ui.label(&c.summary);
}

/// Formats Unix seconds as `YYYY-MM-DD HH:MM` (UTC). Local time comes later.
fn format_time(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil-from-days algorithm (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60
    )
}

impl eframe::App for FerrumApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("toolbar").show(ui, |ui| {
            self.toolbar(ui);
            if !self.tabs.is_empty() {
                ui.separator();
                self.tab_bar(ui);
            }
        });

        if let Some(err) = self.error.clone() {
            egui::Panel::bottom("error").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(ui.visuals().error_fg_color, err);
                    if ui.small_button("✕").clicked() {
                        self.error = None;
                    }
                });
            });
        }

        let Some(tab) = self.tabs.get_mut(self.active) else {
            egui::CentralPanel::default_margins().show(ui, |ui| {
                ui.centered_and_justified(|ui| ui.heading("Open a repository to get started"));
            });
            return;
        };

        egui::Panel::left("sidebar")
            .default_size(220.0)
            .show(ui, |ui| {
                ui.heading(tab.head.as_deref().unwrap_or("(detached)"));
                ui.separator();
                sidebar(ui, tab);
            });
        egui::Panel::bottom("details")
            .resizable(true)
            .default_size(180.0)
            .show(ui, |ui| {
                commit_details(ui, tab);
            });
        egui::CentralPanel::default_margins().show(ui, |ui| commit_list(ui, tab));
    }
}

#[cfg(test)]
mod tests {
    use super::format_time;

    #[test]
    fn formats_unix_time() {
        assert_eq!(format_time(0), "1970-01-01 00:00");
        assert_eq!(format_time(1_700_000_000), "2023-11-14 22:13");
    }
}
