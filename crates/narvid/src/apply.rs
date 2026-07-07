//! Shader file write (atomic) + `hyprctl keyword` apply.
//!
//! Hyprland compiles the shader when the keyword is SET, not on file change,
//! so every write is followed by a keyword re-issue.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

/// Literal token that clears `decoration:screen_shader`.
pub const EMPTY_SHADER: &str = "[[EMPTY]]";

/// Atomic write: `.tmp` + fsync + rename, so Hyprland never reads a partial file.
pub fn write_shader(path: &Path, text: &str) -> Result<()> {
    let dir = path.parent().context("shader path has no parent dir")?;
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension("frag.tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Set `decoration:screen_shader` to `arg` (a path or [`EMPTY_SHADER`]).
pub async fn set_screen_shader(arg: &str) -> Result<()> {
    let out = tokio::process::Command::new("hyprctl")
        .args(["keyword", "decoration:screen_shader", arg])
        .output()
        .await
        .context("failed to run hyprctl")?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() || stdout.trim() != "ok" {
        anyhow::bail!(
            "hyprctl keyword failed: {} {}",
            stdout.trim(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}
