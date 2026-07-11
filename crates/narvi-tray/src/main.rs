//! `narvi-tray` — StatusNotifierItem. Click toggles the GUI; menu = presets + Quit.

use std::sync::mpsc;

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

/// What a tray click should do to the GUI.
#[derive(Debug, PartialEq, Eq)]
enum GuiAction {
    Close,
    Spawn,
}

/// Toggle on window presence; fall back to child liveness if unknown.
fn gui_action(window: Option<bool>, child_alive: bool) -> GuiAction {
    match window {
        Some(true) => GuiAction::Close,
        Some(false) => GuiAction::Spawn,
        None if child_alive => GuiAction::Close,
        None => GuiAction::Spawn,
    }
}

/// Parse `hyprctl -j clients` JSON; Some(has narvi-gui window) or None if malformed.
fn has_gui_window(clients_json: &str) -> Option<bool> {
    let v: serde_json::Value = serde_json::from_str(clients_json).ok()?;
    let clients = v.as_array()?;
    Some(
        clients
            .iter()
            .any(|c| c.get("class").and_then(serde_json::Value::as_str) == Some("narvi-gui")),
    )
}

/// Ask Hyprland whether a narvi-gui window exists; None if hyprctl fails.
fn query_gui_window() -> Option<bool> {
    let out = std::process::Command::new("hyprctl")
        .args(["-j", "clients"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    has_gui_window(&String::from_utf8_lossy(&out.stdout))
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

    /// Kill + reap any lingering GUI child before a fresh spawn.
    fn kill_gui_child(&mut self) {
        if let Some(mut c) = self.gui_child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// Show the GUI if no window is open, otherwise close it.
    fn toggle_gui(&mut self) {
        let window = query_gui_window();
        let alive = self.gui_child_alive();
        match gui_action(window, alive) {
            GuiAction::Close if window == Some(true) => {
                match std::process::Command::new("hyprctl")
                    .args(["dispatch", "closewindow", "class:^(narvi-gui)$"])
                    .output()
                {
                    Ok(o) if !o.status.success() => log::warn!("gui close failed: {}", o.status),
                    Err(e) => log::warn!("gui close failed: {e}"),
                    Ok(_) => {}
                }
            }
            GuiAction::Close => self.kill_gui_child(),
            GuiAction::Spawn => {
                self.kill_gui_child();
                match std::process::Command::new("narvi-gui").spawn() {
                    Ok(child) => self.gui_child = Some(child),
                    Err(e) => log::warn!("gui spawn failed: {e}"),
                }
            }
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
    fn action_follows_window_presence() {
        // Window state wins regardless of child liveness (zombie-proof).
        assert_eq!(gui_action(Some(true), false), GuiAction::Close);
        assert_eq!(gui_action(Some(true), true), GuiAction::Close);
        assert_eq!(gui_action(Some(false), true), GuiAction::Spawn);
        assert_eq!(gui_action(Some(false), false), GuiAction::Spawn);
    }

    #[test]
    fn action_falls_back_to_child_liveness() {
        assert_eq!(gui_action(None, true), GuiAction::Close);
        assert_eq!(gui_action(None, false), GuiAction::Spawn);
    }
}
