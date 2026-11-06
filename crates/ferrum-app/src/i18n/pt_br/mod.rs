//! Brazilian Portuguese. One table per area of the app, so they can be edited
//! independently. The same English key may appear in several tables, but must
//! then have the same translation (checked by a test).

mod common;
mod diff;
mod editing;
mod extras;
mod main;

pub static TABLES: &[&[(&str, &str)]] = &[common::T, main::T, editing::T, extras::T, diff::T];
