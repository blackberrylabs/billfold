//! Billfold, core: what this machine spends on AI — the accounts it is wired to, the keys
//! it holds, the tokens it burned — and what it finds. No UI, no network.
//!
//! The Blackberrylabs promises, the same for every tool: read-only (nothing written outside
//! `~/.config/billfold/`, and only by the app's preferences); no network (the price table
//! is a snapshot embedded at build time); secrets masked the moment they are read and never
//! written; actions shown, never run; every rule sourced.
//!
//! `tests/promises.rs` checks that the source of this crate never mentions a write or a
//! network API, that a planted key never appears in the output, and that `report` conforms
//! to `bbl-report/1`.

#![forbid(unsafe_code)]

pub mod gitstat;
pub mod keys;
pub mod meter;
pub mod model;
pub mod prices;
pub mod providers;
pub mod report;
pub mod scan;
pub mod sheets;
pub mod util;
pub mod wiring;

pub use model::{Finding, Inventory, Kind};
pub use scan::{scan, scan_with, Options};
