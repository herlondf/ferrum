//! Diff viewer preferences (saved in the settings) and the diff options each
//! repository tab loads its diffs with.

use std::collections::HashSet;

use eframe::egui;
use ferrum_core::diff::DiffConfig;
use serde::{Deserialize, Serialize};

use crate::tab::RepoTab;

pub const CONTEXT_CHOICES: [u32; 7] = [0, 1, 3, 5, 10, 25, 100];
pub const FONT_SIZES: std::ops::RangeInclusive<f32> = 8.0..=24.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ImageMode {
    #[default]
    SideBySide,
    Swipe,
    OnionSkin,
    Difference,
}

impl ImageMode {
    pub const ALL: [ImageMode; 4] = [
        ImageMode::SideBySide,
        ImageMode::Swipe,
        ImageMode::OnionSkin,
        ImageMode::Difference,
    ];

    pub fn label(self) -> String {
        match self {
            ImageMode::SideBySide => t!("Side by side"),
            ImageMode::Swipe => t!("Swipe"),
            ImageMode::OnionSkin => t!("Onion skin"),
            ImageMode::Difference => t!("Difference"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiffPrefs {
    pub side_by_side: bool,
    pub wrap: bool,
    pub font_size: f32,
    pub ignore_whitespace: bool,
    pub full_file: bool,
    pub context_lines: u32,
    pub show_whitespace: bool,
    pub tab_width: u8,
    /// `-`/`+` in front of changed lines, besides the colors.
    pub markers: bool,
    pub syntax: bool,
    pub word_diff: bool,
    pub minimap: bool,
    pub image_mode: ImageMode,
    /// Show SVG files as images rather than text.
    pub svg_as_image: bool,
}

impl Default for DiffPrefs {
    fn default() -> Self {
        Self {
            side_by_side: false,
            wrap: false,
            font_size: 12.0,
            ignore_whitespace: false,
            full_file: false,
            context_lines: 3,
            show_whitespace: false,
            tab_width: 4,
            markers: true,
            syntax: true,
            word_diff: true,
            minimap: true,
            image_mode: ImageMode::SideBySide,
            svg_as_image: true,
        }
    }
}

impl DiffPrefs {
    /// The options diffs are computed with.
    pub fn config(&self) -> DiffConfig {
        DiffConfig {
            ignore_whitespace: self.ignore_whitespace,
            context_lines: self.context_lines,
            full_file: self.full_file,
            ..DiffConfig::default()
        }
    }

    pub fn font_size(&self) -> f32 {
        self.font_size.clamp(*FONT_SIZES.start(), *FONT_SIZES.end())
    }

    pub fn tab_width(&self) -> usize {
        self.tab_width.clamp(1, 16) as usize
    }
}

/// Diff options of one tab.
#[derive(Default)]
pub struct TabDiffOptions {
    pub config: DiffConfig,
    /// Large untracked files whose diff the user asked to load.
    pub load_large: HashSet<String>,
}

impl TabDiffOptions {
    pub fn for_path(&self, path: &str) -> DiffConfig {
        let mut cfg = self.config;
        if self.load_large.contains(path) {
            cfg.max_untracked_bytes = None;
        }
        cfg
    }
}

fn prefs_id() -> egui::Id {
    egui::Id::unique("ferrum-diff-prefs")
}

/// Preferences for the views to read this frame.
pub fn get(ctx: &egui::Context) -> DiffPrefs {
    ctx.data(|d| d.get_temp::<(DiffPrefs, DiffPrefs)>(prefs_id()))
        .map(|(p, _)| p)
        .unwrap_or_default()
}

/// Changes the preferences (saved at the end of the frame).
pub fn set(ctx: &egui::Context, prefs: DiffPrefs) {
    ctx.data_mut(|d| {
        let entry = d.get_temp_mut_or_insert_with(prefs_id(), || (prefs.clone(), prefs.clone()));
        entry.0 = prefs;
    });
}

/// Keeps the settings, the views' copy and each tab's diff options in step.
/// Called once per frame; reloads diffs whose options changed.
pub fn sync(ctx: &egui::Context, settings: &mut DiffPrefs, tabs: &mut [RepoTab]) {
    ctx.data_mut(|d| {
        // (current, last synced): a change in either place wins.
        let entry =
            d.get_temp_mut_or_insert_with(prefs_id(), || (settings.clone(), settings.clone()));
        if *settings != entry.1 {
            entry.0 = settings.clone();
        } else if entry.0 != entry.1 {
            *settings = entry.0.clone();
        }
        entry.1 = settings.clone();
    });
    let config = settings.config();
    for tab in tabs {
        if tab.diff_options.config != config {
            tab.diff_options.config = config;
            reload(tab);
        }
    }
}

/// Reloads the diffs a tab shows, e.g. after the options changed.
pub fn reload(tab: &mut RepoTab) {
    if let Some(i) = tab.commit_view.as_ref().and_then(|v| v.selected_file) {
        tab.select_commit_file(i);
    }
    tab.reload_change_diff();
}

/// Loads the full diff of a large untracked file.
pub fn load_large(tab: &mut RepoTab, path: &str) {
    tab.diff_options.load_large.insert(path.to_owned());
    tab.reload_change_diff();
}

/// `FERRUM_START` step `diff:OPTS`, a comma-separated list of `sbs`, `wrap`,
/// `ws` (show whitespace), `full`, `nomarkers`, `nosyntax`, `font=N`,
/// `context=N`, `image=swipe|onion|difference`, `find=TEXT` (opens the find bar),
/// `large` (opens the large window).
pub fn dev_step(ctx: &egui::Context, prefs: &mut DiffPrefs, arg: &str) {
    for opt in arg.split(',').map(str::trim) {
        let (key, value) = opt.split_once('=').unwrap_or((opt, ""));
        match key {
            "sbs" => prefs.side_by_side = true,
            "wrap" => prefs.wrap = true,
            "ws" => prefs.show_whitespace = true,
            "full" => prefs.full_file = true,
            "nomarkers" => prefs.markers = false,
            "nosyntax" => prefs.syntax = false,
            "font" => prefs.font_size = value.parse().unwrap_or(prefs.font_size),
            "context" => prefs.context_lines = value.parse().unwrap_or(prefs.context_lines),
            "image" => {
                prefs.image_mode = match value {
                    "swipe" => ImageMode::Swipe,
                    "onion" => ImageMode::OnionSkin,
                    "difference" => ImageMode::Difference,
                    _ => ImageMode::SideBySide,
                }
            }
            "find" | "large" => {
                for view in ["changes-diff", "commit-diff", "file-history-diff"] {
                    crate::ui::diff_view::dev_open(ctx, view, (key == "find").then_some(value));
                }
            }
            _ => eprintln!("FERRUM_START: unknown diff option {opt}"),
        }
    }
}

/// The diff toolbar and Settings share the code font size and tab width:
/// a change in either place is copied to the other.
pub fn share_with_prefs(
    ctx: &egui::Context,
    diff: &mut DiffPrefs,
    prefs: &mut crate::prefs::Prefs,
) {
    let id = egui::Id::unique("ferrum-diff-shared-metrics");
    let last = ctx.data(|d| d.get_temp::<(f32, u8)>(id));
    let ours = (diff.font_size, diff.tab_width);
    let theirs = (prefs.code_font_size, prefs.tab_width);
    match last {
        // Settings changed (or first frame): the diff follows.
        None => {
            diff.font_size = prefs.code_font_size;
            diff.tab_width = prefs.tab_width;
        }
        Some(l) if theirs != l => {
            diff.font_size = prefs.code_font_size;
            diff.tab_width = prefs.tab_width;
        }
        Some(l) if ours != l => {
            prefs.code_font_size = diff.font_size;
            prefs.tab_width = diff.tab_width;
        }
        _ => {}
    }
    ctx.data_mut(|d| d.insert_temp(id, (diff.font_size, diff.tab_width)));
}
