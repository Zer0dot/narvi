//! Auto-spawn of `narvid` when the daemon socket is unreachable.
//!
//! `DaemonSpawner` waits out a grace window first (so a daemon already
//! starting — e.g. under systemd — can win), rate-limits attempts via
//! `SpawnGuard`, kills a child stuck starting past a deadline, reaps
//! exits, and gives up after repeated failures. When the `narvi.service`
//! systemd user unit exists it starts that instead of exec'ing directly,
//! so the daemon stays supervised in its own cgroup. Single-instance
//! safety comes from `narvid`'s exclusive file lock. Opt out with
//! `NARVI_AUTOSPAWN=0`.

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Default wait between daemon spawn attempts.
pub const SPAWN_COOLDOWN: Duration = Duration::from_secs(30);
/// Unreachable streak required before the first spawn attempt.
pub const SPAWN_GRACE: Duration = Duration::from_secs(5);
/// Max time a spawned child may stay unconnectable before it is killed.
pub const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
/// Consecutive failed attempts before the spawner gives up until reconnect.
pub const MAX_FAILURES: u32 = 3;
/// systemd user unit preferred over a direct exec when it is present.
const UNIT: &str = "narvi.service";
/// Env var: set to `0`/`false`/`off`/`no` to disable auto-spawning.
pub const AUTOSPAWN_ENV: &str = "NARVI_AUTOSPAWN";

/// Outcome of a [`DaemonSpawner::tick`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnStatus {
    /// A spawned child is starting up (or was just spawned).
    Starting,
    /// No child yet, but a spawn is scheduled (grace or cooldown pending).
    Scheduled,
    /// Too many consecutive failures; not retrying until the next connect.
    GaveUp,
    /// Auto-spawning is disabled via [`AUTOSPAWN_ENV`].
    Disabled,
}

/// True when an [`AUTOSPAWN_ENV`] value opts out of auto-spawning.
pub fn autospawn_disabled(value: Option<&str>) -> bool {
    value.is_some_and(|v| {
        let v = v.trim();
        ["0", "false", "off", "no"]
            .iter()
            .any(|d| v.eq_ignore_ascii_case(d))
    })
}

/// True when `systemctl show -p LoadState --value` output means the unit exists.
pub fn unit_loaded(load_state: &str) -> bool {
    load_state.trim() == "loaded"
}

/// How the daemon gets started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    /// `systemctl --user start narvi.service` — supervised, own cgroup.
    Unit,
    /// Direct exec, detached (own process group, null stdio).
    Direct,
}

/// Rate limiter for spawn attempts: at most one per cooldown window.
#[derive(Debug)]
pub struct SpawnGuard {
    cooldown: Duration,
    last_attempt: Option<Instant>,
}

impl SpawnGuard {
    pub fn new(cooldown: Duration) -> Self {
        Self {
            cooldown,
            last_attempt: None,
        }
    }

    /// Records and allows an attempt unless one happened within the cooldown.
    pub fn try_acquire(&mut self) -> bool {
        self.try_acquire_at(Instant::now())
    }

    /// Clock-injected variant of [`Self::try_acquire`] for tests.
    pub fn try_acquire_at(&mut self, now: Instant) -> bool {
        if let Some(last) = self.last_attempt
            && now.saturating_duration_since(last) < self.cooldown
        {
            return false; // blocked; window keeps counting from `last`
        }
        self.last_attempt = Some(now);
        true
    }
}

/// Spawns a daemon binary (found via PATH) at most once per cooldown.
pub struct DaemonSpawner {
    program: String,
    args: Vec<String>,
    guard: SpawnGuard,
    grace: Duration,
    startup_timeout: Duration,
    max_failures: u32,
    /// Consecutive failed attempts; cleared by [`Self::reset`].
    failures: u32,
    disabled: bool,
    /// Lazily probed once; `None` until the first spawn attempt.
    method: Option<Method>,
    /// Start of the current unreachable streak; grace counts from here.
    first_tick: Option<Instant>,
    child: Option<(Child, Instant)>,
}

impl DaemonSpawner {
    pub fn new(program: impl Into<String>, cooldown: Duration) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            guard: SpawnGuard::new(cooldown),
            grace: SPAWN_GRACE,
            startup_timeout: STARTUP_TIMEOUT,
            max_failures: MAX_FAILURES,
            failures: 0,
            disabled: autospawn_disabled(std::env::var(AUTOSPAWN_ENV).ok().as_deref()),
            method: None,
            first_tick: None,
            child: None,
        }
    }

    /// Call after a successful connect: the next outage gets a fresh grace,
    /// so a supervisor (systemd restart) can win the respawn race.
    pub fn reset(&mut self) {
        self.first_tick = None;
        self.failures = 0;
        // The daemon is reachable: reap the child if it exited, else detach
        // it — a later outage must never kill() the live daemon we spawned.
        if let Some((mut child, _)) = self.child.take() {
            let _ = child.try_wait();
        }
    }

    /// Call on each unreachable-daemon retry: reaps a finished child, then
    /// spawns a new one if grace has passed and the cooldown allows.
    pub fn tick(&mut self) -> SpawnStatus {
        self.tick_at(Instant::now())
    }

    /// Clock-injected variant of [`Self::tick`] for tests.
    pub fn tick_at(&mut self, now: Instant) -> SpawnStatus {
        if self.disabled {
            return SpawnStatus::Disabled;
        }
        if let Some((child, started)) = self.child.as_mut() {
            match child.try_wait() {
                Ok(None) => {
                    if now.saturating_duration_since(*started) < self.startup_timeout {
                        return SpawnStatus::Starting;
                    }
                    // Stuck before becoming connectable: kill so the next
                    // cooldown window can retry instead of pending forever.
                    log::warn!("spawned {} stuck starting; killing it", self.program);
                    let _ = child.kill();
                    let _ = child.wait();
                    self.child = None;
                    self.note_failure();
                    return self.wait_status();
                }
                Ok(Some(status)) => {
                    if status.success() {
                        log::info!("spawned {} exited: {status}", self.program);
                    } else {
                        log::warn!(
                            "spawned {} exited: {status} — run it manually to see why",
                            self.program
                        );
                        self.note_failure();
                    }
                    self.child = None;
                }
                Err(e) => {
                    // Can't tell if it lives: kill so it never leaks untracked.
                    log::warn!("could not reap {}: {e}; killing it", self.program);
                    let _ = child.kill();
                    let _ = child.wait();
                    self.child = None;
                    self.note_failure();
                    return self.wait_status();
                }
            }
        }
        if self.failures >= self.max_failures {
            return SpawnStatus::GaveUp;
        }
        // Grace: give an already-starting daemon time to bind its socket.
        let first = *self.first_tick.get_or_insert(now);
        if now.saturating_duration_since(first) < self.grace {
            return SpawnStatus::Scheduled;
        }
        if !self.guard.try_acquire_at(now) {
            return SpawnStatus::Scheduled;
        }
        match self.command().spawn() {
            Ok(child) => {
                log::info!("spawned {}", self.program);
                self.child = Some((child, now));
                SpawnStatus::Starting
            }
            Err(e) => {
                log::warn!("failed to spawn {}: {e}", self.program);
                self.note_failure();
                self.wait_status()
            }
        }
    }

    fn note_failure(&mut self) {
        self.failures += 1;
        if self.failures >= self.max_failures {
            log::warn!(
                "{}: {} failed starts in a row; giving up until reconnect",
                self.program,
                self.failures
            );
        }
    }

    fn wait_status(&self) -> SpawnStatus {
        if self.failures >= self.max_failures {
            SpawnStatus::GaveUp
        } else {
            SpawnStatus::Scheduled
        }
    }

    /// Build the spawn command per the (lazily probed, cached) method.
    fn command(&mut self) -> Command {
        let method = *self.method.get_or_insert_with(|| {
            if user_unit_exists(UNIT) {
                log::info!("{UNIT} found; starting the daemon via systemctl");
                Method::Unit
            } else {
                Method::Direct
            }
        });
        let mut cmd = match method {
            Method::Unit => {
                let mut c = Command::new("systemctl");
                c.args(["--user", "start", UNIT]);
                c
            }
            Method::Direct => {
                let mut c = Command::new(&self.program);
                c.args(&self.args);
                c
            }
        };
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // Own process group: the daemon must not die with the client's tty.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        cmd
    }
}

/// True when the systemd user unit is present (LoadState=loaded).
fn user_unit_exists(unit: &str) -> bool {
    Command::new("systemctl")
        .args(["--user", "show", "-p", "LoadState", "--value", unit])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| unit_loaded(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use SpawnStatus::{GaveUp, Scheduled, Starting};

    const ZERO: Duration = Duration::ZERO;
    const BIG: Duration = Duration::from_secs(60);

    fn spawner(
        program: &str,
        args: &[&str],
        cooldown: Duration,
        grace: Duration,
        startup_timeout: Duration,
    ) -> DaemonSpawner {
        DaemonSpawner {
            program: program.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            guard: SpawnGuard::new(cooldown),
            grace,
            startup_timeout,
            max_failures: MAX_FAILURES,
            failures: 0,
            disabled: false,
            method: Some(Method::Direct), // never probe systemctl in tests
            first_tick: None,
            child: None,
        }
    }

    fn kill(s: &mut DaemonSpawner) {
        if let Some((mut c, _)) = s.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// Process-group id from `/proc/<pid>/stat`, or None if the pid is gone.
    fn pgid_of(pid: u32) -> Option<i64> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        stat.rsplit_once(')')?
            .1
            .split_whitespace()
            .nth(2)?
            .parse()
            .ok()
    }

    /// Process state char from `/proc/<pid>/stat`, or None if the pid is gone.
    fn state_of(pid: u32) -> Option<char> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        stat.rsplit_once(')')?
            .1
            .split_whitespace()
            .next()?
            .chars()
            .next()
    }

    #[test]
    fn guard_allows_first_attempt() {
        let mut g = SpawnGuard::new(Duration::from_secs(10));
        assert!(g.try_acquire_at(Instant::now()));
    }

    #[test]
    fn guard_blocks_within_cooldown() {
        let mut g = SpawnGuard::new(Duration::from_secs(10));
        let t0 = Instant::now();
        assert!(g.try_acquire_at(t0));
        assert!(!g.try_acquire_at(t0));
        assert!(!g.try_acquire_at(t0 + Duration::from_secs(9)));
    }

    #[test]
    fn guard_reallows_after_cooldown_and_rearms() {
        let mut g = SpawnGuard::new(Duration::from_secs(10));
        let t0 = Instant::now();
        assert!(g.try_acquire_at(t0));
        assert!(g.try_acquire_at(t0 + Duration::from_secs(10)));
        assert!(!g.try_acquire_at(t0 + Duration::from_secs(11)));
    }

    #[test]
    fn guard_blocked_attempt_does_not_extend_window() {
        let mut g = SpawnGuard::new(Duration::from_secs(10));
        let t0 = Instant::now();
        assert!(g.try_acquire_at(t0));
        assert!(!g.try_acquire_at(t0 + Duration::from_secs(9)));
        assert!(g.try_acquire_at(t0 + Duration::from_secs(10)));
    }

    #[test]
    fn autospawn_env_opt_out_values() {
        assert!(!autospawn_disabled(None));
        assert!(!autospawn_disabled(Some("")));
        assert!(!autospawn_disabled(Some("1")));
        assert!(!autospawn_disabled(Some("yes")));
        for v in ["0", "false", "off", "no", "FALSE", " Off "] {
            assert!(autospawn_disabled(Some(v)), "{v:?} should disable");
        }
    }

    #[test]
    fn unit_loaded_matches_only_loaded() {
        assert!(unit_loaded("loaded\n"));
        assert!(!unit_loaded("not-found\n"));
        assert!(!unit_loaded(""));
    }

    #[test]
    fn spawner_disabled_never_spawns() {
        let mut s = spawner("sleep", &["30"], ZERO, ZERO, BIG);
        s.disabled = true;
        assert_eq!(s.tick_at(Instant::now()), SpawnStatus::Disabled);
        assert!(s.child.is_none());
    }

    #[test]
    fn spawner_missing_binary_not_pending_and_cooldown_holds() {
        let mut s = spawner("narvi-no-such-binary-xyz", &[], BIG, ZERO, ZERO);
        let t0 = Instant::now();
        assert_eq!(s.tick_at(t0), Scheduled); // spawn fails
        assert_eq!(s.failures, 1);
        assert_eq!(s.tick_at(t0), Scheduled); // failed attempt consumed the window
        assert_eq!(s.failures, 1);
    }

    #[test]
    fn spawner_grace_defers_first_spawn() {
        let grace = Duration::from_secs(5);
        let mut s = spawner("sleep", &["30"], BIG, grace, grace);
        let t0 = Instant::now();
        assert_eq!(s.tick_at(t0), Scheduled);
        assert!(s.child.is_none()); // grace blocks, nothing spawned
        assert_eq!(s.tick_at(t0 + Duration::from_secs(4)), Scheduled);
        assert_eq!(s.tick_at(t0 + grace), Starting);
        assert!(s.child.is_some());
        kill(&mut s);
    }

    #[test]
    fn spawner_alive_child_pends_without_consuming_cooldown() {
        let mut s = spawner("sleep", &["30"], BIG, ZERO, Duration::from_secs(10));
        let t0 = Instant::now();
        assert_eq!(s.tick_at(t0), Starting);
        let pid = s.child.as_ref().map(|(c, _)| c.id());
        let last = s.guard.last_attempt;
        assert_eq!(s.tick_at(t0 + Duration::from_secs(1)), Starting); // alive branch
        assert_eq!(s.child.as_ref().map(|(c, _)| c.id()), pid); // no respawn
        assert_eq!(s.guard.last_attempt, last); // window untouched
        kill(&mut s);
    }

    #[test]
    fn spawner_kills_child_stuck_past_startup_timeout() {
        let timeout = Duration::from_secs(10);
        let mut s = spawner("sleep", &["30"], BIG, ZERO, timeout);
        let t0 = Instant::now();
        assert_eq!(s.tick_at(t0), Starting);
        assert_eq!(s.tick_at(t0 + timeout), Scheduled); // stuck: killed
        assert!(s.child.is_none());
        assert_eq!(s.failures, 1); // stuck start counts as a failure
        assert_eq!(s.tick_at(t0 + timeout), Scheduled); // cooldown still holds
        assert_eq!(s.tick_at(t0 + BIG), Starting); // then a fresh attempt
        kill(&mut s);
    }

    #[test]
    fn spawner_reset_rearms_grace() {
        let grace = Duration::from_secs(5);
        let mut s = spawner("sleep", &["30"], ZERO, grace, grace);
        let t0 = Instant::now();
        assert_eq!(s.tick_at(t0), Scheduled);
        s.reset(); // as after a successful connect
        let t1 = t0 + Duration::from_secs(10);
        assert_eq!(s.tick_at(t1), Scheduled); // grace counts from the new streak
        assert_eq!(s.tick_at(t1 + grace), Starting);
        kill(&mut s);
    }

    #[test]
    fn spawner_reaps_exited_child() {
        // `true` exits 0 immediately; large cooldown blocks a respawn, so
        // tick must flip Starting -> Scheduled once the child is reaped.
        let mut s = spawner("true", &[], BIG, ZERO, BIG);
        assert_eq!(s.tick(), Starting);
        let deadline = Instant::now() + Duration::from_secs(5);
        while s.tick() == Starting {
            assert!(Instant::now() < deadline, "child never reaped");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(s.child.is_none());
        assert_eq!(s.failures, 0); // exit 0 (lost-lock duplicate) is not a failure
    }

    #[test]
    fn reset_detaches_live_child_so_later_tick_cannot_kill_it() {
        // startup_timeout ZERO: a child still held after reset would be
        // killed on the very next tick — the HIGH stale-handle bug.
        let mut s = spawner("sleep", &["30"], BIG, ZERO, ZERO);
        let t0 = Instant::now();
        assert_eq!(s.tick_at(t0), Starting);
        let pid = s.child.as_ref().map(|(c, _)| c.id()).unwrap_or(0);
        s.reset(); // connect succeeded: forget the now-live daemon
        assert!(s.child.is_none());
        assert_eq!(s.tick_at(t0 + Duration::from_secs(30)), Scheduled);
        // Alive and not a zombie: the detached daemon survived the tick.
        let st = state_of(pid);
        assert!(
            st.is_some() && st != Some('Z'),
            "live daemon was killed: {st:?}"
        );
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }

    #[test]
    fn reset_reaps_exited_child() {
        let mut s = spawner("true", &[], BIG, ZERO, BIG);
        assert_eq!(s.tick(), Starting);
        let pid = s.child.as_ref().map(|(c, _)| c.id()).unwrap_or(0);
        let deadline = Instant::now() + Duration::from_secs(5);
        while state_of(pid) != Some('Z') {
            assert!(Instant::now() < deadline, "child never exited");
            std::thread::sleep(Duration::from_millis(10));
        }
        s.reset();
        assert!(s.child.is_none());
        assert_eq!(state_of(pid), None, "zombie was not reaped");
    }

    #[test]
    fn spawner_gives_up_after_repeated_failures_until_reset() {
        // `false` exits 1 every time; zero cooldown makes retries immediate.
        let mut s = spawner("false", &[], ZERO, ZERO, BIG);
        s.max_failures = 2;
        let deadline = Instant::now() + Duration::from_secs(10);
        while s.tick() != GaveUp {
            assert!(Instant::now() < deadline, "never gave up");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(s.failures, 2);
        assert!(s.child.is_none());
        assert_eq!(s.tick(), GaveUp); // stays given up
        s.reset(); // a successful connect re-arms the spawner
        assert_eq!(s.failures, 0);
        assert_eq!(s.tick(), Starting);
        kill(&mut s);
    }

    #[test]
    fn direct_spawn_detaches_into_own_process_group() {
        let mut s = spawner("sleep", &["30"], BIG, ZERO, BIG);
        assert_eq!(s.tick(), Starting);
        let pid = s.child.as_ref().map(|(c, _)| c.id()).unwrap_or(0);
        // process_group(0) makes the child its own group leader.
        assert_eq!(pgid_of(pid), Some(i64::from(pid)));
        kill(&mut s);
    }
}
