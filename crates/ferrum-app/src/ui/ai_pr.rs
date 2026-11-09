//! "Write with AI" next to a pull request description: the commits and the
//! diff of the branch go to the model, whose answer streams into the
//! description, where it can be edited before the request is created.

use std::path::Path;
use std::sync::{Arc, Mutex};

use eframe::egui;
use ferrum_core::cli::Git;

use crate::ai::Stream;
use crate::icons;
use crate::ui::ai_common;

#[derive(Default)]
struct Writing {
    stream: Option<Stream>,
    error: Option<String>,
}

/// The button, its destination, and the request filling `body` while it
/// streams. `base..head` is what the request adds.
pub fn write_button(
    ui: &mut egui::Ui,
    salt: &str,
    dir: &Path,
    base: &str,
    head: &str,
    body: &mut String,
) {
    let id = egui::Id::unique(("ai-pr", salt));
    let state = ui
        .ctx()
        .data_mut(|d| d.get_temp_mut_or_default::<Arc<Mutex<Writing>>>(id).clone());
    let mut w = state.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(stream) = &w.stream {
        let text = ferrum_core::ai::strip_thinking(&stream.text());
        if !text.is_empty() {
            *body = text.trim_start().to_owned();
        }
        if let Some(done) = stream.finished() {
            w.error = done.err();
            w.stream = None;
        }
    }
    let enabled = ai_common::config(ui.ctx()).enabled;
    if let Some(stream) = &w.stream {
        ui.spinner();
        if ui.small_button(t!("Stop")).clicked() {
            stream.cancel();
        }
    } else {
        let ready = enabled && !base.is_empty() && !head.is_empty();
        let resp = ui
            .add_enabled(
                ready,
                egui::Button::new(icons::text(icons::AI, t!("Write with AI"))).small(),
            )
            .on_hover_text(t!(
                "Writes the description from the commits and the diff (replaces it). {}",
                ai_common::where_now(ui.ctx()).long
            ))
            .on_disabled_hover_text(if enabled {
                t!("Choose the target branch first")
            } else {
                t!("AI is off: turn it on in Settings > AI.")
            });
        if resp.clicked() {
            let cfg = ai_common::config(ui.ctx());
            let (dir, base, head) = (dir.to_path_buf(), base.to_owned(), head.to_owned());
            let max = cfg.max;
            w.error = None;
            w.stream = Some(Stream::start(ui.ctx(), &cfg, move || {
                ferrum_core::ai::prompts::pull_request(
                    &Git::new(&dir),
                    &base,
                    &head,
                    crate::ai::language(),
                    max,
                )
                .map_err(|e| e.to_string())
            }));
        }
    }
    ai_common::badge(ui);
    if let Some(e) = &w.error {
        ui.colored_label(ui.visuals().error_fg_color, icons::WARNING)
            .on_hover_text(e);
    }
}
