//! Brazilian Portuguese. One table per area of the app, so they can be edited
//! independently. The same English key may appear in several tables, but must
//! then have the same translation (checked by a test).

mod activity;
mod common;
mod diff;
mod editing;
mod extras;
mod file_history;
mod history;
mod local;
mod main;
mod preferences;
mod sidebar;

pub static TABLES: &[&[(&str, &str)]] = &[
    activity::T,
    common::T,
    diff::T,
    main::T,
    editing::T,
    extras::T,
    file_history::T,
    history::T,
    local::T,
    preferences::T,
    sidebar::T,
];
