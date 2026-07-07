//! Config file watcher: hot-reload `config.toml` on change (same as `reload`).

use std::sync::Arc;
use std::time::Duration;

use narvi_core::Config;
use narvi_core::proto::Event;
use notify::Watcher;
use tokio::sync::Mutex;

use crate::state::Daemon;

pub fn spawn(daemon: Arc<Mutex<Daemon>>, cfg_path: std::path::PathBuf) {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let mut watcher =
        match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res
                && (ev.kind.is_modify() || ev.kind.is_create())
            {
                let _ = tx.send(());
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                log::warn!("config watch unavailable: {e}");
                return;
            }
        };
    // Watch the directory: atomic saves replace the file (rename breaks file watches).
    let dir = cfg_path.parent().unwrap_or(&cfg_path).to_path_buf();
    if let Err(e) = watcher.watch(&dir, notify::RecursiveMode::NonRecursive) {
        log::warn!("config watch failed on {}: {e}", dir.display());
        return;
    }

    // Debounce on a plain thread, hop to the runtime per reload.
    let rt = tokio::runtime::Handle::current();
    std::thread::spawn(move || {
        let _keep_alive = watcher;
        while rx.recv().is_ok() {
            // Debounce editor save bursts.
            std::thread::sleep(Duration::from_millis(250));
            while rx.try_recv().is_ok() {}
            let daemon = daemon.clone();
            let path = cfg_path.clone();
            rt.block_on(async move {
                let mut d = daemon.lock().await;
                match Config::load(&path) {
                    Ok(cfg) => {
                        d.cfg = cfg;
                        let _ = d.tx.send(Event::state(d.status()));
                        log::info!("config reloaded ({})", path.display());
                    }
                    Err(e) => log::warn!("config reload failed: {e}"),
                }
            });
        }
    });
}
