//! Daemon socket protocol: newline-delimited JSON, one object per line.
//!
//! Socket path: `$XDG_RUNTIME_DIR/narvi/narvid.sock` (`SOCK_STREAM`). Clients send a
//! [`Request`] and read a [`Response`]; subscribers additionally read [`Event`] lines.

use serde::{Deserialize, Serialize};

use crate::params::{ColorParams, Param};

/// One request line. `id` is a client-chosen correlation integer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    #[serde(flatten)]
    pub command: Command,
}

/// All daemon commands (see PROTOCOL.md). Tagged by `cmd`, payload under `args`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case")]
pub enum Command {
    Status,
    Get,
    Set {
        param: Param,
        value: f32,
    },
    Nudge {
        param: Param,
        delta: f32,
    },
    Apply {
        params: ColorParams,
    },
    On,
    Off,
    Toggle,
    #[serde(rename = "profile.list")]
    ProfileList,
    #[serde(rename = "profile.load")]
    ProfileLoad {
        name: String,
    },
    #[serde(rename = "profile.save")]
    ProfileSave {
        name: String,
        params: Option<ColorParams>,
    },
    #[serde(rename = "profile.delete")]
    ProfileDelete {
        name: String,
    },
    #[serde(rename = "profile.next")]
    ProfileNext,
    #[serde(rename = "profile.prev")]
    ProfilePrev,
    Subscribe,
    Reload,
}

/// One response line. Exactly one of `data` / `error` is set per `ok`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok(id: u64, data: impl Serialize) -> crate::Result<Self> {
        Ok(Self {
            id,
            ok: true,
            data: Some(serde_json::to_value(data)?),
            error: None,
        })
    }

    pub fn err(id: u64, error: impl Into<String>) -> Self {
        Self {
            id,
            ok: false,
            data: None,
            error: Some(error.into()),
        }
    }
}

/// Server-pushed line to subscribers, emitted after ANY state change.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub event: String,
    pub data: Status,
}

impl Event {
    pub fn state(status: Status) -> Self {
        Self {
            event: "state".into(),
            data: status,
        }
    }
}

/// Full daemon state snapshot.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub enabled: bool,
    /// `None` = transient/unsaved edits.
    pub active_profile: Option<String>,
    pub params: ColorParams,
    pub scheduling: ScheduleStatus,
    /// Window class that triggered the current profile via auto-switch, if any.
    pub auto_switch: Option<String>,
}

/// Scheduling sub-state surfaced in [`Status`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScheduleStatus {
    pub mode: String,
    /// Seconds until the next transition, if scheduled.
    pub next_transition_secs: Option<u64>,
    /// Current day↔night blend in `[0,1]` (0 = day, 1 = night).
    pub blend: f32,
}
