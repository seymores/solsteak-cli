//! SolSteak's reusable application core.
//!
//! Domain, storage, provider, and UI code will be added by their assigned beads.

#![forbid(unsafe_code)]

pub mod app;
pub mod cli;
pub mod domain;
pub mod helius;
pub mod inspection;
pub mod rewards;
pub mod storage;
pub mod tui;
pub mod validators;
