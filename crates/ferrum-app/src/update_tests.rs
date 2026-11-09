//! UI tests of the "Update Ferrum" window.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::app::FerrumApp;
use crate::command::{Command, DevStep};
use crate::icons;
use crate::ui_tests::{frames, harness, repo};

fn handle(h: &mut Harness<'_, FerrumApp>, c: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().handle(c, &ctx);
    frames(h);
}

#[test]
fn the_update_button_opens_the_window() {
    let dir = repo();
    let mut h = harness(dir.path());
    frames(&mut h);
    assert!(h.query_by_label("Update Ferrum").is_none());
    let v = ("9.9.9".to_owned(), "https://example.com".to_owned());
    h.state_mut().settings.background.update_available = Some(v);
    frames(&mut h);
    h.get_by_label(&icons::text(icons::ARROW_CIRCLE_UP, "Update"))
        .click();
    frames(&mut h);
    h.get_by_label("Update Ferrum");
    h.get_by_label("Version 9.9.9 is available");
    h.get_by_label("Release notes");
    h.get_by_label("Close").click();
    frames(&mut h);
    assert!(h.query_by_label("Update Ferrum").is_none());
}

#[test]
fn self_updating_formats_offer_to_install() {
    let dir = repo();
    let mut h = harness(dir.path());
    handle(&mut h, Command::DevStep(DevStep::Update("appimage".into())));
    h.get_by_label("Install and restart");
}

#[test]
fn other_formats_say_how_to_update() {
    let dir = repo();
    let mut h = harness(dir.path());
    handle(&mut h, Command::DevStep(DevStep::Update("script".into())));
    assert!(h.query_by_label("Install and restart").is_none());
    h.get_by_label(ferrum_core::self_update::script_command());
    h.get_by_label("Copy command");

    handle(&mut h, Command::DevStep(DevStep::Update("flatpak".into())));
    h.get_by_label("This copy is updated by Flatpak.");
}
