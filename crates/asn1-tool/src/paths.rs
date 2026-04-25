//! Resolve where the application stores runtime state (logs, crash dumps).
//!
//! Two modes:
//!   * **Portable** — if a file named `portable.txt` sits next to the
//!     executable, state lives under `<exe-dir>/data/`. This makes the whole
//!     distribution self-contained: copy the folder anywhere, including
//!     removable media, and no host configuration is touched.
//!   * **System** — otherwise, state lives under the OS-specific data dir:
//!       - Windows: `%LOCALAPPDATA%\<app_id>\`
//!       - Linux:   `$XDG_DATA_HOME/<app_id>/` (usually
//!         `~/.local/share/<app_id>/`)
//!       - macOS:   `~/Library/Application Support/<app_id>/`

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};

/// Return the directory to use for application state, creating it if needed.
pub(crate) fn data_dir(app_id: &str) -> Result<PathBuf> {
    let dir =
        if let Some(portable) = portable_dir()? { portable } else { system_data_dir(app_id)? };
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating data directory at {}", dir.display()))?;
    Ok(dir)
}

fn portable_dir() -> Result<Option<PathBuf>> {
    let exe = std::env::current_exe().context("locating current executable")?;
    let Some(exe_dir) = exe.parent() else {
        return Ok(None);
    };
    if exe_dir.join("portable.txt").is_file() {
        Ok(Some(exe_dir.join("data")))
    } else {
        Ok(None)
    }
}

fn system_data_dir(app_id: &str) -> Result<PathBuf> {
    let base = dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .ok_or_else(|| anyhow!("no OS data directory available; enable portable mode instead"))?;
    Ok(base.join(app_id))
}
