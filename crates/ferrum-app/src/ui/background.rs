//! Settings for what the app does by itself: watching files, refreshing,
//! notifications and update checks.

use eframe::egui;

use crate::settings::BackgroundSettings;

pub fn settings_section(ui: &mut egui::Ui, bg: &mut BackgroundSettings) {
    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(t!("Refresh and notifications"))
            .size(16.0)
            .strong(),
    );
    ui.add_space(6.0);
    ui.checkbox(
        &mut bg.watch_files,
        t!("Watch the working folder for changes"),
    )
    .on_hover_text(t!(
        "Reload as soon as files, branches or the index change on disk"
    ));
    ui.add_enabled_ui(!bg.watch_files, |ui| {
        ui.horizontal(|ui| {
            ui.label(t!("Otherwise check every"));
            ui.add(
                egui::DragValue::new(&mut bg.poll_secs)
                    .range(1..=600)
                    .suffix(" s"),
            );
        });
    });
    ui.checkbox(
        &mut bg.refresh_on_focus,
        t!("Refresh when the window gets the focus"),
    );
    ui.checkbox(
        &mut bg.notifications,
        t!("Notify when a long operation finishes while the window is in the background"),
    );
    ui.checkbox(
        &mut bg.check_updates,
        t!("Check for new versions once a day"),
    );
    if let Some((version, url)) = &bg.update_available {
        ui.hyperlink_to(t!("Version {} available", version), url);
    }
}
