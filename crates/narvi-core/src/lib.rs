//! Narvi core contracts.
//!
//! Single source of truth for color state, shader generation, config, and the daemon
//! socket protocol. The daemon and all clients depend on these types and never duplicate
//! param logic. M0 freezes the public API; bodies marked `todo!()` land in M1–M4.

pub mod config;
pub mod error;
pub mod kelvin;
pub mod params;
pub mod profile;
pub mod proto;
pub mod shader;

pub use config::Config;
pub use error::{Error, Result};
pub use params::{ColorParams, Param};
pub use profile::Profile;
pub use proto::{Command, Event, Request, Response, ScheduleStatus, Status};
