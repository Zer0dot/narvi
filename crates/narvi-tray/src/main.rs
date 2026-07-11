//! `narvi-tray` — StatusNotifierItem. Click toggles the GUI; menu = presets + Quit.

use std::process::{ExitStatus, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

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
    /// Rendezvous channel to the GUI worker; Full = a toggle is running.
    gui: mpsc::SyncSender<()>,
}

/// Bound on hyprctl/pgrep/pkill; a stalled compositor must not wedge the tray.
const HELPER_TIMEOUT: Duration = Duration::from_secs(2);
/// Grace period after SIGTERM before escalating to SIGKILL.
const TERM_GRACE: Duration = Duration::from_secs(1);
/// Grace for the child to exit after closewindow; SIGKILL follows only
/// once no narvi-gui window remains.
const CLOSE_GRACE: Duration = Duration::from_secs(5);
/// comm match: `narvi-gui` plus the Nix wrapper's `.narvi-gui-wrapped`
/// (comm truncates to 15 chars, so no trailing anchor).
const GUI_PROC_PATTERN: &str = r"^\.?narvi-gui";

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
/// The reader thread may outlive the call if a grandchild keeps the
/// stdout pipe open, but the caller never waits past ~`timeout`.
fn output_with_timeout(mut cmd: std::process::Command, timeout: Duration) -> Option<Output> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().ok()?;
    // Reader thread drains stdout so a full pipe can't block the child.
    let (tx, rx) = mpsc::channel();
    match child.stdout.take() {
        Some(mut out) => {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let res = std::io::Read::read_to_end(&mut out, &mut buf).map(|_| buf);
                let _ = tx.send(res);
            });
        }
        None => {
            let _ = tx.send(Ok(Vec::new()));
        }
    }
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Child exited, but a grandchild holding the pipe write
                // end can stall the reader: bound the wait to the same
                // deadline (plus slack) so total wall time stays ~timeout.
                let left =
                    deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(100);
                let stdout = rx.recv_timeout(left).ok()?.ok()?;
                return Some(Output {
                    status,
                    stdout,
                    stderr: Vec::new(),
                });
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            // Timeout or wait error. We still own the un-reaped child, so
            // kill() cannot race a concurrent reap onto a recycled pid.
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// Wait up to `grace` for exit without killing; Err(child) hands the
/// still-running (or wait-erroring) child back to the caller.
fn reap_within(
    mut child: std::process::Child,
    grace: Duration,
) -> Result<ExitStatus, std::process::Child> {
    let deadline = Instant::now() + grace;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(st)) => return Ok(st),
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => break,
        }
    }
    Err(child)
}

/// Wait up to `grace` for exit, then SIGKILL. Always reaps; None only
/// if the final wait fails.
fn reap_or_kill(child: std::process::Child, grace: Duration) -> Option<ExitStatus> {
    match reap_within(child, grace) {
        Ok(st) => Some(st),
        Err(mut child) => {
            let _ = child.kill();
            child.wait().ok()
        }
    }
}

/// SIGKILL a child still alive after closewindow only when no narvi-gui
/// window remains: a surviving window may be the child's own (with two
/// instances, closewindow may have hit the other one).
fn close_should_kill(window_remains: Option<bool>) -> bool {
    window_remains == Some(false)
}

/// SIGTERM `child`; escalate to SIGKILL after TERM_GRACE. Always reaps.
fn term_and_reap(child: std::process::Child) -> Option<ExitStatus> {
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    reap_or_kill(child, TERM_GRACE)
}

/// Newest Hyprland instance dir name under `hypr_dir` (mtime order).
fn newest_instance_sig(hypr_dir: &std::path::Path) -> Option<std::ffi::OsString> {
    std::fs::read_dir(hypr_dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .max_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH)
        })
        .map(|e| e.file_name())
}

/// hyprctl invocation. Supplies HIS from the newest instance dir when
/// the env var is unset (systemd user services can start without it).
fn hyprctl_cmd() -> std::process::Command {
    let mut cmd = std::process::Command::new("hyprctl");
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none()
        && let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR")
        && let Some(sig) = newest_instance_sig(&std::path::Path::new(&runtime).join("hypr"))
    {
        cmd.env("HYPRLAND_INSTANCE_SIGNATURE", sig);
    }
    cmd
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
    let mut cmd = hyprctl_cmd();
    cmd.args(["-j", "clients"]);
    let out = output_with_timeout(cmd, HELPER_TIMEOUT)?;
    if !out.status.success() {
        return None;
    }
    has_gui_window(&String::from_utf8_lossy(&out.stdout))
}

/// pgrep/pkill argv tail: scoped to `uid` (other users' GUIs are not
/// ours to manage) and matching wrapper-renamed binaries too.
fn gui_match_args(uid: &str) -> [String; 3] {
    ["-u".into(), uid.into(), GUI_PROC_PATTERN.into()]
}

fn uid_string() -> String {
    unsafe { libc::getuid() }.to_string()
}

/// Is any narvi-gui process running (ours or launched externally)?
fn query_gui_process() -> bool {
    let mut cmd = std::process::Command::new("pgrep");
    cmd.args(gui_match_args(&uid_string()));
    output_with_timeout(cmd, HELPER_TIMEOUT).is_some_and(|o| o.status.success())
}

/// pkill success policy: 0 = signalled, 1 = nothing matched (fine);
/// >= 2 (usage/internal error) or signal death = failure.
fn pkill_ok(status: ExitStatus) -> bool {
    status.code().is_some_and(|c| c <= 1)
}

/// pkill narvi-gui with `sig` ("-TERM"/"-KILL"); false on spawn/timeout
/// or pkill error.
fn pkill_gui(sig: &str) -> bool {
    let mut cmd = std::process::Command::new("pkill");
    cmd.arg(sig).args(gui_match_args(&uid_string()));
    output_with_timeout(cmd, HELPER_TIMEOUT).is_some_and(|o| pkill_ok(o.status))
}

/// Owns the spawned GUI child and runs toggles on a worker thread, so
/// slow hyprctl/pgrep calls never block the ksni service.
struct GuiToggler {
    child: Option<std::process::Child>,
}

impl GuiToggler {
    /// Reap-aware liveness of our spawned GUI child; drops finished handles.
    fn child_alive(&mut self) -> bool {
        match self.child.as_mut().map(std::process::Child::try_wait) {
            Some(Ok(None)) => true,
            Some(Ok(Some(_))) => {
                self.child = None;
                false
            }
            Some(Err(e)) => {
                // Transient waitpid error: keep the handle and assume
                // alive; dropping it would leave a live GUI unmanaged.
                log::warn!("gui child wait failed: {e}");
                true
            }
            None => false,
        }
    }

    /// SIGTERM + reap our spawned GUI child, if any.
    fn term_child(&mut self) {
        if let Some(c) = self.child.take() {
            term_and_reap(c);
        }
    }

    /// Graceful close via Hyprland; bounded reap keeps SIGKILL authority
    /// over our child if it hangs after its own window closed.
    fn close_window(&mut self) {
        let mut cmd = hyprctl_cmd();
        cmd.args(["dispatch", "closewindow", "class:^(narvi-gui)$"]);
        match output_with_timeout(cmd, HELPER_TIMEOUT) {
            Some(o) if o.status.success() => {
                if let Some(c) = self.child.take() {
                    std::thread::spawn(move || {
                        if let Err(mut c) = reap_within(c, CLOSE_GRACE) {
                            // SIGKILL only once no window remains; else
                            // just reap whenever the child exits.
                            if close_should_kill(query_gui_window()) {
                                let _ = c.kill();
                            }
                            let _ = c.wait();
                        }
                    });
                }
            }
            Some(o) => log::warn!("gui close failed: {}", o.status),
            None => log::warn!("gui close failed: hyprctl error or timeout"),
        }
    }

    /// Show the GUI if none is open or starting, otherwise close it.
    fn toggle(&mut self) {
        let window = query_gui_window();
        let alive = self.child_alive();
        // pgrep only when the window can't already answer the toggle.
        let gui_process = window != Some(true) && query_gui_process();
        match gui_action(window, alive, gui_process) {
            GuiAction::CloseWindow => self.close_window(),
            GuiAction::TermChild => self.term_child(),
            GuiAction::TermAny => {
                // pkill SIGTERMs every narvi-gui, our child included.
                if !pkill_gui("-TERM") {
                    log::warn!("pkill narvi-gui failed");
                }
                self.term_child();
                // Verify and escalate: a TERM-immune GUI must not leave
                // the toggle wedged (neither killable nor respawnable).
                let deadline = Instant::now() + TERM_GRACE;
                while Instant::now() < deadline && query_gui_process() {
                    std::thread::sleep(Duration::from_millis(50));
                }
                if query_gui_process() && !pkill_gui("-KILL") {
                    log::warn!("pkill -KILL narvi-gui failed");
                }
            }
            GuiAction::Spawn => match std::process::Command::new("narvi-gui").spawn() {
                Ok(child) => self.child = Some(child),
                Err(e) => log::warn!("gui spawn failed: {e}"),
            },
        }
    }
}

impl NarviTray {
    fn send(&self, c: Command) {
        log::debug!("tray action: {c:?}");
        if self.cmd.send(c).is_err() {
            log::warn!("command worker gone");
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
        // Clicks made while a toggle runs are dropped, not queued —
        // replaying them later would flap the GUI open/closed.
        match self.gui.try_send(()) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(())) => {
                log::debug!("gui toggle in flight; click dropped");
            }
            Err(mpsc::TrySendError::Disconnected(())) => log::warn!("gui worker gone"),
        }
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

    // GUI worker: owns the spawned child; the rendezvous channel means at
    // most one toggle runs and clicks never queue behind it.
    let (gui_tx, gui_rx) = mpsc::sync_channel::<()>(0);
    std::thread::spawn(move || {
        let mut gui = GuiToggler { child: None };
        while gui_rx.recv().is_ok() {
            gui.toggle();
        }
    });

    let tray = NarviTray {
        profiles: Vec::new(),
        active: None,
        enabled: true,
        cmd: cmd_tx,
        gui: gui_tx,
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
    use std::os::unix::process::ExitStatusExt;

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
        // A non-array top level is malformed.
        assert_eq!(has_gui_window(r#"{"class":"narvi-gui"}"#), None);
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
    fn output_with_timeout_drains_large_output() {
        // Larger than the pipe buffer: the child must not deadlock on it.
        let mut cmd = std::process::Command::new("sh");
        cmd.args(["-c", "head -c 262144 /dev/zero"]);
        let out = output_with_timeout(cmd, Duration::from_secs(5)).unwrap();
        assert!(out.status.success());
        assert_eq!(out.stdout.len(), 262144);
    }

    #[test]
    fn output_with_timeout_kills_hung_command() {
        let mut cmd = std::process::Command::new("sleep");
        cmd.arg("30");
        let t0 = Instant::now();
        assert!(output_with_timeout(cmd, Duration::from_millis(100)).is_none());
        assert!(t0.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn output_with_timeout_missing_binary_is_none() {
        let cmd = std::process::Command::new("narvi-definitely-missing-bin");
        assert!(output_with_timeout(cmd, Duration::from_secs(1)).is_none());
    }

    #[test]
    fn term_and_reap_is_graceful() {
        let child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        // sleep dies on the SIGTERM itself — no SIGKILL escalation.
        let st = term_and_reap(child).unwrap();
        assert_eq!(st.signal(), Some(libc::SIGTERM));
    }

    #[test]
    fn term_and_reap_escalates_to_sigkill() {
        use std::io::Read;
        // Ignore SIGTERM (survives exec), signal readiness, become sleep.
        let mut child = std::process::Command::new("sh")
            .args(["-c", r#"trap "" TERM; echo r; exec sleep 30"#])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut ready = [0u8; 1];
        // Wait for the trap to be installed before signalling.
        child.stdout.take().unwrap().read_exact(&mut ready).unwrap();
        let t0 = Instant::now();
        let st = term_and_reap(child).unwrap();
        assert!(t0.elapsed() >= TERM_GRACE);
        assert_eq!(st.signal(), Some(libc::SIGKILL));
    }

    #[test]
    fn reap_or_kill_returns_on_exit() {
        let child = std::process::Command::new("true").spawn().unwrap();
        let st = reap_or_kill(child, Duration::from_secs(5)).unwrap();
        assert!(st.success());
    }

    #[test]
    fn reap_or_kill_kills_after_grace() {
        let child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let st = reap_or_kill(child, Duration::from_millis(100)).unwrap();
        assert_eq!(st.signal(), Some(libc::SIGKILL));
    }

    #[test]
    fn pkill_exit_code_policy() {
        // Raw wait status: exit code c is c << 8; a bare signal number
        // means signal death (code() == None).
        assert!(pkill_ok(ExitStatus::from_raw(0))); // signalled a match
        assert!(pkill_ok(ExitStatus::from_raw(1 << 8))); // nothing matched
        assert!(!pkill_ok(ExitStatus::from_raw(2 << 8))); // usage error
        assert!(!pkill_ok(ExitStatus::from_raw(3 << 8))); // internal error
        assert!(!pkill_ok(ExitStatus::from_raw(libc::SIGTERM))); // signal death
    }

    #[test]
    fn close_kill_needs_confirmed_window_absence() {
        assert!(close_should_kill(Some(false)));
        // A surviving window may be the child's own; never SIGKILL it.
        assert!(!close_should_kill(Some(true)));
        // hyprctl failure: unknown state, don't kill.
        assert!(!close_should_kill(None));
    }

    #[test]
    fn reap_within_returns_live_child_on_timeout() {
        let child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        match reap_within(child, Duration::from_millis(50)) {
            Err(c) => {
                let _ = term_and_reap(c);
            }
            Ok(st) => panic!("sleep exited unexpectedly: {st:?}"),
        }
    }

    #[test]
    fn reap_within_reaps_exited_child() {
        let child = std::process::Command::new("true").spawn().unwrap();
        let st = reap_within(child, Duration::from_secs(5))
            .unwrap_or_else(|_| panic!("true did not exit"));
        assert!(st.success());
    }

    #[test]
    fn newest_instance_sig_picks_latest_dir() {
        let base = std::env::temp_dir().join(format!("narvi-his-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        assert_eq!(newest_instance_sig(&base), None); // missing dir
        std::fs::create_dir_all(&base).unwrap();
        assert_eq!(newest_instance_sig(&base), None); // empty dir
        std::fs::create_dir(base.join("older")).unwrap();
        std::thread::sleep(Duration::from_millis(20)); // distinct mtimes
        std::fs::create_dir(base.join("newer")).unwrap();
        std::fs::write(base.join("file"), b"x").unwrap(); // files ignored
        assert_eq!(
            newest_instance_sig(&base),
            Some(std::ffi::OsString::from("newer"))
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn output_wall_time_bounded_with_pipe_holding_grandchild() {
        // Child exits at once but a grandchild inherits the stdout write
        // end: total wait must stay ~timeout, not 2x.
        let mut cmd = std::process::Command::new("sh");
        cmd.args(["-c", "sleep 2 & exit 0"]);
        let t0 = Instant::now();
        assert!(output_with_timeout(cmd, Duration::from_millis(400)).is_none());
        assert!(t0.elapsed() < Duration::from_millis(1200));
    }

    #[test]
    fn proc_match_is_user_scoped_and_wrapper_aware() {
        let args = gui_match_args("1000");
        assert_eq!(args[0], "-u");
        assert_eq!(args[1], "1000");
        // Anchored, optional leading dot, no -x: catches both `narvi-gui`
        // and the comm-truncated `.narvi-gui-wrap(ped)`.
        assert_eq!(args[2], r"^\.?narvi-gui");
    }

    #[test]
    fn gui_clicks_drop_while_toggle_in_flight() {
        let (tx, rx) = mpsc::sync_channel::<()>(0);
        // No worker blocked in recv (= busy): click dropped, not queued.
        assert!(matches!(tx.try_send(()), Err(mpsc::TrySendError::Full(()))));
        let worker = std::thread::spawn(move || rx.recv());
        // Once the worker is idle in recv, a click goes through.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match tx.try_send(()) {
                Ok(()) => break,
                Err(mpsc::TrySendError::Full(())) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => panic!("worker never became idle: {e}"),
            }
        }
        assert!(worker.join().unwrap().is_ok());
    }

    #[test]
    fn child_liveness_no_child() {
        let mut gui = GuiToggler { child: None };
        assert!(!gui.child_alive());
    }

    #[test]
    fn child_liveness_running_then_reaped() {
        let mut gui = GuiToggler { child: None };
        gui.child = std::process::Command::new("sleep").arg("30").spawn().ok();
        assert!(gui.child.is_some());
        assert!(gui.child_alive()); // still running, handle kept
        assert!(gui.child.is_some());
        gui.term_child();
        assert!(gui.child.is_none()); // terminated and reaped
        assert!(!gui.child_alive());
    }

    #[test]
    fn child_liveness_drops_exited_child() {
        let mut gui = GuiToggler { child: None };
        let mut child = std::process::Command::new("true").spawn().unwrap();
        // wait() reaps, so try_wait then reports Ok(Some).
        let _ = child.wait();
        gui.child = Some(child);
        assert!(!gui.child_alive());
        assert!(gui.child.is_none()); // handle dropped after reap
    }
}
