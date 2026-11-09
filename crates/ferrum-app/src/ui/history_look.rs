//! How commit rows look: ref badges, short relative dates, the "has a
//! description" mark and the preview card shown when hovering a commit.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, pos2, vec2};
use eframe::epaint::text::{LayoutJob, TextFormat};
use ferrum_core::{Oid, RefKind};

use crate::icons;
use crate::tab::RepoTab;
use crate::util::{format_time, now_secs};

const BADGE_FONT: f32 = 11.5;
const BADGE_HEIGHT: f32 = 18.0;

/// What a ref badge shows.
pub struct Badge<'a> {
    pub text: &'a str,
    pub kind: RefKind,
    pub is_head: bool,
    /// A local branch drawn together with its remote branch (compact labels).
    pub tracked: bool,
}

/// Colors of a badge: (fill, outline, icon, text).
fn badge_colors(
    v: &egui::Visuals,
    b: &Badge,
    lane: Color32,
    accent: Color32,
) -> (Color32, Color32, Color32, Color32) {
    let text = v.text_color();
    if b.is_head {
        let on = crate::theme::on_accent();
        return (accent, accent, on, on);
    }
    match b.kind {
        RefKind::Local => (
            lane.gamma_multiply(if v.dark_mode { 0.20 } else { 0.14 }),
            lane.gamma_multiply(0.65),
            lane,
            text,
        ),
        RefKind::Remote => (
            Color32::TRANSPARENT,
            v.widgets.noninteractive.bg_stroke.color,
            v.weak_text_color(),
            v.weak_text_color(),
        ),
        RefKind::Tag => {
            let gold = tag_color(v.dark_mode);
            (
                gold.gamma_multiply(if v.dark_mode { 0.18 } else { 0.16 }),
                gold.gamma_multiply(0.7),
                gold,
                text,
            )
        }
    }
}

fn tag_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0xE3, 0xB3, 0x41)
    } else {
        Color32::from_rgb(0xA8, 0x74, 0x00)
    }
}

/// Draws a ref badge starting at `pos` (left, vertical center) and returns
/// its rectangle. Local branches take the color of their graph lane; the
/// checked-out branch is filled with the accent; remote branches are outlined
/// and tags are gold.
pub fn paint_badge(
    ui: &egui::Ui,
    painter: &egui::Painter,
    pos: Pos2,
    b: &Badge,
    lane: Color32,
    dimmed: bool,
) -> Rect {
    let v = ui.visuals();
    let accent = crate::theme::accent(ui);
    let (mut fill, mut stroke, mut icon_c, mut text_c) = badge_colors(v, b, lane, accent);
    if dimmed {
        for c in [&mut fill, &mut stroke, &mut icon_c, &mut text_c] {
            *c = c.gamma_multiply(0.45);
        }
    }
    let icon = if b.is_head {
        icons::CHECK
    } else {
        match b.kind {
            RefKind::Local => icons::BRANCH,
            RefKind::Remote => icons::REMOTE,
            RefKind::Tag => icons::TAG,
        }
    };
    let font = FontId::proportional(BADGE_FONT);
    let strong = if b.is_head {
        crate::theme::strong_font(BADGE_FONT)
    } else {
        font.clone()
    };
    let mut job = LayoutJob::default();
    job.append(icon, 0.0, TextFormat::simple(font.clone(), icon_c));
    job.append(b.text, 4.0, TextFormat::simple(strong, text_c));
    if b.tracked {
        job.append(icons::REMOTE, 5.0, TextFormat::simple(font, icon_c));
    }
    let galley = painter.layout_job(job);
    let size = vec2(galley.size().x + 12.0, BADGE_HEIGHT);
    let rect = Rect::from_min_size(pos2(pos.x, pos.y - size.y / 2.0), size);
    painter.rect(
        rect,
        5.0,
        fill,
        Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        pos2(rect.left() + 6.0, rect.center().y - galley.size().y / 2.0),
        galley,
        text_c,
    );
    rect
}

/// A short relative date for the commit list: "5 min ago", "3 h ago",
/// "2 weeks ago", "1 year ago".
pub fn short_relative(secs: i64, now: i64) -> String {
    const DAY: i64 = 86_400;
    let ago = (now - secs).max(0);
    match ago {
        0..60 => t!("just now"),
        60..3600 => t!("{} min ago", ago / 60),
        3600..DAY => t!("{} h ago", ago / 3600),
        DAY..172_800 => t!("yesterday"),
        172_800..1_209_600 => t!("{} days ago", ago / DAY),
        1_209_600..5_184_000 => t!("{} weeks ago", ago / (7 * DAY)),
        5_184_000..31_536_000 => t!("{} months ago", ago / (30 * DAY)),
        31_536_000..63_072_000 => t!("1 year ago"),
        _ => t!("{} years ago", ago / (365 * DAY)),
    }
}

/// The date shown in the commit list's date column.
pub fn list_date(secs: i64, offset_minutes: i32, relative: bool) -> String {
    if relative {
        short_relative(secs, now_secs())
    } else {
        crate::util::format_date(secs, offset_minutes)
    }
}

/// The discreet mark after a summary when the message has more lines.
pub fn paint_body_mark(painter: &egui::Painter, pos: Pos2, color: Color32) -> Rect {
    painter.text(
        pos,
        Align2::LEFT_CENTER,
        icons::DOTS_THREE,
        FontId::proportional(13.0),
        color.gamma_multiply(0.7),
    )
}

/// The full message and number of changed files of a commit, for the card.
#[derive(Clone)]
pub struct Preview {
    pub message: String,
    pub files: Option<usize>,
}

fn preview_of(tab: &mut RepoTab, id: Oid) -> Preview {
    let cache = &mut tab.history.ext.previews;
    if cache.len() > 500 {
        cache.clear();
    }
    cache
        .entry(id)
        .or_insert_with(|| Preview {
            message: tab
                .repo
                .commit_details(id)
                .map(|d| d.message)
                .unwrap_or_default(),
            files: tab.repo.commit_files(id).ok().map(|f| f.len()),
        })
        .clone()
}

/// The card shown when hovering a commit: summary and description, author,
/// date, refs and how many files it changed.
pub fn preview_card(ui: &mut egui::Ui, tab: &mut RepoTab, idx: usize, network_avatars: bool) {
    let c = tab.commits[idx].clone();
    let p = preview_of(tab, c.id);
    let lane_index = tab.graph.rows.get(idx).map_or(0, |r| r.color);
    ui.set_max_width(440.0);
    ui.spacing_mut().item_spacing.y = 6.0;
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(30.0, 30.0), egui::Sense::hover());
        crate::ui::avatars::paint(ui, ui.painter(), rect, &c.author, &c.email, network_avatars);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(crate::theme::strong(&c.author));
            ui.weak(format!(
                "{}  ·  {}",
                short_relative(c.time, now_secs()),
                format_time(c.time, c.offset_minutes)
            ));
        });
    });
    let message = p.message.trim();
    let (summary, body) = message.split_once('\n').unwrap_or((message, ""));
    ui.add(egui::Label::new(crate::theme::strong(summary.trim())).wrap());
    let body = body.trim();
    if !body.is_empty() {
        const MAX_LINES: usize = 12;
        let body = reflow(body);
        let lines: Vec<&str> = body.lines().collect();
        const MAX_CHARS: usize = 700;
        let mut shown = lines[..lines.len().min(MAX_LINES)].join("\n");
        let cut = shown.chars().count() > MAX_CHARS;
        if cut {
            shown = shown.chars().take(MAX_CHARS).collect();
        }
        if cut || lines.len() > MAX_LINES {
            shown = shown.trim_end().to_owned() + " …";
        }
        ui.add(
            egui::Label::new(egui::RichText::new(shown).color(ui.visuals().weak_text_color()))
                .wrap(),
        );
    }
    if let Some(labels) = tab.labels.get(&c.id).filter(|l| !l.is_empty()) {
        ui.horizontal_wrapped(|ui| {
            for l in labels {
                let b = Badge {
                    text: &l.name,
                    kind: l.kind,
                    is_head: l.is_head,
                    tracked: false,
                };
                let galley_w = ui
                    .painter()
                    .layout_no_wrap(
                        l.name.clone(),
                        FontId::proportional(BADGE_FONT),
                        Color32::WHITE,
                    )
                    .size()
                    .x;
                let (rect, _) = ui
                    .allocate_exact_size(vec2(galley_w + 32.0, BADGE_HEIGHT), egui::Sense::hover());
                let lane = crate::ui::history::lane_color(lane_index);
                paint_badge(ui, ui.painter(), rect.left_center(), &b, lane, false);
            }
        });
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.monospace(c.short_id());
        if let Some(n) = p.files {
            ui.weak("·");
            ui.weak(if n == 1 {
                t!("1 file changed")
            } else {
                t!("{} files changed", n)
            });
        }
        if c.parents.len() > 1 {
            ui.weak("·");
            ui.weak(t!("merge"));
        }
    });
}

/// Joins the hard-wrapped lines of a commit message into paragraphs, so the
/// card wraps them at its own width. List items and blank lines are kept.
pub fn reflow(text: &str) -> String {
    let is_item = |l: &str| {
        let t = l.trim_start();
        t.starts_with("- ")
            || t.starts_with("* ")
            || t.starts_with("+ ")
            || t.split_once(". ")
                .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    };
    let mut out = String::new();
    let mut prev_blank = true;
    for line in text.lines() {
        let blank = line.trim().is_empty();
        if blank {
            out.push_str("\n\n");
        } else if prev_blank || is_item(line) {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(line.trim_end());
        } else {
            out.push(' ');
            out.push_str(line.trim());
        }
        prev_blank = blank;
    }
    // Several blank lines read as one paragraph break.
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflows_messages() {
        let text = "First line\nwrapped here.\n\n- item one\n  continues\n- item two\n\n\nEnd";
        assert_eq!(
            reflow(text),
            "First line wrapped here.\n\n- item one continues\n- item two\n\nEnd"
        );
    }

    #[test]
    fn short_relative_dates() {
        let now = 1_700_000_000;
        assert_eq!(short_relative(now - 10, now), "just now");
        assert_eq!(short_relative(now - 300, now), "5 min ago");
        assert_eq!(short_relative(now - 2 * 3600, now), "2 h ago");
        assert_eq!(short_relative(now - 100_000, now), "yesterday");
        assert_eq!(short_relative(now - 3 * 86_400, now), "3 days ago");
        assert_eq!(short_relative(now - 15 * 86_400, now), "2 weeks ago");
        assert_eq!(short_relative(now - 90 * 86_400, now), "3 months ago");
        assert_eq!(short_relative(now - 400 * 86_400, now), "1 year ago");
        assert_eq!(short_relative(now - 800 * 86_400, now), "2 years ago");
    }
}
