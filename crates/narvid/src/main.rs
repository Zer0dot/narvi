//! `narvid` — the Narvi daemon. Single source of truth for color state.
//!
//! M2: listen on the Unix socket, apply shader on change, persist config, push events.
//! M7: sun-based scheduler + Hyprland socket2 auto-switch.

mod socket;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    eprintln!(
        "narvid {} — not yet implemented (M2)",
        env!("CARGO_PKG_VERSION")
    );
    let _ = socket::socket_path()?;
    Ok(())
}
