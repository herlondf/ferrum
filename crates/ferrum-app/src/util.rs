//! Small helpers shared by the views.

use eframe::egui::{self, Color32};
use ferrum_core::{ChangeKind, RepoState};

use crate::prefs::DateFormat;

/// Formats Unix seconds as `YYYY-MM-DD HH:MM` in the given time zone, like `git log`.
pub fn format_time(secs: i64, offset_minutes: i32) -> String {
    let (y, m, d, h, min) = civil(secs, offset_minutes);
    format!("{y:04}-{m:02}-{d:02} {h:02}:{min:02}")
}

/// Formats a date in the format chosen in Settings > Appearance.
/// Use this for every date shown to the user.
pub fn format_date(secs: i64, offset_minutes: i32) -> String {
    match crate::prefs::date_format() {
        DateFormat::Absolute => format_time(secs, offset_minutes),
        DateFormat::Relative => relative_time(secs, now_secs()),
        DateFormat::Long => long_date(secs, offset_minutes, true),
    }
}

/// Like `format_date`, without the time of day (for narrow columns).
pub fn format_day(secs: i64, offset_minutes: i32) -> String {
    match crate::prefs::date_format() {
        DateFormat::Absolute => format_time(secs, offset_minutes)[..10].to_owned(),
        DateFormat::Relative => relative_time(secs, now_secs()),
        DateFormat::Long => long_date(secs, offset_minutes, false),
    }
}

/// `5 March 2024, 14:30`, with the month name translated.
pub fn long_date(secs: i64, offset_minutes: i32, with_time: bool) -> String {
    let (y, m, d, h, min) = civil(secs, offset_minutes);
    let month = match m {
        1 => t!("January"),
        2 => t!("February"),
        3 => t!("March"),
        4 => t!("April"),
        5 => t!("May"),
        6 => t!("June"),
        7 => t!("July"),
        8 => t!("August"),
        9 => t!("September"),
        10 => t!("October"),
        11 => t!("November"),
        _ => t!("December"),
    };
    if with_time {
        t!("{} {} {}, {}", d, month, y, format!("{h:02}:{min:02}"))
    } else {
        t!("{} {} {}", d, month, y)
    }
}

/// "5 minutes ago", "yesterday", or a date for anything older than a month.
pub fn relative_time(secs: i64, now: i64) -> String {
    let ago = (now - secs).max(0);
    match ago {
        0..60 => t!("just now"),
        60..120 => t!("1 minute ago"),
        120..3600 => t!("{} minutes ago", ago / 60),
        3600..7200 => t!("1 hour ago"),
        7200..86_400 => t!("{} hours ago", ago / 3600),
        86_400..172_800 => t!("yesterday"),
        172_800..2_592_000 => t!("{} days ago", ago / 86_400),
        _ => format_time(secs, 0)[..10].to_owned(),
    }
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Year, month, day, hour and minute of Unix seconds in a time zone.
fn civil(secs: i64, offset_minutes: i32) -> (i64, i64, i64, i64, i64) {
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
    (y, m, d, rem / 3600, rem % 3600 / 60)
}

/// Monospace font for code at `size` points, scaled by the code font size
/// chosen in Settings > Appearance (the family is set there too).
pub fn code_font(size: f32) -> egui::FontId {
    egui::FontId::monospace(size * crate::prefs::code_scale())
}

/// Height of a code row designed for the default size, grown for larger fonts.
pub fn code_row_height(base: f32) -> f32 {
    base * crate::prefs::code_scale().max(1.0)
}

/// Replaces tabs with spaces, as many as the tab width in Settings.
pub fn expand_tabs(text: &str) -> String {
    if text.contains('\t') {
        text.replace('\t', &" ".repeat(crate::prefs::tab_width()))
    } else {
        text.to_owned()
    }
}

/// What the repository is in the middle of, e.g. "Merge in progress".
/// (The core's `RepoState::label` stays English.)
pub fn state_label(s: RepoState) -> String {
    match s {
        RepoState::Clean => String::new(),
        RepoState::Merge => t!("Merge in progress"),
        RepoState::Rebase => t!("Rebase in progress"),
        RepoState::CherryPick => t!("Cherry-pick in progress"),
        RepoState::Revert => t!("Revert in progress"),
        RepoState::Bisect => t!("Bisect in progress"),
        RepoState::Other => t!("Operation in progress"),
    }
}

/// The operation's name for use inside sentences, e.g. "merge".
pub fn state_name(s: RepoState) -> String {
    match s {
        RepoState::Merge => t!("merge"),
        RepoState::Rebase => t!("rebase"),
        RepoState::CherryPick => t!("cherry-pick"),
        RepoState::Revert => t!("revert"),
        RepoState::Bisect => t!("bisect"),
        _ => t!("operation"),
    }
}

pub fn added_color(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(0x4c, 0xc3, 0x8a)
    } else {
        Color32::from_rgb(0x1a, 0x7f, 0x37)
    }
}

pub fn removed_color(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(0xe8, 0x5d, 0x75)
    } else {
        Color32::from_rgb(0xc0, 0x26, 0x3d)
    }
}

pub fn modified_color(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(0xf5, 0xa6, 0x23)
    } else {
        Color32::from_rgb(0xb0, 0x6a, 0x00)
    }
}

pub fn change_color(ui: &egui::Ui, kind: ChangeKind) -> Color32 {
    match kind {
        ChangeKind::Added | ChangeKind::Untracked | ChangeKind::Copied => added_color(ui),
        ChangeKind::Deleted | ChangeKind::Conflicted => removed_color(ui),
        _ => modified_color(ui),
    }
}

/// A colored status letter (M, A, D...) followed by the path.
pub fn change_label(
    ui: &egui::Ui,
    kind: ChangeKind,
    path: &str,
    old_path: Option<&str>,
) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let mut job = LayoutJob::default();
    let mono = egui::FontId::monospace(12.0);
    job.append(
        kind.letter(),
        0.0,
        TextFormat {
            font_id: mono,
            color: change_color(ui, kind),
            ..Default::default()
        },
    );
    let body = egui::FontId::proportional(13.0);
    let (dir, file) = match path.rsplit_once('/') {
        Some((d, f)) => (Some(d), f),
        None => (None, path),
    };
    job.append(
        file,
        8.0,
        TextFormat {
            font_id: body.clone(),
            color: ui.visuals().text_color(),
            ..Default::default()
        },
    );
    let mut extra = String::new();
    if let Some(d) = dir {
        extra.push_str(d);
    }
    if let Some(old) = old_path {
        extra.push_str(&format!(" ⬅ {old}"));
    }
    if !extra.is_empty() {
        job.append(
            &extra,
            6.0,
            TextFormat {
                font_id: body,
                color: ui.visuals().weak_text_color(),
                ..Default::default()
            },
        );
    }
    job.wrap = egui::text::TextWrapping::truncate_at_width(f32::INFINITY);
    job
}

pub fn is_image_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp", ".ico", ".tga", ".svg", ".svgz", ".pdf",
    ]
    .iter()
    .any(|e| lower.ends_with(e))
}

/// Checks that a branch or tag name is acceptable to git.
pub fn valid_ref_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty()
        && !n.starts_with('-')
        && !n.starts_with('/')
        && !n.ends_with('/')
        && !n.ends_with('.')
        && !n.ends_with(".lock")
        && !n.contains("..")
        && !n.contains("@{")
        && !n.contains("//")
        && !n
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_unix_time() {
        assert_eq!(format_time(0, 0), "1970-01-01 00:00");
        assert_eq!(format_time(1_700_000_000, 0), "2023-11-14 22:13");
        assert_eq!(format_time(1_700_000_000, -180), "2023-11-14 19:13");
        assert_eq!(long_date(1_700_000_000, 0, true), "14 November 2023, 22:13");
        assert_eq!(long_date(0, 0, false), "1 January 1970");
    }

    #[test]
    fn formats_relative_time() {
        let now = 1_700_000_000;
        assert_eq!(relative_time(now - 5, now), "just now");
        assert_eq!(relative_time(now - 60, now), "1 minute ago");
        assert_eq!(relative_time(now - 3 * 3600, now), "3 hours ago");
        assert_eq!(relative_time(now - 90_000, now), "yesterday");
        assert_eq!(relative_time(now - 5 * 86_400, now), "5 days ago");
        assert_eq!(relative_time(now - 60 * 86_400, now), "2023-09-15");
        assert_eq!(relative_time(now + 30, now), "just now");
    }

    #[test]
    fn checks_ref_names() {
        assert!(valid_ref_name("feature/login-2"));
        for bad in ["", "a b", "a..b", "-x", "x/", "x.lock", "a:b", "a~1", "x."] {
            assert!(!valid_ref_name(bad), "{bad}");
        }
    }
}
