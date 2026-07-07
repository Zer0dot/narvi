//! `narvid` — the Narvi daemon. Single source of truth for color state.

mod apply;
mod server;
mod socket;
mod state;

use std::sync::Arc;

use anyhow::{Context, Result};
use narvi_core::Config;
use tokio::sync::Mutex;

use state::Daemon;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cfg_path = match std::env::var_os("NARVI_CONFIG") {
        Some(p) => p.into(),
        None => Config::default_path()?,
    };
    let cfg = if cfg_path.exists() {
        Config::load(&cfg_path).with_context(|| format!("load {}", cfg_path.display()))?
    } else {
        let mut cfg = Config::default();
        cfg.profiles = narvi_core::profile::builtin_presets();
        cfg.save(&cfg_path)
            .with_context(|| format!("seed {}", cfg_path.display()))?;
        log::info!("seeded default config at {}", cfg_path.display());
        cfg
    };

    let mut daemon = Daemon::new(cfg, cfg_path);
    daemon.restore();
    if let Err(e) = daemon.apply().await {
        log::warn!("initial apply failed (no Hyprland session?): {e:#}");
    }
    log::info!(
        "narvid {} up — profile={:?} enabled={}",
        env!("CARGO_PKG_VERSION"),
        daemon.active_profile,
        daemon.enabled
    );

    let sock = socket::socket_path()?;
    let listener = server::bind(&sock).await?;
    log::info!("listening on {}", sock.display());
    let daemon = Arc::new(Mutex::new(daemon));

    tokio::select! {
        _ = server::serve(listener, daemon.clone()) => {}
        _ = shutdown() => {
            log::info!("shutting down");
            let _ = std::fs::remove_file(&sock);
        }
    }
    Ok(())
}

async fn shutdown() {
    let ctrl_c = tokio::signal::ctrl_c();
    let mut term = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("SIGTERM handler failed: {e}");
            let _ = ctrl_c.await;
            return;
        }
    };
    tokio::select! {
        _ = ctrl_c => {}
        _ = term.recv() => {}
    }
}
