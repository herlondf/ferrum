//! The status bar: current branch with its upstream and ahead/behind, the
//! repository state, the running operation (spinner, progress, cancel), and
//! on the right the graph filter, LFS, the last fetch, the identity and the
//! Activity window. Messages are shown as toasts (`toasts.rs`).

use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::{self, RichText};
use ferrum_core::RepoState;

use crate::command::{Command, Commands};
use crate::icons;
use crate::jobs::Job;
use crate::tab::RepoTab;
use crate::theme;
use crate::ui::dialogs::Dialog;
use crate::ui::history_options;

/// Text size of the status bar.
const SIZE: f32 = 12.0;

fn small(text: impl Into<String>) -> RichText {
    RichText::new(text).size(SIZE)
}

/// A flat button that only shows a frame on hover.
fn flat(text: impl Into<egui::WidgetText>) -> egui::Button<'static> {
    egui::Button::new(text).frame_when_inactive(false).small()
}

pub fn show(
    ui: &mut egui::Ui,
    tab: Option<&RepoTab>,
    global_job: Option<&Job>,
    cmds: &mut Commands,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SPACE_M;
        if let Some(tab) = tab {
            branch(ui, tab, cmds);
            state(ui, tab.state);
        }
        let job = tab.and_then(|t| t.job.as_ref()).or(global_job);
        if let Some(job) = job {
            running(ui, job, tab.map_or(0, |t| t.queue.len()), cmds);
        } else if tab.is_some_and(|t| t.live.refreshing()) {
            ui.add(egui::Spinner::new().size(12.0));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(flat(small(icons::PULSE)))
                .on_hover_text(t!("Activity: operations and their git commands"))
                .clicked()
            {
                cmds.push(Command::ShowActivity);
            }
            let Some(tab) = tab else { return };
            identity(ui, tab, cmds);
            last_fetch(ui, tab);
            if tab.uses_lfs {
                ui.label(small("LFS").weak())
                    .on_hover_text(t!("This repository uses Git LFS"));
            }
            // Leaves the room left of the identity to the graph filter.
            ui.scope(|ui| {
                ui.set_max_width(ui.available_width() * 0.5);
                history_options::filter_status(ui, tab, cmds);
            });
        });
    });
}

/// "⑂ main → origin/main ↑1 ↓2"; a click opens the quick checkout (Ctrl+B).
fn branch(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    let current = tab.current_branch();
    let name = match (&tab.head, tab.head_oid) {
        (Some(b), _) => b.clone(),
        (None, Some(id)) => format!("{} {}", t!("detached"), &id.to_string()[..7]),
        (None, None) => t!("detached"),
    };
    let text = RichText::new(icons::text(icons::BRANCH, &name))
        .size(SIZE)
        .strong();
    if ui
        .add(flat(text))
        .on_hover_text(t!("Check out a branch (Ctrl+B)"))
        .clicked()
    {
        cmds.push(Command::Dialog(crate::ui::ref_dialogs::quick_checkout()));
    }
    let Some(b) = current else { return };
    let ui_weak = ui.visuals().weak_text_color();
    match &b.upstream {
        Some(up) => {
            ui.label(small(icons::text(icons::ARROW_RIGHT, up)).color(ui_weak))
                .on_hover_text(t!("Upstream branch"));
        }
        None => {
            ui.label(small(t!("not published")).color(ui_weak).italics())
                .on_hover_text(t!("The branch has no upstream yet: Push publishes it"));
        }
    }
    match b.ahead_behind {
        Some((0, 0)) => {
            ui.label(small(icons::CHECK).color(ui_weak))
                .on_hover_text(t!("Up to date with the upstream"));
        }
        Some((ahead, behind)) => {
            let accent = theme::accent(ui);
            let resp = ui
                .horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme::SPACE_S;
                    if ahead > 0 {
                        ui.label(small(format!("{}{ahead}", icons::ARROW_UP)).color(accent));
                    }
                    if behind > 0 {
                        ui.label(small(format!("{}{behind}", icons::ARROW_DOWN)).color(accent));
                    }
                })
                .response;
            let mut tip = Vec::new();
            if ahead > 0 {
                tip.push(t!("{} commits to push", ahead));
            }
            if behind > 0 {
                tip.push(t!("{} commits to pull", behind));
            }
            resp.on_hover_text(tip.join("\n"));
        }
        None => {}
    }
}

/// "Merge in progress" and the like, in a warning pill.
fn state(ui: &mut egui::Ui, state: RepoState) {
    if state == RepoState::Clean {
        return;
    }
    let warn = ui.visuals().warn_fg_color;
    egui::Frame::new()
        .fill(warn.gamma_multiply(0.16))
        .corner_radius(theme::RADIUS)
        .inner_margin(egui::Margin::symmetric(6, 0))
        .show(ui, |ui| {
            ui.label(
                small(icons::text(icons::WARNING, crate::util::state_label(state))).color(warn),
            );
        });
}

/// The running operation: spinner, label and progress, queue, cancel.
fn running(ui: &mut egui::Ui, job: &Job, queued: usize, cmds: &mut Commands) {
    ui.add(egui::Spinner::new().size(12.0).color(theme::accent(ui)));
    let progress = job.progress();
    let mut text = if progress.is_empty() {
        job.label.clone()
    } else {
        format!("{} — {progress}", job.label)
    };
    if queued > 0 {
        text = format!("{text}  {}", t!("(+{} queued)", queued));
    }
    let resp = ui
        .add(
            egui::Label::new(small(text))
                .truncate()
                .sense(egui::Sense::click()),
        )
        .on_hover_text(t!("Activity: operations and their git commands"));
    if resp.clicked() {
        cmds.push(Command::ShowActivity);
    }
    if job.cancellable()
        && !job.cancel_requested()
        && ui
            .add(flat(small("✖")))
            .on_hover_text(t!("Cancel {}", job.label))
            .clicked()
    {
        cmds.push(Command::CancelJob);
    }
}

fn identity(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    let id = &tab.identity;
    let (text, tip) = match (&id.name, &id.email) {
        (Some(n), Some(e)) => (
            icons::text(icons::IDENTITY, n),
            format!("{n} <{e}>\n{}", t!("Author identity for this repository")),
        ),
        _ => (
            t!("👤 No identity"),
            t!("Author identity for this repository"),
        ),
    };
    let mut text = small(text);
    if id.name.is_none() || id.email.is_none() {
        text = text.color(ui.visuals().warn_fg_color);
    }
    if ui.add(flat(text)).on_hover_text(tip).clicked() {
        cmds.push(Command::Dialog(Dialog::identity(&tab.identity)));
    }
}

/// When `git fetch` last ran: the time FETCH_HEAD was written.
fn last_fetch(ui: &mut egui::Ui, tab: &RepoTab) {
    if tab.remotes.is_empty() {
        return;
    }
    let Some(at) = fetch_time(ui, tab.repo.git_dir()) else {
        ui.label(small(icons::text(icons::CLOUD, t!("never fetched"))).weak());
        return;
    };
    let secs = at
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let now = crate::util::now_secs();
    let offset = ferrum_core::local_offset_minutes();
    ui.label(
        small(icons::text(
            icons::CLOUD_CHECK,
            crate::util::relative_time(secs, now),
        ))
        .weak(),
    )
    .on_hover_text(t!(
        "Last fetch: {}",
        crate::util::long_date(secs, offset, true)
    ));
    // Keeps "5 minutes ago" current.
    ui.ctx().request_repaint_after(Duration::from_secs(30));
}

/// FETCH_HEAD's time, looked up at most every few seconds.
fn fetch_time(ui: &egui::Ui, git_dir: &Path) -> Option<SystemTime> {
    let id = egui::Id::unique("status-fetch-time").with(git_dir);
    type Cached = (Instant, Option<SystemTime>);
    if let Some((checked, at)) = ui.data(|d| d.get_temp::<Cached>(id))
        && checked.elapsed() < Duration::from_secs(3)
    {
        return at;
    }
    let at = std::fs::metadata(git_dir.join("FETCH_HEAD"))
        .and_then(|m| m.modified())
        .ok();
    ui.data_mut(|d| d.insert_temp::<Cached>(id, (Instant::now(), at)));
    at
}
