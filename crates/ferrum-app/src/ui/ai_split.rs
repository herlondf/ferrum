//! "Split into commits": the local changes cut into hunks, grouped by the
//! model (or by folder) into commits with messages. The user edits the
//! groups and messages, moves hunks between them, and applies: one job
//! commits group by group (`Op::Ai`), after a restore point.

use std::collections::HashSet;
use std::path::PathBuf;

use eframe::egui::{self, RichText};
use ferrum_core::ai::json;
use ferrum_core::ai::split::{self, Changes, Group, Item};

use crate::ai::{AiOp, Stream};
use crate::command::{Command, Commands, Op};
use crate::hosted::Pending;
use crate::icons;
use crate::settings::Settings;
use crate::tab::RepoTab;
use crate::theme;
use crate::ui::ai_common;

pub struct SplitWindow {
    pub repo: PathBuf,
    loading: Option<Pending<Changes>>,
    changes: Option<Changes>,
    pub groups: Vec<Group>,
    proposal: Option<Stream>,
    /// Why the groups are what they are ("grouped by folder: …").
    note: Option<String>,
    error: Option<String>,
    /// Items whose hunk is shown.
    open_items: HashSet<String>,
}

impl SplitWindow {
    pub fn new(ctx: &egui::Context, tab: &RepoTab) -> Self {
        let dir = tab.path().to_path_buf();
        let git = ferrum_core::cli::Git::new(&dir);
        Self {
            repo: dir,
            loading: Some(Pending::spawn(ctx, move || {
                split::collect(&git).map_err(|e| e.to_string())
            })),
            changes: None,
            groups: Vec::new(),
            proposal: None,
            note: None,
            error: None,
            open_items: HashSet::new(),
        }
    }

    /// Asks the model for groups (or groups by folder when AI is off).
    fn propose(&mut self, ctx: &egui::Context) {
        let Some(changes) = &self.changes else { return };
        let cfg = ai_common::config(ctx);
        if !cfg.enabled {
            self.by_folder(Some(t!("AI is off: the changes are grouped by folder.")));
            return;
        }
        let prompt = split::prompt(changes, cfg.max);
        self.note = None;
        self.proposal = Some(Stream::start(ctx, &cfg, move || Ok(prompt)));
    }

    fn by_folder(&mut self, note: Option<String>) {
        if let Some(c) = &self.changes {
            self.groups = split::by_folder(c);
        }
        self.note = note;
    }

    /// Takes in the loaded changes and the model's answer.
    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(r) = crate::hosted::take_done(&mut self.loading) {
            match r {
                Ok(c) if c.items.is_empty() => {
                    self.error = Some(t!("There are no local changes to split."))
                }
                Ok(c) => {
                    self.changes = Some(c);
                    self.propose(ctx);
                }
                Err(e) => self.error = Some(e),
            }
        }
        let Some(done) = self.proposal.as_ref().and_then(Stream::finished) else {
            return;
        };
        self.proposal = None;
        let Some(changes) = &self.changes else { return };
        match done.map(|text| json::extract(&text)) {
            Ok(Some(v)) => {
                let groups = split::parse_groups(&v, changes);
                if groups.iter().all(|g| g.message.is_empty()) {
                    self.by_folder(Some(t!(
                        "The model's answer had no commits: the changes are grouped by folder."
                    )));
                } else {
                    self.groups = groups;
                }
            }
            Ok(None) => self.by_folder(Some(t!(
                "The model's answer could not be read: the changes are grouped by folder."
            ))),
            Err(e) => self.by_folder(Some(t!("{} The changes are grouped by folder.", e))),
        }
    }

    fn item(&self, id: &str) -> Option<&Item> {
        self.changes.as_ref()?.items.iter().find(|i| i.id == id)
    }

    /// Items in no group: they stay local changes.
    fn left_out(&self) -> Vec<String> {
        let Some(c) = &self.changes else {
            return Vec::new();
        };
        let used: HashSet<&String> = self.groups.iter().flat_map(|g| &g.items).collect();
        c.items
            .iter()
            .filter(|i| !used.contains(&i.id))
            .map(|i| i.id.clone())
            .collect()
    }
}

enum Move {
    To(String, Option<usize>),
}

/// Returns `false` when the window is closed.
pub fn show(
    ctx: &egui::Context,
    w: &mut SplitWindow,
    tab: Option<&RepoTab>,
    settings: &Settings,
    cmds: &mut Commands,
) -> bool {
    w.poll(ctx);
    let mut open = true;
    let mut close = false;
    egui::Window::new(t!("Split into commits"))
        .id(egui::Id::unique("ai-split"))
        .open(&mut open)
        .default_size([760.0, 580.0])
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .resizable(true)
        .collapsible(false)
        .frame(theme::dialog_frame(&ctx.global_style()))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(icons::AI).size(16.0).color(theme::accent(ui)));
                ui.label(t!(
                    "Group the local changes into focused commits, then review them."
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ai_common::badge(ui);
                });
            });
            ui.add_space(theme::SPACE_S);
            if let Some(e) = &w.error {
                ui.colored_label(ui.visuals().error_fg_color, icons::text(icons::WARNING, e));
                return;
            }
            if w.loading.is_some() || w.proposal.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(if w.loading.is_some() {
                        t!("Reading the local changes…")
                    } else {
                        t!("Asking the model for a split…")
                    });
                    if let Some(p) = &w.proposal {
                        if let Some(sent) = p.sent() {
                            ai_common::sent_line(ui, &sent);
                        }
                        if ui.button(t!("Cancel")).clicked() {
                            p.cancel();
                        }
                    }
                });
                return;
            }
            if let Some(n) = &w.note {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(icons::INFO).color(ui.visuals().weak_text_color()));
                    ui.weak(n);
                });
            }
            toolbar(ui, w);
            ui.add_space(theme::SPACE_S);
            let footer = 44.0;
            egui::ScrollArea::vertical()
                .max_height((ui.available_height() - footer).max(120.0))
                .auto_shrink([false, false])
                .show(ui, |ui| groups(ui, w));
            ui.separator();
            close = footer_row(ui, w, tab, settings, cmds);
        });
    open && !close
}

fn toolbar(ui: &mut egui::Ui, w: &mut SplitWindow) {
    ui.horizontal(|ui| {
        let enabled = ai_common::config(ui.ctx()).enabled;
        if ui
            .add_enabled(
                enabled,
                egui::Button::new(icons::text(icons::AI, t!("Propose again"))),
            )
            .on_hover_text(ai_common::where_now(ui.ctx()).long)
            .on_disabled_hover_text(t!("AI is off"))
            .clicked()
        {
            w.propose(ui.ctx());
        }
        if ui
            .button(icons::text(icons::FOLDER, t!("Group by folder")))
            .clicked()
        {
            w.by_folder(None);
        }
        if ui
            .button(icons::text(icons::PLUS, t!("Add commit")))
            .clicked()
        {
            w.groups.push(Group::default());
        }
    });
}

fn groups(ui: &mut egui::Ui, w: &mut SplitWindow) {
    let mut moves: Vec<Move> = Vec::new();
    let mut remove = None;
    let mut swap = None;
    let n = w.groups.len();
    let names: Vec<String> = w
        .groups
        .iter()
        .enumerate()
        .map(|(i, g)| group_name(i, g))
        .collect();
    for i in 0..n {
        theme::card(ui)
            .inner_margin(egui::Margin::same(theme::SPACE_M as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        theme::strong(format!("{}", i + 1))
                            .color(theme::accent(ui))
                            .size(15.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        if ui
                            .add(egui::Button::new(icons::TRASH).frame_when_inactive(false))
                            .on_hover_text(t!("Remove this commit; its changes stay local changes"))
                            .clicked()
                        {
                            remove = Some(i);
                        }
                        ui.add_enabled_ui(i + 1 < n, |ui| {
                            if ui
                                .add(egui::Button::new(icons::ARROW_DOWN).frame_when_inactive(false))
                                .on_hover_text(t!("Move down"))
                                .clicked()
                            {
                                swap = Some((i, i + 1));
                            }
                        });
                        ui.add_enabled_ui(i > 0, |ui| {
                            if ui
                                .add(egui::Button::new(icons::ARROW_UP).frame_when_inactive(false))
                                .on_hover_text(t!("Move up"))
                                .clicked()
                            {
                                swap = Some((i, i - 1));
                            }
                        });
                        ui.add(
                            egui::TextEdit::multiline(&mut w.groups[i].message)
                                .hint_text(t!("Commit message"))
                                .desired_rows(1)
                                .desired_width(ui.available_width()),
                        );
                    });
                });
                if w.groups[i].items.is_empty() {
                    ui.weak(t!("No changes yet: move some here."));
                }
                let ids = w.groups[i].items.clone();
                for id in ids {
                    item_row(ui, w, &id, Some(i), &names, &mut moves);
                }
            });
        ui.add_space(theme::SPACE_S);
    }
    let rest = w.left_out();
    if !rest.is_empty() {
        theme::card(ui)
            .inner_margin(egui::Margin::same(theme::SPACE_M as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(theme::strong(t!("Not committed")));
                ui.weak(t!("These changes stay local changes."));
                for id in rest {
                    item_row(ui, w, &id, None, &names, &mut moves);
                }
            });
    }
    for Move::To(id, to) in moves {
        for g in &mut w.groups {
            g.items.retain(|x| *x != id);
        }
        match to {
            Some(t) if t < w.groups.len() => w.groups[t].items.push(id),
            Some(_) => w.groups.push(Group {
                message: String::new(),
                items: vec![id],
            }),
            None => {}
        }
    }
    if let Some((a, b)) = swap {
        w.groups.swap(a, b);
    }
    if let Some(i) = remove {
        w.groups.remove(i);
    }
}

fn group_name(i: usize, g: &Group) -> String {
    let first = g.message.lines().next().unwrap_or_default().trim();
    if first.is_empty() {
        t!("Commit {}", i + 1)
    } else {
        format!("{} · {}", i + 1, first)
    }
}

fn item_row(
    ui: &mut egui::Ui,
    w: &mut SplitWindow,
    id: &str,
    group: Option<usize>,
    names: &[String],
    moves: &mut Vec<Move>,
) {
    let Some(item) = w.item(id).cloned() else {
        return;
    };
    let open = w.open_items.contains(id);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SPACE_S;
        let caret = if open {
            icons::EXPANDED
        } else {
            icons::COLLAPSED
        };
        let file = item
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&item.path)
            .to_owned();
        let resp = ui
            .add(egui::Button::new(icons::text(caret, file)).frame_when_inactive(false))
            .on_hover_text(&item.path);
        if resp.clicked() {
            if open {
                w.open_items.remove(id);
            } else {
                w.open_items.insert(id.to_owned());
            }
        }
        let title = item.title().to_owned();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(icons::text(icons::ARROW_RIGHT, t!("Move")), |ui| {
                for (k, name) in names.iter().enumerate() {
                    if Some(k) != group && ui.button(name).clicked() {
                        moves.push(Move::To(id.to_owned(), Some(k)));
                        ui.close();
                    }
                }
                if ui
                    .button(icons::text(icons::PLUS, t!("New commit")))
                    .clicked()
                {
                    moves.push(Move::To(id.to_owned(), Some(usize::MAX)));
                    ui.close();
                }
                if group.is_some() && ui.button(t!("Leave out")).clicked() {
                    moves.push(Move::To(id.to_owned(), None));
                    ui.close();
                }
            });
            ui.label(
                RichText::new(format!("−{}", item.removed))
                    .color(crate::util::removed_color(ui))
                    .size(11.0),
            );
            ui.label(
                RichText::new(format!("+{}", item.added))
                    .color(crate::util::added_color(ui))
                    .size(11.0),
            );
            // The hunk header takes what is left.
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(&title)
                            .monospace()
                            .size(11.0)
                            .color(ui.visuals().weak_text_color()),
                    )
                    .truncate(),
                );
            });
        });
    });
    if open {
        hunk_text(ui, &item);
    }
}

fn hunk_text(ui: &mut egui::Ui, item: &Item) {
    let (add, del) = (crate::util::added_color(ui), crate::util::removed_color(ui));
    let weak = ui.visuals().weak_text_color();
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .corner_radius(theme::RADIUS)
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            for line in item.text.lines().skip(1).take(60) {
                let color = match line.chars().next() {
                    Some('+') => add,
                    Some('-') => del,
                    _ => weak,
                };
                ui.add(
                    egui::Label::new(
                        RichText::new(crate::util::expand_tabs(line))
                            .monospace()
                            .size(11.0)
                            .color(color),
                    )
                    .extend(),
                );
            }
        });
}

/// Returns `true` once applied (the window closes).
fn footer_row(
    ui: &mut egui::Ui,
    w: &mut SplitWindow,
    tab: Option<&RepoTab>,
    settings: &Settings,
    cmds: &mut Commands,
) -> bool {
    let real: Vec<&Group> = w.groups.iter().filter(|g| !g.items.is_empty()).collect();
    let unnamed = real.iter().any(|g| g.message.trim().is_empty());
    let busy = tab.is_none_or(RepoTab::busy);
    let mut applied = false;
    let mut cancelled = false;
    ui.horizontal(|ui| {
        if unnamed {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                t!("Write a message for every commit."),
            );
        } else {
            ui.weak(t!(
                "A restore point is saved first; the working copy is not touched."
            ));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ready = !real.is_empty() && !unnamed && !busy;
            let label = if real.len() == 1 {
                t!("Make 1 commit")
            } else {
                t!("Make {} commits", real.len())
            };
            if ui
                .add_enabled(ready, theme::primary(icons::text(icons::CHECK, label)))
                .clicked()
                && let (Some(changes), Some(tab)) = (&w.changes, tab)
            {
                let key = crate::repo_settings::shared_key(tab);
                cmds.run(Op::Ai(AiOp::Split {
                    changes: Box::new(changes.clone()),
                    groups: w.groups.clone(),
                    secrets: settings.safety.secret_rules.clone(),
                    allow: settings
                        .safety
                        .allowlists
                        .get(&key)
                        .cloned()
                        .unwrap_or_default(),
                }));
                applied = true;
            }
            if ui.add(theme::secondary(t!("Cancel"))).clicked() {
                cancelled = true;
            }
        });
    });
    if applied {
        cmds.push(Command::SetView(crate::tab::View::History));
    }
    applied || cancelled
}
