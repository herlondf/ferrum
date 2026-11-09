//! Brazilian Portuguese. One table per area of the app, so they can be edited
//! independently. The same English key may appear in several tables, but must
//! then have the same translation (checked by a test).

mod activity;
mod ai;
mod commands;
mod common;
mod details;
mod diff;
mod editing;
mod extras;
mod file_history;
mod flow;
mod fork_tools;
mod history;
mod hosted;
mod local;
mod main;
mod main_menu;
mod parts;
mod preferences;
mod refs;
mod repos;
mod safety;
mod sidebar;
mod stacks;
mod structural;
mod sync;
mod toolbar;
mod ux;
mod views;
mod visual;

pub static TABLES: &[&[(&str, &str)]] = &[
    activity::T,
    ai::T,
    commands::T,
    common::T,
    details::T,
    diff::T,
    main::T,
    main_menu::T,
    parts::T,
    editing::T,
    extras::T,
    file_history::T,
    flow::T,
    fork_tools::T,
    history::T,
    hosted::T,
    local::T,
    preferences::T,
    refs::T,
    repos::T,
    safety::T,
    sidebar::T,
    stacks::T,
    sync::T,
    toolbar::T,
    views::T,
    ux::T,
    structural::T,
    visual::T,
];
