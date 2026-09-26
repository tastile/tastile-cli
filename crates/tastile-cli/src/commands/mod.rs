//! Command implementations. Each subcommand has its own module so that the
//! code stays grep-able and the `cli.rs` surface stays slim.
//!
//! All HTTP work goes through `crate::app::*` so the CLI and TUI share
//! one implementation.

pub mod auth;
pub mod completions;
pub mod doctor;
pub mod executions;
pub mod prompts;
pub mod schedule;
pub mod source_tiles;
pub mod tiles;
pub mod today;
