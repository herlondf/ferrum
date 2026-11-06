//! External tools for the files of the details, the compare view and the
//! local changes: open in the editor, show in the file manager, external diff
//! (Ctrl+D) and saving a revision of a file.

use std::path::Path;

use eframe::egui;
use ferrum_core::{FileDiff, Oid, Repo, short};

use crate::command::{Command, Commands};
use crate::external::{self, Launch};

/// Where one side of an external diff comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSide {
    /// The blob in the diff (`old_id` or `new_id`), empty when missing.
    Blob,
    /// The file on disk.
    WorkDir,
}

/// "Open in editor" and "Show in file manager" for a file of the working copy.
pub fn workdir_menu(ui: &mut egui::Ui, root: &Path, path: &str, cmds: &mut Commands) {
    let full = root.join(path);
    let exists = full.exists();
    let tip = t!("The file is not in the working copy.");
    let open = ui.add_enabled(exists, egui::Button::new(t!("Open in editor")));
    if open.on_disabled_hover_text(&tip).clicked() {
        cmds.push(Command::External(Launch::Editor {
            path: full.clone(),
            line: None,
        }));
        ui.close();
    }
    let reveal = ui.add_enabled(exists, egui::Button::new(t!("Show in file manager")));
    if reveal.on_disabled_hover_text(&tip).clicked() {
        cmds.push(Command::External(Launch::Reveal { path: full }));
        ui.close();
    }
}

/// Opens the file of a diff in the editor at `line`.
pub fn reveal_line(root: &Path, file: &FileDiff, line: u32) -> Command {
    Command::External(Launch::Editor {
        path: root.join(&file.path),
        line: Some(line),
    })
}

/// A temporary file name for one version: `main.abc1234.rs`.
fn version_name(path: &str, tag: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem}.{tag}.{ext}"),
        _ => format!("{name}.{tag}"),
    }
}

fn side_path(
    repo: &Repo,
    path: &str,
    id: Option<Oid>,
    side: DiffSide,
    tag: &str,
) -> Result<std::path::PathBuf, String> {
    if side == DiffSide::WorkDir {
        return Ok(repo.path().join(path));
    }
    let (bytes, tag) = match id {
        Some(id) => (repo.blob(id).map_err(|e| e.to_string())?, short(id)),
        None => (Vec::new(), tag.to_owned()),
    };
    external::temp_file(&version_name(path, &tag), &bytes)
}

/// Compares the two sides of a diff in the external diff tool.
pub fn external_diff(repo: &Repo, file: &FileDiff, left: DiffSide, right: DiffSide) -> Command {
    let old = file.old_path.as_deref().unwrap_or(&file.path);
    let paths = side_path(repo, old, file.old_id, left, "before")
        .and_then(|l| Ok((l, side_path(repo, &file.path, file.new_id, right, "after")?)));
    match paths {
        Ok((left, right)) => Command::External(Launch::Diff {
            left,
            right,
            cwd: repo.path().to_path_buf(),
        }),
        Err(e) => Command::Error(e),
    }
}

/// Ctrl+D was pressed (and no text field has the keyboard).
pub fn external_diff_pressed(ctx: &egui::Context) -> bool {
    let typing = ctx.memory(|m| m.focused().is_some());
    let shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::D);
    !typing && ctx.input_mut(|i| i.consume_shortcut(&shortcut))
}

/// Asks where to save a file and writes it.
pub fn save_as(name: &str, bytes: impl FnOnce() -> Result<Vec<u8>, String>, cmds: &mut Commands) {
    let Some(dest) = rfd::FileDialog::new().set_file_name(name).save_file() else {
        return;
    };
    match bytes().and_then(|b| std::fs::write(&dest, b).map_err(|e| e.to_string())) {
        Ok(()) => {}
        Err(e) => cmds.push(Command::Error(t!("Could not save {}: {}", name, e))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_versions_of_a_file() {
        assert_eq!(version_name("src/main.rs", "abc1234"), "main.abc1234.rs");
        assert_eq!(version_name("Makefile", "before"), "Makefile.before");
        assert_eq!(version_name(".gitignore", "x"), ".gitignore.x");
    }
}
