//! solenv — a Python-venv-like project-local Solana toolchain manager.

pub mod cli;
pub mod commands;
pub mod compatibility;
pub mod config;
pub mod download;
pub mod environment;
pub mod errors;
pub mod managers;
pub mod package_manager;
pub mod platform;
pub mod process;
pub mod version;

/// The embedded compatibility dataset (source of truth for version rules).
pub const DATA_COMPATIBILITY: &str = include_str!("../data/compatibility.toml");
