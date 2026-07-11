//! Auto-spawn of `narvid` when the daemon socket is unreachable.
//!
//! `DaemonSpawner` waits out a grace window first (so a daemon already
//! starting — e.g. under systemd — can win), rate-limits attempts via
//! `SpawnGuard`, kills a child stuck starting past a deadline, and reaps
//! exits. Single-instance safety comes from `narvid`'s exclusive file
//! lock: a losing duplicate exits before touching any state.

use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// Default wait between daemon spawn attempts.
pub const SPAWN_COOLDOWN: Duration = Duration::from_secs(30);
/// Unreachable streak required before the first spawn attempt.
pub const SPAWN_GRACE: Duration = Duration::from_secs(5);
/// Max time a spawned child may stay unconnectable before it is killed.
pub const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

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
            first_tick: None,
            child: None,
        }
    }

    /// Call after a successful connect: the next outage gets a fresh grace,
    /// so a supervisor (systemd restart) can win the respawn race.
    pub fn reset(&mut self) {
        self.first_tick = None;
    }

    /// Call on each unreachable-daemon retry: reaps a finished child, then
    /// spawns a new one if grace has passed and the cooldown allows.
    /// True while a spawn is pending (child alive, within startup timeout).
    pub fn tick(&mut self) -> bool {
        self.tick_at(Instant::now())
    }

    /// Clock-injected variant of [`Self::tick`] for tests.
    pub fn tick_at(&mut self, now: Instant) -> bool {
        if let Some((child, started)) = self.child.as_mut() {
            match child.try_wait() {
                Ok(None) => {
                    if now.saturating_duration_since(*started) < self.startup_timeout {
                        return true; // still starting up
                    }
                    // Stuck before becoming connectable: kill so the next
                    // cooldown window can retry instead of pending forever.
                    log::warn!("spawned {} stuck starting; killing it", self.program);
                    let _ = child.kill();
                    let _ = child.wait();
                    self.child = None;
                    return false;
                }
                Ok(Some(status)) => {
                    if status.success() {
                        log::info!("spawned {} exited: {status}", self.program);
                    } else {
                        log::warn!(
                            "spawned {} exited: {status} — run it manually to see why",
                            self.program
                        );
                    }
                    self.child = None;
                }
                Err(e) => {
                    log::warn!("could not reap {}: {e}", self.program);
                    self.child = None;
                }
            }
        }
        // Grace: give an already-starting daemon time to bind its socket.
        let first = *self.first_tick.get_or_insert(now);
        if now.saturating_duration_since(first) < self.grace {
            return false;
        }
        if !self.guard.try_acquire_at(now) {
            return false;
        }
        match Command::new(&self.program).args(&self.args).spawn() {
            Ok(child) => {
                log::info!("spawned {}", self.program);
                self.child = Some((child, now));
                true
            }
            Err(e) => {
                log::warn!("failed to spawn {}: {e}", self.program);
                false
            }
        }
    }
}

// A spawned child intentionally outlives the spawner: it is the daemon.
// If it exits later, init reaps it; while the client lives, tick() reaps.

#[cfg(test)]
mod tests {
    use super::*;

    const ZERO: Duration = Duration::ZERO;

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
    fn spawner_missing_binary_not_pending_and_cooldown_holds() {
        let mut s = spawner(
            "narvi-no-such-binary-xyz",
            &[],
            Duration::from_secs(60),
            ZERO,
            ZERO,
        );
        let t0 = Instant::now();
        assert!(!s.tick_at(t0)); // spawn fails
        assert!(!s.tick_at(t0)); // failed attempt still consumed the window
    }

    #[test]
    fn spawner_grace_defers_first_spawn() {
        let grace = Duration::from_secs(5);
        let mut s = spawner("sleep", &["30"], Duration::from_secs(60), grace, grace);
        let t0 = Instant::now();
        assert!(!s.tick_at(t0));
        assert!(s.child.is_none()); // grace blocks, nothing spawned
        assert!(!s.tick_at(t0 + Duration::from_secs(4)));
        assert!(s.tick_at(t0 + grace));
        assert!(s.child.is_some());
        kill(&mut s);
    }

    #[test]
    fn spawner_alive_child_pends_without_consuming_cooldown() {
        let timeout = Duration::from_secs(10);
        let mut s = spawner("sleep", &["30"], Duration::from_secs(60), ZERO, timeout);
        let t0 = Instant::now();
        assert!(s.tick_at(t0));
        let pid = s.child.as_ref().map(|(c, _)| c.id());
        let last = s.guard.last_attempt;
        assert!(s.tick_at(t0 + Duration::from_secs(1))); // alive branch
        assert_eq!(s.child.as_ref().map(|(c, _)| c.id()), pid); // no respawn
        assert_eq!(s.guard.last_attempt, last); // window untouched
        kill(&mut s);
    }

    #[test]
    fn spawner_kills_child_stuck_past_startup_timeout() {
        let timeout = Duration::from_secs(10);
        let cooldown = Duration::from_secs(60);
        let mut s = spawner("sleep", &["30"], cooldown, ZERO, timeout);
        let t0 = Instant::now();
        assert!(s.tick_at(t0));
        assert!(!s.tick_at(t0 + timeout)); // stuck: killed, no longer pending
        assert!(s.child.is_none());
        assert!(!s.tick_at(t0 + timeout)); // cooldown still holds
        assert!(s.tick_at(t0 + cooldown)); // then a fresh attempt is allowed
        kill(&mut s);
    }

    #[test]
    fn spawner_reset_rearms_grace() {
        let grace = Duration::from_secs(5);
        let mut s = spawner("sleep", &["30"], ZERO, grace, grace);
        let t0 = Instant::now();
        assert!(!s.tick_at(t0));
        s.reset(); // as after a successful connect
        let t1 = t0 + Duration::from_secs(10);
        assert!(!s.tick_at(t1)); // grace counts from the new streak
        assert!(s.tick_at(t1 + grace));
        kill(&mut s);
    }

    #[test]
    fn spawner_reaps_exited_child() {
        // `true` exits immediately; large cooldown blocks a respawn, so tick
        // must flip pending -> false once the child is reaped.
        let big = Duration::from_secs(60);
        let mut s = spawner("true", &[], big, ZERO, big);
        assert!(s.tick());
        let deadline = Instant::now() + Duration::from_secs(5);
        while s.tick() {
            assert!(Instant::now() < deadline, "child never reaped");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(s.child.is_none());
    }
}
