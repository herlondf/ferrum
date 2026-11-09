//! The operation timeline: restore points saved before operations that
//! rewrite or drop work, and backup refs, newest first. "Restore…" asks
//! what to bring back (`confirm`), then runs `TimelineOp::Restore`.

use std::path::PathBuf;

use eframe::egui::{self, RichText};
use ferrum_core::restore::{self, Backup, Meta, RefChange, RestoreMode, RestorePoint};

use crate::command::{Command, Commands, DevStep, Op};
use crate::icons;
use crate::safety::SafetyCmd;
use crate::settings::Settings;
use crate::tab::RepoTab;
use crate::theme;
use crate::timeline::TimelineOp;
use crate::ui::dialogs::Dialog;

#[derive(Default)]
pub struct TimelineWindow {
    pub open: bool,
    /// What was loaded: repository and `TabTimeline::generation`.
    loaded: Option<(PathBuf, u64)>,
    points: Vec<RestorePoint>,
    backups: Vec<Backup>,
    error: Option<String>,
    selected: Option<String>,
}

impl TimelineWindow {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.loaded = None;
    }

    /// Reads the points again when the tab took or removed some.
    pub fn load(&mut self, tab: &RepoTab) {
        let key = (tab.path().to_path_buf(), tab.timeline.generation);
        if self.loaded.as_ref() == Some(&key) {
            return;
        }
        self.loaded = Some(key);
        match restore::list(&tab.repo) {
            Ok(p) => {
                self.points = p;
                self.error = None;
            }
            Err(e) => {
                self.points.clear();
                self.error = Some(e.to_string());
            }
        }
        self.backups = restore::backups(&tab.repo);
    }
}

/// The question before a restore.
pub struct RestoreConfirm {
    pub repo: PathBuf,
    pub point: RestorePoint,
    pub mode: RestoreMode,
    /// Branches and tags that would move, computed when it opened.
    changes: Vec<RefChange>,
    stashes: usize,
}

impl RestoreConfirm {
    pub fn new(tab: &RepoTab, point: RestorePoint) -> Self {
        let changes = restore::preview(&tab.repo, &point.meta);
        let stashes = point
            .meta
            .stashes
            .iter()
            .filter(|s| !tab.stashes.iter().any(|t| t.id.to_string() == s.oid))
            .count();
        let mode = if point.meta.worktree.is_some() {
            RestoreMode::Everything
        } else {
            RestoreMode::RefsOnly
        };
        Self {
            repo: tab.path().to_path_buf(),
            point,
            mode,
            changes,
            stashes,
        }
    }
}

fn short(oid: &str) -> String {
    oid.chars().take(7).collect()
}

fn ref_short(name: &str) -> &str {
    name.trim_start_matches("refs/heads/")
        .trim_start_matches("refs/tags/")
        .trim_start_matches("refs/remotes/")
}

/// "main a1b2c3d → e4f5a6b", "deleted topic", "created x".
fn change_text(c: &RefChange) -> String {
    let name = ref_short(&c.name);
    match (&c.old, &c.new) {
        (Some(o), Some(n)) => format!("{name}  {} → {}", short(o), short(n)),
        (Some(o), None) => t!("{} deleted (was {})", name, short(o)),
        (None, Some(n)) => t!("{} created at {}", name, short(n)),
        (None, None) => name.to_owned(),
    }
}

/// One line saying what the operation changed.
fn summary(meta: &Meta) -> String {
    let mut parts = Vec::new();
    let head = meta.head_short();
    if !head.is_empty() {
        parts.push(t!("on {}", head));
    }
    let local: Vec<&RefChange> = meta
        .changed
        .iter()
        .filter(|c| !c.name.starts_with("refs/remotes/"))
        .collect();
    match local.as_slice() {
        [] => {}
        [c] => parts.push(change_text(c)),
        list => parts.push(t!("{} refs changed", list.len())),
    }
    if !meta.dropped_stashes.is_empty() {
        parts.push(t!("{} stashes dropped", meta.dropped_stashes.len()));
    }
    match meta.files_total {
        0 => {}
        1 => parts.push(t!("1 changed file saved")),
        n => parts.push(t!("{} changed files saved", n)),
    }
    parts.join("  ·  ")
}

fn clock(secs: i64) -> String {
    let t = crate::util::format_time(secs, ferrum_core::local_offset_minutes());
    t[11..].to_owned()
}

fn day(secs: i64) -> String {
    let offset = ferrum_core::local_offset_minutes();
    let now = crate::util::now_secs();
    let d = |s: i64| crate::util::format_time(s, offset)[..10].to_owned();
    if d(secs) == d(now) {
        t!("Today")
    } else if d(secs) == d(now - 86_400) {
        t!("Yesterday")
    } else {
        crate::util::long_date(secs, offset, false)
    }
}

pub fn show(
    ctx: &egui::Context,
    win: &mut TimelineWindow,
    tab: Option<&RepoTab>,
    settings: &Settings,
    cmds: &mut Commands,
) {
    if !win.open {
        return;
    }
    if let Some(t) = tab {
        win.load(t);
    }
    let mut open = true;
    egui::Window::new(t!("Operation timeline"))
        .id(egui::Id::unique("timeline-window"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([560.0, 520.0])
        .min_size([420.0, 260.0])
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| match tab {
            None => crate::ui::empty_state::show(
                ui,
                icons::CLOCK_COUNTER_CLOCKWISE,
                &t!("No repository"),
                &t!("Open a repository to see its operations."),
                |_| {},
            ),
            Some(tab) => body(ui, win, tab, settings, cmds),
        });
    if !open {
        win.open = false;
    }
}

fn body(
    ui: &mut egui::Ui,
    win: &mut TimelineWindow,
    tab: &RepoTab,
    settings: &Settings,
    cmds: &mut Commands,
) {
    let weak = ui.visuals().weak_text_color();
    ui.horizontal_top(|ui| {
        let on = settings.safety.restore_points;
        let text = if on {
            t!("A restore point is saved before every operation that rewrites or drops work. They are kept for {} days.", settings.safety.keep_days)
        } else {
            t!("Restore points are turned off.")
        };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            let resp = crate::ui::toolbar::icon_button(ui, icons::GEAR_SIX, t!("Settings"));
            if resp.clicked() {
                cmds.push(Command::DevStep(DevStep::SettingsSection("safety".into())));
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                ui.add(egui::Label::new(RichText::new(text).color(weak).size(12.0)).wrap());
            });
        });
    });
    ui.add_space(theme::SPACE_S);
    ui.separator();
    if let Some(e) = &win.error {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
    if win.points.is_empty() && win.backups.is_empty() {
        crate::ui::empty_state::show(
            ui,
            icons::CLOCK_COUNTER_CLOCKWISE,
            &t!("No restore points yet"),
            &t!(
                "Ferrum saves one before a rebase, reset, discard, amend, branch delete, stash drop or force push, so any of them can be undone."
            ),
            |_| {},
        );
        return;
    }
    let busy = tab.busy();
    // The footer is laid out first, from the bottom; the list fills the rest.
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.horizontal(|ui| {
            let n = win.points.len();
            ui.label(
                RichText::new(t!("{} restore points", n))
                    .color(weak)
                    .size(12.0),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let resp = ui.add_enabled(n > 0 && !busy, egui::Button::new(t!("Delete all…")));
                if resp.clicked() {
                    let ids = win.points.iter().map(|p| p.id.clone()).collect();
                    cmds.push(Command::Dialog(Dialog::confirm(
                        &t!("Delete all restore points?"),
                        &t!(
                            "Operations done so far can then no longer be undone from the timeline."
                        ),
                        Op::Timeline(TimelineOp::Delete(ids)),
                        true,
                    )));
                }
            });
        });
        ui.separator();
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut last_day = String::new();
                    for p in &win.points {
                        let d = day(p.meta.time);
                        if d != last_day {
                            ui.add_space(theme::SPACE_S);
                            ui.label(theme::strong(&d).size(12.0).color(weak));
                            last_day = d;
                        }
                        let selected = win.selected.as_deref() == Some(p.id.as_str());
                        if point_row(ui, tab.path(), selected, p, busy, cmds) {
                            win.selected = (!selected).then(|| p.id.clone());
                        }
                    }
                    if !win.backups.is_empty() {
                        ui.add_space(theme::SPACE_L);
                        ui.label(theme::strong(t!("Backups")).size(12.0).color(weak));
                        for b in &win.backups {
                            backup_row(ui, b, cmds);
                        }
                    }
                });
        });
    });
}

fn point_row(
    ui: &mut egui::Ui,
    repo: &std::path::Path,
    is_selected: bool,
    p: &RestorePoint,
    busy: bool,
    cmds: &mut Commands,
) -> bool {
    let v = ui.visuals().clone();
    let fill = if is_selected {
        v.faint_bg_color
    } else {
        egui::Color32::TRANSPARENT
    };
    let mut toggle = false;
    egui::Frame::new()
        .fill(fill)
        .corner_radius(theme::RADIUS)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                let (icon, color) = if p.meta.error.is_some() {
                    (icons::WARNING, v.warn_fg_color)
                } else {
                    (icons::ARROW_COUNTER_CLOCKWISE, theme::accent(ui))
                };
                ui.label(RichText::new(icon).size(16.0).color(color));
                // Buttons first, on the right; the text takes what is left.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    let trash = egui::Button::new(icons::TRASH)
                        .small()
                        .frame_when_inactive(false);
                    let del = ui
                        .add_enabled(!busy, trash)
                        .on_hover_text(t!("Delete restore point"));
                    if del.clicked() {
                        cmds.run(Op::Timeline(TimelineOp::Delete(vec![p.id.clone()])));
                    }
                    let restore = ui.add_enabled(!busy, egui::Button::new(t!("Restore…")).small());
                    if restore.clicked() {
                        cmds.push(Command::Safety(SafetyCmd::ConfirmRestore(
                            repo.to_path_buf(),
                            p.id.clone(),
                        )));
                    }
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = theme::SPACE_XS;
                        let caret = if is_selected {
                            icons::EXPANDED
                        } else {
                            icons::COLLAPSED
                        };
                        let title = ui.horizontal(|ui| {
                            let click = egui::Sense::click();
                            let title = theme::strong(&p.meta.label).color(v.strong_text_color());
                            let a = ui.add(egui::Label::new(title).truncate().sense(click));
                            let weak = v.weak_text_color();
                            let time = RichText::new(clock(p.meta.time)).size(11.5).color(weak);
                            let b = ui.add(egui::Label::new(time).sense(click));
                            let more = RichText::new(caret).size(11.0).color(weak);
                            let c = ui.add(egui::Label::new(more).sense(click));
                            a.clicked() || b.clicked() || c.clicked()
                        });
                        let s = ui.add(
                            egui::Label::new(
                                RichText::new(summary(&p.meta))
                                    .size(11.5)
                                    .color(v.weak_text_color()),
                            )
                            .truncate()
                            .sense(egui::Sense::click()),
                        );
                        toggle = title.inner || s.clicked();
                        if is_selected {
                            details(ui, &p.meta);
                        }
                    });
                });
            });
        });
    toggle
}

fn details(ui: &mut egui::Ui, meta: &Meta) {
    let weak = ui.visuals().weak_text_color();
    ui.add_space(theme::SPACE_S);
    let code = crate::util::code_font(11.5);
    if let Some(e) = &meta.error {
        ui.add(
            egui::Label::new(
                RichText::new(t!("Failed: {}", e.lines().next().unwrap_or_default()))
                    .size(11.5)
                    .color(ui.visuals().warn_fg_color),
            )
            .wrap(),
        );
    }
    if !meta.state.is_empty() {
        ui.label(
            RichText::new(t!("A {} was in progress", meta.state))
                .size(11.5)
                .color(weak),
        );
    }
    for c in &meta.changed {
        ui.label(RichText::new(change_text(c)).font(code.clone()).color(weak));
    }
    for s in &meta.dropped_stashes {
        ui.label(
            RichText::new(t!("stash dropped: {}", s.message))
                .size(11.5)
                .color(weak),
        );
    }
    if !meta.files.is_empty() {
        ui.add_space(theme::SPACE_XS);
        ui.label(RichText::new(t!("Saved files")).size(11.5).color(weak));
        for f in meta.files.iter().take(8) {
            ui.label(RichText::new(f).font(code.clone()).color(weak));
        }
        if meta.files_total > 8 {
            ui.label(
                RichText::new(t!("and {} more", meta.files_total - 8))
                    .size(11.5)
                    .color(weak),
            );
        }
    }
    if meta.untracked_skipped {
        ui.label(
            RichText::new(t!("Untracked files were not saved (too large)."))
                .size(11.5)
                .color(ui.visuals().warn_fg_color),
        );
    }
}

fn backup_row(ui: &mut egui::Ui, b: &Backup, cmds: &mut Commands) {
    let v = ui.visuals().clone();
    let weak = v.weak_text_color();
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.label(RichText::new(icons::ARCHIVE).size(16.0).color(weak));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    let id = b.target.to_string();
                    if ui.small_button(t!("New branch…")).clicked() {
                        let label = format!("{} ({})", b.name, short(&id));
                        cmds.push(Command::Dialog(Dialog::create_branch(&id, &label)));
                    }
                    if ui.small_button(t!("Show")).clicked() {
                        cmds.push(Command::SelectCommit(b.target));
                    }
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = theme::SPACE_XS;
                        ui.horizontal(|ui| {
                            ui.label(theme::strong(&b.name).color(v.strong_text_color()));
                            let offset = ferrum_core::local_offset_minutes();
                            let when = crate::util::format_date(b.time, offset);
                            ui.label(RichText::new(when).size(11.5).color(weak));
                        });
                        let text = format!("{}  {}", short(&id), b.summary);
                        let text = RichText::new(text).size(11.5).color(weak);
                        ui.add(egui::Label::new(text).truncate());
                    });
                });
            });
        });
}

/// The restore question. Returns whether it stays open.
pub fn confirm(
    ctx: &egui::Context,
    c: &mut RestoreConfirm,
    tab: &RepoTab,
    cmds: &mut Commands,
) -> bool {
    let mut keep = true;
    let modal = egui::Modal::new(egui::Id::unique("restore-confirm"))
        .frame(theme::dialog_frame(&ctx.global_style()))
        .show(ctx, |ui| {
            ui.set_width(480.0);
            let meta = &c.point.meta;
            let weak = ui.visuals().weak_text_color();
            ui.horizontal(|ui| {
                ui.label(RichText::new(icons::ARROW_COUNTER_CLOCKWISE).size(22.0).color(theme::accent(ui)));
                ui.vertical(|ui| {
                    ui.label(theme::strong(t!("Restore to before “{}”?", meta.label)).size(16.0));
                    let when = crate::util::long_date(meta.time, ferrum_core::local_offset_minutes(), true);
                    ui.label(RichText::new(when).color(weak));
                });
            });
            ui.add_space(theme::SPACE_M);

            let code = crate::util::code_font(12.0);
            let head_now = tab.head.clone().unwrap_or_else(|| {
                tab.head_oid.map(|o| short(&o.to_string())).unwrap_or_default()
            });
            let head_then = meta.head_short();
            if !head_then.is_empty() && head_then != head_now {
                ui.label(t!("HEAD goes back to {}", head_then));
            }
            if c.changes.is_empty() {
                ui.label(RichText::new(t!("Branches and tags are already as they were.")).color(weak));
            } else {
                ui.label(t!("Branches and tags that move back:"));
                egui::ScrollArea::vertical()
                    .id_salt("restore-changes")
                    .max_height(120.0)
                    .show(ui, |ui| {
                        for ch in &c.changes {
                            ui.label(RichText::new(change_text(ch)).font(code.clone()));
                        }
                    });
            }
            if c.stashes > 0 {
                ui.label(t!("{} stashes come back", c.stashes));
            }
            ui.add_space(theme::SPACE_M);

            let has_files = meta.worktree.is_some();
            ui.add_enabled_ui(has_files, |ui| {
                ui.radio_value(&mut c.mode, RestoreMode::Everything, t!("Branches, tags and files"));
            });
            ui.radio_value(
                &mut c.mode,
                RestoreMode::RefsOnly,
                t!("Only branches, tags and stashes (files stay as they are now)"),
            );
            if c.mode == RestoreMode::Everything && meta.files_total > 0 {
                ui.label(
                    RichText::new(t!("Uncommitted changes of that time come back: {} files.", meta.files_total))
                        .size(12.0)
                        .color(weak),
                );
            }
            ui.add_space(theme::SPACE_M);
            theme::card(ui).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new(icons::INFO).color(theme::accent(ui)));
                    let mut text = t!("The current state, uncommitted changes included, is saved as a restore point first, so this can be undone too.");
                    if tab.state != ferrum_core::RepoState::Clean {
                        text = format!("{text}\n{}", t!("The {} in progress is ended.", crate::util::state_name(tab.state)));
                    }
                    ui.add(egui::Label::new(RichText::new(text).size(12.0)).wrap());
                });
            });
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            match crate::ui::dialogs::buttons(ui, &t!("Restore"), !tab.busy(), false, enter) {
                Some(true) => {
                    cmds.run(Op::Timeline(TimelineOp::Restore {
                        id: c.point.id.clone(),
                        label: meta.label.clone(),
                        mode: c.mode,
                        untracked: true,
                    }));
                    keep = false;
                }
                Some(false) => keep = false,
                None => {}
            }
        });
    if modal.should_close() {
        keep = false;
    }
    keep
}
