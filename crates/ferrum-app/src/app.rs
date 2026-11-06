use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use eframe::epaint::CubicBezierShape;
use ferrum_core::graph::{self, Graph, GraphRow, Half};
use ferrum_core::{BranchInfo, BranchKind, CommitInfo, Oid, RefKind, RefLabel, Repo};

const COMMIT_LIMIT: usize = 5_000;

/// One open repository (one tab).
struct RepoTab {
    repo: Repo,
    head: Option<String>,
    branches: Vec<BranchInfo>,
    tags: Vec<String>,
    commits: Vec<CommitInfo>,
    graph: Graph,
    labels: HashMap<Oid, Vec<RefLabel>>,
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
            graph: Graph::default(),
            labels: HashMap::new(),
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
        self.graph = graph::layout(&self.commits);
        self.labels = self.repo.ref_labels().map_err(err)?;
        self.selected = self.selected.filter(|&i| i < self.commits.len());
        Ok(())
    }
}

pub struct FerrumApp {
    tabs: Vec<RepoTab>,
    active: usize,
    error: Option<String>,
    /// Dev aid: with `FERRUM_SCREENSHOT=out.ppm`, save the window after a few frames and quit.
    screenshot: Option<(PathBuf, u32)>,
}

impl FerrumApp {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        let screenshot = std::env::var_os("FERRUM_SCREENSHOT").map(|p| (PathBuf::from(p), 0));
        let mut app = Self {
            tabs: Vec::new(),
            active: 0,
            error: None,
            screenshot,
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

    fn maybe_screenshot(&mut self, ctx: &egui::Context) {
        let Some((path, frames)) = &mut self.screenshot else {
            return;
        };
        *frames += 1;
        ctx.request_repaint();
        if *frames == 10 {
            let (path, ctx2) = (path.clone(), ctx.clone());
            ctx.request_screenshot(move |img| {
                let mut out = format!("P6\n{} {}\n255\n", img.size[0], img.size[1]).into_bytes();
                out.extend(img.pixels.iter().flat_map(|c| [c.r(), c.g(), c.b()]));
                if let Err(e) = std::fs::write(&path, out) {
                    eprintln!("screenshot: {e}");
                }
                ctx2.send_viewport_cmd(egui::ViewportCommand::Close);
            });
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
                        let tracking = b
                            .upstream
                            .as_deref()
                            .map_or("no upstream".into(), |u| format!("tracks {u}"));
                        ui.add(egui::Label::new(text).truncate())
                            .on_hover_text(format!("{}\n{tracking}", b.name));
                    }
                });
        };
        section(ui, "Branches", BranchKind::Local);
        section(ui, "Remotes", BranchKind::Remote);
        egui::CollapsingHeader::new("Tags").show(ui, |ui| {
            for t in &tab.tags {
                ui.add(egui::Label::new(t).truncate());
            }
        });
    });
}

const ROW_HEIGHT: f32 = 24.0;
const LANE_WIDTH: f32 = 14.0;
/// Very wide graphs are clipped so the description stays readable.
const MAX_GRAPH_LANES: usize = 16;
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

fn lane_color(index: usize) -> Color32 {
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
        let graph_w = graph_lanes.clamp(1, MAX_GRAPH_LANES) as f32 * LANE_WIDTH + 8.0;
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
        let description = Rect::from_x_y_ranges(graph.right()..=author.left() - 8.0, row.y_range());
        Self {
            graph,
            description,
            author,
            date,
            hash,
        }
    }
}

fn commit_list(ui: &mut egui::Ui, tab: &mut RepoTab) {
    let lanes = tab.graph.width;

    // Header.
    let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    let cols = Columns::new(header, lanes);
    let font = FontId::proportional(12.0);
    let weak = ui.visuals().weak_text_color();
    let painter = ui.painter();
    for (rect, title) in [
        (cols.graph, "Graph"),
        (cols.description, "Description"),
        (cols.author, "Author"),
        (cols.date, "Date"),
        (cols.hash, "Commit"),
    ] {
        painter.text(
            rect.left_center() + vec2(4.0, 0.0),
            Align2::LEFT_CENTER,
            title,
            font.clone(),
            weak,
        );
    }
    ui.separator();

    egui::ScrollArea::vertical().auto_shrink(false).show_rows(
        ui,
        ROW_HEIGHT,
        tab.commits.len(),
        |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for i in range {
                let (rect, resp) =
                    ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
                if resp.clicked() {
                    tab.selected = Some(i);
                }
                let selected = tab.selected == Some(i);
                commit_row(ui, rect, tab, i, selected, resp.hovered());
            }
        },
    );
}

fn commit_row(ui: &egui::Ui, rect: Rect, tab: &RepoTab, i: usize, selected: bool, hovered: bool) {
    let painter = ui.painter();
    let visuals = ui.visuals();
    let commit = &tab.commits[i];
    let row = &tab.graph.rows[i];
    let labels = tab
        .labels
        .get(&commit.id)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let is_head = labels.iter().any(|l| l.is_head);

    if selected {
        painter.rect_filled(rect, 0.0, visuals.selection.bg_fill);
    } else if hovered {
        painter.rect_filled(rect, 0.0, visuals.widgets.hovered.weak_bg_fill);
    }

    let cols = Columns::new(rect, tab.graph.width);
    draw_graph(
        &painter.with_clip_rect(cols.graph),
        cols.graph,
        row,
        commit.parents.len() > 1,
        is_head,
    );

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

    // Description: ref labels, then the summary.
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
    let summary_color = if is_head || !labels.is_empty() {
        text
    } else {
        text.gamma_multiply(0.9)
    };
    desc.text(
        pos2(x, rect.center().y),
        Align2::LEFT_CENTER,
        &commit.summary,
        font.clone(),
        summary_color,
    );

    for (col, value, color) in [
        (cols.author, commit.author.as_str(), weak),
        (
            cols.date,
            &format_time(commit.time, commit.offset_minutes),
            weak,
        ),
    ] {
        painter.with_clip_rect(col.shrink2(vec2(4.0, 0.0))).text(
            col.left_center() + vec2(4.0, 0.0),
            Align2::LEFT_CENTER,
            value,
            font.clone(),
            color,
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

fn draw_graph(painter: &egui::Painter, rect: Rect, row: &GraphRow, is_merge: bool, is_head: bool) {
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
        painter.circle_filled(center, 4.5, painter.ctx().global_style().visuals.panel_fill);
        painter.circle_stroke(center, 4.0, Stroke::new(2.0, color));
    } else {
        painter.circle_filled(center, 4.5, color);
    }
    if is_head {
        painter.circle_stroke(center, 7.0, Stroke::new(1.5, color));
    }
}

/// Draws a branch/tag badge starting at `pos` (left, vertical center) and returns its right edge.
fn draw_label(painter: &egui::Painter, pos: Pos2, label: &RefLabel, lane: Color32) -> f32 {
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
            ui.label(format_time(c.time, c.offset_minutes));
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

/// Formats Unix seconds as `YYYY-MM-DD HH:MM` in the commit's own time zone, like `git log`.
fn format_time(secs: i64, offset_minutes: i32) -> String {
    let secs = secs + i64::from(offset_minutes) * 60;
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
        self.maybe_screenshot(ui.ctx());
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
        assert_eq!(format_time(0, 0), "1970-01-01 00:00");
        assert_eq!(format_time(1_700_000_000, 0), "2023-11-14 22:13");
        assert_eq!(format_time(1_700_000_000, -180), "2023-11-14 19:13");
    }
}
