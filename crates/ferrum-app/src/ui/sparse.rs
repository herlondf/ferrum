//! Sparse checkout (Insights > Sparse checkout): pick the folders of a large
//! repository to check out, in cone mode (a folder brings its subfolders;
//! files at the root are always there).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::{self, Receiver};

use eframe::egui::{self, RichText};
use ferrum_core::sparse::{self, SparseState};

use crate::cleanup_ops::CleanupOp;
use crate::command::{Commands, Op};
use crate::icons;
use crate::tab::RepoTab;
use crate::theme;

type Loaded = Result<(SparseState, Vec<String>), String>;

#[derive(Default)]
pub struct SparseUi {
    loaded: Option<Loaded>,
    loading: Option<Receiver<Loaded>>,
    loaded_at: Option<Option<ferrum_core::Oid>>,
    /// The folders picked (outermost ones count).
    pub picked: BTreeSet<String>,
    open: BTreeSet<String>,
    filter: String,
}

impl SparseUi {
    #[cfg(test)]
    pub fn folders(&self) -> Option<&[String]> {
        Some(self.loaded.as_ref()?.as_ref().ok()?.1.as_slice())
    }
}

fn poll(tab: &mut RepoTab, ctx: &egui::Context) {
    let st = &mut tab.insights.sparse;
    if let Some(rx) = &st.loading
        && let Ok(r) = rx.try_recv()
    {
        if let Ok((s, _)) = &r {
            st.picked = s.folders.iter().cloned().collect();
        }
        st.loaded = Some(r);
        st.loading = None;
    }
    // Again after a checkout or an apply (the job refreshes the tab).
    let key = Some(tab.head_oid);
    if st.loaded_at != key && st.loading.is_none() && tab.job.is_none() {
        st.loaded_at = key;
        let path = tab.repo.path().to_path_buf();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = ferrum_core::Repo::open(&path)
                .and_then(|repo| Ok((sparse::state(&repo)?, sparse::folders(&path)?)))
                .map_err(|e| e.to_string());
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        st.loading = Some(rx);
    }
}

pub fn show(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    poll(tab, ui.ctx());
    let busy = tab.busy();
    let st = &mut tab.insights.sparse;
    let (state, folders) = match &st.loaded {
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(t!("Reading the folders…"));
            });
            return;
        }
        Some(Err(e)) => {
            ui.colored_label(ui.visuals().error_fg_color, e);
            return;
        }
        Some(Ok((s, f))) => (s.clone(), f.clone()),
    };
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(icons::TREE_VIEW)
                    .size(16.0)
                    .color(theme::accent(ui)),
            );
            ui.label(theme::strong(t!("Sparse checkout")).size(14.0));
            let (text, color) = if !state.enabled {
                (t!("Off: every folder is checked out"), ui.visuals().weak_text_color())
            } else if state.cone {
                (
                    t!("On (cone mode): {} folders", state.folders.len()),
                    theme::accent(ui),
                )
            } else {
                (
                    t!("On, with patterns (not cone mode)"),
                    ui.visuals().warn_fg_color,
                )
            };
            ui.label(RichText::new(text).color(color));
        });
        ui.weak(t!(
            "Check out only the folders you work on: large repositories get faster. Files at the root are always checked out; a folder brings all of its subfolders."
        ));
        if state.enabled && !state.cone {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                t!("Applying a folder list switches to cone mode and replaces these patterns."),
            );
        }
    });
    ui.add_space(theme::SPACE_M);
    if folders.is_empty() {
        crate::ui::empty_state::show(
            ui,
            icons::FOLDER_OPEN,
            &t!("No folders"),
            &t!("Everything is at the root of the repository."),
            |_| {},
        );
        return;
    }
    let chosen = sparse::outermost(&st.picked.iter().cloned().collect::<Vec<_>>());
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut st.filter)
                .hint_text(t!("Filter folders"))
                .desired_width(220.0),
        );
        ui.weak(t!("{} of {} folders picked", chosen.len(), folders.len()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = icons::text(icons::CHECK, t!("Check out {} folders", chosen.len()));
            let changed = chosen != sparse::outermost(&state.folders) || !state.enabled;
            if ui
                .add_enabled(!busy && changed, theme::primary(label))
                .on_hover_text("git sparse-checkout set --cone")
                .clicked()
            {
                cmds.run(Op::Cleanup(CleanupOp::SparseSet(chosen.clone())));
                st.loaded_at = None;
            }
            if state.enabled
                && ui
                    .add_enabled(!busy, theme::secondary(t!("Check out everything")))
                    .clicked()
            {
                cmds.run(Op::Cleanup(CleanupOp::SparseDisable));
                st.loaded_at = None;
            }
            if ui.button(t!("Clear")).clicked() {
                st.picked.clear();
            }
        });
    });
    ui.add_space(theme::SPACE_S);
    let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for f in &folders {
        let parent = f.rsplit_once('/').map_or("", |(p, _)| p);
        children.entry(parent).or_default().push(f);
    }
    theme::card(ui)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    let q = st.filter.trim().to_lowercase();
                    if q.is_empty() {
                        tree(ui, st, &children, "", 0);
                    } else {
                        for f in folders
                            .iter()
                            .filter(|f| f.to_lowercase().contains(&q))
                            .take(500)
                        {
                            row(ui, st, f, f, 0, false, false);
                        }
                    }
                });
        });
}

fn tree(
    ui: &mut egui::Ui,
    st: &mut SparseUi,
    children: &BTreeMap<&str, Vec<&str>>,
    parent: &str,
    depth: usize,
) {
    let Some(list) = children.get(parent) else {
        return;
    };
    for f in list.clone() {
        let has_children = children.contains_key(f);
        let name = f.rsplit('/').next().unwrap_or(f);
        let open = row(ui, st, f, name, depth, has_children, true);
        if open {
            tree(ui, st, children, f, depth + 1);
        }
    }
}

/// One folder with its checkbox; returns whether it is expanded.
fn row(
    ui: &mut egui::Ui,
    st: &mut SparseUi,
    path: &str,
    name: &str,
    depth: usize,
    has_children: bool,
    expandable: bool,
) -> bool {
    // A picked folder above this one covers it.
    let covered = st.picked.iter().any(|p| path.starts_with(&format!("{p}/")));
    let mut open = st.open.contains(path);
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * 18.0);
        // The caret (or room for it), the same width on every row.
        let (rect, caret) = ui.allocate_exact_size(egui::vec2(16.0, 18.0), egui::Sense::click());
        if expandable && has_children {
            let icon = if open {
                icons::EXPANDED
            } else {
                icons::COLLAPSED
            };
            let color = if caret.hovered() {
                ui.visuals().strong_text_color()
            } else {
                ui.visuals().weak_text_color()
            };
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                icon,
                egui::FontId::proportional(12.0),
                color,
            );
            if caret.clicked() {
                open = !open;
                if open {
                    st.open.insert(path.to_owned());
                } else {
                    st.open.remove(path);
                }
            }
        }
        let mut on = covered || st.picked.contains(path);
        let r = ui.add_enabled(!covered, egui::Checkbox::new(&mut on, ""));
        if r.changed() {
            if on {
                st.picked.insert(path.to_owned());
            } else {
                st.picked.remove(path);
            }
        }
        let color = if on {
            ui.visuals().strong_text_color()
        } else {
            ui.visuals().text_color()
        };
        ui.label(RichText::new(icons::text(icons::FOLDER, name)).color(color))
            .on_hover_text(path);
        if covered {
            ui.weak(t!("(included)"));
        }
    });
    open
}
