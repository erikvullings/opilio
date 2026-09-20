//! Shared application surface for every Opilio entry point.

pub mod app;
pub mod cli;
pub mod config;
pub mod domain;
pub mod history;
pub mod power;
pub mod scheduler;
pub mod service;
pub mod ssh;
pub mod status;
pub mod target;
pub mod telemetry;
pub mod transfer;
pub mod tui;

pub use app::run;
