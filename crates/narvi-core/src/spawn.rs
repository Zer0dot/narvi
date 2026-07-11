//! Auto-spawn of `narvid` when the daemon socket is unreachable.
//!
//! `SpawnGuard` rate-limits attempts (no spawn storm from a broken binary);
//! `DaemonSpawner` owns the child and reaps it via `try_wait` (no zombies).
//! `narvid` itself refuses to double-run, so spawning is always safe.

use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// Default wait between daemon spawn attempts.
pub const SPAWN_COOLDOWN: Duration = Duration::from_secs(30);

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
        if let Some(last) = self.last_attempt {
            if now.saturating_duration_since(last) < self.cooldown {
                return false; // blocked; window keeps counting from `last`
            }
        }
        self.last_attempt = Some(now);
        true
    }
}

/// Spawns a daemon binary (found via PATH) at most once per cooldown.
pub struct DaemonSpawner {
    program: String,
    guard: SpawnGuard,
    child: Option<Child>,
}

impl DaemonSpawner {
    pub fn new(program: impl Into<String>, cooldown: Duration) -> Self {
        Self {
            program: program.into(),
            guard: SpawnGuard::new(cooldown),
            child: None,
        }
    }

    /// Call on each unreachable-daemon retry: reaps a finished child, then
    /// spawns a new one if the cooldown allows. True while a spawn is pending.
    pub fn tick(&mut self) -> bool {
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(None) => return true, // still starting up
                Ok(Some(status)) => {
                    log::info!("spawned {} exited: {status}", self.program);
                    self.child = None;
                }
                Err(e) => {
                    log::warn!("could not reap {}: {e}", self.program);
                    self.child = None;
                }
            }
        }
        if !self.guard.try_acquire() {
            return false;
        }
        match Command::new(&self.program).spawn() {
            Ok(child) => {
                log::info!("spawned {}", self.program);
                self.child = Some(child);
                true
            }
            Err(e) => {
                log::warn!("failed to spawn {}: {e}", self.program);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut s = DaemonSpawner::new("narvi-no-such-binary-xyz", Duration::from_secs(60));
        assert!(!s.tick()); // spawn fails
        assert!(!s.tick()); // failed attempt still consumed the window
    }

    #[test]
    fn spawner_reaps_exited_child() {
        // `true` exits immediately; large cooldown blocks a respawn, so tick
        // must flip pending -> false once the child is reaped.
        let mut s = DaemonSpawner::new("true", Duration::from_secs(60));
        assert!(s.tick());
        let deadline = Instant::now() + Duration::from_secs(5);
        while s.tick() {
            assert!(Instant::now() < deadline, "child never reaped");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(s.child.is_none());
    }
}
