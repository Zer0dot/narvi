//! Daemon socket location. Shared with clients via the well-known path.

use std::path::PathBuf;

use anyhow::{Context, Result};

/// `$XDG_RUNTIME_DIR/narvi/narvid.sock`. Created (parent dir) by the daemon on start.
pub fn socket_path() -> Result<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .context("XDG_RUNTIME_DIR unset; cannot locate the daemon socket")?;
    Ok(PathBuf::from(runtime).join("narvi").join("narvid.sock"))
}
