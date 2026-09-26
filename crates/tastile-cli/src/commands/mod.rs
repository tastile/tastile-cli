//! Command implementations. Each subcommand has its own module so that the
//! code stays grep-able and the `cli.rs` surface stays slim.

pub mod auth;
pub mod completions;
pub mod doctor;
pub mod prompts;
pub mod schedule;
pub mod source_tiles;
pub mod tiles;
pub mod today;
