//! Blocking socket client shared by the CLI, GUI, and tray.
//!
//! std-only (`UnixStream`); GUI runs it on a worker thread.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::proto::{Command, Event, Request, Response};
use crate::{Error, Result};

/// Cap on each read while awaiting a response, so a daemon that accepted
/// the connection but never serves (e.g. stalled startup) cannot hang us.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

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
    /// Per-read cap inside [`request`]; [`next_event`] stays unbounded.
    request_timeout: Duration,
    /// Events read while waiting for a response; drained by [`next_event`].
    pending: std::collections::VecDeque<Event>,
}

impl Client {
    pub fn connect(path: &Path) -> Result<Self> {
        let stream = UnixStream::connect(path)?;
        let writer = stream.try_clone()?;
        Ok(Self {
            reader: BufReader::new(stream),
            writer,
            next_id: 1,
            request_timeout: REQUEST_TIMEOUT,
            pending: Default::default(),
        })
    }

    fn read_line(&mut self) -> Result<String> {
        let mut buf = String::new();
        if self.reader.read_line(&mut buf)? == 0 {
            return Err(Error::Daemon("connection closed by daemon".into()));
        }
        Ok(buf)
    }

    /// Send one command, return its `data` payload (`Error::Daemon` on `ok:false`).
    /// Event lines arriving in between are queued for [`next_event`].
    /// Reads are capped at [`REQUEST_TIMEOUT`] so a stalled daemon errors
    /// out instead of hanging the caller forever.
    pub fn request(&mut self, command: Command) -> Result<serde_json::Value> {
        self.reader
            .get_ref()
            .set_read_timeout(Some(self.request_timeout))?;
        let result = self.request_inner(command);
        // Best effort: next_event must go back to blocking indefinitely.
        if let Err(e) = self.reader.get_ref().set_read_timeout(None) {
            log::warn!("could not clear socket read timeout: {e}");
        }
        result
    }

    fn request_inner(&mut self, command: Command) -> Result<serde_json::Value> {
        let id = self.next_id;
        self.next_id += 1;
        let mut line = serde_json::to_vec(&Request { id, command })?;
        line.push(b'\n');
        self.writer.write_all(&line)?;
        loop {
            let buf = self.read_line()?;
            if let Ok(resp) = serde_json::from_str::<Response>(&buf) {
                if resp.id != id {
                    continue;
                }
                return match (resp.ok, resp.data, resp.error) {
                    (true, Some(data), _) => Ok(data),
                    (true, None, _) => Ok(serde_json::Value::Null),
                    (_, _, err) => Err(Error::Daemon(err.unwrap_or_else(|| "unknown".into()))),
                };
            }
            if let Ok(ev) = serde_json::from_str::<Event>(&buf) {
                self.pending.push_back(ev);
            }
        }
    }

    /// Block until the next pushed event (call after a `subscribe` request).
    pub fn next_event(&mut self) -> Result<Event> {
        if let Some(ev) = self.pending.pop_front() {
            return Ok(ev);
        }
        loop {
            let buf = self.read_line()?;
            if let Ok(ev) = serde_json::from_str::<Event>(&buf) {
                return Ok(ev);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::time::Instant;

    #[test]
    fn request_times_out_when_daemon_never_responds() {
        // Bound-but-silent listener: connects land in the backlog, no reply.
        let path =
            std::env::temp_dir().join(format!("narvi-client-test-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let _listener = UnixListener::bind(&path).unwrap();
        let mut client = Client::connect(&path).unwrap();
        client.request_timeout = Duration::from_millis(100);
        let t0 = Instant::now();
        let res = client.request(Command::Status);
        assert!(res.is_err(), "request must fail instead of hanging");
        assert!(t0.elapsed() < Duration::from_secs(5), "hung too long");
        let _ = std::fs::remove_file(&path);
    }
}
