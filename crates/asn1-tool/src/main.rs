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

    let options = asn1_viz::LaunchOptions { icon: load_icon() };

    asn1_viz::launch_with_options(cli.inputs, options).map_err(|e| {
        tracing::error!(error = %e, "eframe exited with error");
        anyhow::anyhow!("visualizer failed: {e}")
    })
}

/// Decode the embedded window icon. Returns `None` if decoding fails — the app
/// still launches, just without a custom taskbar icon.
fn load_icon() -> Option<asn1_viz::Icon> {
    const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");
    match image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            let (width, height) = rgba.dimensions();
            Some(asn1_viz::Icon { rgba: rgba.into_raw(), width, height })
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to decode window icon; continuing without one");
            None
        }
    }
}
