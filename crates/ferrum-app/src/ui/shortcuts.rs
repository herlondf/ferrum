//! Settings > Shortcuts: every action with its keys. Click a key to record
//! a new one, "+" adds another, Reset goes back to the default.

use eframe::egui::{self, RichText};

use crate::keymap::{self, Action, Combo, Keymap};
use crate::settings::Settings;

/// What is being recorded: the action and the key replaced (`None` adds one).
type Recording = (Action, Option<usize>);

pub fn section(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.label(RichText::new(t!("Keyboard shortcuts")).size(16.0).strong());
    ui.add_space(6.0);
    let rec_id = ui.make_persistent_id("shortcut-recording");
    let filter_id = ui.make_persistent_id("shortcut-filter");
    let mut recording: Option<Recording> = ui.data(|d| d.get_temp(rec_id)).flatten();
    if recording.is_some() && !keymap::capturing() {
        // Cancelled with Escape, or stopped elsewhere.
        recording = None;
    }
    if let (Some((a, slot)), Some(combo)) = (recording, keymap::take_captured()) {
        let mut keys = Keymap::new(&settings.keymap).keys(a).to_vec();
        match slot {
            Some(i) if i < keys.len() => keys[i] = combo,
            _ if !keys.contains(&combo) => keys.push(combo),
            _ => {}
        }
        keymap::set_keys(&mut settings.keymap, a, &keys);
        recording = None;
    }
    if recording.is_some() {
        keymap::keep_capture();
    }

    let mut filter: String = ui.data(|d| d.get_temp(filter_id)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut filter)
                .hint_text(t!("Filter…"))
                .desired_width(200.0),
        );
        if ui
            .add_enabled(
                !settings.keymap.is_empty(),
                egui::Button::new(t!("Reset all")),
            )
            .clicked()
        {
            settings.keymap.clear();
            recording = None;
            keymap::stop_capture();
        }
    });
    ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
    ui.weak(t!("Click a shortcut to record a new one (Esc cancels)."));
    ui.add_space(4.0);

    let map = Keymap::new(&settings.keymap);
    let mut change: Option<(Action, Vec<Combo>)> = None;
    egui::Grid::new("settings-shortcuts")
        .num_columns(3)
        .striped(true)
        .spacing([16.0, 6.0])
        .show(ui, |ui| {
            for &a in Action::ALL {
                let label = a.label();
                if !crate::ui::filter_combo::matches(&label, &filter)
                    && !crate::ui::filter_combo::matches(&keymap::all_text(&map, a), &filter)
                {
                    continue;
                }
                ui.label(label);
                ui.horizontal(|ui| {
                    let keys = map.keys(a);
                    for (i, c) in keys.iter().enumerate() {
                        let this = recording == Some((a, Some(i)));
                        let text = if this { t!("Press keys…") } else { c.text() };
                        let others = map.users_of(c, a);
                        let mut rich = RichText::new(text).monospace();
                        if !others.is_empty() {
                            rich = rich.color(ui.visuals().warn_fg_color);
                        }
                        let mut resp = ui.selectable_label(this, rich);
                        if !others.is_empty() {
                            let names: Vec<String> = others.iter().map(|o| o.label()).collect();
                            resp = resp.on_hover_text(t!("⚠ Also used by: {}", names.join(", ")));
                        }
                        if resp.clicked() {
                            recording = Some((a, Some(i)));
                            keymap::start_capture();
                        }
                        if ui
                            .small_button("×")
                            .on_hover_text(t!("Remove this shortcut"))
                            .clicked()
                        {
                            let mut list = keys.to_vec();
                            list.remove(i);
                            change = Some((a, list));
                        }
                    }
                    let adding = recording == Some((a, None));
                    let add = if adding {
                        RichText::new(t!("Press keys…")).monospace()
                    } else {
                        RichText::new("+")
                    };
                    if ui
                        .selectable_label(adding, add)
                        .on_hover_text(t!("Add a shortcut"))
                        .clicked()
                    {
                        recording = Some((a, None));
                        keymap::start_capture();
                    }
                });
                let custom = settings.keymap.contains_key(a.id());
                if ui
                    .add_enabled(custom, egui::Button::new(t!("Reset")).small())
                    .on_hover_text(t!("Default: {}", default_text(a)))
                    .clicked()
                {
                    settings.keymap.remove(a.id());
                }
                ui.end_row();
            }
        });
    if let Some((a, list)) = change {
        keymap::set_keys(&mut settings.keymap, a, &list);
    }
    ui.data_mut(|d| d.insert_temp(rec_id, recording));

    let conflicts = map.conflicts();
    if !conflicts.is_empty() {
        ui.add_space(6.0);
        for (c, actions) in conflicts {
            let names: Vec<String> = actions.iter().map(|a| a.label()).collect();
            ui.colored_label(
                ui.visuals().warn_fg_color,
                t!("⚠ {} is used by: {}", c.text(), names.join(", ")),
            );
        }
    }
    ui.add_space(8.0);
    ui.weak(t!(
        "Fixed keys: arrows, Home/End, Enter and Escape in lists and dialogs, Tab / Shift+Tab between panes, Ctrl+C copy, P R E S F D in the interactive rebase."
    ));
}

fn default_text(a: Action) -> String {
    let list: Vec<String> = a
        .defaults()
        .iter()
        .filter_map(|s| Combo::parse(s))
        .map(|c| c.text())
        .collect();
    if list.is_empty() {
        t!("none")
    } else {
        list.join(" / ")
    }
}
