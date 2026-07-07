//! Hyprland socket2 listener: per-app auto-switch on `activewindow` events.
//!
//! Matches window class against profile `match` globs; first match wins.
//! Focusing a non-matching window reverts to the pre-switch state.

use std::path::PathBuf;
use std::sync::Arc;

use globset::Glob;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::Mutex;

use crate::state::Daemon;

/// `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock`.
/// Falls back to the newest instance dir when HIS is unset (systemd race).
fn socket2_path() -> Option<PathBuf> {
    let runtime = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?);
    let hypr = runtime.join("hypr");
    let dir = match std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE") {
        Some(his) => hypr.join(his),
        None => std::fs::read_dir(&hypr)
            .ok()?
            .flatten()
            .filter(|e| e.path().is_dir())
            .max_by_key(|e| {
                e.metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH)
            })?
            .path(),
    };
    Some(dir.join(".socket2.sock"))
}

/// First profile whose `match` globs cover `class`.
fn matching_profile(d: &Daemon, class: &str) -> Option<String> {
    for p in &d.cfg.profiles {
        for pat in &p.r#match {
            if let Ok(glob) = Glob::new(pat)
                && glob.compile_matcher().is_match(class)
            {
                return Some(p.name.clone());
            }
        }
    }
    None
}

async fn on_focus(daemon: &Arc<Mutex<Daemon>>, class: &str) {
    let mut d = daemon.lock().await;
    match (matching_profile(&d, class), d.auto_class.clone()) {
        (Some(name), auto) => {
            if d.active_profile.as_deref() != Some(name.as_str()) {
                // Remember what to revert to on the first auto activation.
                if auto.is_none() {
                    d.revert = Some((d.params, d.active_profile.clone()));
                }
                if d.load_profile(&name, Some(class.to_string())).is_ok() {
                    log::info!("auto-switch: {class} -> {name}");
                    if let Err(e) = d.apply().await {
                        log::warn!("auto-switch apply failed: {e:#}");
                    }
                }
            } else {
                d.auto_class = Some(class.to_string());
            }
        }
        (None, Some(_)) => {
            let (params, profile) = d.revert.take().unwrap_or_default();
            d.params = params;
            d.active_profile = profile;
            d.auto_class = None;
            log::info!("auto-switch: revert ({class})");
            if let Err(e) = d.apply().await {
                log::warn!("auto-switch revert failed: {e:#}");
            }
        }
        (None, None) => {}
    }
}

pub fn spawn(daemon: Arc<Mutex<Daemon>>) {
    tokio::spawn(async move {
        loop {
            let Some(path) = socket2_path() else {
                log::error!("no Hyprland instance found; auto-switch idle");
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                continue;
            };
            let stream = match UnixStream::connect(&path).await {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("socket2 connect failed ({}): {e}", path.display());
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    continue;
                }
            };
            log::info!("listening for focus events on {}", path.display());
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                // `activewindow>>CLASS,TITLE` — title may itself contain commas.
                if let Some(data) = line.strip_prefix("activewindow>>") {
                    let class = data.split_once(',').map_or(data, |(c, _)| c);
                    on_focus(&daemon, class).await;
                }
            }
            log::warn!("socket2 stream ended; reconnecting");
        }
    });
}
