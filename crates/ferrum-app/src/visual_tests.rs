//! UI tests for the theme presets, density and icons.

use eframe::egui::Color32;
use egui_kittest::kittest::Queryable;

use crate::command::{Command, DevStep};
use crate::settings::ThemeChoice;
use crate::theme::Density;
use crate::ui_tests::{frames, harness, repo};

#[test]
fn density_and_high_contrast_from_settings() {
    let dir = repo();
    let mut h = harness(dir.path());
    let ctx = h.ctx.clone();
    frames(&mut h);
    let comfortable = ctx.global_style().spacing.interact_size.y;

    h.state_mut().handle(
        Command::DevStep(DevStep::SettingsSection("appearance".into())),
        &ctx,
    );
    frames(&mut h);
    h.get_by_label("Compact").click();
    frames(&mut h);
    assert_eq!(h.state().settings.prefs.density, Density::Compact);
    assert!(ctx.global_style().spacing.interact_size.y < comfortable);

    h.state_mut().handle(
        Command::DevStep(DevStep::SettingsSection("general".into())),
        &ctx,
    );
    frames(&mut h);
    h.get_by_label("High contrast").click();
    frames(&mut h);
    assert_eq!(h.state().settings.theme, ThemeChoice::HighContrast);
    let visuals = ctx.global_style().visuals.clone();
    assert!(visuals.dark_mode);
    assert_eq!(visuals.panel_fill, Color32::BLACK);
}

#[test]
fn toolbar_buttons_have_icons() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    use crate::icons;
    for label in [
        icons::text(icons::FETCH, "Fetch"),
        icons::text(icons::BRANCH, "Branch"),
        icons::text(icons::STASH, "Stash"),
        icons::SETTINGS.to_owned(),
        icons::COMMAND_PALETTE.to_owned(),
    ] {
        assert!(h.query_by_label(&label).is_some(), "{label}");
    }
}
