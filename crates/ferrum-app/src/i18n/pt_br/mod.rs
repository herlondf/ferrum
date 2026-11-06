//! Brazilian Portuguese. One table per area of the app, so they can be edited
//! independently. The same English key may appear in several tables, but must
//! then have the same translation (checked by a test).

mod activity;
mod common;
mod details;
mod diff;
mod editing;
mod extras;
mod file_history;
mod flow;
mod history;
mod hosted;
mod local;
mod main;
mod parts;
mod preferences;
mod refs;
mod sidebar;
mod sync;

pub static TABLES: &[&[(&str, &str)]] = &[
    activity::T,
    common::T,
    details::T,
    diff::T,
    main::T,
    parts::T,
    editing::T,
    extras::T,
    file_history::T,
    flow::T,
    history::T,
    hosted::T,
    local::T,
    preferences::T,
    refs::T,
    sidebar::T,
    sync::T,
];
