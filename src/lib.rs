//! Shared application surface for every Opilio entry point.

pub mod action;
pub mod alias;
pub mod app;
pub mod cli;
pub mod config;
pub mod doctor;
pub mod domain;
pub mod history;
pub mod lifecycle;
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
