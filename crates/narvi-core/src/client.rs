//! Blocking socket client shared by the CLI, GUI, and tray.
//!
//! std-only (`UnixStream`); GUI runs it on a worker thread.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use crate::proto::{Command, Event, Request, Response};
use crate::{Error, Result};

/// `$NARVI_SOCKET` override, else `$XDG_RUNTIME_DIR/narvi/narvid.sock`.
pub fn socket_path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("NARVI_SOCKET") {
        return Ok(PathBuf::from(p));
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .ok_or_else(|| Error::Config("XDG_RUNTIME_DIR unset; cannot locate socket".into()))?;
    Ok(PathBuf::from(runtime).join("narvi").join("narvid.sock"))
}

pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
}

impl Client {
    pub fn connect(path: &Path) -> Result<Self> {
        let stream = UnixStream::connect(path)?;
        let writer = stream.try_clone()?;
        Ok(Self {
            reader: BufReader::new(stream),
            writer,
            next_id: 1,
        })
    }

    /// Send one command, return its `data` payload (`Error::Daemon` on `ok:false`).
    pub fn request(&mut self, command: Command) -> Result<serde_json::Value> {
        let id = self.next_id;
        self.next_id += 1;
        let mut line = serde_json::to_vec(&Request { id, command })?;
        line.push(b'\n');
        self.writer.write_all(&line)?;
        // Skip any interleaved event lines (possible after `subscribe`).
        loop {
            let mut buf = String::new();
            if self.reader.read_line(&mut buf)? == 0 {
                return Err(Error::Daemon("connection closed by daemon".into()));
            }
            if let Ok(resp) = serde_json::from_str::<Response>(&buf)
                && resp.id == id
            {
                return match (resp.ok, resp.data, resp.error) {
                    (true, Some(data), _) => Ok(data),
                    (true, None, _) => Ok(serde_json::Value::Null),
                    (_, _, err) => Err(Error::Daemon(err.unwrap_or_else(|| "unknown".into()))),
                };
            }
        }
    }

    /// Block until the next pushed event (call after a `subscribe` request).
    pub fn next_event(&mut self) -> Result<Event> {
        loop {
            let mut buf = String::new();
            if self.reader.read_line(&mut buf)? == 0 {
                return Err(Error::Daemon("connection closed by daemon".into()));
            }
            if let Ok(ev) = serde_json::from_str::<Event>(&buf) {
                return Ok(ev);
            }
        }
    }
}
