//! Settings window. (Placeholder — to be implemented.)

use eframe::egui;

use crate::command::Commands;
use crate::settings::Settings;

#[derive(Default)]
pub struct SettingsWindow {
    pub open: bool,
}

pub fn show(
    ctx: &egui::Context,
    win: &mut SettingsWindow,
    settings: &mut Settings,
    _cmds: &mut Commands,
) {
    egui::Window::new("Settings")
        .open(&mut win.open)
        .show(ctx, |ui| {
            ui.label(format!("Theme: {:?}", settings.theme));
        });
}
