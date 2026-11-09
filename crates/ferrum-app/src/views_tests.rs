//! Tests of the history, details, diff and overview polish: the diff toolbar
//! and its Options popover, empty states, relative dates and README images.

use eframe::egui;
use egui_kittest::kittest::Queryable;
use ferrum_core::DiffSource;

use crate::icons;
use crate::tab::View;
use crate::ui_tests::{frames, git, harness, repo};

#[test]
fn diff_toolbar_icons_and_options_popover() {
    let dir = repo();
    let mut h = harness(dir.path());
    let tab = &mut h.state_mut().tabs[0];
    tab.view = View::Changes;
    tab.select_change(DiffSource::Unstaged, "a.txt".into());
    frames(&mut h);

    // Unified / side by side as two icons.
    h.get_by_label(icons::COLUMNS).click();
    frames(&mut h);
    assert!(h.state().settings.diff.side_by_side);
    h.get_by_label(icons::ROWS).click();
    frames(&mut h);
    assert!(!h.state().settings.diff.side_by_side);

    // Every option is in the popover, with its name.
    h.get_by_label(icons::SLIDERS_HORIZONTAL).click();
    frames(&mut h);
    for name in [
        "Side by side",
        "Ignore whitespace",
        "Entire file",
        "Show whitespace",
    ] {
        assert!(h.query_by_label(name).is_some(), "{name} missing");
    }
    h.get_by_label("Show whitespace").click();
    frames(&mut h);
    assert!(h.state().settings.diff.show_whitespace);
    h.get_by_label("A+").click();
    frames(&mut h);
    assert_eq!(h.state().settings.diff.font_size, 13.0);
}

#[test]
fn empty_details_offer_shortcuts() {
    let dir = repo();
    let mut h = harness(dir.path());
    h.state_mut().tabs[0].view = View::History;
    frames(&mut h);
    assert!(h.query_by_label("No commit selected").is_some());
    assert!(h.state().tabs[0].history.ext.goto.is_none());
    h.get_by_label("Go to commit").click();
    frames(&mut h);
    assert!(h.state().tabs[0].history.ext.goto.is_some());
}

#[test]
fn no_matches_offer_to_clear_the_search() {
    let dir = repo();
    let mut h = harness(dir.path());
    let tab = &mut h.state_mut().tabs[0];
    tab.view = View::History;
    tab.history.search = "no such commit".into();
    tab.apply_search();
    frames(&mut h);
    assert!(h.query_by_label("No commits match the search.").is_some());
    h.get_by_label("Clear search").click();
    frames(&mut h);
    let tab = &h.state().tabs[0];
    assert!(tab.history.search.is_empty());
    assert!(tab.history.visible.is_none());
}

#[test]
fn relative_dates_option_in_the_view_menu() {
    let dir = repo();
    let mut h = harness(dir.path());
    h.state_mut().tabs[0].view = View::History;
    frames(&mut h);
    h.query_all_by_label("View").last().unwrap().click();
    frames(&mut h);
    h.get_by_label("Relative dates (3 h ago)").click();
    frames(&mut h);
    assert!(h.state().settings.history.relative_dates);
}

#[test]
fn readme_images_load_from_the_repository() {
    let dir = repo();
    let p = dir.path();
    std::fs::create_dir_all(p.join("docs/img")).unwrap();
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(3, 2, image::Rgba([200, 80, 40, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    std::fs::write(p.join("docs/img/shot.png"), &png).unwrap();
    git(p, &["add", "-A"]);

    let ctx = egui::Context::default();
    crate::ui::readme_images::install(&ctx);
    let prefix = crate::ui::readme_images::uri_prefix(p, "docs");
    let load = |link: &str| ctx.try_load_image(&format!("{prefix}{link}"), Default::default());
    match load("img/shot.png") {
        Ok(egui::load::ImagePoll::Ready { image }) => assert_eq!(image.size, [3, 2]),
        other => panic!("image not loaded: {:?}", other.err()),
    }
    // From the repository root, and a missing file.
    assert!(load("/docs/img/shot.png").is_ok());
    assert!(load("img/missing.png").is_err());

    // The overview shows the README with the image.
    std::fs::write(
        p.join("README.md"),
        "# Demo\n\n![shot](docs/img/shot.png)\n",
    )
    .unwrap();
    let mut h = harness(p);
    crate::ui::overview::open(&mut h.state_mut().tabs[0]);
    frames(&mut h);
    assert!(h.query_by_label("Demo").is_some());
}
