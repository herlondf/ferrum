//! The Activity window: every operation with the exact git commands it ran
//! and their output (opened from the status bar, see `status_bar.rs`).

use std::path::Path;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Color32, RichText};
use ferrum_core::cli::Stream;

use crate::jobs::{self, Operation, Outcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    User,
    Background,
}

#[derive(Default)]
pub struct ActivityWindow {
    pub open: bool,
    filter: Filter,
    /// Show operations of every repository, not only the current one.
    all_repos: bool,
    selected: Option<u64>,
}

impl ActivityWindow {
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }
}

/// Short local time of day, `14:21:13`.
fn clock(t: SystemTime) -> String {
    static OFFSET: LazyLock<i64> = LazyLock::new(|| ferrum_core::local_offset_minutes() as i64);
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let s = (secs + *OFFSET * 60).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
}

fn date_time(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let day = crate::util::format_time(secs, ferrum_core::local_offset_minutes());
    let day = day.split(' ').next().unwrap_or_default().to_owned();
    format!("{day} {}", clock(t))
}

fn duration(d: Duration) -> String {
    if d.as_secs() >= 60 {
        format!("{}m {:02}s", d.as_secs() / 60, d.as_secs() % 60)
    } else {
        format!("{:.1}s", d.as_secs_f32())
    }
}

/// One line saying how it went, for the list.
fn summary(op: &Operation) -> String {
    match op.outcome() {
        None => {
            let p = op.progress();
            if p.is_empty() { t!("Running…") } else { p }
        }
        Some((_, Outcome::Failed(e))) => e.lines().next().unwrap_or_default().to_owned(),
        Some((_, Outcome::Cancelled)) => t!("Cancelled"),
        Some((_, Outcome::Succeeded)) => op
            .commands()
            .last()
            .and_then(|c| {
                c.output
                    .iter()
                    .rev()
                    .flat_map(|(_, text)| text.lines().rev())
                    .find(|l| !l.trim().is_empty())
                    .map(|l| l.trim().to_owned())
            })
            .unwrap_or_else(|| t!("Done")),
    }
}

fn status_icon(ui: &mut egui::Ui, op: &Operation) {
    match op.outcome() {
        None => {
            ui.spinner();
        }
        Some((_, Outcome::Succeeded)) => {
            ui.colored_label(crate::util::added_color(ui), "✔");
        }
        Some((_, Outcome::Failed(_))) => {
            ui.colored_label(ui.visuals().error_fg_color, "⚠");
        }
        Some((_, Outcome::Cancelled)) => {
            ui.weak("✖");
        }
    }
}

fn status_text(op: &Operation) -> String {
    match op.outcome() {
        None => t!("running since {}", clock(op.started)),
        Some((at, Outcome::Succeeded)) => t!("succeeded {}", date_time(at)),
        Some((at, Outcome::Failed(_))) => t!("failed {}", date_time(at)),
        Some((at, Outcome::Cancelled)) => t!("cancelled {}", date_time(at)),
    }
}

/// The operations shown for a filter, newest first.
pub fn visible(ops: &[Arc<Operation>], filter: Filter, repo: Option<&Path>) -> Vec<Arc<Operation>> {
    ops.iter()
        .rev()
        .filter(|o| match filter {
            Filter::All => true,
            Filter::User => !o.background,
            Filter::Background => o.background,
        })
        // Operations without a repository (clone…) show everywhere.
        .filter(|o| repo.is_none_or(|r| o.repo.as_deref().is_none_or(|p| p == r)))
        .cloned()
        .collect()
}

pub fn show(ctx: &egui::Context, win: &mut ActivityWindow, repo: Option<&Path>) {
    if !win.open {
        return;
    }
    let mut open = true;
    let ops = jobs::operations();
    egui::Window::new(t!("Activity"))
        .id(egui::Id::unique("activity-window"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([860.0, 440.0])
        .pivot(egui::Align2::CENTER_TOP)
        .default_pos(ctx.content_rect().center_top() + egui::vec2(0.0, 70.0))
        .min_size([520.0, 240.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (f, name) in [
                    (Filter::All, t!("All")),
                    (Filter::User, t!("User")),
                    (Filter::Background, t!("Background")),
                ] {
                    ui.selectable_value(&mut win.filter, f, name);
                }
                ui.separator();
                ui.checkbox(&mut win.all_repos, t!("All repositories"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(t!("Clear finished")).clicked() {
                        jobs::clear_finished();
                    }
                });
            });
            ui.separator();
            let list = visible(&ops, win.filter, repo.filter(|_| !win.all_repos));
            if win
                .selected
                .is_none_or(|id| !list.iter().any(|o| o.id == id))
            {
                win.selected = list.first().map(|o| o.id);
            }
            if list.is_empty() {
                ui.weak(t!("No operations yet."));
                return;
            }
            egui::Panel::left("activity-list")
                .resizable(true)
                .default_size(300.0)
                .min_size(180.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("activity-list-scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for op in &list {
                                operation_row(ui, op, &mut win.selected);
                            }
                        });
                });
            let selected = list.iter().find(|o| Some(o.id) == win.selected);
            egui::CentralPanel::default().show(ui, |ui| {
                if let Some(op) = selected {
                    details(ui, op);
                }
            });
        });
    if !open {
        win.open = false;
    }
    if ops.iter().any(|o| o.running()) {
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

fn operation_row(ui: &mut egui::Ui, op: &Operation, selected: &mut Option<u64>) {
    let is_selected = *selected == Some(op.id);
    let resp = egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(4, 3))
        .fill(if is_selected {
            ui.visuals().selection.bg_fill
        } else {
            Color32::TRANSPARENT
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                status_icon(ui, op);
                ui.add(egui::Label::new(RichText::new(&op.label).strong()).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(clock(op.started));
                });
            });
            ui.add(egui::Label::new(RichText::new(summary(op)).weak().small()).truncate());
        })
        .response
        .interact(egui::Sense::click());
    if resp.clicked() {
        *selected = Some(op.id);
    }
}

pub(crate) fn details(ui: &mut egui::Ui, op: &Operation) {
    let commands = op.commands();
    ui.horizontal(|ui| {
        ui.label(RichText::new(&op.label).strong().size(15.0));
        if op.background {
            ui.weak(t!("(background)"));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !commands.is_empty() && ui.small_button(t!("Copy output")).clicked() {
                let mut text = String::new();
                for c in &commands {
                    text.push_str(&format!("$ {}\n", c.command));
                    for (_, line) in &c.output {
                        text.push_str(line);
                        text.push('\n');
                    }
                }
                ui.ctx().copy_text(text);
            }
            if op.running()
                && op.cancellable
                && !op.cancel.is_cancelled()
                && ui.button(t!("✖ Cancel")).clicked()
            {
                op.cancel.cancel();
            }
            let color = match op.outcome() {
                Some((_, Outcome::Failed(_))) => ui.visuals().error_fg_color,
                Some((_, Outcome::Succeeded)) => crate::util::added_color(ui),
                _ => ui.visuals().weak_text_color(),
            };
            ui.colored_label(color, status_text(op));
        });
    });
    if let Some(repo) = &op.repo {
        ui.weak(repo.display().to_string());
    }
    ui.separator();
    egui::ScrollArea::both()
        .id_salt("activity-output")
        .auto_shrink([false, false])
        .stick_to_bottom(op.running())
        .show(ui, |ui| {
            if commands.is_empty() {
                match op.outcome() {
                    Some((_, Outcome::Failed(e))) => {
                        ui.colored_label(ui.visuals().error_fg_color, e);
                    }
                    _ => {
                        ui.weak(t!("No git commands were run."));
                    }
                }
            }
            for c in &commands {
                let mono = |t: &str| RichText::new(t).monospace();
                ui.label(mono(&format!("$ {}", c.command)).strong());
                let failed = c.finished.is_some() && !c.cancelled && c.exit != Some(0);
                for (stream, text) in &c.output {
                    let color = match stream {
                        Stream::Stdout => ui.visuals().text_color(),
                        Stream::Stderr if failed => ui.visuals().error_fg_color,
                        Stream::Stderr => ui.visuals().warn_fg_color,
                    };
                    ui.label(mono(text).color(color));
                }
                let took = c
                    .finished
                    .and_then(|f| f.duration_since(c.started).ok())
                    .map(duration);
                let end = match (c.cancelled, c.exit, took) {
                    (true, _, _) => t!("cancelled"),
                    (_, Some(code), Some(took)) => t!("exit code {} · {}", code, took),
                    (_, None, Some(_)) => t!("did not start"),
                    _ => t!("running…"),
                };
                ui.weak(RichText::new(end).small());
                ui.add_space(8.0);
            }
            if let Some((_, Outcome::Failed(e))) = op.outcome()
                && !commands.is_empty()
                && commands.iter().all(|c| c.exit == Some(0))
            {
                // Failed outside git (e.g. a forge request).
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_times() {
        assert_eq!(duration(Duration::from_millis(1500)), "1.5s");
        assert_eq!(duration(Duration::from_secs(75)), "1m 15s");
        assert_eq!(clock(UNIX_EPOCH).len(), 8);
    }
}
