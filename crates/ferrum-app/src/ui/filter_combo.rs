//! A combo box with a filter field above its list, for long lists of branches.

use eframe::egui;

/// Shows a combo box listing `items` with a filter field. With `custom`,
/// the typed text can be used even when no item matches it (e.g. a new
/// remote branch name). Returns true when `value` changed.
pub fn filter_combo(
    ui: &mut egui::Ui,
    id_salt: &str,
    value: &mut String,
    items: &[String],
    width: f32,
    custom: bool,
) -> bool {
    let id = ui.make_persistent_id(("filter-combo", id_salt));
    let mut changed = false;
    let shown = if value.is_empty() {
        t!("Choose…")
    } else {
        value.clone()
    };
    let resp = egui::ComboBox::from_id_salt(id_salt)
        .selected_text(shown)
        .width(width)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            ui.set_min_width(width.max(200.0));
            let mut filter: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
            let edit = ui.add(
                egui::TextEdit::singleline(&mut filter)
                    .hint_text(t!("Filter…"))
                    .desired_width(f32::INFINITY),
            );
            let focus_id = id.with("focused");
            if !ui.data(|d| d.get_temp::<bool>(focus_id)).unwrap_or(false) {
                edit.request_focus();
                ui.data_mut(|d| d.insert_temp(focus_id, true));
            }
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let matching: Vec<&String> = items.iter().filter(|i| matches(i, &filter)).collect();
            let typed = filter.trim().to_owned();
            let mut pick = None;
            if enter {
                pick = matching
                    .first()
                    .map(|s| (*s).clone())
                    .or_else(|| (custom && !typed.is_empty()).then(|| typed.clone()));
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    if custom && !typed.is_empty() && !items.contains(&typed) {
                        if ui.button(t!("Use “{}”", typed)).clicked() {
                            pick = Some(typed.clone());
                        }
                    } else if matching.is_empty() {
                        ui.weak(t!("No matches."));
                    }
                    for item in matching {
                        if ui.selectable_label(item == value, item.as_str()).clicked() {
                            pick = Some(item.clone());
                        }
                    }
                });
            ui.data_mut(|d| d.insert_temp(id, filter));
            if let Some(p) = pick {
                changed = *value != p;
                *value = p;
                ui.close();
            }
        });
    if resp.inner.is_none() {
        // Closed: start with an empty filter next time.
        ui.data_mut(|d| {
            d.remove::<String>(id);
            d.remove::<bool>(id.with("focused"));
        });
    }
    changed
}

/// Case-insensitive; every word of the filter must appear.
pub fn matches(item: &str, filter: &str) -> bool {
    let item = item.to_lowercase();
    filter
        .split_whitespace()
        .all(|w| item.contains(&w.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn filters_by_words() {
        assert!(matches("origin/feature/Login", "login"));
        assert!(matches("origin/feature/login", "feat log"));
        assert!(!matches("origin/main", "dev"));
        assert!(matches("anything", "  "));
    }
}
