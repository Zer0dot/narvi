//! Daemon socket location. Shared with clients via the well-known path.

use std::path::PathBuf;

use anyhow::{Context, Result};

/// `$NARVI_SOCKET` override, else `$XDG_RUNTIME_DIR/narvi/narvid.sock`.
pub fn socket_path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("NARVI_SOCKET") {
        return Ok(PathBuf::from(p));
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .context("XDG_RUNTIME_DIR unset; cannot locate the daemon socket")?;
    Ok(PathBuf::from(runtime).join("narvi").join("narvid.sock"))
}
