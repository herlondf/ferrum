//! How the local changes lists look: file rows with a colored status badge,
//! list headers, the files being dragged and the list they can be dropped on.

use eframe::egui::{self, Color32, Rect, Sense, vec2};
use ferrum_core::ChangeKind;

use crate::icons;
use crate::theme;
use crate::util;

/// The color of a status badge.
pub fn kind_color(ui: &egui::Ui, kind: ChangeKind) -> Color32 {
    let dark = ui.visuals().dark_mode;
    match kind {
        ChangeKind::Untracked => {
            if dark {
                Color32::from_rgb(0x5a, 0xa9, 0xe6)
            } else {
                Color32::from_rgb(0x1f, 0x6f, 0xc5)
            }
        }
        ChangeKind::Renamed | ChangeKind::Copied => {
            if dark {
                Color32::from_rgb(0xb3, 0x8b, 0xf0)
            } else {
                Color32::from_rgb(0x7a, 0x48, 0xc8)
            }
        }
        k => util::change_color(ui, k),
    }
}

/// What a badge letter means, for tooltips.
pub fn kind_name(kind: ChangeKind) -> String {
    match kind {
        ChangeKind::Added => t!("Added"),
        ChangeKind::Deleted => t!("Deleted"),
        ChangeKind::Modified => t!("Modified"),
        ChangeKind::Renamed => t!("Renamed"),
        ChangeKind::Copied => t!("Copied"),
        ChangeKind::Untracked => t!("New file"),
        ChangeKind::Conflicted => t!("Conflicted"),
        ChangeKind::TypeChange => t!("Type changed"),
    }
}

/// Width of a badge.
pub const BADGE_WIDTH: f32 = 18.0;

/// A rounded status badge (M, A, D…) centered on `center`.
pub fn badge(painter: &egui::Painter, ui: &egui::Ui, center: egui::Pos2, kind: ChangeKind) {
    let color = kind_color(ui, kind);
    let rect = Rect::from_center_size(center, vec2(BADGE_WIDTH, 16.0));
    let alpha = if ui.visuals().dark_mode { 46 } else { 34 };
    let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha);
    painter.rect_filled(rect, 4.0, fill);
    painter.text(
        rect.center() + vec2(0.0, 0.5),
        egui::Align2::CENTER_CENTER,
        kind.letter(),
        theme::strong_font(10.5),
        color,
    );
}

/// How a row is drawn.
pub struct RowLook<'a> {
    pub kind: ChangeKind,
    pub path: &'a str,
    pub old_path: Option<&'a str>,
    /// Only the file name (tree view); otherwise name, then folder.
    pub name_only: bool,
    pub selected: bool,
    /// Being dragged: drawn faded.
    pub dimmed: bool,
    /// Room left of the badge (the combined list's checkbox).
    pub lead: f32,
}

/// A file row: badge, file name, folder (weak). Senses clicks and drags.
pub fn row(ui: &mut egui::Ui, look: &RowLook) -> egui::Response {
    let height = theme::row_height();
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
    let (dir, file) = match look.path.rsplit_once('/') {
        Some((d, f)) => (Some(d), f),
        None => (None, look.path),
    };
    let label = if look.name_only {
        file.to_owned()
    } else {
        look.path.to_owned()
    };
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, true, look.selected, &label)
    });
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let v = ui.visuals();
    if look.selected {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, v.selection.bg_fill);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, v.widgets.hovered.weak_bg_fill);
    }
    let fade = |c: Color32| {
        if look.dimmed {
            c.gamma_multiply(0.45)
        } else {
            c
        }
    };
    let (text, weak) = if look.selected {
        let c = v.selection.stroke.color;
        (c, c.gamma_multiply(0.7))
    } else {
        (v.text_color(), v.weak_text_color())
    };
    let painter = ui.painter_at(rect);
    let x = rect.left() + look.lead + theme::SPACE_S;
    badge(
        &painter,
        ui,
        egui::pos2(x + BADGE_WIDTH / 2.0, rect.center().y),
        look.kind,
    );
    let mut job = egui::text::LayoutJob::default();
    let font = egui::FontId::proportional(13.0);
    let fmt = |c: Color32| egui::TextFormat::simple(font.clone(), fade(c));
    job.append(file, 0.0, fmt(text));
    let mut extra = String::new();
    if !look.name_only
        && let Some(d) = dir
    {
        extra.push_str(d);
    }
    if let Some(old) = look.old_path {
        if !extra.is_empty() {
            extra.push_str("  ");
        }
        extra.push_str(&format!("{} {old}", icons::ARROW_LEFT));
    }
    if !extra.is_empty() {
        job.append(&extra, 8.0, fmt(weak));
    }
    let left = x + BADGE_WIDTH + theme::SPACE_M;
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - left - 6.0).max(10.0));
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    painter.galley(
        egui::pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        text,
    );
    resp
}

/// A list title with a count pill. Returns the rect left for buttons.
pub fn header(ui: &mut egui::Ui, title: &str, count: usize, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(theme::row_height());
        ui.label(theme::strong(title));
        count_pill(ui, count);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
    });
}

/// A small rounded number.
pub fn count_pill(ui: &mut egui::Ui, count: usize) {
    let v = ui.visuals();
    let color = v.weak_text_color();
    let galley = ui
        .painter()
        .layout_no_wrap(count.to_string(), theme::strong_font(10.5), color);
    let size = vec2((galley.size().x + 10.0).max(18.0), 16.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let v = ui.visuals();
    ui.painter()
        .rect_filled(rect, 8.0, v.widgets.inactive.weak_bg_fill);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, color);
}

/// A small icon button with a tooltip; `label` is its accessible name.
pub fn tool_button(ui: &mut egui::Ui, enabled: bool, icon: &str, label: &str) -> egui::Response {
    let button = egui::Button::new(egui::RichText::new(icon).size(14.0)).frame_when_inactive(false);
    let resp = ui.add_enabled(enabled, button);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    resp.on_hover_text(label)
}

/// The files being dragged, next to the pointer: the first name (or how
/// many) and where they go.
pub fn drag_preview(
    ctx: &egui::Context,
    first: &str,
    count: usize,
    kind: ChangeKind,
    to_staged: bool,
) {
    let Some(pos) = ctx.pointer_interact_pos() else {
        return;
    };
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    let name = first.rsplit('/').next().unwrap_or(first);
    let text = if count == 1 {
        name.to_owned()
    } else {
        t!("{} files", count)
    };
    egui::Area::new(egui::Id::unique("changes-drag-preview"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos + vec2(16.0, 10.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .corner_radius(theme::RADIUS_LARGE)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if count == 1 {
                            let (r, _) =
                                ui.allocate_exact_size(vec2(BADGE_WIDTH, 16.0), Sense::hover());
                            badge(ui.painter(), ui, r.center(), kind);
                        } else {
                            // A stack of badges.
                            let (r, _) = ui
                                .allocate_exact_size(vec2(BADGE_WIDTH + 6.0, 20.0), Sense::hover());
                            let p = ui.painter();
                            badge(p, ui, r.center() + vec2(3.0, -3.0), kind);
                            badge(p, ui, r.center() + vec2(-3.0, 3.0), kind);
                        }
                        ui.label(theme::strong(text));
                        let (icon, hint) = if to_staged {
                            (icons::ARROW_DOWN, t!("Stage"))
                        } else {
                            (icons::ARROW_UP, t!("Unstage"))
                        };
                        ui.label(
                            egui::RichText::new(icons::text(icon, hint))
                                .color(ui.visuals().weak_text_color()),
                        );
                    });
                });
        });
}

/// A list that accepts the dragged files: a tinted, outlined area with a
/// hint at the bottom.
pub fn drop_highlight(ui: &egui::Ui, rect: Rect, hint: &str, hovered: bool) {
    let accent = theme::accent(ui);
    let alpha = if hovered { 44 } else { 16 };
    let fill = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), alpha);
    let painter = ui.painter();
    painter.rect_filled(rect, theme::RADIUS, fill);
    let stroke = if hovered {
        egui::Stroke::new(1.5, accent)
    } else {
        egui::Stroke::new(1.0, accent.gamma_multiply(0.5))
    };
    painter.rect_stroke(rect, theme::RADIUS, stroke, egui::StrokeKind::Inside);
    if hovered {
        let font = theme::strong_font(12.0);
        let galley = painter.layout_no_wrap(hint.to_owned(), font, theme::on_accent());
        let size = galley.size() + vec2(16.0, 8.0);
        let pill = Rect::from_center_size(
            egui::pos2(rect.center().x, rect.bottom() - size.y / 2.0 - 8.0),
            size,
        );
        painter.rect_filled(pill, theme::RADIUS_LARGE, accent);
        painter.galley(
            pill.center() - galley.size() / 2.0,
            galley,
            theme::on_accent(),
        );
    }
}
