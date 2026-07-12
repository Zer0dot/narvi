//! Daemon link: a command thread (fire commands) + a subscribe thread (events in).
//! Both reconnect with backoff so the GUI survives daemon restarts; the
//! subscribe thread also auto-spawns `narvid` when it is unreachable.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use narvi_core::proto::{Command, Status};
use narvi_core::spawn::{DaemonSpawner, SPAWN_COOLDOWN, SpawnStatus};
use narvi_core::{Client, socket_path};

#[derive(Default)]
pub struct Shared {
    pub status: Option<Status>,
    pub profiles: Vec<String>,
    pub error: Option<String>,
    /// Bumped on every daemon-side update; the app syncs sliders when it moves.
    pub stamp: u64,
}

pub struct Conn {
    tx: mpsc::Sender<Command>,
    pub shared: Arc<Mutex<Shared>>,
}

impl Conn {
    pub fn spawn(ctx: eframe::egui::Context) -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (tx, rx) = mpsc::channel::<Command>();

        let s = shared.clone();
        let c = ctx.clone();
        std::thread::spawn(move || subscribe_loop(s, c));

        let s = shared.clone();
        std::thread::spawn(move || command_loop(rx, s, ctx));

        Self { tx, shared }
    }

    /// Fire-and-forget; results come back via the subscribe stream.
    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }
}

fn set_error(shared: &Arc<Mutex<Shared>>, err: Option<String>) {
    if let Ok(mut s) = shared.lock() {
        s.error = err;
        s.stamp += 1;
    }
}

fn command_loop(
    rx: mpsc::Receiver<Command>,
    shared: Arc<Mutex<Shared>>,
    ctx: eframe::egui::Context,
) {
    let mut client: Option<Client> = None;
    while let Ok(cmd) = rx.recv() {
        if client.is_none() {
            client = socket_path().ok().and_then(|p| Client::connect(&p).ok());
        }
        let Some(c) = client.as_mut() else {
            set_error(&shared, Some("daemon unreachable".into()));
            ctx.request_repaint();
            continue;
        };
        match c.request(cmd) {
            Ok(_) => {}
            Err(narvi_core::Error::Daemon(e)) => {
                set_error(&shared, Some(e));
                ctx.request_repaint();
            }
            Err(_) => client = None, // connection broke; retry on next command
        }
    }
}

/// User-facing message for an unreachable daemon, per spawner status.
fn spawn_message(status: SpawnStatus) -> &'static str {
    match status {
        SpawnStatus::Starting | SpawnStatus::Scheduled => "starting daemon...",
        SpawnStatus::GaveUp => "daemon keeps failing — run narvid manually",
        SpawnStatus::Disabled => "daemon unreachable — start narvid",
    }
}

fn subscribe_loop(shared: Arc<Mutex<Shared>>, ctx: eframe::egui::Context) {
    let mut spawner = DaemonSpawner::new("narvid", SPAWN_COOLDOWN);
    loop {
        let client = socket_path().ok().and_then(|p| Client::connect(&p).ok());
        let Some(mut client) = client else {
            // Unreachable daemon: auto-start it (graced + rate-limited).
            set_error(&shared, Some(spawn_message(spawner.tick()).into()));
            ctx.request_repaint();
            std::thread::sleep(Duration::from_secs(2));
            continue;
        };
        spawner.reset(); // fresh grace for the next outage

        let update = |shared: &Arc<Mutex<Shared>>, st: Status, profiles: Option<Vec<String>>| {
            if let Ok(mut s) = shared.lock() {
                s.status = Some(st);
                if let Some(p) = profiles {
                    s.profiles = p;
                }
                s.error = None;
                s.stamp += 1;
            }
            ctx.request_repaint();
        };

        let init = client.request(Command::Subscribe).and_then(|d| {
            let st: Status = serde_json::from_value(d)?;
            let names = client.request(Command::ProfileList)?;
            Ok((st, serde_json::from_value::<Vec<String>>(names)?))
        });
        match init {
            Ok((st, names)) => update(&shared, st, Some(names)),
            Err(_) => {
                // Connected but got no answer (request timeout / hiccup):
                // surface it instead of leaving a stale "connected" UI.
                set_error(&shared, Some("daemon not responding".into()));
                ctx.request_repaint();
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
        }

        // On stream error, fall through and reconnect.
        while let Ok(ev) = client.next_event() {
            // Profile set may have changed (save/delete); refresh the names.
            let names = client
                .request(Command::ProfileList)
                .ok()
                .and_then(|v| serde_json::from_value(v).ok());
            update(&shared, ev.data, names);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_message_covers_every_status() {
        assert_eq!(spawn_message(SpawnStatus::Starting), "starting daemon...");
        assert_eq!(spawn_message(SpawnStatus::Scheduled), "starting daemon...");
        assert_eq!(
            spawn_message(SpawnStatus::GaveUp),
            "daemon keeps failing — run narvid manually"
        );
        assert_eq!(
            spawn_message(SpawnStatus::Disabled),
            "daemon unreachable — start narvid"
        );
    }
}
