//! Empty panels ("Select a commit…"): a large icon, a title, a short hint
//! and the shortcuts or actions that help next, centered in the panel.

use eframe::egui::{self, Align, Layout, RichText, vec2};

use crate::keymap::{self, Action};

/// Shows an empty state filling the rest of `ui`. `extra` adds actions
/// (buttons, shortcut rows) under the hint.
pub fn show(
    ui: &mut egui::Ui,
    icon: &str,
    title: &str,
    hint: &str,
    extra: impl FnOnce(&mut egui::Ui),
) {
    let avail = ui.available_rect_before_wrap();
    // Roughly centered: a bit above the middle reads better.
    let top = ((avail.height() - 180.0) * 0.42).max(8.0);
    ui.allocate_ui_with_layout(avail.size(), Layout::top_down(Align::Center), |ui| {
        ui.set_min_size(avail.size());
        ui.add_space(top);
        let weak = ui.visuals().weak_text_color();
        ui.label(
            RichText::new(icon)
                .size(34.0)
                .color(weak.gamma_multiply(0.8)),
        );
        ui.add_space(crate::theme::SPACE_M);
        ui.label(crate::theme::strong(title).size(15.0));
        if !hint.is_empty() {
            ui.add_space(crate::theme::SPACE_S);
            ui.add(egui::Label::new(RichText::new(hint).color(weak)).wrap());
        }
        ui.add_space(crate::theme::SPACE_L);
        extra(ui);
    });
}

/// A row "[keys]  label", clickable when `action` is given (it then runs the
/// shortcut's action).
pub fn shortcut(ui: &mut egui::Ui, keys: &str, label: &str, action: Option<Action>) {
    let width = 260.0_f32.min(ui.available_width());
    ui.allocate_ui_with_layout(
        vec2(width, 22.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            keycap(ui, keys);
            let text = RichText::new(label).color(ui.visuals().weak_text_color());
            match action {
                Some(a) => {
                    if ui.add(egui::Link::new(text)).clicked() {
                        keymap::trigger(ui.ctx(), a);
                    }
                }
                None => {
                    ui.label(text);
                }
            }
        },
    );
}

/// The bound keys of an action as a shortcut row.
pub fn action(ui: &mut egui::Ui, a: Action, label: &str) {
    let keys = keymap::text(a);
    if !keys.is_empty() {
        shortcut(ui, &keys, label, Some(a));
    }
}

/// Keys drawn like a key cap.
pub fn keycap(ui: &mut egui::Ui, keys: &str) {
    let v = ui.visuals();
    egui::Frame::new()
        .fill(v.widgets.inactive.weak_bg_fill)
        .stroke(v.widgets.noninteractive.bg_stroke)
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(keys).size(11.5).monospace());
        });
}
