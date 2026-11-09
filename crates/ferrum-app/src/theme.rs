//! Ferrum's look: theme presets (dark, light, high contrast), the accent
//! color, density, spacing and button styles. `apply` installs it; views use
//! the building blocks below instead of hard-coded colors and sizes:
//!
//! - `accent(ui)`, `on_accent()`: the accent color of the current theme and
//!   the text color on top of it. `ui.visuals().selection.bg_fill` is the
//!   softer accent used for selected rows.
//! - `primary(text)` / `danger(text)` / `secondary(text)`: dialog buttons.
//!   The main action (Create branch, Commit, Push…) is `primary`, Cancel is
//!   `secondary`: `ui.add_enabled(ok, theme::primary(t!("Push")))`.
//! - `row_height()`, `tall_row_height()`: list row heights for the density
//!   chosen in Settings > Appearance; `density()` for anything else.
//! - `SPACE_XS`..`SPACE_XL`, `RADIUS`, `RADIUS_LARGE`: spacing and corners.
//! - `strong(text)`, `strong_font(size)`: Inter SemiBold (headings, titles).
//! - `card(ui)`: a rounded frame in a slightly different shade, for grouping.
//! - `dialog_frame(style)`: the frame for `egui::Modal`s (more padding).

// Building blocks for views: not all of them are used yet.
#![allow(dead_code)]

use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Margin, RichText, Shadow, Stroke, TextStyle,
    Theme, Vec2, WidgetText, vec2,
};
use serde::{Deserialize, Serialize};

use crate::settings::ThemeChoice;

pub const SPACE_XS: f32 = 2.0;
pub const SPACE_S: f32 = 4.0;
pub const SPACE_M: f32 = 8.0;
pub const SPACE_L: f32 = 12.0;
pub const SPACE_XL: f32 = 20.0;
/// Corner radius of buttons, fields and badges.
pub const RADIUS: u8 = 6;
/// Corner radius of windows, dialogs and cards.
pub const RADIUS_LARGE: u8 = 10;

/// Ferrum's accent: the rust orange of the app icon.
pub const FERRUM_ACCENT: Color32 = Color32::from_rgb(0xE0, 0x6A, 0x2E);

/// Row heights and padding (Settings > Appearance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Density {
    Compact,
    #[default]
    Comfortable,
}

impl Density {
    pub const ALL: [Density; 2] = [Density::Compact, Density::Comfortable];

    pub fn label(self) -> String {
        match self {
            Density::Compact => t!("Compact"),
            Density::Comfortable => t!("Comfortable"),
        }
    }
}

static DENSITY: AtomicU8 = AtomicU8::new(1);
static ACCENT_DARK: AtomicU32 = AtomicU32::new(0);
static ACCENT_LIGHT: AtomicU32 = AtomicU32::new(0);

pub fn density() -> Density {
    match DENSITY.load(Ordering::Relaxed) {
        0 => Density::Compact,
        _ => Density::Comfortable,
    }
}

/// Height of a one-line list row (commits, files, search results).
pub fn row_height() -> f32 {
    match density() {
        Density::Compact => 22.0,
        Density::Comfortable => 26.0,
    }
}

/// Height of a two-line list row.
pub fn tall_row_height() -> f32 {
    match density() {
        Density::Compact => 38.0,
        Density::Comfortable => 44.0,
    }
}

/// The accent color for the current theme.
pub fn accent(ui: &egui::Ui) -> Color32 {
    let a = if ui.visuals().dark_mode {
        &ACCENT_DARK
    } else {
        &ACCENT_LIGHT
    };
    match a.load(Ordering::Relaxed) {
        0 => FERRUM_ACCENT,
        c => unpack(c),
    }
}

/// Text and icon color on an accent background.
pub fn on_accent() -> Color32 {
    Color32::WHITE
}

/// Semibold text, for headings and titles.
pub fn strong(text: impl Into<String>) -> RichText {
    RichText::new(text).family(FontFamily::Name(crate::fonts::STRONG.into()))
}

pub fn strong_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(crate::fonts::STRONG.into()))
}

/// The main action of a dialog, filled with the accent color.
pub fn primary(text: impl Into<WidgetText>) -> Filled {
    Filled {
        text: text.into(),
        danger: false,
        min_size: vec2(BUTTON_MIN_WIDTH, 0.0),
    }
}

/// A destructive main action (Delete, Discard…), filled red.
pub fn danger(text: impl Into<WidgetText>) -> Filled {
    Filled {
        danger: true,
        ..primary(text)
    }
}

/// A secondary dialog button (Cancel, Back…).
pub fn secondary(text: impl Into<WidgetText>) -> egui::Button<'static> {
    egui::Button::new(text).min_size(vec2(BUTTON_MIN_WIDTH, 0.0))
}

const BUTTON_MIN_WIDTH: f32 = 76.0;

/// A button filled with the accent (or danger) color; see `primary`.
pub struct Filled {
    text: WidgetText,
    danger: bool,
    min_size: Vec2,
}

impl Filled {
    pub fn min_size(mut self, size: Vec2) -> Self {
        self.min_size = size;
        self
    }
}

impl egui::Widget for Filled {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let base = if self.danger {
            danger_color(ui.visuals().dark_mode)
        } else {
            accent(ui)
        };
        ui.scope(|ui| {
            let w = &mut ui.visuals_mut().widgets;
            for (v, fill) in [
                (&mut w.inactive, base),
                (&mut w.hovered, mix(base, Color32::WHITE, 0.12)),
                (&mut w.active, mix(base, Color32::BLACK, 0.12)),
            ] {
                v.weak_bg_fill = fill;
                v.bg_fill = fill;
                v.bg_stroke = Stroke::NONE;
                v.fg_stroke.color = on_accent();
            }
            ui.add(egui::Button::new(self.text).min_size(self.min_size))
        })
        .inner
    }
}

fn danger_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0xD0, 0x45, 0x45)
    } else {
        Color32::from_rgb(0xC6, 0x30, 0x30)
    }
}

/// The frame of a dialog (`egui::Modal::frame`): roomier than a popup.
pub fn dialog_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::window(style).inner_margin(Margin::same(18))
}

/// A rounded frame in a slightly different shade, for grouping content.
pub fn card(ui: &egui::Ui) -> egui::Frame {
    let v = ui.visuals();
    egui::Frame::new()
        .fill(v.faint_bg_color)
        .stroke(v.widgets.noninteractive.bg_stroke)
        .corner_radius(RADIUS_LARGE)
        .inner_margin(Margin::same(SPACE_L as i8))
}

// ----- Installing the theme -----

#[derive(Clone, Copy, PartialEq)]
struct Applied {
    high_contrast: bool,
    density: Density,
    accent: Option<Color32>,
}

/// Installs the styles for the chosen preset, density and accent (`None` =
/// Ferrum's). Cheap when nothing changed; called every frame.
pub fn apply(ctx: &egui::Context, choice: ThemeChoice, density: Density, accent: Option<Color32>) {
    let want = Applied {
        high_contrast: choice == ThemeChoice::HighContrast,
        density,
        accent,
    };
    let id = egui::Id::unique("ferrum-applied-theme");
    if ctx.data(|d| d.get_temp::<Applied>(id)) == Some(want) {
        return;
    }
    DENSITY.store(density as u8, Ordering::Relaxed);
    let accent = accent.unwrap_or(FERRUM_ACCENT);
    for theme in [Theme::Dark, Theme::Light] {
        let palette = match theme {
            Theme::Dark if want.high_contrast => Palette::high_contrast(accent),
            Theme::Dark => Palette::dark(accent),
            Theme::Light => Palette::light(accent),
        };
        let store = match theme {
            Theme::Dark => &ACCENT_DARK,
            Theme::Light => &ACCENT_LIGHT,
        };
        store.store(pack(palette.accent), Ordering::Relaxed);
        ctx.style_mut_of(theme, |style| {
            style.visuals = palette.visuals(theme);
            spacing(style, density);
            text_styles(style);
        });
    }
    ctx.data_mut(|d| d.insert_temp(id, want));
}

fn spacing(style: &mut egui::Style, density: Density) {
    let s = &mut style.spacing;
    let comfortable = density == Density::Comfortable;
    s.item_spacing = if comfortable {
        vec2(8.0, 5.0)
    } else {
        vec2(6.0, 3.0)
    };
    s.button_padding = if comfortable {
        vec2(9.0, 3.0)
    } else {
        vec2(6.0, 1.0)
    };
    s.interact_size = Vec2::new(40.0, if comfortable { 22.0 } else { 18.0 });
    s.window_margin = Margin::same(if comfortable { 16 } else { 12 });
    s.menu_margin = Margin::same(6);
    s.menu_spacing = 2.0;
    s.indent = if comfortable { 18.0 } else { 16.0 };
    s.combo_height = 320.0;
    // Scroll handles in the control color rather than the text color.
    s.scroll.foreground_color = false;
}

fn text_styles(style: &mut egui::Style) {
    let t = &mut style.text_styles;
    t.insert(TextStyle::Small, FontId::proportional(10.5));
    t.insert(TextStyle::Body, FontId::proportional(13.0));
    t.insert(TextStyle::Button, FontId::proportional(13.0));
    t.insert(TextStyle::Monospace, FontId::monospace(12.5));
    t.insert(TextStyle::Heading, strong_font(17.0));
}

/// The colors a theme is built from.
struct Palette {
    dark: bool,
    accent: Color32,
    /// Main panels.
    panel: Color32,
    /// Windows, dialogs, menus.
    window: Color32,
    /// Text fields, scroll bars.
    field: Color32,
    /// Stripes and cards.
    faint: Color32,
    /// Separators and window borders.
    line: Color32,
    border: Color32,
    text: Color32,
    weak: Color32,
    /// Buttons: normal, hovered, pressed.
    button: [Color32; 3],
    /// Checkbox/radio boxes and slider rails.
    control: Color32,
    /// Outline of buttons and fields.
    outline: Stroke,
    warn: Color32,
    error: Color32,
    selection: Color32,
    selection_text: Color32,
    link: Color32,
    shadow: u8,
}

impl Palette {
    fn dark(accent: Color32) -> Self {
        let panel = Color32::from_rgb(0x1E, 0x1F, 0x23);
        Self {
            dark: true,
            accent,
            panel,
            window: Color32::from_rgb(0x26, 0x27, 0x2C),
            field: Color32::from_rgb(0x16, 0x17, 0x1A),
            faint: Color32::from_rgb(0x24, 0x25, 0x2A),
            line: Color32::from_rgb(0x2E, 0x30, 0x35),
            border: Color32::from_rgb(0x3A, 0x3C, 0x43),
            text: Color32::from_rgb(0xDA, 0xDC, 0xE0),
            weak: Color32::from_rgb(0x8C, 0x90, 0x98),
            button: [
                Color32::from_rgb(0x2E, 0x30, 0x36),
                Color32::from_rgb(0x38, 0x3B, 0x42),
                Color32::from_rgb(0x43, 0x46, 0x4E),
            ],
            control: Color32::from_rgb(0x3A, 0x3D, 0x44),
            outline: Stroke::NONE,
            warn: Color32::from_rgb(0xE8, 0xA8, 0x3E),
            error: Color32::from_rgb(0xF0, 0x6A, 0x60),
            selection: mix(mix(accent, Color32::BLACK, 0.5), panel, 0.25),
            selection_text: mix(accent, Color32::WHITE, 0.82),
            link: mix(accent, Color32::WHITE, 0.3),
            shadow: 110,
        }
    }

    fn light(accent: Color32) -> Self {
        // A darker accent reads better on white.
        let accent = mix(accent, Color32::BLACK, 0.1);
        Self {
            dark: false,
            accent,
            panel: Color32::from_rgb(0xF7, 0xF7, 0xF8),
            window: Color32::WHITE,
            field: Color32::WHITE,
            faint: Color32::from_rgb(0xF0, 0xF0, 0xF2),
            line: Color32::from_rgb(0xE3, 0xE3, 0xE7),
            border: Color32::from_rgb(0xD6, 0xD6, 0xDC),
            text: Color32::from_rgb(0x22, 0x24, 0x29),
            weak: Color32::from_rgb(0x6C, 0x70, 0x78),
            button: [
                Color32::from_rgb(0xEC, 0xEC, 0xEF),
                Color32::from_rgb(0xE2, 0xE2, 0xE6),
                Color32::from_rgb(0xD6, 0xD6, 0xDB),
            ],
            control: Color32::from_rgb(0xE4, 0xE4, 0xE8),
            outline: Stroke::new(1.0, Color32::from_rgb(0xDA, 0xDA, 0xDF)),
            warn: Color32::from_rgb(0xB0, 0x6A, 0x00),
            error: Color32::from_rgb(0xC6, 0x30, 0x30),
            selection: mix(accent, Color32::WHITE, 0.8),
            selection_text: mix(accent, Color32::BLACK, 0.55),
            link: mix(accent, Color32::BLACK, 0.15),
            shadow: 40,
        }
    }

    fn high_contrast(accent: Color32) -> Self {
        let accent = mix(accent, Color32::WHITE, 0.15);
        Self {
            dark: true,
            accent,
            panel: Color32::BLACK,
            window: Color32::from_rgb(0x0C, 0x0C, 0x0C),
            field: Color32::BLACK,
            faint: Color32::from_rgb(0x14, 0x14, 0x14),
            line: Color32::from_gray(0x80),
            border: Color32::WHITE,
            text: Color32::WHITE,
            weak: Color32::from_gray(0xC8),
            button: [
                Color32::from_gray(0x1A),
                Color32::from_gray(0x33),
                Color32::from_gray(0x4A),
            ],
            control: Color32::from_gray(0x2A),
            outline: Stroke::new(1.0, Color32::from_gray(0xB0)),
            warn: Color32::from_rgb(0xFF, 0xCC, 0x4D),
            error: Color32::from_rgb(0xFF, 0x70, 0x70),
            selection: mix(accent, Color32::BLACK, 0.45),
            selection_text: Color32::WHITE,
            link: mix(accent, Color32::WHITE, 0.4),
            shadow: 0,
        }
    }

    fn visuals(&self, theme: Theme) -> egui::Visuals {
        let mut v = match theme {
            Theme::Dark => egui::Visuals::dark(),
            Theme::Light => egui::Visuals::light(),
        };
        v.dark_mode = self.dark;
        v.panel_fill = self.panel;
        v.window_fill = self.window;
        v.extreme_bg_color = self.field;
        v.text_edit_bg_color = Some(self.field);
        v.faint_bg_color = self.faint;
        v.code_bg_color = self.faint;
        v.warn_fg_color = self.warn;
        v.error_fg_color = self.error;
        v.hyperlink_color = self.link;
        v.weak_text_color = Some(self.weak);
        v.selection.bg_fill = self.selection;
        v.selection.stroke = Stroke::new(1.0, self.selection_text);
        v.window_stroke = Stroke::new(1.0, self.border);
        v.window_corner_radius = CornerRadius::same(RADIUS_LARGE);
        v.menu_corner_radius = CornerRadius::same(8);
        v.window_shadow = Shadow {
            offset: [0, 8],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(self.shadow),
        };
        v.popup_shadow = Shadow {
            offset: [0, 4],
            blur: 14,
            spread: 0,
            color: Color32::from_black_alpha(self.shadow / 2 + self.shadow / 4),
        };
        v.window_highlight_topmost = false;
        v.indent_has_left_vline = false;
        v.slider_trailing_fill = true;
        v.handle_shape = egui::style::HandleShape::Circle;
        v.text_cursor.stroke = Stroke::new(2.0, self.accent);

        let radius = CornerRadius::same(RADIUS);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = self.panel;
        w.noninteractive.weak_bg_fill = self.panel;
        w.noninteractive.bg_stroke = Stroke::new(1.0, self.line);
        w.noninteractive.fg_stroke = Stroke::new(1.0, self.text);
        w.noninteractive.corner_radius = radius;
        let strong = if self.dark {
            Color32::WHITE
        } else {
            Color32::BLACK
        };
        for (i, wv) in [&mut w.inactive, &mut w.hovered, &mut w.active]
            .into_iter()
            .enumerate()
        {
            wv.weak_bg_fill = self.button[i];
            wv.bg_fill = if i == 0 {
                self.control
            } else {
                mix(self.control, strong, 0.06 * i as f32)
            };
            wv.corner_radius = radius;
            wv.expansion = 0.0;
            wv.fg_stroke = Stroke::new(1.5, if i == 0 { self.text } else { strong });
            wv.bg_stroke = match i {
                0 => self.outline,
                1 if self.outline.width > 0.0 => {
                    Stroke::new(1.0, mix(self.outline.color, strong, 0.15))
                }
                1 => Stroke::new(1.0, self.border),
                _ => Stroke::new(1.0, self.accent),
            };
        }
        w.open = w.hovered;
        w.open.weak_bg_fill = self.button[1];
        v
    }
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) * (1.0 - t) + f32::from(y) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

fn pack(c: Color32) -> u32 {
    u32::from_be_bytes([0xFF, c.r(), c.g(), c.b()])
}

fn unpack(c: u32) -> Color32 {
    let [_, r, g, b] = c.to_be_bytes();
    Color32::from_rgb(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_keeps_the_accent_hue() {
        let p = Palette::dark(Color32::from_rgb(0x35, 0x84, 0xe4));
        assert!(p.selection.b() > p.selection.r() && p.link.b() > p.link.r());
        let p = Palette::light(Color32::from_rgb(0x35, 0x84, 0xe4));
        assert!(p.selection.b() > p.selection.r());
    }

    #[test]
    fn packs_colors() {
        assert_eq!(unpack(pack(FERRUM_ACCENT)), FERRUM_ACCENT);
    }

    #[test]
    fn old_settings_get_comfortable() {
        let d: Density = serde_json::from_str("\"Compact\"").unwrap();
        assert_eq!(d, Density::Compact);
        assert_eq!(Density::default(), Density::Comfortable);
    }
}
