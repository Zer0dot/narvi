//! Typed errors for `narvi-core`. Binaries wrap these with `anyhow` at the edges.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("toml parse: {0}")]
    TomlDe(#[from] toml::de::Error),

    #[error("toml serialize: {0}")]
    TomlSer(#[from] toml::ser::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("unknown param: {0}")]
    UnknownParam(String),

    #[error("unknown profile: {0}")]
    UnknownProfile(String),

    #[error("config: {0}")]
    Config(String),
}
