//! The commit history: search bar, graph and commit list.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use eframe::epaint::CubicBezierShape;
use eframe::epaint::text::{LayoutJob, TextFormat};
use ferrum_core::graph::{self, GraphRow, Half};
use ferrum_core::ops::ResetMode;
use ferrum_core::rebase::{Action, Step};
use ferrum_core::{BranchKind, CommitInfo, Oid, RefKind, RefLabel};

use crate::command::{Command, Commands, Op};
use crate::settings::Settings;
use crate::tab::RepoTab;
use crate::ui::dialogs::Dialog;
use crate::ui::history_model::{self, HistorySettings, SearchMode, label_ref};
use crate::ui::history_options::{self, cmd};
use crate::ui::{avatars, history_flow, history_look, issue_links};

const LANE_WIDTH: f32 = 14.0;
/// Very wide graphs are clipped (Shift+wheel scrolls them) so the description stays readable.
const MAX_GRAPH_LANES: usize = 10;
const AUTHOR_WIDTH: f32 = 150.0;
const DATE_WIDTH: f32 = 120.0;
const HASH_WIDTH: f32 = 64.0;
/// Load the next page when the list is scrolled this close to its end.
const LOAD_MORE_MARGIN: usize = 200;

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
    fn new(row: Rect, graph_lanes: usize, tall: bool) -> Self {
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
        // Tall rows put the author, date and hash under the description.
        let (hash, date, author) = if tall {
            let none = Rect::from_x_y_ranges(x..=x, row.y_range());
            (none, none, none)
        } else {
            (take(HASH_WIDTH), take(DATE_WIDTH), take(AUTHOR_WIDTH))
        };
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

fn search_hint(mode: SearchMode) -> String {
    match mode {
        SearchMode::Text => t!("Filter by message, author or hash"),
        SearchMode::Changes => t!("Search changes (git log -S), press Enter"),
        SearchMode::Paths => t!("Search file paths, press Enter"),
    }
}

fn mode_label(mode: SearchMode) -> String {
    match mode {
        SearchMode::Text => t!("Message"),
        SearchMode::Changes => t!("In changes"),
        SearchMode::Paths => t!("File path"),
    }
}

pub fn search_bar(ui: &mut egui::Ui, tab: &mut RepoTab, settings: &Settings, cmds: &mut Commands) {
    ui.horizontal(|ui| {
        history_options::view_menu(ui, tab, settings, cmds);
        ui.label("🔍");
        let mode = tab.history.ext.mode;
        let resp = ui.add(
            egui::TextEdit::singleline(&mut tab.history.search)
                .hint_text(search_hint(mode))
                .desired_width(300.0),
        );
        if tab.history.focus_search {
            resp.request_focus();
            tab.history.focus_search = false;
        }
        let mut changed = resp.changed();
        let (enter, shift) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.modifiers.shift));
        let enter = resp.lost_focus() && enter;
        let mut new_mode = mode;
        egui::ComboBox::from_id_salt("search-mode")
            .selected_text(mode_label(mode))
            .width(90.0)
            .show_ui(ui, |ui| {
                for m in [SearchMode::Text, SearchMode::Changes, SearchMode::Paths] {
                    ui.selectable_value(&mut new_mode, m, mode_label(m));
                }
            });
        if new_mode != mode {
            tab.history.ext.mode = new_mode;
            tab.history.content_hits = None;
            tab.history.ext.path_hits.clear();
            tab.history.ext.searched = None;
            changed = true;
        }
        if ui
            .checkbox(&mut tab.history.ext.scope_branch, t!("Current branch"))
            .on_hover_text(t!("Search only the commits of the current branch"))
            .changed()
        {
            tab.history.ext.searched = None;
            changed = true;
        }
        let term = tab.history.search.trim().to_owned();
        let job_mode = new_mode != SearchMode::Text;
        if job_mode {
            if changed && tab.history.ext.searched.as_deref() != Some(term.as_str()) {
                tab.history.content_hits = None;
            }
            let button = ui.button(t!("Search")).clicked();
            let fresh = tab.history.ext.searched.as_deref() != Some(term.as_str());
            if (button || (enter && fresh)) && !term.is_empty() {
                cmds.push(Command::FocusSearch);
                start_search_job(tab, ui.ctx());
                cmds.push(remember(&term));
            } else if enter {
                history_model::step_match(tab, !shift);
                cmds.push(Command::FocusSearch);
            }
        } else if enter && !term.is_empty() {
            history_model::step_match(tab, !shift);
            cmds.push(Command::FocusSearch);
            cmds.push(remember(&term));
        }
        let recent = &settings.history.recent_searches;
        if !recent.is_empty() {
            ui.menu_button("🕓", |ui| {
                for q in recent {
                    if ui.button(q).clicked() {
                        tab.history.search = q.clone();
                        tab.history.content_hits = None;
                        tab.history.ext.searched = None;
                        changed = true;
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text(t!("Recent searches"));
        }
        if !tab.history.search.is_empty()
            && ui
                .small_button(crate::icons::CLOSE)
                .on_hover_text(t!("Clear"))
                .clicked()
        {
            tab.history.search.clear();
            tab.history.content_hits = None;
            tab.history.ext.path_hits.clear();
            tab.history.ext.searched = None;
            changed = true;
        }
        if changed {
            tab.apply_search();
        }
        if let Some(v) = &tab.history.visible {
            ui.weak(t!("{} of {} commits", v.len(), tab.commits.len()))
                .on_hover_text(t!("Enter / F3: next match, Shift+Enter: previous"));
        } else if tab.history.ext.truncated {
            ui.weak(t!(
                "{} commits loaded",
                tab.history.ext.loaded(tab.commits.len())
            ));
        }
    });
}

fn remember(q: &str) -> Command {
    let q = q.to_owned();
    cmd(move |_, s| s.remember_search(&q))
}

pub(crate) fn start_search_job(tab: &mut RepoTab, ctx: &egui::Context) {
    let term = tab.history.search.trim().to_owned();
    let path = tab.path().to_path_buf();
    let all = !tab.history.ext.scope_branch;
    tab.history.ext.searched = Some(term.clone());
    if tab.history.ext.mode == SearchMode::Paths {
        tab.spawn_job(&t!("Search file paths"), ctx, move |_| {
            let repo = ferrum_core::Repo::open(&path).map_err(|e| e.to_string())?;
            let hits = repo.search_paths(&term, all).map_err(|e| e.to_string())?;
            Ok(crate::jobs::JobOutput::PathHits(hits))
        });
        return;
    }
    tab.spawn_job(&t!("Search in changes"), ctx, move |_| {
        let repo = ferrum_core::Repo::open(&path).map_err(|e| e.to_string())?;
        let hits = repo.search_content(&term, all).map_err(|e| e.to_string())?;
        Ok(crate::jobs::JobOutput::SearchHits(
            hits.into_iter().collect(),
        ))
    });
}

/// Up/Down move the selection, Home/End jump to the ends, Left/Right collapse
/// and expand the selected merge (when no text field has focus).
fn keyboard_navigation(ui: &egui::Ui, tab: &mut RepoTab) {
    if ui.ctx().memory(|m| m.focused().is_some()) || egui::Popup::is_any_open(ui.ctx()) {
        return;
    }
    let (up, down, home, end, left, right) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::Home),
            i.key_pressed(egui::Key::End),
            i.key_pressed(egui::Key::ArrowLeft),
            i.key_pressed(egui::Key::ArrowRight),
        )
    });
    if (left || right)
        && let Some(id) = tab.history.selected
    {
        history_model::set_merge_collapsed(tab, id, left);
        return;
    }
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

/// Where a dragged commit would go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Drop {
    Above(Oid),
    Below(Oid),
    Squash(Oid),
}

pub fn commit_list(ui: &mut egui::Ui, tab: &mut RepoTab, settings: &Settings, cmds: &mut Commands) {
    keyboard_navigation(ui, tab);
    let hs = &settings.history;
    avatars::set_network(hs.show_avatars && hs.network_avatars);
    let tall = hs.vertical_layout && !hs.single_line_rows;
    let row_height = if tall {
        crate::theme::tall_row_height()
    } else {
        crate::theme::row_height()
    };
    // The graph only makes sense for the full, unfiltered history.
    let filtered = tab.history.visible.is_some();
    let lanes = if filtered { 0 } else { tab.graph.width.max(1) };

    let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    let cols = Columns::new(header, lanes, tall);
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
        no_commits(ui, tab, filtered);
        return;
    }

    // Shift+wheel scrolls a graph wider than its column.
    let hidden_lanes = tab.graph.width.saturating_sub(MAX_GRAPH_LANES);
    if lanes > 0 && hidden_lanes > 0 && ui.rect_contains_pointer(ui.max_rect()) {
        let dx = ui.input_mut(|i| std::mem::take(&mut i.smooth_scroll_delta.x));
        let max = hidden_lanes as f32 * LANE_WIDTH;
        tab.history.ext.graph_offset = (tab.history.ext.graph_offset - dx).clamp(0.0, max);
    } else if hidden_lanes == 0 {
        tab.history.ext.graph_offset = 0.0;
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
            let target = pos as f32 * row_height - ui.available_height() / 3.0;
            scroll = scroll.vertical_scroll_offset(target.max(0.0));
        }
    }

    // Commits that can be dragged: the current branch down to its first merge.
    let chain = if tab.history.ext.drag.is_some() || !filtered {
        rewritable_chain(tab)
    } else {
        Vec::new()
    };
    let mut drop_target: Option<Drop> = None;
    let mut near_end = false;
    let mut toggle: Option<Oid> = None;
    let mut open_url: Option<String> = None;
    let mut hover_path: Option<usize> = None;

    // Rows must touch so graph lines are continuous.
    ui.spacing_mut().item_spacing.y = 0.0;
    scroll.show_rows(ui, row_height, count, |ui, range| {
        near_end = range.end + LOAD_MORE_MARGIN >= count;
        for i in range {
            let idx = row_of(tab, i);
            let (rect, resp) = ui.allocate_exact_size(
                vec2(ui.available_width(), row_height),
                Sense::click_and_drag(),
            );
            let id = tab.commits[idx].id;
            let cols = Columns::new(rect, lanes, tall);
            let draw = paint_row(ui, rect, &cols, tab, hs, idx, lanes, resp.hovered());

            let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
            let on_toggle = draw.toggle.zip(pointer).is_some_and(|(r, p)| r.contains(p));
            let on_link = pointer.and_then(|p| {
                draw.links
                    .iter()
                    .find(|(r, _)| r.contains(p))
                    .map(|(_, u)| u.clone())
            });
            if on_toggle || on_link.is_some() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                if on_toggle {
                    toggle = Some(id);
                } else if let Some(url) = on_link {
                    open_url = Some(url);
                } else if history_flow::click(tab, id, ui.input(|i| i.modifiers)) {
                    // Ctrl/Shift+click: several commits; two are compared.
                    crate::ui::compare::show_selection(tab);
                } else {
                    tab.history.selected = Some(id);
                    tab.load_commit(id, false);
                }
            }
            let on_label = pointer.and_then(|p| draw.labels.iter().find(|(r, _)| r.contains(p)));
            if resp.double_clicked()
                && let Some((_, l)) = on_label
            {
                // The double-clicked branch or tag label.
                history_flow::checkout_label(tab, l.kind, &l.name, l.is_head, cmds);
            } else if resp.double_clicked() && !on_toggle {
                // Check out the local branch at this commit, if there is one.
                let local = tab
                    .labels
                    .get(&id)
                    .and_then(|ls| ls.iter().find(|l| l.kind == RefKind::Local && !l.is_head));
                if let Some(l) = local {
                    cmds.run(Op::Checkout(l.name.clone()));
                }
            }
            // Branch labels: drag one onto another branch (see branch_dnd).
            let refs: Vec<_> = draw
                .labels
                .iter()
                .map(|(r, l)| (*r, l.kind, l.name.as_str()))
                .collect();
            let label_drag = crate::ui::branch_dnd::graph_row(ui, tab, &resp, &refs);
            if resp.drag_started() && !label_drag && chain.contains(&id) && !tab.busy() {
                tab.history.ext.drag = Some(id);
            }
            if let Some(src) = tab.history.ext.drag
                && src != id
                && chain.contains(&id)
                && let Some(p) = ui.input(|i| i.pointer.hover_pos())
                && rect.y_range().contains(p.y)
            {
                let f = (p.y - rect.top()) / rect.height();
                let target = if f < 0.3 {
                    Drop::Above(id)
                } else if f > 0.7 {
                    Drop::Below(id)
                } else {
                    Drop::Squash(id)
                };
                paint_drop(ui, rect, target);
                drop_target = Some(target);
            }
            if resp.hovered() {
                hover_path = Some(tab.graph.rows[idx].color);
            }
            let is_merge = tab.commits[idx].parents.len() > 1;
            let in_col = |r: Rect| pointer.is_some_and(|p| r.contains(p));
            let resp = if is_merge && in_col(cols.graph) {
                resp.on_hover_ui(|ui| merge_tooltip(ui, tab, id))
            } else if in_col(cols.date) {
                let c = &tab.commits[idx];
                resp.on_hover_text(crate::util::format_time(c.time, c.offset_minutes))
            } else if (in_col(cols.graph) || in_col(cols.description))
                && on_label.is_none()
                && !on_toggle
                && tab.history.ext.drag.is_none()
            {
                resp.on_hover_ui(|ui| history_look::preview_card(ui, tab, idx, hs.network_avatars))
            } else {
                resp
            };
            resp.context_menu(|ui| commit_menu(ui, tab, idx, cmds));
        }
    });

    if tab.history.ext.hover_path != hover_path {
        tab.history.ext.hover_path = hover_path;
        ui.ctx().request_repaint();
    }
    if let Some(src) = tab.history.ext.drag {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        if ui.input(|i| i.pointer.any_released()) {
            tab.history.ext.drag = None;
            if let Some(target) = drop_target
                && let Some(d) = drag_plan(tab, &chain, src, target)
            {
                cmds.push(Command::Dialog(d));
            }
        }
    }
    if let Some(id) = toggle {
        history_model::toggle_merge(tab, id);
    }
    if let Some(url) = open_url {
        cmds.push(Command::OpenUrl(url));
    }
    if near_end && !filtered && tab.history.ext.truncated {
        history_model::load_more(tab);
    }
}

/// The empty list: nothing matches the search, or no commit yet.
fn no_commits(ui: &mut egui::Ui, tab: &mut RepoTab, filtered: bool) {
    use crate::ui::empty_state;
    if filtered {
        empty_state::show(
            ui,
            crate::icons::SEARCH,
            &t!("No commits match the search."),
            &t!("Try other words, or search in the changes or file paths."),
            |ui| {
                if ui.button(t!("Clear search")).clicked() {
                    tab.history.search.clear();
                    tab.history.content_hits = None;
                    tab.history.ext.path_hits.clear();
                    tab.history.ext.searched = None;
                    tab.apply_search();
                }
            },
        );
    } else {
        empty_state::show(
            ui,
            crate::icons::GIT_COMMIT,
            &t!("No commits yet."),
            &t!("Your first commit will show up here."),
            |ui| {
                empty_state::action(ui, crate::keymap::Action::ShowChanges, &t!("Local changes"));
            },
        );
    }
}

/// What a painted row exposes for clicks.
struct RowHits {
    /// The collapse / expand box of a merge.
    toggle: Option<Rect>,
    links: Vec<(Rect, String)>,
    /// Branch and tag labels, for double-click checkout.
    labels: Vec<(Rect, Label)>,
}

#[allow(clippy::too_many_arguments)]
fn paint_row(
    ui: &egui::Ui,
    rect: Rect,
    cols: &Columns,
    tab: &RepoTab,
    hs: &HistorySettings,
    i: usize,
    lanes: usize,
    hovered: bool,
) -> RowHits {
    let painter = ui.painter();
    let visuals = ui.visuals();
    let ext = &tab.history.ext;
    let commit = &tab.commits[i];
    let selected = tab.history.selected == Some(commit.id)
        || tab.details.picked.contains(&commit.id)
        || crate::ui::history_flow::is_picked(tab, commit.id);
    let is_head = tab.head_oid == Some(commit.id);
    let row = &tab.graph.rows[i];
    let unreachable = ext.unreachable.contains(&commit.id);
    let dimmed = unreachable
        || ext
            .highlight_author
            .as_ref()
            .is_some_and(|a| !a.eq_ignore_ascii_case(&commit.email));
    let mut hits = RowHits {
        toggle: None,
        links: Vec::new(),
        labels: Vec::new(),
    };

    if selected {
        painter.rect_filled(rect, 0.0, visuals.selection.bg_fill);
    } else if hovered {
        painter.rect_filled(rect, 0.0, visuals.widgets.hovered.weak_bg_fill);
    }
    if ext.drag == Some(commit.id) {
        painter.rect_stroke(
            rect.shrink(1.0),
            2.0,
            Stroke::new(1.0, visuals.selection.stroke.color),
            egui::StrokeKind::Inside,
        );
    }

    let bg = if selected {
        visuals.selection.bg_fill
    } else {
        visuals.panel_fill
    };
    if lanes > 0 {
        draw_graph(
            &painter.with_clip_rect(cols.graph.intersect(painter.clip_rect())),
            cols.graph,
            row,
            ext.graph_offset,
            Dot {
                merge: commit.parents.len() > 1,
                head: is_head,
                grey: unreachable,
                stash: ext.stashes.contains_key(&commit.id),
            },
            bg,
            ext.hover_path,
        );
    }

    let font = FontId::proportional(13.0);
    let mut text = if selected {
        visuals.selection.stroke.color
    } else {
        visuals.text_color()
    };
    let mut weak = if selected {
        text
    } else {
        visuals.weak_text_color()
    };
    if dimmed {
        text = text.gamma_multiply(0.45);
        weak = weak.gamma_multiply(0.45);
    }
    let tall = cols.author.width() == 0.0;
    let line1 = if tall {
        rect.top() + 13.0
    } else {
        rect.center().y
    };

    let desc_rect = cols.description;
    let desc = painter.with_clip_rect(desc_rect.intersect(painter.clip_rect()));
    let mut x = desc_rect.left() + 4.0;
    if tall && hs.show_avatars {
        let r = Rect::from_center_size(pos2(x + 13.0, rect.center().y), vec2(26.0, 26.0));
        avatars::paint(
            ui,
            &desc,
            r,
            &commit.author,
            &commit.email,
            hs.network_avatars,
        );
        x += 32.0;
    }
    let text_left = x;

    // Collapse box of merges.
    if commit.parents.len() > 1 && !ext.prefs.log.first_parent {
        let collapsed = ext.is_collapsed(commit.id);
        let r = Rect::from_center_size(pos2(x + 6.0, line1), vec2(12.0, 12.0));
        desc.rect_stroke(r, 2.0, Stroke::new(1.0, weak), egui::StrokeKind::Inside);
        desc.text(
            r.center(),
            Align2::CENTER_CENTER,
            if collapsed { "+" } else { "-" },
            FontId::monospace(11.0),
            weak,
        );
        hits.toggle = Some(r.expand(2.0));
        x += 16.0;
        if let Some(n) = ext.hidden_under.get(&commit.id).filter(|_| collapsed) {
            x = draw_badge(
                &desc,
                pos2(x, line1),
                &format!("+{n}"),
                weak.gamma_multiply(0.35),
                text,
                false,
            ) + 4.0;
        }
    }
    x = crate::ui::bisect::paint_mark(
        &desc,
        rect,
        pos2(x, line1),
        tab,
        commit.id,
        visuals.dark_mode,
    );
    if ext.unpushed.contains(&commit.id) {
        x = icon(&desc, pos2(x, line1), "⬆", lane_color(2));
    }
    if ext.unpulled.contains(&commit.id) {
        x = icon(&desc, pos2(x, line1), "⬇", lane_color(3));
    }
    if let Some(index) = ext.stashes.get(&commit.id) {
        x = draw_badge(
            &desc,
            pos2(x, line1),
            &format!("📦 stash@{{{index}}}"),
            visuals.widgets.inactive.weak_bg_fill,
            visuals.text_color(),
            false,
        ) + 4.0;
    }
    let lane = lane_color(row.color);
    for label in visible_labels(tab, hs, commit.id) {
        let badge = history_look::Badge {
            text: &label.text,
            kind: label.kind,
            is_head: label.is_head,
            tracked: label.tracked,
        };
        let r = history_look::paint_badge(ui, &desc, pos2(x, line1), &badge, lane, dimmed);
        x = r.right() + 4.0;
        hits.labels.push((r.intersect(desc_rect), label));
    }

    // Summary, with the conventional prefix and issue links highlighted.
    let summary = if ext.stashes.contains_key(&commit.id) {
        commit.summary.trim_start_matches("On ").to_owned()
    } else {
        commit.summary.clone()
    };
    let links = issue_links::find(&summary, &ext.prefs.issue_rules, tab.hosted.as_ref());
    let prefix = conventional_prefix(&summary);
    let link_color = visuals.hyperlink_color;
    let mut job = LayoutJob::default();
    let mut pos = 0;
    let push = |job: &mut LayoutJob, s: &str, color: Color32, underline: bool| {
        if s.is_empty() {
            return;
        }
        let mut f = TextFormat::simple(font.clone(), color);
        if underline {
            f.underline = Stroke::new(1.0, color);
        }
        job.append(s, 0.0, f);
    };
    if let Some((end, kind)) = prefix {
        let c = if dimmed {
            prefix_color(kind).gamma_multiply(0.45)
        } else {
            prefix_color(kind)
        };
        push(&mut job, &summary[..end], c, false);
        pos = end;
    }
    let prefix_end = pos;
    for l in links.iter().filter(|l| l.range.start >= prefix_end) {
        push(&mut job, &summary[pos..l.range.start], text, false);
        push(&mut job, &summary[l.range.clone()], link_color, true);
        pos = l.range.end;
    }
    push(&mut job, &summary[pos..], text, false);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let origin = pos2(x, line1 - galley.size().y / 2.0);
    // Link rectangles, from character positions in the laid-out text.
    for l in links
        .iter()
        .filter(|l| l.range.start >= prefix.map_or(0, |p| p.0))
    {
        let start = summary[..l.range.start].chars().count();
        let end = summary[..l.range.end].chars().count();
        let a = galley.pos_from_cursor(egui::text::CCursor::new(start));
        let b = galley.pos_from_cursor(egui::text::CCursor::new(end));
        let r = Rect::from_min_max(origin + a.min.to_vec2(), origin + b.max.to_vec2())
            .intersect(desc_rect);
        if r.width() > 0.0 {
            hits.links.push((r, l.url.clone()));
        }
    }
    let summary_end = origin.x + galley.size().x;
    desc.galley(origin, galley, text);
    let mut tail = summary_end + 6.0;
    if ext.has_body.contains(&commit.id) {
        tail = history_look::paint_body_mark(&desc, pos2(tail, line1), weak).right() + 4.0;
    }
    if let Some(files) = ext
        .path_hits
        .get(&commit.id)
        .filter(|_| tab.history.visible.is_some())
    {
        let shown: Vec<&str> = files.iter().take(3).map(String::as_str).collect();
        let more = files.len().saturating_sub(3);
        let mut s = format!("📄 {}", shown.join(", "));
        if more > 0 {
            s.push_str(&t!(" and {} more", more));
        }
        desc.text(
            pos2(tail, line1),
            Align2::LEFT_CENTER,
            s,
            FontId::proportional(12.0),
            weak,
        );
    }

    let date = history_look::list_date(commit.time, commit.offset_minutes, hs.relative_dates);
    if tall {
        let y = rect.top() + 29.0;
        let line = format!("{}  ·  {}  ·  {}", commit.author, date, commit.short_id());
        desc.text(
            pos2(text_left, y),
            Align2::LEFT_CENTER,
            line,
            FontId::proportional(12.0),
            weak,
        );
        return hits;
    }
    let mut author_x = cols.author.left() + 4.0;
    let author_clip = painter.with_clip_rect(
        cols.author
            .shrink2(vec2(4.0, 0.0))
            .intersect(painter.clip_rect()),
    );
    if hs.show_avatars {
        let r = Rect::from_center_size(pos2(author_x + 8.0, rect.center().y), vec2(16.0, 16.0));
        avatars::paint(
            ui,
            &author_clip,
            r,
            &commit.author,
            &commit.email,
            hs.network_avatars,
        );
        author_x += 21.0;
    }
    author_clip.text(
        pos2(author_x, rect.center().y),
        Align2::LEFT_CENTER,
        &commit.author,
        font.clone(),
        weak,
    );
    painter
        .with_clip_rect(
            cols.date
                .shrink2(vec2(4.0, 0.0))
                .intersect(painter.clip_rect()),
        )
        .text(
            cols.date.left_center() + vec2(4.0, 0.0),
            Align2::LEFT_CENTER,
            date,
            font,
            weak,
        );
    painter.text(
        cols.hash.left_center() + vec2(4.0, 0.0),
        Align2::LEFT_CENTER,
        commit.short_id(),
        FontId::monospace(12.0),
        weak,
    );
    hits
}

/// Draws a small icon and returns where the next item starts.
fn icon(painter: &egui::Painter, pos: Pos2, glyph: &str, color: Color32) -> f32 {
    let r = painter.text(
        pos,
        Align2::LEFT_CENTER,
        glyph,
        FontId::proportional(12.0),
        color,
    );
    r.right() + 4.0
}

/// Kind of a conventional commit prefix (`feat`, `fix(ui)!`…) and where it ends.
pub fn conventional_prefix(summary: &str) -> Option<(usize, &str)> {
    const KINDS: [&str; 11] = [
        "feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore",
        "revert",
    ];
    let colon = summary.find(':')?;
    let head = summary[..colon].trim_end_matches('!');
    let kind = match head.split_once('(') {
        Some((k, scope)) if scope.ends_with(')') && !scope.contains(' ') => k,
        Some(_) => return None,
        None => head,
    };
    let kind = KINDS.iter().find(|k| k.eq_ignore_ascii_case(kind))?;
    Some((colon + 1, kind))
}

fn prefix_color(kind: &str) -> Color32 {
    match kind {
        "feat" => lane_color(2),
        "fix" => lane_color(1),
        "perf" => lane_color(3),
        "refactor" => lane_color(4),
        "docs" => lane_color(0),
        "test" => lane_color(5),
        "revert" => lane_color(6),
        _ => Color32::from_gray(140),
    }
}

/// A label to draw: its text and how it looks.
struct Label {
    text: String,
    /// The full ref name (`text` may be shortened or decorated).
    name: String,
    kind: RefKind,
    is_head: bool,
    /// Also stands for its remote branch (compact labels).
    tracked: bool,
}

/// Branch and tag labels of a commit after the graph filters, merged into
/// one label per branch in compact mode (`main ☁` for main and origin/main).
fn visible_labels(tab: &RepoTab, hs: &HistorySettings, id: Oid) -> Vec<Label> {
    let log = &tab.history.ext.prefs.log;
    let Some(all) = tab.labels.get(&id) else {
        return Vec::new();
    };
    let shown: Vec<&RefLabel> = all
        .iter()
        .filter(|l| !(l.kind == RefKind::Tag && log.hide_tags))
        .filter(|l| l.kind == RefKind::Tag || !history_options::is_hidden(tab, &label_ref(l)))
        .collect();
    let mut out = Vec::new();
    let mut merged = Vec::new();
    for l in &shown {
        let mut tracked = false;
        if hs.compact_labels && l.kind == RefKind::Local {
            let remote = shown.iter().find(|r| {
                r.kind == RefKind::Remote
                    && r.name.split_once('/').is_some_and(|(_, b)| b == l.name)
            });
            if let Some(r) = remote {
                tracked = true;
                merged.push(r.name.clone());
            }
        }
        out.push(Label {
            text: l.name.clone(),
            tracked,
            name: l.name.clone(),
            kind: l.kind,
            is_head: l.is_head,
        });
    }
    let mut labels: Vec<Label> = out
        .into_iter()
        .zip(&shown)
        .filter(|(_, l)| !(l.kind == RefKind::Remote && merged.contains(&l.name)))
        .map(|(x, _)| x)
        .collect();
    if hs.compact_labels {
        for l in &mut labels {
            if l.text.chars().count() > 24 {
                l.text = l.text.chars().take(23).collect::<String>() + "…";
            }
        }
    }
    labels
}

struct Dot {
    merge: bool,
    head: bool,
    grey: bool,
    stash: bool,
}

fn draw_graph(
    painter: &egui::Painter,
    rect: Rect,
    row: &GraphRow,
    offset: f32,
    dot: Dot,
    bg: Color32,
    focus: Option<usize>,
) {
    // The hovered branch path is drawn a bit thicker, the others fainter.
    let look = |color: usize| match focus {
        Some(f) if f == color => (lane_color(color), 2.6),
        Some(_) => (lane_color(color).gamma_multiply(0.45), 2.0),
        None => (lane_color(color), 2.0),
    };
    let x = |lane: usize| rect.left() + 4.0 + LANE_WIDTH * (lane as f32 + 0.5) - offset;
    let (top, mid, bottom) = (rect.top(), rect.center().y, rect.bottom());

    for line in &row.lines {
        let (y0, y1) = match line.half {
            Half::Top => (top, mid),
            Half::Bottom => (mid, bottom),
        };
        let (p0, p1) = (pos2(x(line.from), y0), pos2(x(line.to), y1));
        let (c, w) = look(line.color);
        let stroke = Stroke::new(w, c);
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
    let color = if dot.grey {
        Color32::from_gray(128)
    } else {
        look(row.color).0
    };
    if dot.stash {
        let r = Rect::from_center_size(center, vec2(9.0, 9.0));
        painter.rect_filled(r, 1.0, bg);
        painter.rect_stroke(r, 1.0, Stroke::new(2.0, color), egui::StrokeKind::Inside);
    } else if dot.merge || dot.grey {
        // Merge and unreachable commits are drawn hollow.
        painter.circle_filled(center, 4.5, bg);
        painter.circle_stroke(center, 4.0, Stroke::new(2.0, color));
    } else {
        painter.circle_filled(center, 4.5, color);
    }
    if dot.head {
        painter.circle_stroke(center, 7.0, Stroke::new(1.5, color));
    }
}

/// Draws a rounded badge starting at `pos` (left, vertical center) and returns its right edge.
fn draw_badge(
    painter: &egui::Painter,
    pos: Pos2,
    text: &str,
    fill: Color32,
    fg: Color32,
    bold: bool,
) -> f32 {
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(11.5), fg);
    let size = galley.size() + vec2(10.0, 4.0);
    let rect = Rect::from_min_size(pos2(pos.x, pos.y - size.y / 2.0), size);
    painter.rect_filled(rect, 4.0, fill);
    if bold {
        // The current branch: outlined, and the text drawn twice for weight.
        painter.rect_stroke(rect, 4.0, Stroke::new(1.5, fg), egui::StrokeKind::Inside);
        painter.galley(rect.min + vec2(5.4, 2.0), galley.clone(), fg);
    }
    painter.galley(rect.min + vec2(5.0, 2.0), galley, fg);
    rect.right()
}

/// Tooltip of a merge: the commits it brought in, with their own small graph.
fn merge_tooltip(ui: &mut egui::Ui, tab: &mut RepoTab, id: Oid) {
    const SHOWN: usize = 15;
    let ext = &mut tab.history.ext;
    if let std::collections::hash_map::Entry::Vacant(e) = ext.merge_cache.entry(id) {
        e.insert(tab.repo.merge_contents(id, 1_000).unwrap_or_default());
    }
    let list = &ext.merge_cache[&id];
    let n = list.len();
    ui.strong(if n == 1 {
        t!("This merge brought in 1 commit")
    } else {
        t!("This merge brought in {} commits", n)
    });
    let shown = &list[..n.min(SHOWN)];
    let g = graph::layout(shown);
    let width = g.width.min(6) as f32 * LANE_WIDTH + 8.0;
    ui.spacing_mut().item_spacing.y = 0.0;
    for (c, row) in shown.iter().zip(&g.rows) {
        let (rect, _) = ui.allocate_exact_size(vec2(420.0, 20.0), Sense::hover());
        let gr = Rect::from_min_size(rect.min, vec2(width, rect.height()));
        draw_graph(
            &ui.painter().with_clip_rect(gr),
            gr,
            row,
            0.0,
            Dot {
                merge: c.parents.len() > 1,
                head: false,
                grey: false,
                stash: false,
            },
            ui.visuals().panel_fill,
            None,
        );
        let text = format!("{}  {}", c.short_id(), c.summary);
        ui.painter().with_clip_rect(rect).text(
            pos2(gr.right() + 4.0, rect.center().y),
            Align2::LEFT_CENTER,
            text,
            FontId::proportional(12.0),
            ui.visuals().text_color(),
        );
    }
    if n > SHOWN {
        ui.weak(t!("and {} more", n - SHOWN));
    }
}

/// Commits of the current branch that drag and drop can rewrite: from HEAD
/// along first parents, down to (not including) the first merge.
pub(crate) fn rewritable_chain(tab: &RepoTab) -> Vec<Oid> {
    let mut out = Vec::new();
    if tab.head.is_none() || tab.state != ferrum_core::RepoState::Clean {
        return out;
    }
    let mut cur = tab.head_oid;
    while let Some(id) = cur {
        let Some(&i) = tab.index.get(&id) else { break };
        let c = &tab.commits[i];
        if c.parents.len() > 1 || out.len() >= 500 {
            break;
        }
        out.push(id);
        cur = c.parents.first().copied();
    }
    out
}

fn paint_drop(ui: &egui::Ui, rect: Rect, target: Drop) {
    let color = ui.visuals().selection.stroke.color;
    let p = ui.painter();
    match target {
        Drop::Above(_) => {
            p.line_segment([rect.left_top(), rect.right_top()], Stroke::new(2.0, color));
        }
        Drop::Below(_) => {
            p.line_segment(
                [rect.left_bottom(), rect.right_bottom()],
                Stroke::new(2.0, color),
            );
        }
        Drop::Squash(_) => {
            p.rect_stroke(
                rect.shrink(1.0),
                3.0,
                Stroke::new(2.0, color),
                egui::StrokeKind::Inside,
            );
        }
    }
}

/// Builds the interactive rebase for a drop and asks for confirmation.
pub(crate) fn drag_plan(tab: &RepoTab, chain: &[Oid], src: Oid, target: Drop) -> Option<Dialog> {
    let (Drop::Above(t) | Drop::Below(t) | Drop::Squash(t)) = target;
    let si = chain.iter().position(|&c| c == src)?;
    let ti = chain.iter().position(|&c| c == t)?;
    let deepest = si.max(ti);
    let info = |id: Oid| tab.index.get(&id).map(|&i| tab.commits[i].clone());
    let base = info(chain[deepest])?.parents.first().copied();
    // Oldest first, as in a rebase todo list.
    let mut order: Vec<CommitInfo> = chain[..=deepest]
        .iter()
        .rev()
        .filter_map(|&id| info(id))
        .collect();
    let moved = order.iter().position(|c| c.id == src)?;
    let moved = order.remove(moved);
    let at = order.iter().position(|c| c.id == t)?;
    let (insert, action) = match target {
        // Above in the list means newer: after the target in the todo list.
        Drop::Above(_) => (at + 1, Action::Pick),
        Drop::Below(_) => (at, Action::Pick),
        Drop::Squash(_) => (at + 1, Action::Squash),
    };
    let src_label = format!("{} {}", moved.short_id(), moved.summary);
    let t_info = info(t)?;
    let t_label = format!("{} {}", t_info.short_id(), t_info.summary);
    order.insert(insert, moved);
    let steps: Vec<Step> = order
        .into_iter()
        .map(|commit| {
            let action = if commit.id == src {
                action.clone()
            } else {
                Action::Pick
            };
            Step { commit, action }
        })
        .collect();
    let unchanged = steps
        .iter()
        .map(|s| s.commit.id)
        .eq(chain[..=deepest].iter().rev().copied())
        && action == Action::Pick;
    if unchanged {
        return None;
    }
    let branch = tab.head.clone().unwrap_or_default();
    let text = match target {
        Drop::Above(_) => t!(
            "Move {} above {}? The commits of {} will be rewritten.",
            src_label,
            t_label,
            branch
        ),
        Drop::Below(_) => t!(
            "Move {} below {}? The commits of {} will be rewritten.",
            src_label,
            t_label,
            branch
        ),
        Drop::Squash(_) => t!(
            "Squash {} into {}? The commits of {} will be rewritten.",
            src_label,
            t_label,
            branch
        ),
    };
    Some(Dialog::confirm(
        &t!("Rewrite history"),
        &text,
        Op::InteractiveRebase { base, steps },
        false,
    ))
}

/// Right-click menu of a commit.
fn commit_menu(ui: &mut egui::Ui, tab: &RepoTab, i: usize, cmds: &mut Commands) {
    let c: &CommitInfo = &tab.commits[i];
    let id = c.id;
    if history_flow::multi_menu(ui, tab, id, cmds) {
        return;
    }
    let sha = id.to_string();
    let short = c.short_id();
    let labels = tab.labels.get(&id).cloned().unwrap_or_default();
    let current = tab.head.clone();
    let is_merge = c.parents.len() > 1;
    let busy = tab.busy();

    ui.set_min_width(240.0);
    crate::ui::custom_commands::commit_menu(ui, tab, id, cmds);
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
            // A branch name, else a remote branch or tag, else the hash.
            let best = [RefKind::Local, RefKind::Remote, RefKind::Tag].into_iter().find_map(|k| labels.iter().find(|l| l.kind == k));
            let target = best.map_or(short.clone(), |l| l.name.clone());
            let rev = best.map_or(sha.clone(), |l| l.name.clone());
            if tab.head_oid != Some(id) {
                if ui.button(t!("Merge {} into {}…", target, cur)).clicked() {
                    cmds.push(Command::Dialog(Dialog::merge(&rev, &target)));
                    ui.close();
                }
                if ui.button(t!("Rebase {} onto {}…", cur, target)).clicked() {
                    cmds.push(Command::Dialog(crate::ui::flow_dialogs::rebase(tab, &rev)));
                    ui.close();
                }
            }
        }
        let in_head = tab.head_oid.is_some_and(|h| tab.repo.is_ancestor(id, h));
        history_flow::rebase_menu(ui, tab, c, cmds);
        if !in_head && ui.button(t!("Cherry-pick {}…", short)).clicked() {
            cmds.push(Command::Dialog(crate::ui::flow_dialogs::cherry_pick(tab, &[id])));
            ui.close();
        }
        if ui.button(t!("Revert {}…", short)).clicked() {
            cmds.push(Command::Dialog(crate::ui::flow_dialogs::revert(tab, id)));
            ui.close();
        }
        crate::ui::bisect::commit_menu(ui, tab, id, cmds);
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
    if is_merge && !tab.history.ext.prefs.log.first_parent {
        let collapsed = tab.history.ext.is_collapsed(id);
        let label = if collapsed {
            t!("Expand merge  (Right)")
        } else {
            t!("Collapse merge  (Left)")
        };
        if ui.button(label).clicked() {
            cmds.push(cmd(move |tab, _| history_model::toggle_merge(tab, id)));
            ui.close();
        }
    }
    let email = c.email.clone();
    if tab.history.ext.highlight_author.as_deref() == Some(email.as_str()) {
        if ui.button(t!("Stop highlighting {}", c.author)).clicked() {
            cmds.push(cmd(|tab, _| tab.history.ext.highlight_author = None));
            ui.close();
        }
    } else if ui.button(t!("Highlight commits by {}", c.author)).clicked() {
        cmds.push(cmd(move |tab, _| {
            tab.history.ext.highlight_author = Some(email)
        }));
        ui.close();
    }
    for l in issue_links::find(
        &c.summary,
        &tab.history.ext.prefs.issue_rules,
        tab.hosted.as_ref(),
    ) {
        if ui
            .button(t!("Open {}", &c.summary[l.range.clone()]))
            .clicked()
        {
            cmds.push(Command::OpenUrl(l.url.clone()));
            ui.close();
        }
    }
    ui.separator();
    if ui.button(t!("Compare with working copy")).clicked() {
        cmds.push(crate::ui::details::compare_with_workdir(id, None));
        ui.close();
    }
    if ui.button(t!("Open in new window")).clicked() {
        cmds.push(cmd(move |tab, _| {
            crate::ui::details::open_window(tab, id, &Default::default())
        }));
        ui.close();
    }
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
    crate::ui::patches::commit_menu(ui, id, cmds);
    crate::ui::ref_dialogs::label_menu(ui, tab, &labels, cmds);
    if let Some(h) = &tab.hosted {
        let url = h.commit_url(&sha);
        if ui.button(t!("Open on {}", h.kind.name())).clicked() {
            cmds.push(Command::OpenUrl(url));
            ui.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_conventional_prefixes() {
        assert_eq!(conventional_prefix("feat: add x"), Some((5, "feat")));
        assert_eq!(conventional_prefix("fix(ui)!: crash"), Some((9, "fix")));
        assert_eq!(conventional_prefix("Fix: case"), Some((4, "fix")));
        assert_eq!(conventional_prefix("Merge branch 'a': b"), None);
        assert_eq!(conventional_prefix("note: x"), None);
        assert_eq!(conventional_prefix("no colon"), None);
    }
}
