// Suppress the console window on Windows in release builds. Debug builds keep
// the console so `eprintln!` and `tracing` output is visible when launched
// from a terminal.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![deny(rust_2018_idioms)]

//! Standalone desktop entry point for `asn1-tool`.
//!
//! This binary is the production-grade packaging of [`asn1_viz`]: it loads a
//! window icon, installs a tracing subscriber and panic hook that write to a
//! portable-aware data directory, parses a small clap surface for the initial
//! input paths, and then hands off to [`asn1_viz::launch_with_options`].

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

mod logging;
mod paths;

const APP_ID: &str = "asn1-tool";

#[derive(Parser, Debug)]
#[command(
    name = "asn1-tool",
    version,
    about = "Interactive ASN.1 tree visualizer",
    long_about = None,
)]
struct Cli {
    /// Optional `.asn` files or directories to load at startup. If omitted,
    /// the visualizer opens empty and sources can be imported via File → Open.
    #[arg(value_name = "INPUT")]
    inputs: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let data_dir = paths::data_dir(APP_ID).context("resolving application data directory")?;
    let _log_guard = logging::init(&data_dir);
    logging::install_panic_hook(&data_dir);

    tracing::info!(version = env!("CARGO_PKG_VERSION"), data_dir = %data_dir.display(), "starting asn1-tool");

    let opts =
        asn1_viz::LaunchOptions { icon: None, theme_store_path: Some(data_dir.join("theme.txt")) };
    asn1_viz::launch_with_options(cli.inputs, opts).map_err(|e| {
        tracing::error!(error = %e, "eframe exited with error");
        anyhow::anyhow!("visualizer failed: {e}")
    })
}
