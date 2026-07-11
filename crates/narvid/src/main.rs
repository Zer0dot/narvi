//! `narvid` — the Narvi daemon. Single source of truth for color state.

mod apply;
mod hypr;
mod sched;
mod server;
mod state;
mod watch;

use std::sync::Arc;

use anyhow::{Context, Result};
use narvi_core::Config;
use tokio::sync::Mutex;

use state::Daemon;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Single-instance guard first: a losing duplicate must exit before it
    // touches the shader or config. Exit 0 — desired state already holds
    // (auto-spawners race systemd here; a failure exit would trip Restart=).
    let sock = narvi_core::socket_path()?;
    let Some(_lock) = server::try_lock(&sock.with_extension("lock"))? else {
        log::info!("narvid already running; exiting");
        return Ok(());
    };
    // Bind early so clients can connect (and stop auto-spawning) while the
    // initial config load / apply below is still running.
    let listener = server::bind(&sock).await?;
    log::info!("listening on {}", sock.display());

    let cfg_path = match std::env::var_os("NARVI_CONFIG") {
        Some(p) => p.into(),
        None => Config::default_path()?,
    };
    let cfg = if cfg_path.exists() {
        Config::load(&cfg_path).with_context(|| format!("load {}", cfg_path.display()))?
    } else {
        let cfg = Config {
            profiles: narvi_core::profile::builtin_presets(),
            ..Default::default()
        };
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

    let cfg_path = daemon.cfg_path.clone();
    let daemon = Arc::new(Mutex::new(daemon));
    sched::spawn(daemon.clone());
    hypr::spawn(daemon.clone());
    watch::spawn(daemon.clone(), cfg_path);

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
