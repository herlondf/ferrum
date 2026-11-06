//! Side-by-side image diff. (Placeholder — to be implemented.)

use eframe::egui;
use ferrum_core::{DiffSource, FileDiff, Repo};

pub fn show(ui: &mut egui::Ui, _repo: &Repo, file: &FileDiff, _source: DiffSource) {
    ui.weak(format!(
        "Binary image {} — preview not implemented yet",
        file.path
    ));
}
