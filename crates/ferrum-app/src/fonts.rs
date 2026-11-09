//! The embedded fonts: Inter for the interface, JetBrains Mono for code and
//! Phosphor for icons (see `icons`). egui's own fonts stay as fallbacks, for
//! characters these do not have (emoji, other scripts).
//!
//! The files in `assets/fonts` are cut down by `scripts/build-fonts.py`.
//! Use `FontFamily::Name(STRONG.into())` (or `theme::strong_font`) for
//! semibold text such as headings.

use std::sync::Arc;

use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontTweak};

const INTER: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");
const JETBRAINS_MONO: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");
const PHOSPHOR: &[u8] = include_bytes!("../assets/fonts/Phosphor.ttf");

/// Font family name of Inter SemiBold.
pub const STRONG: &str = "strong";

/// Name of the built-in code font, for Settings.
pub const CODE_FONT_NAME: &str = "JetBrains Mono";

/// Ferrum's fonts with egui's defaults as fallbacks. `code` is a font chosen
/// in Settings (name, data, face index), used before JetBrains Mono.
pub fn definitions(code: Option<(String, egui::epaint::text::Blob, u32)>) -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let mut add = |name: &str, data: FontData| {
        defs.font_data.insert(name.to_owned(), Arc::new(data));
    };
    add("Inter", FontData::from_static(INTER));
    add("Inter-SemiBold", FontData::from_static(INTER_SEMIBOLD));
    add(
        "JetBrainsMono",
        FontData::from_static(JETBRAINS_MONO).tweak(FontTweak {
            // Its letters are taller than Hack's; keep code the same visual size.
            scale: 0.95,
            ..Default::default()
        }),
    );
    add(
        "Phosphor",
        FontData::from_static(PHOSPHOR).tweak(FontTweak {
            // The icons fill the whole em box: a bit smaller and lower sits
            // them on the text's baseline like a capital letter.
            scale: 1.1,
            y_offset_factor: 0.1,
            ..Default::default()
        }),
    );
    let mut code_key = None;
    if let Some((name, data, index)) = code {
        let key = format!("code:{name}");
        defs.font_data
            .insert(key.clone(), Arc::new(FontData::from_blob(data, index)));
        code_key = Some(key);
    }

    let fallbacks = |family: &FontFamily, defs: &FontDefinitions| -> Vec<String> {
        defs.families.get(family).cloned().unwrap_or_default()
    };
    let proportional = fallbacks(&FontFamily::Proportional, &defs);
    let monospace = fallbacks(&FontFamily::Monospace, &defs);

    let chain = |first: &[&str], rest: &[String]| -> Vec<String> {
        first
            .iter()
            .map(|s| (*s).to_owned())
            .chain(std::iter::once("Phosphor".to_owned()))
            .chain(rest.iter().cloned())
            .collect()
    };
    defs.families
        .insert(FontFamily::Proportional, chain(&["Inter"], &proportional));
    defs.families.insert(
        FontFamily::Name(STRONG.into()),
        chain(&["Inter-SemiBold", "Inter"], &proportional),
    );
    let mut mono = chain(&["JetBrainsMono"], &monospace);
    if let Some(key) = code_key {
        mono.insert(0, key);
    }
    defs.families.insert(FontFamily::Monospace, mono);
    defs
}

/// Installs the fonts with the built-in code font (tests use this).
#[cfg(test)]
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(definitions(None));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fonts_parse_and_cover_the_basics() {
        let mut harness = egui_kittest::Harness::new_ui(|_| {});
        install(&harness.ctx);
        harness.run();
        harness.ctx.fonts_mut(|f| {
            for c in ['A', 'ç', 'ã', '…', '—', '→'] {
                assert!(f.has_glyph(&egui::FontId::proportional(13.0), c), "{c}");
                assert!(f.has_glyph(&egui::FontId::monospace(12.0), c), "{c}");
            }
            let strong = egui::FontId::new(13.0, FontFamily::Name(STRONG.into()));
            assert!(f.has_glyph(&strong, 'W'));
        });
    }
}
