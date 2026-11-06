//! Appearance and behaviour preferences (Settings > Appearance / Git).
//!
//! Views read the few values they need (date format, tab width, code font
//! size, default merge type) through the functions here instead of being
//! handed `Settings`; `apply` publishes them once per frame.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use eframe::egui::{self, Color32};
use ferrum_core::ops::MergeMode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DateFormat {
    /// "5 minutes ago", then the date after a week.
    Relative,
    /// `2024-03-05 14:30`
    #[default]
    Absolute,
    /// `5 March 2024 14:30`
    Long,
}

impl DateFormat {
    pub const ALL: [DateFormat; 3] = [DateFormat::Relative, DateFormat::Absolute, DateFormat::Long];

    pub fn label(self) -> String {
        match self {
            DateFormat::Relative => t!("Relative (5 minutes ago)"),
            DateFormat::Absolute => t!("Absolute (2024-03-05 14:30)"),
            DateFormat::Long => t!("International long (5 March 2024 14:30)"),
        }
    }
}

/// The merge type preselected in the merge dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MergeChoice {
    #[default]
    Default,
    NoFastForward,
    FastForwardOnly,
    Squash,
}

impl MergeChoice {
    pub const ALL: [MergeChoice; 4] = [
        MergeChoice::Default,
        MergeChoice::NoFastForward,
        MergeChoice::FastForwardOnly,
        MergeChoice::Squash,
    ];

    pub fn mode(self) -> MergeMode {
        match self {
            MergeChoice::Default => MergeMode::Default,
            MergeChoice::NoFastForward => MergeMode::NoFastForward,
            MergeChoice::FastForwardOnly => MergeMode::FastForwardOnly,
            MergeChoice::Squash => MergeMode::Squash,
        }
    }

    pub fn label(self) -> String {
        match self {
            MergeChoice::Default => t!("Fast-forward when possible"),
            MergeChoice::NoFastForward => t!("Always create a merge commit"),
            MergeChoice::FastForwardOnly => t!("Fast-forward only"),
            MergeChoice::Squash => t!("Squash"),
        }
    }
}

/// A toolbar button that can be hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolbarButton {
    Fetch,
    Pull,
    Push,
    Branch,
    Stash,
    Pop,
    Undo,
    Terminal,
    FileManager,
}

impl ToolbarButton {
    pub const ALL: [ToolbarButton; 9] = [
        ToolbarButton::Fetch,
        ToolbarButton::Pull,
        ToolbarButton::Push,
        ToolbarButton::Branch,
        ToolbarButton::Stash,
        ToolbarButton::Pop,
        ToolbarButton::Undo,
        ToolbarButton::Terminal,
        ToolbarButton::FileManager,
    ];

    pub fn label(self) -> String {
        match self {
            ToolbarButton::Fetch => "Fetch".into(),
            ToolbarButton::Pull => "Pull".into(),
            ToolbarButton::Push => "Push".into(),
            ToolbarButton::Branch => t!("New branch"),
            ToolbarButton::Stash => t!("Stash changes"),
            ToolbarButton::Pop => t!("Pop the latest stash"),
            ToolbarButton::Undo => t!("Undo"),
            ToolbarButton::Terminal => t!("Open in terminal"),
            ToolbarButton::FileManager => t!("Show in file manager"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub date_format: DateFormat,
    /// Font family for code (diffs, blame); empty = the built-in font.
    pub code_font: String,
    pub code_font_size: f32,
    pub tab_width: u8,
    pub system_accent: bool,
    pub hidden_toolbar: Vec<ToolbarButton>,
    /// Typed spaces in new branch names become this ("" keeps them, which git refuses).
    pub branch_space: String,
    pub merge: MergeChoice,
    /// Git executable; empty = `git` on the PATH.
    pub git_path: String,
    pub verbose_git: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            date_format: DateFormat::Absolute,
            code_font: String::new(),
            code_font_size: DEFAULT_CODE_SIZE,
            tab_width: 4,
            system_accent: false,
            hidden_toolbar: Vec::new(),
            branch_space: "-".into(),
            merge: MergeChoice::Default,
            git_path: String::new(),
            verbose_git: false,
        }
    }
}

pub const DEFAULT_CODE_SIZE: f32 = 12.0;

impl Prefs {
    pub fn shows(&self, b: ToolbarButton) -> bool {
        !self.hidden_toolbar.contains(&b)
    }

    /// A branch name as typed, with spaces replaced.
    pub fn branch_name(&self, typed: &str) -> String {
        if self.branch_space.is_empty() {
            typed.to_owned()
        } else {
            typed.replace(' ', &self.branch_space)
        }
    }
}

static DATE: AtomicU8 = AtomicU8::new(1);
static TAB: AtomicU8 = AtomicU8::new(4);
static CODE_SIZE: AtomicU32 = AtomicU32::new(DEFAULT_CODE_SIZE.to_bits());
static MERGE: AtomicU8 = AtomicU8::new(0);

pub fn date_format() -> DateFormat {
    match DATE.load(Ordering::Relaxed) {
        0 => DateFormat::Relative,
        2 => DateFormat::Long,
        _ => DateFormat::Absolute,
    }
}

pub fn tab_width() -> usize {
    usize::from(TAB.load(Ordering::Relaxed).clamp(1, 16))
}

/// Code font size relative to the default (1.0 = 12 points).
pub fn code_scale() -> f32 {
    f32::from_bits(CODE_SIZE.load(Ordering::Relaxed)) / DEFAULT_CODE_SIZE
}

pub fn default_merge() -> MergeMode {
    MergeChoice::ALL[usize::from(MERGE.load(Ordering::Relaxed)).min(3)].mode()
}

/// Publishes the preferences and applies fonts and accent color. Cheap when
/// nothing changed; called every frame.
pub fn apply(ctx: &egui::Context, p: &Prefs) {
    let date = match p.date_format {
        DateFormat::Relative => 0,
        DateFormat::Absolute => 1,
        DateFormat::Long => 2,
    };
    DATE.store(date, Ordering::Relaxed);
    TAB.store(p.tab_width.clamp(1, 16), Ordering::Relaxed);
    CODE_SIZE.store(
        p.code_font_size.clamp(7.0, 32.0).to_bits(),
        Ordering::Relaxed,
    );
    let merge = MergeChoice::ALL.iter().position(|m| *m == p.merge);
    MERGE.store(merge.unwrap_or(0) as u8, Ordering::Relaxed);

    let id = egui::Id::unique("ferrum-applied-look");
    let mut applied: Applied = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    let before = applied.clone();

    // Git executable and verbose output.
    if applied.git.as_ref() != Some(&(p.git_path.clone(), p.verbose_git)) {
        let path = p.git_path.trim();
        ferrum_core::cli::set_program((!path.is_empty()).then(|| path.into()));
        ferrum_core::cli::set_verbose(p.verbose_git);
        applied.git = Some((p.git_path.clone(), p.verbose_git));
    }

    // Code font: loaded on a background thread, installed once it is ready.
    let family = p.code_font.trim().to_owned();
    if applied.font.as_ref() != Some(&family) {
        let font = if family.is_empty() {
            Some(None)
        } else {
            font_data(ctx, &family).map(|f| f.map(|(data, index)| (family.clone(), data, index)))
        };
        if let Some(font) = font {
            let mut defs = egui::FontDefinitions::default();
            if let Some((name, data, index)) = font {
                let key = format!("code:{name}");
                let fd = egui::FontData::from_blob(data, index);
                defs.font_data.insert(key.clone(), Arc::new(fd));
                if let Some(list) = defs.families.get_mut(&egui::FontFamily::Monospace) {
                    list.insert(0, key);
                }
            }
            ctx.set_fonts(defs);
            applied.font = Some(family);
        }
    }

    // Accent color.
    let accent = if p.system_accent {
        system_accent(ctx)
    } else {
        None
    };
    if applied.accent != Some(accent) {
        apply_accent(ctx, accent);
        applied.accent = Some(accent);
    }

    if applied != before {
        ctx.data_mut(|d| d.insert_temp(id, applied));
    }
}

/// What `apply` last installed in this context.
#[derive(Clone, Default, PartialEq)]
struct Applied {
    git: Option<(String, bool)>,
    font: Option<String>,
    accent: Option<Option<Color32>>,
}

fn apply_accent(ctx: &egui::Context, accent: Option<Color32>) {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        let base = match theme {
            egui::Theme::Dark => egui::Visuals::dark(),
            egui::Theme::Light => egui::Visuals::light(),
        };
        ctx.style_mut_of(theme, |style| {
            let v = &mut style.visuals;
            match accent {
                None => {
                    v.selection = base.selection;
                    v.hyperlink_color = base.hyperlink_color;
                }
                Some(c) => {
                    let (fill, stroke, link) = accent_shades(c, theme == egui::Theme::Dark);
                    v.selection.bg_fill = fill;
                    v.selection.stroke.color = stroke;
                    v.hyperlink_color = link;
                }
            }
        });
    }
}

/// Selection fill, selection text and link colors derived from an accent color.
fn accent_shades(c: Color32, dark: bool) -> (Color32, Color32, Color32) {
    let mix = |a: Color32, b: Color32, t: f32| {
        let m = |x: u8, y: u8| (f32::from(x) * (1.0 - t) + f32::from(y) * t).round() as u8;
        Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
    };
    if dark {
        (
            mix(c, Color32::BLACK, 0.45),
            mix(c, Color32::WHITE, 0.8),
            mix(c, Color32::WHITE, 0.35),
        )
    } else {
        (
            mix(c, Color32::WHITE, 0.6),
            mix(c, Color32::BLACK, 0.6),
            mix(c, Color32::BLACK, 0.2),
        )
    }
}

// ----- System fonts -----

type LoadedFont = Option<(egui::epaint::text::Blob, u32)>;

/// Fonts loaded so far by family; `None` inside = not installed.
static FONTS: Mutex<Option<HashMap<String, LoadedFont>>> = Mutex::new(None);
static FONT_DB: OnceLock<fontdb::Database> = OnceLock::new();

fn font_db() -> &'static fontdb::Database {
    FONT_DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        db
    })
}

/// The data of an installed font family: `None` while it loads, `Some(None)` when missing.
fn font_data(ctx: &egui::Context, family: &str) -> Option<LoadedFont> {
    let mut fonts = FONTS.lock().ok()?;
    let map = fonts.get_or_insert_with(HashMap::new);
    if let Some(f) = map.get(family) {
        return Some(f.clone());
    }
    let pending = format!("\0{family}");
    if map.contains_key(&pending) {
        return None;
    }
    map.insert(pending.clone(), None);
    let (family, ctx) = (family.to_owned(), ctx.clone());
    std::thread::spawn(move || {
        let db = font_db();
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(&family)],
            ..Default::default()
        };
        let data = db.query(&query).and_then(|id| {
            db.with_face_data(id, |data, index| {
                let blob: egui::epaint::text::Blob = Arc::new(data.to_vec());
                (blob, index)
            })
        });
        if let Ok(mut fonts) = FONTS.lock()
            && let Some(map) = fonts.as_mut()
        {
            map.remove(&pending);
            map.insert(family, data);
        }
        ctx.request_repaint();
    });
    None
}

/// Monospace font families installed on the system, sorted. `None` while the
/// list is built (the first call starts it).
pub fn monospace_families(ctx: &egui::Context) -> Option<&'static [String]> {
    static LIST: OnceLock<Vec<String>> = OnceLock::new();
    static STARTED: std::sync::Once = std::sync::Once::new();
    if let Some(l) = LIST.get() {
        return Some(l);
    }
    let ctx = ctx.clone();
    STARTED.call_once(|| {
        std::thread::spawn(move || {
            let mut names: Vec<String> = font_db()
                .faces()
                .filter(|f| f.monospaced)
                .filter_map(|f| f.families.first().map(|(n, _)| n.clone()))
                .collect();
            names.sort_by_key(|n| n.to_lowercase());
            names.dedup();
            let _ = LIST.set(names);
            ctx.request_repaint();
        });
    });
    None
}

// ----- System accent color -----

/// The operating system's accent color, if it has one. Looked up once, in the background.
pub fn system_accent(ctx: &egui::Context) -> Option<Color32> {
    static ACCENT: OnceLock<Option<Color32>> = OnceLock::new();
    static STARTED: std::sync::Once = std::sync::Once::new();
    if let Some(a) = ACCENT.get() {
        return *a;
    }
    let ctx = ctx.clone();
    STARTED.call_once(|| {
        std::thread::spawn(move || {
            let _ = ACCENT.set(crate::accent::query());
            ctx.request_repaint();
        });
    });
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_spaces_in_branch_names() {
        let mut p = Prefs::default();
        assert_eq!(p.branch_name("fix login page"), "fix-login-page");
        p.branch_space = "_".into();
        assert_eq!(p.branch_name("fix login"), "fix_login");
        p.branch_space.clear();
        assert_eq!(p.branch_name("fix login"), "fix login");
    }

    #[test]
    fn old_settings_get_defaults() {
        let p: Prefs = serde_json::from_str("{}").unwrap();
        assert_eq!(p, Prefs::default());
        assert!(p.shows(ToolbarButton::Fetch));
    }

    #[test]
    fn accent_shades_keep_the_hue() {
        let (fill, _, link) = accent_shades(Color32::from_rgb(0x35, 0x84, 0xe4), true);
        assert!(fill.b() > fill.r() && link.b() > link.r());
    }
}
