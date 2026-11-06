//! Open Quickly (Ctrl+Shift+O): fuzzy search over every known repository.

use std::path::PathBuf;

use eframe::egui;

use crate::command::{Command, Commands};
use crate::settings::Settings;
use crate::ui::palette::fuzzy_score;
use crate::ui::repo_manager::display_name;

#[derive(Default)]
pub struct OpenQuickly {
    pub open: bool,
    query: String,
    selected: usize,
    just_opened: bool,
}

impl OpenQuickly {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.query.clear();
        self.selected = 0;
        self.just_opened = self.open;
    }

    pub fn set_query(&mut self, q: &str) {
        self.query = q.to_owned();
    }
}

/// Listed, recent and scanned repositories, without duplicates.
pub fn candidates(settings: &Settings) -> Vec<PathBuf> {
    let lib = &settings.library;
    let mut out: Vec<PathBuf> = Vec::new();
    let all = settings
        .recent
        .iter()
        .chain(lib.repos.iter().map(|r| &r.path))
        .chain(&lib.found);
    for p in all {
        if !out.contains(p) && p.exists() {
            out.push(p.clone());
        }
    }
    out
}

pub fn show(ctx: &egui::Context, q: &mut OpenQuickly, settings: &Settings, cmds: &mut Commands) {
    if !q.open {
        return;
    }
    let query = q.query.trim().to_lowercase();
    let mut matches: Vec<(i32, PathBuf, String)> = candidates(settings)
        .into_iter()
        .filter_map(|p| {
            let name = display_name(&settings.library, &p);
            // The name counts more than the rest of the path.
            let score = fuzzy_score(&name, &query)
                .map(|s| s + 2000)
                .or_else(|| fuzzy_score(&p.to_string_lossy(), &query))?;
            Some((score, p, name))
        })
        .collect();
    // Stable: equal scores keep the recent-first order.
    matches.sort_by_key(|m| std::cmp::Reverse(m.0));
    q.selected = q.selected.min(matches.len().saturating_sub(1));
    let (up, down, enter, esc) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if up {
        q.selected = q.selected.saturating_sub(1);
    }
    if down && q.selected + 1 < matches.len() {
        q.selected += 1;
    }
    let mut run = None;
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::unique("open-quickly"))
        .fixed_pos(egui::pos2(screen.center().x - 280.0, screen.top() + 80.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(560.0);
                ui.weak(t!("Open Quickly"));
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut q.query)
                        .hint_text(t!("Type a repository name…"))
                        .desired_width(f32::INFINITY),
                );
                if q.just_opened {
                    resp.request_focus();
                    q.just_opened = false;
                }
                if resp.changed() {
                    q.selected = 0;
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(380.0)
                    .show(ui, |ui| {
                        for (i, (_, p, name)) in matches.iter().enumerate() {
                            let selected = i == q.selected;
                            let resp = ui.add(
                                egui::Button::selectable(
                                    selected,
                                    egui::RichText::new(name).strong(),
                                )
                                .right_text(egui::RichText::new(p.display().to_string()).weak())
                                .min_size(egui::vec2(ui.available_width(), 24.0)),
                            );
                            if selected && (up || down) {
                                resp.scroll_to_me(None);
                            }
                            if resp.clicked() {
                                run = Some(i);
                            }
                        }
                        if matches.is_empty() {
                            ui.weak(t!("No matching repository."));
                        }
                    });
            });
        });
    if enter && !matches.is_empty() {
        run = Some(q.selected);
    }
    if let Some(i) = run {
        cmds.push(Command::OpenRepo(matches[i].1.clone()));
        q.open = false;
    }
    if esc {
        q.open = false;
    }
}
