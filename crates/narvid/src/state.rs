//! Daemon state + command dispatch. Single source of truth for color state.

use std::path::PathBuf;

use anyhow::{Context, Result};
use narvi_core::config::expand_tilde;
use narvi_core::proto::{Command, Event, ScheduleStatus, Status};
use narvi_core::shader::render_shader;
use narvi_core::{ColorParams, Config};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::broadcast;

use crate::apply;

/// Snapshot persisted across restarts (`restore_on_start`).
#[derive(Serialize, Deserialize)]
struct PersistState {
    enabled: bool,
    active_profile: Option<String>,
    params: ColorParams,
}

pub struct Daemon {
    pub cfg: Config,
    pub cfg_path: PathBuf,
    pub params: ColorParams,
    pub enabled: bool,
    /// `Some` only while params match the loaded profile; `None` = transient edits.
    pub active_profile: Option<String>,
    /// Last explicitly chosen profile: next/prev anchor + auto-switch revert target.
    pub manual_profile: Option<String>,
    /// Class that triggered the current profile via auto-switch, if any.
    pub auto_class: Option<String>,
    /// Scheduling sub-state, maintained by the scheduler task.
    pub schedule: ScheduleStatus,
    pub tx: broadcast::Sender<Event>,
}

impl Daemon {
    pub fn new(cfg: Config, cfg_path: PathBuf) -> Self {
        let (tx, _) = broadcast::channel(64);
        let mode = if cfg.scheduling.enabled {
            format!("{:?}", cfg.scheduling.mode).to_lowercase()
        } else {
            "off".into()
        };
        Self {
            cfg,
            cfg_path,
            params: ColorParams::default(),
            enabled: true,
            active_profile: None,
            manual_profile: None,
            auto_class: None,
            schedule: ScheduleStatus {
                mode,
                next_transition_secs: None,
                blend: 0.0,
            },
            tx,
        }
    }

    pub fn shader_path(&self) -> PathBuf {
        expand_tilde(&self.cfg.hyprland.shader_path)
    }

    pub fn status(&self) -> Status {
        Status {
            enabled: self.enabled,
            active_profile: self.active_profile.clone(),
            params: self.params,
            scheduling: self.schedule.clone(),
            auto_switch: self.auto_class.clone(),
        }
    }

    /// Regenerate + write the shader, re-issue the keyword, persist, notify.
    pub async fn apply(&mut self) -> Result<()> {
        let path = self.shader_path();
        apply::write_shader(&path, &render_shader(&self.params))?;
        if self.enabled {
            apply::set_screen_shader(&path.to_string_lossy()).await?;
        } else {
            apply::set_screen_shader(apply::EMPTY_SHADER).await?;
        }
        self.persist();
        let _ = self.tx.send(Event::state(self.status()));
        Ok(())
    }

    /// Activate a named profile (case-insensitive). Marks it manual unless `auto`.
    pub fn load_profile(&mut self, name: &str, auto: Option<String>) -> Result<(), String> {
        let p = self
            .cfg
            .profile(name)
            .ok_or_else(|| format!("unknown profile: {name}"))?;
        let canonical = p.name.clone();
        self.params = p.params;
        self.active_profile = Some(canonical.clone());
        if auto.is_none() {
            self.manual_profile = Some(canonical);
        }
        self.auto_class = auto;
        Ok(())
    }

    fn cycle(&mut self, step: isize) -> Result<(), String> {
        if self.cfg.profiles.is_empty() {
            return Err("no profiles".into());
        }
        let len = self.cfg.profiles.len() as isize;
        let anchor = self
            .active_profile
            .as_deref()
            .or(self.manual_profile.as_deref());
        let idx = anchor
            .and_then(|n| {
                self.cfg
                    .profiles
                    .iter()
                    .position(|p| p.name.eq_ignore_ascii_case(n))
            })
            .map(|i| (i as isize + step).rem_euclid(len))
            .unwrap_or(0);
        let name = self.cfg.profiles[idx as usize].name.clone();
        self.load_profile(&name, None)
    }

    fn profile_names(&self) -> Vec<String> {
        self.cfg.profiles.iter().map(|p| p.name.clone()).collect()
    }

    fn save_config(&self) -> Result<(), String> {
        self.cfg.save(&self.cfg_path).map_err(|e| e.to_string())
    }

    /// Handle one request; returns the `data` payload or an error string.
    pub async fn dispatch(&mut self, cmd: Command) -> Result<Value, String> {
        match cmd {
            Command::Status | Command::Subscribe => Ok(json!(self.status())),
            Command::Get => Ok(json!(self.params)),
            Command::Set { param, value } => {
                self.params.set(param, value);
                self.active_profile = None;
                self.apply_ok().await?;
                Ok(json!(self.params))
            }
            Command::Nudge { param, delta } => {
                self.params.nudge(param, delta);
                self.active_profile = None;
                self.apply_ok().await?;
                Ok(json!(self.params))
            }
            Command::Apply { params } => {
                self.params = params.clamped();
                self.active_profile = None;
                self.apply_ok().await?;
                Ok(json!(self.params))
            }
            Command::On => {
                self.enabled = true;
                self.apply_ok().await?;
                Ok(json!(self.status()))
            }
            Command::Off => {
                self.enabled = false;
                self.apply_ok().await?;
                Ok(json!(self.status()))
            }
            Command::Toggle => {
                self.enabled = !self.enabled;
                self.apply_ok().await?;
                Ok(json!(self.status()))
            }
            Command::ProfileList => Ok(json!(self.profile_names())),
            Command::ProfileLoad { name } => {
                self.load_profile(&name, None)?;
                self.apply_ok().await?;
                Ok(json!(self.status()))
            }
            Command::ProfileSave { name, params } => {
                let params = params.map(|p| p.clamped()).unwrap_or(self.params);
                match self
                    .cfg
                    .profiles
                    .iter_mut()
                    .find(|p| p.name.eq_ignore_ascii_case(&name))
                {
                    Some(p) => p.params = params,
                    None => self
                        .cfg
                        .profiles
                        .push(narvi_core::Profile::new(name.clone(), params)),
                }
                self.save_config()?;
                self.params = params;
                self.active_profile = Some(name.clone());
                self.manual_profile = Some(name);
                self.apply_ok().await?;
                Ok(json!(self.profile_names()))
            }
            Command::ProfileDelete { name } => {
                let before = self.cfg.profiles.len();
                self.cfg
                    .profiles
                    .retain(|p| !p.name.eq_ignore_ascii_case(&name));
                if self.cfg.profiles.len() == before {
                    return Err(format!("unknown profile: {name}"));
                }
                if self
                    .active_profile
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(&name))
                {
                    self.active_profile = None;
                }
                if self
                    .manual_profile
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(&name))
                {
                    self.manual_profile = None;
                }
                self.save_config()?;
                let _ = self.tx.send(Event::state(self.status()));
                Ok(json!(self.profile_names()))
            }
            Command::ProfileNext => {
                self.cycle(1)?;
                self.apply_ok().await?;
                Ok(json!(self.status()))
            }
            Command::ProfilePrev => {
                self.cycle(-1)?;
                self.apply_ok().await?;
                Ok(json!(self.status()))
            }
            Command::Reload => {
                let cfg = Config::load(&self.cfg_path).map_err(|e| e.to_string())?;
                self.cfg = cfg;
                let _ = self.tx.send(Event::state(self.status()));
                Ok(json!(self.status()))
            }
        }
    }

    async fn apply_ok(&mut self) -> Result<(), String> {
        self.apply().await.map_err(|e| {
            log::warn!("apply failed: {e:#}");
            format!("apply failed: {e:#}")
        })
    }

    fn state_path() -> Option<PathBuf> {
        let dirs = directories::ProjectDirs::from("dev", "zer0dot", "narvi")?;
        let dir = dirs
            .state_dir()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| dirs.data_local_dir().to_path_buf());
        Some(dir.join("state.json"))
    }

    /// Best-effort save of the runtime snapshot; failures are logged, not fatal.
    fn persist(&self) {
        let Some(path) = Self::state_path() else {
            return;
        };
        let state = PersistState {
            enabled: self.enabled,
            active_profile: self.active_profile.clone(),
            params: self.params,
        };
        let write = || -> Result<()> {
            let dir = path.parent().context("no state dir")?;
            std::fs::create_dir_all(dir)?;
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, serde_json::to_vec(&state)?)?;
            std::fs::rename(&tmp, &path)?;
            Ok(())
        };
        if let Err(e) = write() {
            log::warn!("state persist failed: {e:#}");
        }
    }

    /// Restore the last snapshot (or the configured profile) on startup.
    pub fn restore(&mut self) {
        if !self.cfg.general.restore_on_start {
            return;
        }
        if let Some(path) = Self::state_path()
            && let Ok(bytes) = std::fs::read(&path)
            && let Ok(st) = serde_json::from_slice::<PersistState>(&bytes)
        {
            self.enabled = st.enabled;
            self.params = st.params.clamped();
            self.active_profile = st.active_profile.clone();
            self.manual_profile = st.active_profile;
            return;
        }
        let name = self.cfg.general.active_profile.clone();
        if self.load_profile(&name, None).is_err() {
            if let Some(first) = self.cfg.profiles.first() {
                let n = first.name.clone();
                let _ = self.load_profile(&n, None);
            } else {
                self.params = ColorParams::default();
            }
        }
    }
}
