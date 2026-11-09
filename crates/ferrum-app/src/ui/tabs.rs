//! The tab bar (a strip of tabs on top, or a list on the left) and the tab
//! menu. The active tab joins the view below it; × shows on hover (always on
//! the active tab); tabs can be dragged to a new place.

use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, pos2, vec2};

use crate::command::{Command, Commands};
use crate::external::Launch;
use crate::icons;
use crate::repos::{Library, RepoCmd, TAB_COLORS};
use crate::tab::RepoTab;
use crate::theme;
use crate::ui::toolbar::chrome_fill;

/// What the tab bar shows.
pub struct Bar<'a> {
    pub tabs: &'a [RepoTab],
    /// The selected repository tab (`None` while the Repository Manager tab is).
    pub active: Option<usize>,
    /// The Repository Manager tab is open (Ctrl+T).
    pub home: bool,
    /// Tabs shown in windows of their own.
    pub in_window: &'a [std::path::PathBuf],
}

/// Labels of all tabs, with same-named tabs told apart.
pub fn labels(tabs: &[RepoTab], lib: &Library) -> Vec<String> {
    let items: Vec<(String, std::path::PathBuf)> = tabs
        .iter()
        .map(|t| (crate::repos::base_label(lib, t), t.path().to_path_buf()))
        .collect();
    crate::repos::unique_labels(&items)
}

pub fn color32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// What one tab shows.
struct Item {
    label: String,
    branch: Option<String>,
    color: Option<Color32>,
    dirty: bool,
    busy: bool,
    in_window: bool,
    selected: bool,
}

impl Item {
    fn new(bar: &Bar, lib: &Library, i: usize, label: &str) -> Self {
        let tab = &bar.tabs[i];
        Self {
            label: label.to_owned(),
            branch: tab.head.clone(),
            color: lib.color(tab.path()).map(color32),
            dirty: tab.status.total() > 0,
            busy: tab.busy(),
            in_window: bar.in_window.iter().any(|p| p == tab.path()),
            selected: bar.active == Some(i),
        }
    }

    fn tip(&self, tab: &RepoTab) -> String {
        let mut tip = tab.path().display().to_string();
        if let Some(b) = &self.branch {
            tip.push_str(&format!("\n{} {b}", icons::BRANCH));
        }
        if self.dirty {
            let files = t!("{} changed files", tab.status.total());
            tip.push_str(&format!("\n{}: {files}", t!("Uncommitted changes")));
        }
        tip
    }
}

/// Shortens `s` to `max` characters with an ellipsis.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    } else {
        s.to_owned()
    }
}

fn tab_height() -> f32 {
    match theme::density() {
        theme::Density::Compact => 28.0,
        theme::Density::Comfortable => 32.0,
    }
}

const CLOSE_SIZE: f32 = 18.0;
const NAME_SIZE: f32 = 13.0;
const BRANCH_SIZE: f32 = 11.5;

/// What happened to a tab this frame.
struct Clicks {
    resp: egui::Response,
    close: bool,
}

/// The × of a tab, drawn when `shown`; returns whether it was clicked.
fn close_button(ui: &mut egui::Ui, id: egui::Id, center: egui::Pos2, shown: bool) -> bool {
    let rect = Rect::from_center_size(center, vec2(CLOSE_SIZE, CLOSE_SIZE));
    let resp = ui.interact(
        rect,
        id,
        if shown {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if !shown {
        return false;
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, icons::CLOSE));
    let v = ui.visuals();
    let mut color = v.weak_text_color();
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, theme::RADIUS / 2, v.widgets.hovered.weak_bg_fill);
        color = v.strong_text_color();
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icons::CLOSE,
        FontId::proportional(12.0),
        color,
    );
    resp.on_hover_text(t!("Close (Ctrl+W)")).clicked()
}

/// A dot for uncommitted changes.
fn dirty_dot(ui: &egui::Ui, center: egui::Pos2) {
    ui.painter()
        .circle_filled(center, 3.5, ui.visuals().warn_fg_color);
}

/// One tab of the strip.
fn strip_tab(ui: &mut egui::Ui, item: &Item, id: egui::Id) -> Clicks {
    let v = ui.visuals().clone();
    let h = tab_height();
    let painter = ui.painter().clone();
    let name_font = theme::strong_font(NAME_SIZE);
    let text_color = if item.selected {
        v.strong_text_color()
    } else {
        v.widgets.inactive.fg_stroke.color
    };
    let name = painter.layout_no_wrap(clip(&item.label, 32), name_font, text_color);
    let branch = item.branch.as_ref().map(|b| {
        painter.layout_no_wrap(
            icons::text(icons::BRANCH, clip(b, 24)),
            FontId::proportional(BRANCH_SIZE),
            v.weak_text_color(),
        )
    });
    // Widths: padding, [spinner], name, [branch], [dot], [window], ×.
    let mut w = 12.0 + name.size().x;
    if item.busy {
        w += 18.0;
    }
    if let Some(b) = &branch {
        w += 8.0 + b.size().x;
    }
    if item.dirty {
        w += 12.0;
    }
    if item.in_window {
        w += 18.0;
    }
    w += 6.0 + CLOSE_SIZE + 6.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w.max(90.0), h), Sense::click_and_drag());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Tab, true, item.selected, &item.label)
    });
    let hovered = ui.rect_contains_pointer(rect);
    let top = CornerRadius {
        nw: theme::RADIUS + 1,
        ne: theme::RADIUS + 1,
        sw: 0,
        se: 0,
    };
    if item.selected {
        // Outline, then the view's color inside: the tab opens into the view.
        let line = v.widgets.noninteractive.bg_stroke.color;
        painter.rect_filled(rect, top, line);
        let inner = Rect::from_min_max(
            pos2(rect.left() + 1.0, rect.top() + 1.0),
            pos2(rect.right() - 1.0, rect.bottom() + 1.0),
        );
        painter.rect_filled(inner, top, v.panel_fill);
    } else if hovered {
        let fill = chrome_fill(&v).lerp_to_gamma(v.panel_fill, 0.6);
        painter.rect_filled(rect.shrink2(vec2(2.0, 3.0)), theme::RADIUS, fill);
    }
    if let Some(c) = item.color {
        let inset = if item.selected { 1.0 } else { 8.0 };
        let stripe = Rect::from_min_max(
            pos2(
                rect.left() + inset,
                rect.top() + if item.selected { 1.0 } else { 3.0 },
            ),
            pos2(
                rect.right() - inset,
                rect.top() + if item.selected { 3.5 } else { 5.0 },
            ),
        );
        let r = if item.selected {
            top
        } else {
            CornerRadius::same(2)
        };
        painter.rect_filled(stripe, r, c);
    }
    let cy = rect.center().y + 0.5;
    let mut x = rect.left() + 12.0;
    if item.busy {
        let r = Rect::from_center_size(pos2(x + 6.0, cy), vec2(12.0, 12.0));
        egui::Spinner::new()
            .size(12.0)
            .color(theme::accent(ui))
            .paint_at(ui, r);
        x += 18.0;
    }
    let size = name.size();
    painter.galley(pos2(x, cy - size.y / 2.0), name, text_color);
    x += size.x;
    if let Some(b) = branch {
        x += 8.0;
        let size = b.size();
        painter.galley(pos2(x, cy - size.y / 2.0), b, v.weak_text_color());
        x += size.x;
    }
    if item.dirty {
        dirty_dot(ui, pos2(x + 8.0, cy));
        x += 12.0;
    }
    if item.in_window {
        painter.text(
            pos2(x + 10.0, cy),
            Align2::CENTER_CENTER,
            icons::ARROW_SQUARE_OUT,
            FontId::proportional(12.0),
            v.weak_text_color(),
        );
    }
    let close_center = pos2(rect.right() - 6.0 - CLOSE_SIZE / 2.0, cy);
    let close = close_button(ui, id.with("close"), close_center, item.selected || hovered);
    Clicks { resp, close }
}

/// One row of the vertical list: name, branch below, color on the left.
fn list_row(ui: &mut egui::Ui, item: &Item, id: egui::Id) -> Clicks {
    let v = ui.visuals().clone();
    let w = ui.available_width();
    let two_lines = item.branch.is_some();
    let h = if two_lines { 40.0 } else { 28.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Tab, true, item.selected, &item.label)
    });
    let painter = ui.painter().clone();
    let hovered = ui.rect_contains_pointer(rect);
    let body = rect.shrink2(vec2(4.0, 1.0));
    if item.selected {
        painter.rect_filled(body, theme::RADIUS, v.selection.bg_fill);
    } else if hovered {
        painter.rect_filled(body, theme::RADIUS, v.widgets.hovered.weak_bg_fill);
    }
    if let Some(c) = item.color {
        let bar = Rect::from_min_max(
            pos2(body.left(), body.top() + 6.0),
            pos2(body.left() + 3.0, body.bottom() - 6.0),
        );
        painter.rect_filled(bar, 2, c);
    }
    let text_color = if item.selected {
        v.selection.stroke.color
    } else {
        v.widgets.inactive.fg_stroke.color
    };
    let mut x = body.left() + 12.0;
    let right = body.right() - 6.0 - CLOSE_SIZE - 4.0 - if item.dirty { 12.0 } else { 0.0 };
    let name_y = if two_lines {
        body.top() + 7.0
    } else {
        body.center().y - 8.0
    };
    if item.busy {
        let r = Rect::from_center_size(pos2(x + 6.0, name_y + 8.0), vec2(12.0, 12.0));
        egui::Spinner::new()
            .size(12.0)
            .color(theme::accent(ui))
            .paint_at(ui, r);
        x += 18.0;
    }
    let mut name = item.label.clone();
    if item.in_window {
        name = format!("{name} {}", icons::ARROW_SQUARE_OUT);
    }
    let font = if item.selected {
        theme::strong_font(NAME_SIZE)
    } else {
        FontId::proportional(NAME_SIZE)
    };
    let max = (right - x).max(10.0);
    let galley = |text: String, font: FontId, color: Color32| {
        let mut job =
            egui::text::LayoutJob::single_section(text, egui::TextFormat::simple(font, color));
        job.wrap = egui::text::TextWrapping::truncate_at_width(max);
        ui.painter().layout_job(job)
    };
    painter.galley(pos2(x, name_y), galley(name, font, text_color), text_color);
    if let Some(b) = &item.branch {
        let weak = if item.selected {
            text_color.gamma_multiply(0.75)
        } else {
            v.weak_text_color()
        };
        let text = icons::text(icons::BRANCH, b);
        painter.galley(
            pos2(x, name_y + 17.0),
            galley(text, FontId::proportional(BRANCH_SIZE), weak),
            weak,
        );
    }
    let cy = body.center().y;
    if item.dirty {
        dirty_dot(ui, pos2(body.right() - 6.0 - CLOSE_SIZE - 8.0, cy));
    }
    let close_center = pos2(body.right() - 4.0 - CLOSE_SIZE / 2.0, cy);
    let close = close_button(ui, id.with("close"), close_center, item.selected || hovered);
    Clicks { resp, close }
}

/// Where a dragged tab lands: the index among the others before the pointer.
fn drop_index(rects: &[Rect], from: usize, pointer: egui::Pos2, vertical: bool) -> usize {
    rects
        .iter()
        .enumerate()
        .filter(|(j, r)| {
            *j != from
                && if vertical {
                    r.center().y < pointer.y
                } else {
                    r.center().x < pointer.x
                }
        })
        .count()
}

/// The tabs, then the Repository Manager tab. Returns the active tab's place.
fn tabs(
    ui: &mut egui::Ui,
    bar: &Bar,
    lib: &mut Library,
    cmds: &mut Commands,
    vertical: bool,
) -> Option<Rect> {
    let mut selected = None;
    let names = labels(bar.tabs, lib);
    let mut rects = Vec::new();
    let mut dragging = None;
    let last_active = egui::Id::unique("tab-bar-active").with(vertical);
    let scroll_to = ui.data(|d| d.get_temp::<Option<usize>>(last_active)) != Some(bar.active);
    for (i, tab) in bar.tabs.iter().enumerate() {
        let item = Item::new(bar, lib, i, &names[i]);
        let id = ui.scope_id().with(("tab", tab.path()));
        let clicks = if vertical {
            list_row(ui, &item, id)
        } else {
            strip_tab(ui, &item, id)
        };
        let resp = clicks.resp;
        rects.push(resp.rect);
        if item.selected {
            selected = Some(resp.rect);
        }
        if item.selected && scroll_to {
            resp.scroll_to_me(None);
        }
        if resp.dragged() || resp.drag_stopped() {
            dragging = Some((i, resp.drag_stopped()));
        }
        let resp = resp.on_hover_text(item.tip(tab));
        if clicks.close || resp.middle_clicked() {
            cmds.push(Command::CloseTab(i));
        } else if resp.clicked() {
            cmds.push(Command::SelectTab(i));
        }
        resp.context_menu(|ui| menu(ui, i, tab, &names[i], lib, cmds));
    }
    ui.data_mut(|d| d.insert_temp(last_active, bar.active));
    if let Some((from, stopped)) = dragging
        && let Some(p) = ui.ctx().pointer_latest_pos()
    {
        let to = drop_index(&rects, from, p, vertical);
        if stopped {
            if to != from {
                cmds.push(Command::Repos(RepoCmd::MoveTab(from, to)));
            }
        } else {
            // A line where the tab will go.
            let others: Vec<Rect> = rects
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != from)
                .map(|(_, r)| *r)
                .collect();
            let stroke = Stroke::new(2.0, theme::accent(ui));
            if let (Some(first), Some(last)) = (others.first(), others.last()) {
                if vertical {
                    let y = others.get(to).map_or(last.bottom(), |r| r.top());
                    ui.painter().hline(first.x_range(), y, stroke);
                } else {
                    let x = others.get(to).map_or(last.right(), |r| r.left());
                    ui.painter().vline(x, first.y_range().shrink(4.0), stroke);
                }
            }
        }
    }
    if bar.home {
        let item = Item {
            label: t!("📁 Repositories"),
            branch: None,
            color: None,
            dirty: false,
            busy: false,
            in_window: false,
            selected: bar.active.is_none(),
        };
        let id = ui.scope_id().with("home-tab");
        let clicks = if vertical {
            list_row(ui, &item, id)
        } else {
            strip_tab(ui, &item, id)
        };
        if item.selected {
            selected = Some(clicks.resp.rect);
            if scroll_to {
                clicks.resp.scroll_to_me(None);
            }
        }
        if clicks.close || clicks.resp.middle_clicked() {
            cmds.push(Command::Repos(RepoCmd::CloseHome));
        } else if clicks.resp.clicked() {
            cmds.push(Command::Repos(RepoCmd::NewTab));
        }
    }
    selected
}

/// Draws the tabs in a strip, attached to the view below.
pub fn row(ui: &mut egui::Ui, bar: &Bar, lib: &mut Library, cmds: &mut Commands) {
    let h = tab_height();
    let r = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let plus = if bar.home { 0.0 } else { 34.0 };
        let out = egui::ScrollArea::horizontal()
            .id_salt("tab-strip")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .max_width((ui.available_width() - plus).max(60.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.set_height(h);
                    tabs(ui, bar, lib, cmds, false)
                })
                .inner
            });
        let clip = out.inner_rect;
        let selected = out
            .inner
            .map(|r| r.intersect(clip))
            .filter(|r| r.width() > 0.0);
        if !bar.home {
            ui.add_space(4.0);
            add_button(ui, cmds);
        }
        selected
    });
    // The line under the strip, open under the active tab.
    let strip = r.response.rect;
    let y = strip.bottom() - 0.5;
    let width = ui.ctx().content_rect().x_range();
    let stroke = Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
    match r.inner {
        Some(sel) => {
            ui.painter().hline(width.min..=sel.left() + 0.5, y, stroke);
            ui.painter().hline(sel.right() - 0.5..=width.max, y, stroke);
        }
        None => {
            ui.painter().hline(width, y, stroke);
        }
    }
}

/// The "+" after the tabs: opens the Repository Manager tab.
fn add_button(ui: &mut egui::Ui, cmds: &mut Commands) {
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, icons::ADD));
    let v = ui.visuals();
    let mut color = v.weak_text_color();
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, v.widgets.hovered.weak_bg_fill);
        color = v.strong_text_color();
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icons::ADD,
        FontId::proportional(14.0),
        color,
    );
    if resp.on_hover_text(t!("New tab (Ctrl+T)")).clicked() {
        cmds.push(Command::Repos(RepoCmd::NewTab));
    }
}

/// Draws the tabs as a list (Settings: vertical tabs).
pub fn list(ui: &mut egui::Ui, bar: &Bar, lib: &mut Library, cmds: &mut Commands) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        tabs(ui, bar, lib, cmds, true);
        if !bar.home {
            ui.add_space(theme::SPACE_S);
            if ui
                .add(
                    egui::Button::new(icons::text(icons::ADD, t!("New tab")))
                        .frame_when_inactive(false),
                )
                .on_hover_text(t!("New tab (Ctrl+T)"))
                .clicked()
            {
                cmds.push(Command::Repos(RepoCmd::NewTab));
            }
        }
    });
}

fn menu(
    ui: &mut egui::Ui,
    i: usize,
    tab: &RepoTab,
    label: &str,
    lib: &mut Library,
    cmds: &mut Commands,
) {
    let path = tab.path().to_path_buf();
    let mut act = |ui: &mut egui::Ui, text: String, cmd: Command| {
        if ui.button(text).clicked() {
            cmds.push(cmd);
            ui.close();
        }
    };
    act(ui, t!("Close"), Command::CloseTab(i));
    act(
        ui,
        t!("Close Others"),
        Command::Repos(RepoCmd::CloseOthers(i)),
    );
    act(ui, t!("Close All"), Command::Repos(RepoCmd::CloseAll));
    ui.separator();
    ui.menu_button(t!("Rename…"), |ui| {
        let entry = lib.entry(&path);
        let resp = ui.add(
            egui::TextEdit::singleline(&mut entry.name)
                .hint_text(label)
                .desired_width(200.0),
        );
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            ui.close();
        }
        if ui.button(t!("Use the folder name")).clicked() {
            entry.name.clear();
            ui.close();
        }
    });
    ui.menu_button(t!("🎨 Color"), |ui| {
        ui.horizontal(|ui| {
            for &c in TAB_COLORS {
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
                ui.painter().rect_filled(rect, 3.0, color32(c));
                if lib.color(&path) == Some(c) {
                    ui.painter().rect_stroke(
                        rect,
                        3.0,
                        egui::Stroke::new(2.0, ui.visuals().strong_text_color()),
                        egui::StrokeKind::Outside,
                    );
                }
                if resp.clicked() {
                    lib.entry(&path).color = Some(c);
                    ui.close();
                }
            }
        });
        if ui.button(t!("No color")).clicked() {
            lib.entry(&path).color = None;
            ui.close();
        }
    });
    ui.separator();
    act(
        ui,
        t!("Open in terminal"),
        Command::External(Launch::Terminal { dir: path.clone() }),
    );
    act(
        ui,
        t!("Show in file manager"),
        Command::External(Launch::Reveal { path: path.clone() }),
    );
    act(
        ui,
        t!("Copy path"),
        Command::Copy(path.display().to_string()),
    );
    act(
        ui,
        t!("Open in new window"),
        Command::Repos(RepoCmd::OpenInWindow(i)),
    );
    act(
        ui,
        t!("Repository settings…"),
        Command::Repos(RepoCmd::Settings),
    );
    ui.separator();
    ui.menu_button(t!("Move to workspace"), |ui| {
        for name in lib.workspace_names() {
            if name != lib.workspace && ui.button(&name).clicked() {
                cmds.push(Command::Repos(RepoCmd::MoveToWorkspace(i, name)));
                ui.close();
            }
        }
        if !lib.workspace.is_empty() && ui.button(t!("Default workspace")).clicked() {
            cmds.push(Command::Repos(RepoCmd::MoveToWorkspace(i, String::new())));
            ui.close();
        }
        let id = ui.scope_id().with("new-ws");
        let mut name: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
        let resp = ui.add(
            egui::TextEdit::singleline(&mut name)
                .hint_text(t!("New workspace"))
                .desired_width(180.0),
        );
        if resp.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && !crate::repos::clean_workspace_name(&name).is_empty()
        {
            cmds.push(Command::Repos(RepoCmd::MoveToWorkspace(i, name.clone())));
            name.clear();
            ui.close();
        }
        ui.data_mut(|d| d.insert_temp(id, name));
    });
    ui.checkbox(&mut lib.vertical_tabs, t!("Vertical tab list"));
}
