//! The commit history: search bar, graph and commit list.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use eframe::epaint::CubicBezierShape;
use ferrum_core::graph::{GraphRow, Half};
use ferrum_core::ops::ResetMode;
use ferrum_core::{BranchKind, CommitInfo, RefKind, RefLabel};

use crate::command::{Command, Commands, Op};
use crate::tab::RepoTab;
use crate::ui::dialogs::Dialog;
use crate::util::format_date;

const ROW_HEIGHT: f32 = 24.0;
const LANE_WIDTH: f32 = 14.0;
/// Very wide graphs are clipped so the description stays readable.
const MAX_GRAPH_LANES: usize = 10;
const AUTHOR_WIDTH: f32 = 150.0;
const DATE_WIDTH: f32 = 120.0;
const HASH_WIDTH: f32 = 64.0;

const LANE_COLORS: [Color32; 8] = [
    Color32::from_rgb(0x3d, 0x8b, 0xfd),
    Color32::from_rgb(0xe8, 0x5d, 0x75),
    Color32::from_rgb(0x4c, 0xc3, 0x8a),
    Color32::from_rgb(0xf5, 0xa6, 0x23),
    Color32::from_rgb(0x9b, 0x6c, 0xf6),
    Color32::from_rgb(0x2c, 0xc4, 0xd4),
    Color32::from_rgb(0xe0, 0x6c, 0x3c),
    Color32::from_rgb(0xb5, 0xc9, 0x3a),
];

pub fn lane_color(index: usize) -> Color32 {
    LANE_COLORS[index % LANE_COLORS.len()]
}

/// Column positions shared by the header and every row.
struct Columns {
    graph: Rect,
    description: Rect,
    author: Rect,
    date: Rect,
    hash: Rect,
}

impl Columns {
    fn new(row: Rect, graph_lanes: usize) -> Self {
        let graph_w = if graph_lanes == 0 {
            0.0
        } else {
            graph_lanes.min(MAX_GRAPH_LANES) as f32 * LANE_WIDTH + 8.0
        };
        let mut x = row.right();
        let mut take = |w: f32| {
            let r = Rect::from_x_y_ranges(x - w..=x, row.y_range());
            x -= w;
            r
        };
        let hash = take(HASH_WIDTH);
        let date = take(DATE_WIDTH);
        let author = take(AUTHOR_WIDTH);
        let graph = Rect::from_x_y_ranges(row.left()..=row.left() + graph_w, row.y_range());
        let description = Rect::from_x_y_ranges(
            graph.right()..=(author.left() - 8.0).max(graph.right()),
            row.y_range(),
        );
        Self {
            graph,
            description,
            author,
            date,
            hash,
        }
    }
}

pub fn search_bar(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    ui.horizontal(|ui| {
        ui.label("🔍");
        let hint = if tab.history.content_search {
            t!("Search changes (git log -S), press Enter")
        } else {
            t!("Filter by message, author or hash")
        };
        let resp = ui.add(
            egui::TextEdit::singleline(&mut tab.history.search)
                .hint_text(hint)
                .desired_width(320.0),
        );
        if tab.history.focus_search {
            resp.request_focus();
            tab.history.focus_search = false;
        }
        let mut changed = resp.changed();
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui
            .checkbox(&mut tab.history.content_search, t!("In changes"))
            .changed()
        {
            tab.history.content_hits = None;
            changed = true;
        }
        if tab.history.content_search {
            if changed {
                tab.history.content_hits = None;
            }
            if (enter || ui.button(t!("Search")).clicked()) && !tab.history.search.trim().is_empty()
            {
                cmds.push(Command::FocusSearch);
                start_content_search(tab, ui.ctx());
            }
        }
        if !tab.history.search.is_empty()
            && ui.small_button("×").on_hover_text(t!("Clear")).clicked()
        {
            tab.history.search.clear();
            tab.history.content_hits = None;
            changed = true;
        }
        if changed {
            tab.apply_search();
        }
        if let Some(v) = &tab.history.visible {
            ui.weak(t!("{} of {} commits", v.len(), tab.commits.len()));
        }
    });
}

fn start_content_search(tab: &mut RepoTab, ctx: &egui::Context) {
    let term = tab.history.search.trim().to_owned();
    let path = tab.path().to_path_buf();
    tab.spawn_job(&t!("Search in changes"), ctx, move |_| {
        let repo = ferrum_core::Repo::open(&path).map_err(|e| e.to_string())?;
        let hits = repo.search_content(&term).map_err(|e| e.to_string())?;
        Ok(crate::jobs::JobOutput::SearchHits(
            hits.into_iter().collect(),
        ))
    });
}

/// Up/Down move the selection, Home/End jump to the ends (when no text field has focus).
fn keyboard_navigation(ui: &egui::Ui, tab: &mut RepoTab) {
    if ui.ctx().memory(|m| m.focused().is_some()) || egui::Popup::is_any_open(ui.ctx()) {
        return;
    }
    let (up, down, home, end) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::Home),
            i.key_pressed(egui::Key::End),
        )
    });
    if !(up || down || home || end) {
        return;
    }
    let rows: Vec<usize> = match &tab.history.visible {
        Some(v) => v.clone(),
        None => (0..tab.commits.len()).collect(),
    };
    if rows.is_empty() {
        return;
    }
    let current = tab
        .history
        .selected
        .and_then(|id| tab.index.get(&id))
        .and_then(|idx| rows.iter().position(|r| r == idx));
    let next = match (current, up, down, home) {
        (_, _, _, true) => 0,
        (None, _, _, _) => 0,
        (Some(p), true, _, _) => p.saturating_sub(1),
        (Some(p), _, true, _) => (p + 1).min(rows.len() - 1),
        _ => rows.len() - 1,
    };
    let id = tab.commits[rows[next]].id;
    if tab.history.selected != Some(id) {
        tab.history.selected = Some(id);
        tab.load_commit(id, false);
        tab.history.scroll_to_selected = true;
    }
}

pub fn commit_list(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    keyboard_navigation(ui, tab);
    // The graph only makes sense for the full, unfiltered history.
    let filtered = tab.history.visible.is_some();
    let lanes = if filtered { 0 } else { tab.graph.width.max(1) };

    let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    let cols = Columns::new(header, lanes);
    let weak = ui.visuals().weak_text_color();
    for (rect, title) in [
        (cols.graph, t!("Graph")),
        (cols.description, t!("Description")),
        (cols.author, t!("Author")),
        (cols.date, t!("Date")),
        // Same word in every language; "Commit" alone is the button's key.
        (cols.hash, "Commit".to_owned()),
    ] {
        if rect.width() > 0.0 {
            ui.painter().text(
                rect.left_center() + vec2(4.0, 0.0),
                Align2::LEFT_CENTER,
                title,
                FontId::proportional(12.0),
                weak,
            );
        }
    }
    ui.separator();

    let count = tab
        .history
        .visible
        .as_ref()
        .map_or(tab.commits.len(), Vec::len);
    if count == 0 {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.weak(if filtered {
                t!("No commits match the search.")
            } else {
                t!("No commits yet.")
            });
        });
        return;
    }

    let row_of = |tab: &RepoTab, i: usize| tab.history.visible.as_ref().map_or(i, |v| v[i]);
    let mut scroll = egui::ScrollArea::vertical()
        .auto_shrink(false)
        .id_salt("history");
    if tab.history.scroll_to_selected {
        tab.history.scroll_to_selected = false;
        let pos = tab.history.selected.and_then(|id| {
            let idx = *tab.index.get(&id)?;
            match &tab.history.visible {
                Some(v) => v.iter().position(|&i| i == idx),
                None => Some(idx),
            }
        });
        if let Some(pos) = pos {
            let target = pos as f32 * ROW_HEIGHT - ui.available_height() / 3.0;
            scroll = scroll.vertical_scroll_offset(target.max(0.0));
        }
    }

    // Rows must touch so graph lines are continuous.
    ui.spacing_mut().item_spacing.y = 0.0;
    scroll.show_rows(ui, ROW_HEIGHT, count, |ui, range| {
        for i in range {
            let idx = row_of(tab, i);
            let (rect, resp) =
                ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
            let id = tab.commits[idx].id;
            if resp.clicked() {
                tab.history.selected = Some(id);
                tab.load_commit(id, false);
            }
            if resp.double_clicked() {
                // Check out the local branch at this commit, if there is one.
                let local = tab
                    .labels
                    .get(&id)
                    .and_then(|ls| ls.iter().find(|l| l.kind == RefKind::Local && !l.is_head));
                if let Some(l) = local {
                    cmds.run(Op::Checkout(l.name.clone()));
                }
            }
            let selected = tab.history.selected == Some(id);
            commit_row(ui, rect, tab, idx, lanes, selected, resp.hovered());
            resp.context_menu(|ui| commit_menu(ui, tab, idx, cmds));
        }
    });
}

fn commit_row(
    ui: &egui::Ui,
    rect: Rect,
    tab: &RepoTab,
    i: usize,
    lanes: usize,
    selected: bool,
    hovered: bool,
) {
    let painter = ui.painter();
    let visuals = ui.visuals();
    let commit = &tab.commits[i];
    let labels = tab
        .labels
        .get(&commit.id)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let is_head = tab.head_oid == Some(commit.id);
    let row = &tab.graph.rows[i];

    if selected {
        painter.rect_filled(rect, 0.0, visuals.selection.bg_fill);
    } else if hovered {
        painter.rect_filled(rect, 0.0, visuals.widgets.hovered.weak_bg_fill);
    }

    let cols = Columns::new(rect, lanes);
    if lanes > 0 {
        let bg = if selected {
            visuals.selection.bg_fill
        } else {
            visuals.panel_fill
        };
        draw_graph(
            &painter.with_clip_rect(cols.graph),
            cols.graph,
            row,
            commit.parents.len() > 1,
            is_head,
            bg,
        );
    }

    let font = FontId::proportional(13.0);
    let text = if selected {
        visuals.selection.stroke.color
    } else {
        visuals.text_color()
    };
    let weak = if selected {
        text
    } else {
        visuals.weak_text_color()
    };

    let desc = painter.with_clip_rect(cols.description);
    let mut x = cols.description.left() + 4.0;
    for label in labels {
        x = draw_label(
            &desc,
            pos2(x, rect.center().y),
            label,
            lane_color(row.color),
        ) + 4.0;
    }
    desc.text(
        pos2(x, rect.center().y),
        Align2::LEFT_CENTER,
        &commit.summary,
        font.clone(),
        text,
    );

    for (col, value) in [
        (cols.author, commit.author.clone()),
        (cols.date, format_date(commit.time, commit.offset_minutes)),
    ] {
        painter.with_clip_rect(col.shrink2(vec2(4.0, 0.0))).text(
            col.left_center() + vec2(4.0, 0.0),
            Align2::LEFT_CENTER,
            value,
            font.clone(),
            weak,
        );
    }
    painter.text(
        cols.hash.left_center() + vec2(4.0, 0.0),
        Align2::LEFT_CENTER,
        commit.short_id(),
        FontId::monospace(12.0),
        weak,
    );
}

fn draw_graph(
    painter: &egui::Painter,
    rect: Rect,
    row: &GraphRow,
    is_merge: bool,
    is_head: bool,
    bg: Color32,
) {
    let x = |lane: usize| rect.left() + 4.0 + LANE_WIDTH * (lane as f32 + 0.5);
    let (top, mid, bottom) = (rect.top(), rect.center().y, rect.bottom());

    for line in &row.lines {
        let (y0, y1) = match line.half {
            Half::Top => (top, mid),
            Half::Bottom => (mid, bottom),
        };
        let (p0, p1) = (pos2(x(line.from), y0), pos2(x(line.to), y1));
        let stroke = Stroke::new(2.0, lane_color(line.color));
        if line.from == line.to {
            painter.line_segment([p0, p1], stroke);
        } else {
            let ym = (y0 + y1) / 2.0;
            painter.add(CubicBezierShape::from_points_stroke(
                [p0, pos2(p0.x, ym), pos2(p1.x, ym), p1],
                false,
                Color32::TRANSPARENT,
                stroke,
            ));
        }
    }

    let center = pos2(x(row.lane), mid);
    let color = lane_color(row.color);
    if is_merge {
        // Merge commits are drawn hollow.
        painter.circle_filled(center, 4.5, bg);
        painter.circle_stroke(center, 4.0, Stroke::new(2.0, color));
    } else {
        painter.circle_filled(center, 4.5, color);
    }
    if is_head {
        painter.circle_stroke(center, 7.0, Stroke::new(1.5, color));
    }
}

/// Draws a branch/tag badge starting at `pos` (left, vertical center) and returns its right edge.
pub fn draw_label(painter: &egui::Painter, pos: Pos2, label: &RefLabel, lane: Color32) -> f32 {
    let (fill, text) = match label.kind {
        RefKind::Local => (lane, Color32::WHITE),
        RefKind::Remote => (lane.gamma_multiply(0.45), Color32::WHITE),
        RefKind::Tag => (Color32::from_rgb(0xd9, 0xb3, 0x4a), Color32::BLACK),
    };
    let icon = match label.kind {
        RefKind::Tag => "🏷 ",
        _ if label.is_head => "✔ ",
        _ => "",
    };
    let galley = painter.layout_no_wrap(
        format!("{icon}{}", label.name),
        FontId::proportional(11.5),
        text,
    );
    let size = galley.size() + vec2(10.0, 4.0);
    let rect = Rect::from_min_size(pos2(pos.x, pos.y - size.y / 2.0), size);
    painter.rect_filled(rect, 4.0, fill);
    painter.galley(rect.min + vec2(5.0, 2.0), galley, text);
    rect.right()
}

/// Right-click menu of a commit.
fn commit_menu(ui: &mut egui::Ui, tab: &RepoTab, i: usize, cmds: &mut Commands) {
    let c: &CommitInfo = &tab.commits[i];
    let id = c.id;
    let sha = id.to_string();
    let short = c.short_id();
    let labels = tab.labels.get(&id).cloned().unwrap_or_default();
    let current = tab.head.clone();
    let is_merge = c.parents.len() > 1;
    let busy = tab.busy();

    ui.set_min_width(240.0);
    ui.add_enabled_ui(!busy, |ui| {
        for l in labels.iter().filter(|l| l.kind == RefKind::Local && !l.is_head) {
            if ui.button(t!("Check out {}", l.name)).clicked() {
                cmds.run(Op::Checkout(l.name.clone()));
                ui.close();
            }
        }
        for l in labels.iter().filter(|l| l.kind == RefKind::Remote) {
            let has_local = tab.branches.iter().any(|b| b.kind == BranchKind::Local && b.upstream.as_deref() == Some(&l.name));
            if !has_local && ui.button(t!("Check out {}", l.name)).clicked() {
                let local = l.name.split_once('/').map_or(l.name.as_str(), |(_, b)| b).to_owned();
                cmds.push(Command::Dialog(Dialog::checkout_remote(&l.name, &local)));
                ui.close();
            }
        }
        if ui.button(t!("Check out {} (detached)", short)).clicked() {
            cmds.push(Command::Dialog(Dialog::confirm(
                &t!("Check out commit"),
                &t!("Check out {}? HEAD will be detached: new commits will not belong to any branch until you create one.", short),
                Op::CheckoutDetached(sha.clone()),
                false,
            )));
            ui.close();
        }
        ui.separator();
        if ui.button(t!("New branch here…")).clicked() {
            cmds.push(Command::Dialog(Dialog::create_branch(&sha, &short)));
            ui.close();
        }
        if ui.button(t!("New tag here…")).clicked() {
            cmds.push(Command::Dialog(Dialog::create_tag(&sha, &short)));
            ui.close();
        }
        ui.separator();
        if let Some(cur) = &current {
            let target = labels.iter().find(|l| l.kind != RefKind::Tag).map_or(short.clone(), |l| l.name.clone());
            let rev = labels.iter().find(|l| l.kind != RefKind::Tag).map_or(sha.clone(), |l| l.name.clone());
            if tab.head_oid != Some(id) {
                if ui.button(t!("Merge {} into {}…", target, cur)).clicked() {
                    cmds.push(Command::Dialog(Dialog::merge(&rev, &target)));
                    ui.close();
                }
                if ui.button(t!("Rebase {} onto {}…", cur, target)).clicked() {
                    cmds.push(Command::Dialog(Dialog::confirm(
                        &t!("Rebase"),
                        &t!("Rebase {} onto {}? The commits of {} will be rewritten.", cur, target, cur),
                        Op::Rebase { onto: rev.clone() },
                        false,
                    )));
                    ui.close();
                }
            }
        }
        let in_head = tab.head_oid.is_some_and(|h| tab.repo.is_ancestor(id, h));
        if in_head && tab.head_oid != Some(id) && ui.button(t!("Interactive rebase after {}…", short)).clicked() {
            cmds.push(Command::PlanRebase(Some(id)));
            ui.close();
        }
        if in_head && c.parents.is_empty() && ui.button(t!("Interactive rebase from the first commit…")).clicked() {
            cmds.push(Command::PlanRebase(None));
            ui.close();
        }
        if !in_head && ui.button(t!("Cherry-pick {}", short)).clicked() {
            cmds.run(Op::CherryPick { id, is_merge });
            ui.close();
        }
        if ui.button(t!("Revert {}", short)).clicked() {
            cmds.run(Op::Revert { id, is_merge });
            ui.close();
        }
        // Branches that are not checked out can simply be moved here.
        let movable: Vec<String> = tab
            .branches
            .iter()
            .filter(|b| b.kind == BranchKind::Local && !b.is_head && b.target != Some(id))
            .map(|b| b.name.clone())
            .collect();
        if !movable.is_empty() {
            ui.menu_button(t!("Move branch to here"), |ui| {
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for name in movable {
                        if ui.button(&name).clicked() {
                            cmds.push(Command::Dialog(Dialog::confirm(
                                &t!("Move branch"),
                                &t!("Point {} at {}? Commits only on {} may become unreachable (you can undo right after).", name, short, name),
                                Op::MoveBranch { name: name.clone(), target: sha.clone() },
                                false,
                            )));
                            ui.close();
                        }
                    }
                });
            });
        }
        if let Some(cur) = &current {
            ui.menu_button(t!("Reset {} to here", cur), |ui| {
                for (mode, label, help) in [
                    (ResetMode::Soft, "Soft", t!("keep all changes staged")),
                    (ResetMode::Mixed, "Mixed", t!("keep changes, unstaged")),
                    (ResetMode::Hard, "Hard", t!("discard all changes")),
                ] {
                    if ui.button(format!("{label} — {help}")).clicked() {
                        cmds.push(Command::Dialog(Dialog::reset(&sha, &short, cur, mode)));
                        ui.close();
                    }
                }
            });
        }
    });
    ui.separator();
    if ui.button(t!("Copy commit hash")).clicked() {
        cmds.push(Command::Copy(sha.clone()));
        ui.close();
    }
    if ui.button(t!("Copy message")).clicked() {
        let msg = tab
            .repo
            .commit_details(id)
            .map(|d| d.message)
            .unwrap_or_else(|_| c.summary.clone());
        cmds.push(Command::Copy(msg.trim_end().to_owned()));
        ui.close();
    }
    if let Some(h) = &tab.hosted {
        let url = match h.kind {
            ferrum_core::forge::ForgeKind::GitHub => format!("{}/commit/{sha}", h.web_url()),
            ferrum_core::forge::ForgeKind::GitLab => format!("{}/-/commit/{sha}", h.web_url()),
        };
        if ui.button(t!("Open on {}", h.kind.name())).clicked() {
            cmds.push(Command::OpenUrl(url));
            ui.close();
        }
    }
}
