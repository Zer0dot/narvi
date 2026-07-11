//! `narvi-tray` — StatusNotifierItem. Click toggles the GUI; menu = presets + Quit.

use std::process::{Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use ksni::TrayMethods;
use ksni::menu::{CheckmarkItem, MenuItem, StandardItem};
use narvi_core::proto::Command;
use narvi_core::{Client, socket_path};

struct NarviTray {
    profiles: Vec<String>,
    active: Option<String>,
    enabled: bool,
    cmd: mpsc::Sender<Command>,
    /// Our spawned GUI process; kept so exits are reaped (no zombies).
    gui_child: Option<std::process::Child>,
}

/// Bound on hyprctl/pgrep/pkill; a stalled compositor must not wedge the tray.
const HELPER_TIMEOUT: Duration = Duration::from_secs(2);
/// Grace period after SIGTERM before escalating to SIGKILL.
const TERM_GRACE: Duration = Duration::from_secs(1);

/// What a tray click should do to the GUI.
#[derive(Debug, PartialEq, Eq)]
enum GuiAction {
    /// Graceful `hyprctl dispatch closewindow`.
    CloseWindow,
    /// SIGTERM our own spawned child (window not mapped yet).
    TermChild,
    /// SIGTERM any narvi-gui process (ours or external).
    TermAny,
    Spawn,
}

/// Decide from window presence, our child's liveness, and whether any
/// narvi-gui process runs (catches external launches and startup races).
fn gui_action(window: Option<bool>, child_alive: bool, gui_process: bool) -> GuiAction {
    match window {
        Some(true) => GuiAction::CloseWindow,
        // Window not mapped yet but a GUI is starting: close it, never respawn.
        Some(false) if child_alive => GuiAction::TermChild,
        Some(false) if gui_process => GuiAction::TermAny,
        Some(false) => GuiAction::Spawn,
        None if child_alive || gui_process => GuiAction::TermAny,
        None => GuiAction::Spawn,
    }
}

/// Run `cmd`, killing it past `timeout`; None on spawn/timeout/IO error.
fn output_with_timeout(mut cmd: std::process::Command, timeout: Duration) -> Option<Output> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let child = cmd.spawn().ok()?;
    let pid = child.id() as i32;
    let (tx, rx) = mpsc::channel();
    // Waiter thread owns the child; it reaps even after a timeout kill.
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(timeout) {
        Ok(res) => res.ok(),
        Err(_) => {
            // Kill by pid: child is owned by the waiter, which then reaps it.
            unsafe { libc::kill(pid, libc::SIGKILL) };
            None
        }
    }
}

/// SIGTERM `child`; escalate to SIGKILL after TERM_GRACE. Always reaps.
fn term_and_reap(mut child: std::process::Child) {
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let deadline = std::time::Instant::now() + TERM_GRACE;
    while std::time::Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Parse clients JSON: Some(narvi-gui window present); None if malformed.
fn has_gui_window(clients_json: &str) -> Option<bool> {
    let v: serde_json::Value = serde_json::from_str(clients_json).ok()?;
    let clients = v.as_array()?;
    Some(
        clients
            .iter()
            .any(|c| c.get("class").and_then(serde_json::Value::as_str) == Some("narvi-gui")),
    )
}

/// Ask Hyprland whether a narvi-gui window exists; None if hyprctl
/// fails, hangs, or returns malformed output.
fn query_gui_window() -> Option<bool> {
    let mut cmd = std::process::Command::new("hyprctl");
    cmd.args(["-j", "clients"]);
    let out = output_with_timeout(cmd, HELPER_TIMEOUT)?;
    if !out.status.success() {
        return None;
    }
    has_gui_window(&String::from_utf8_lossy(&out.stdout))
}

/// Is any narvi-gui process running (ours or launched externally)?
fn query_gui_process() -> bool {
    let mut cmd = std::process::Command::new("pgrep");
    cmd.args(["-x", "narvi-gui"]);
    output_with_timeout(cmd, HELPER_TIMEOUT).is_some_and(|o| o.status.success())
}

impl NarviTray {
    fn send(&self, c: Command) {
        log::debug!("tray action: {c:?}");
        if self.cmd.send(c).is_err() {
            log::warn!("command worker gone");
        }
    }

    /// Reap-aware liveness of our spawned GUI child; drops finished handles.
    fn gui_child_alive(&mut self) -> bool {
        match self.gui_child.as_mut().map(std::process::Child::try_wait) {
            Some(Ok(None)) => true,
            Some(Ok(Some(_))) => {
                self.gui_child = None;
                false
            }
            Some(Err(e)) => {
                log::warn!("gui child wait failed: {e}");
                self.gui_child = None;
                false
            }
            None => false,
        }
    }

    /// SIGTERM + reap our spawned GUI child, if any.
    fn term_gui_child(&mut self) {
        if let Some(c) = self.gui_child.take() {
            term_and_reap(c);
        }
    }

    /// Graceful close via Hyprland; reap our child off-thread once it exits.
    fn close_gui_window(&mut self) {
        let mut cmd = std::process::Command::new("hyprctl");
        cmd.args(["dispatch", "closewindow", "class:^(narvi-gui)$"]);
        match output_with_timeout(cmd, HELPER_TIMEOUT) {
            Some(o) if o.status.success() => {
                if let Some(mut c) = self.gui_child.take() {
                    // Child exits soon after its window closes; reap it then.
                    std::thread::spawn(move || {
                        let _ = c.wait();
                    });
                }
            }
            Some(o) => log::warn!("gui close failed: {}", o.status),
            None => log::warn!("gui close failed: hyprctl error or timeout"),
        }
    }

    /// Show the GUI if none is open or starting, otherwise close it.
    fn toggle_gui(&mut self) {
        let window = query_gui_window();
        let alive = self.gui_child_alive();
        // pgrep only when the window can't already answer the toggle.
        let gui_process = window != Some(true) && query_gui_process();
        match gui_action(window, alive, gui_process) {
            GuiAction::CloseWindow => self.close_gui_window(),
            GuiAction::TermChild => self.term_gui_child(),
            GuiAction::TermAny => {
                // pkill sends SIGTERM to every narvi-gui, our child included.
                let mut cmd = std::process::Command::new("pkill");
                cmd.args(["-x", "narvi-gui"]);
                if output_with_timeout(cmd, HELPER_TIMEOUT).is_none() {
                    log::warn!("pkill narvi-gui failed");
                }
                self.term_gui_child();
            }
            GuiAction::Spawn => match std::process::Command::new("narvi-gui").spawn() {
                Ok(child) => self.gui_child = Some(child),
                Err(e) => log::warn!("gui spawn failed: {e}"),
            },
        }
    }
}

impl ksni::Tray for NarviTray {
    fn id(&self) -> String {
        "narvi".into()
    }

    fn title(&self) -> String {
        "Narvi".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![icon(self.enabled)]
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.toggle_gui();
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = self
            .profiles
            .iter()
            .map(|name| {
                let n = name.clone();
                CheckmarkItem {
                    label: name.clone(),
                    checked: self.active.as_deref() == Some(name),
                    activate: Box::new(move |t: &mut Self| {
                        t.send(Command::ProfileLoad { name: n.clone() });
                    }),
                    ..Default::default()
                }
                .into()
            })
            .collect();
        items.push(MenuItem::Separator);
        items.push(
            CheckmarkItem {
                label: "Enabled".into(),
                checked: self.enabled,
                activate: Box::new(|t: &mut Self| t.send(Command::Toggle)),
                ..Default::default()
            }
            .into(),
        );
        items.push(
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|_: &mut Self| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        );
        items
    }
}

/// 22x22 ARGB planet-with-ring, gold; dimmed when the shader is off.
fn icon(enabled: bool) -> ksni::Icon {
    const S: i32 = 22;
    let (r, g, b) = if enabled {
        (0xE0u8, 0xA2u8, 0x3Cu8)
    } else {
        (0x5C, 0x66, 0x78)
    };
    let mut data = vec![0u8; (S * S * 4) as usize];
    let mut put = |x: i32, y: i32, a: u8| {
        if (0..S).contains(&x) && (0..S).contains(&y) {
            let i = ((y * S + x) * 4) as usize;
            if data[i] < a {
                data[i] = a;
                data[i + 1] = r;
                data[i + 2] = g;
                data[i + 3] = b;
            }
        }
    };
    let c = (S as f32 - 1.0) / 2.0;
    for y in 0..S {
        for x in 0..S {
            let (dx, dy) = (x as f32 - c, y as f32 - c);
            // Planet disc.
            let d = (dx * dx + dy * dy).sqrt();
            if d < 5.2 {
                put(x, y, 255);
            } else if d < 6.0 {
                put(x, y, ((6.0 - d) / 0.8 * 255.0) as u8); // soft edge
            }
            // Tilted ring (ellipse band).
            let (rx, ry) = (dx * 0.94 + dy * 0.34, -dx * 0.34 + dy * 0.94);
            let e = (rx / 10.0).powi(2) + (ry / 3.4).powi(2);
            if (0.72..=1.0).contains(&e) {
                put(x, y, 210);
            }
        }
    }
    ksni::Icon {
        width: S,
        height: S,
        data,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();

    // Command worker: forwards menu actions to the daemon, reconnecting as needed.
    std::thread::spawn(move || {
        let mut client: Option<Client> = None;
        while let Ok(cmd) = cmd_rx.recv() {
            if client.is_none() {
                client = socket_path().ok().and_then(|p| Client::connect(&p).ok());
            }
            match client.as_mut() {
                Some(c) => {
                    if let Err(e) = c.request(cmd) {
                        log::warn!("daemon request failed: {e}");
                        client = None;
                    }
                }
                None => log::warn!("daemon unreachable; action dropped"),
            }
        }
    });

    let tray = NarviTray {
        profiles: Vec::new(),
        active: None,
        enabled: true,
        cmd: cmd_tx,
        gui_child: None,
    };
    let handle = tray.spawn().await?;

    // Subscribe loop: mirror daemon state into the tray (menu checkmarks, icon).
    let rt = tokio::runtime::Handle::current();
    let sub = std::thread::spawn(move || {
        loop {
            let Ok(path) = socket_path() else { return };
            let Ok(mut client) = Client::connect(&path) else {
                std::thread::sleep(std::time::Duration::from_secs(2));
                continue;
            };
            // Only touch the tray when menu-relevant state changes; every
            // update rebuilds the dbusmenu (param streams would thrash it).
            let mut last: Option<(Option<String>, bool, Vec<String>)> = None;
            let mut apply = |st: narvi_core::proto::Status, profiles: Option<Vec<String>>| {
                let profiles = profiles
                    .or_else(|| last.as_ref().map(|l| l.2.clone()))
                    .unwrap_or_default();
                let snap = (st.active_profile.clone(), st.enabled, profiles);
                if last.as_ref() == Some(&snap) {
                    return Some(());
                }
                last = Some(snap.clone());
                rt.block_on(handle.update(|t: &mut NarviTray| {
                    (t.active, t.enabled, t.profiles) = snap;
                }))
                .map(|_| ())
            };
            let init = client.request(Command::Subscribe).and_then(|d| {
                let st = serde_json::from_value(d)?;
                let names = client.request(Command::ProfileList)?;
                Ok((st, serde_json::from_value::<Vec<String>>(names)?))
            });
            match init {
                Ok((st, names)) => {
                    if apply(st, Some(names)).is_none() {
                        return; // tray shut down
                    }
                }
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    continue;
                }
            }
            while let Ok(ev) = client.next_event() {
                let names = client
                    .request(Command::ProfileList)
                    .ok()
                    .and_then(|v| serde_json::from_value(v).ok());
                if apply(ev.data, names).is_none() {
                    return;
                }
            }
        }
    });

    // Exits with the process on Quit; otherwise idles with the subscriber.
    let _ = tokio::task::spawn_blocking(move || sub.join()).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_present() {
        let json = r#"[{"class":"firefox","title":"x"},{"class":"narvi-gui","title":"narvi-gui"}]"#;
        assert_eq!(has_gui_window(json), Some(true));
    }

    #[test]
    fn window_absent() {
        assert_eq!(has_gui_window(r#"[{"class":"firefox"}]"#), Some(false));
        assert_eq!(has_gui_window("[]"), Some(false));
        // Missing/non-string class fields are skipped, not matched.
        assert_eq!(
            has_gui_window(r#"[{"title":"x"},{"class":42}]"#),
            Some(false)
        );
    }

    #[test]
    fn window_malformed() {
        assert_eq!(has_gui_window("not json"), None);
        assert_eq!(has_gui_window(r#"{"class":"narvi-gui"}"#), None); // not an array
        assert_eq!(has_gui_window(""), None);
    }

    #[test]
    fn action_window_present_closes_gracefully() {
        assert_eq!(gui_action(Some(true), false, false), GuiAction::CloseWindow);
        assert_eq!(gui_action(Some(true), true, true), GuiAction::CloseWindow);
    }

    #[test]
    fn action_startup_race_closes_instead_of_respawning() {
        // Child spawned but window not mapped yet: a second click must
        // terminate the launching GUI, never kill-and-respawn it.
        assert_eq!(gui_action(Some(false), true, true), GuiAction::TermChild);
        assert_eq!(gui_action(Some(false), true, false), GuiAction::TermChild);
        // Same race for an externally launched GUI.
        assert_eq!(gui_action(Some(false), false, true), GuiAction::TermAny);
    }

    #[test]
    fn action_spawns_only_when_nothing_runs() {
        assert_eq!(gui_action(Some(false), false, false), GuiAction::Spawn);
        assert_eq!(gui_action(None, false, false), GuiAction::Spawn);
    }

    #[test]
    fn action_hyprctl_unknown_falls_back_to_processes() {
        assert_eq!(gui_action(None, true, false), GuiAction::TermAny);
        assert_eq!(gui_action(None, false, true), GuiAction::TermAny);
        assert_eq!(gui_action(None, true, true), GuiAction::TermAny);
    }

    #[test]
    fn output_with_timeout_returns_output() {
        let mut cmd = std::process::Command::new("echo");
        cmd.arg("hi");
        let out = output_with_timeout(cmd, Duration::from_secs(5));
        let out = out.unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hi");
    }

    #[test]
    fn output_with_timeout_kills_hung_command() {
        let mut cmd = std::process::Command::new("sleep");
        cmd.arg("30");
        let t0 = std::time::Instant::now();
        assert!(output_with_timeout(cmd, Duration::from_millis(100)).is_none());
        assert!(t0.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn output_with_timeout_missing_binary_is_none() {
        let cmd = std::process::Command::new("narvi-definitely-missing-bin");
        assert!(output_with_timeout(cmd, Duration::from_secs(1)).is_none());
    }

    #[test]
    fn term_and_reap_is_graceful_and_bounded() {
        // sleep exits on SIGTERM, so we return well before the SIGKILL path.
        let child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let t0 = std::time::Instant::now();
        term_and_reap(child);
        assert!(t0.elapsed() < TERM_GRACE);
    }

    fn test_tray() -> (NarviTray, mpsc::Receiver<Command>) {
        let (tx, rx) = mpsc::channel();
        (
            NarviTray {
                profiles: Vec::new(),
                active: None,
                enabled: true,
                cmd: tx,
                gui_child: None,
            },
            rx,
        )
    }

    #[test]
    fn child_liveness_no_child() {
        let (mut tray, _rx) = test_tray();
        assert!(!tray.gui_child_alive());
    }

    #[test]
    fn child_liveness_running_then_reaped() {
        let (mut tray, _rx) = test_tray();
        tray.gui_child = std::process::Command::new("sleep").arg("30").spawn().ok();
        assert!(tray.gui_child.is_some());
        assert!(tray.gui_child_alive()); // still running, handle kept
        assert!(tray.gui_child.is_some());
        tray.term_gui_child();
        assert!(tray.gui_child.is_none()); // terminated and reaped
        assert!(!tray.gui_child_alive());
    }

    #[test]
    fn child_liveness_drops_exited_child() {
        let (mut tray, _rx) = test_tray();
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let _ = child.wait(); // ensure it has exited; wait() leaves try_wait Ok(Some)
        tray.gui_child = Some(child);
        assert!(!tray.gui_child_alive());
        assert!(tray.gui_child.is_none()); // handle dropped after reap
    }
}
