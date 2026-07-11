//! Unix-socket server: newline-delimited JSON, request→response, plus pushed
//! `state` events to connections that sent `subscribe`.

use std::sync::Arc;

use anyhow::{Context, Result};
use narvi_core::proto::{Command, Event, Request, Response};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, broadcast};

use crate::state::Daemon;

/// Take the exclusive daemon lock (flock); `None` = another narvid has it.
/// Held for the process lifetime, it makes stale-socket cleanup race-free.
pub fn try_lock(path: &std::path::Path) -> Result<Option<std::fs::File>> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => {
            Err(e).with_context(|| format!("lock {}", path.display()))
        }
    }
}

/// Bind the listener. Caller holds the instance lock, so an existing socket
/// file is stale by definition; the connect probe is defense in depth.
/// `None` = a live daemon (a pre-lock version) still owns the socket; the
/// caller should exit 0 so `Restart=on-failure` is not tripped.
pub async fn bind(path: &std::path::Path) -> Result<Option<UnixListener>> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if path.exists() {
        if UnixStream::connect(path).await.is_ok() {
            return Ok(None);
        }
        std::fs::remove_file(path)?; // stale socket from a dead daemon
    }
    UnixListener::bind(path)
        .map(Some)
        .with_context(|| format!("bind {}", path.display()))
}

pub async fn serve(listener: UnixListener, daemon: Arc<Mutex<Daemon>>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let daemon = daemon.clone();
                tokio::spawn(async move {
                    if let Err(e) = connection(stream, daemon).await {
                        log::debug!("connection ended: {e:#}");
                    }
                });
            }
            Err(e) => log::warn!("accept failed: {e}"),
        }
    }
}

async fn connection(stream: UnixStream, daemon: Arc<Mutex<Daemon>>) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let mut events: Option<broadcast::Receiver<Event>> = None;

    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else { break };
                if line.trim().is_empty() {
                    continue;
                }
                let response = match serde_json::from_str::<Request>(&line) {
                    Ok(req) => {
                        let subscribe = matches!(req.command, Command::Subscribe);
                        let result = daemon.lock().await.dispatch(req.command).await;
                        if subscribe && events.is_none() {
                            events = Some(daemon.lock().await.tx.subscribe());
                        }
                        match result {
                            Ok(data) => Response { id: req.id, ok: true, data: Some(data), error: None },
                            Err(e) => Response::err(req.id, e),
                        }
                    }
                    Err(e) => Response::err(0, format!("bad request: {e}")),
                };
                let mut out = serde_json::to_vec(&response)?;
                out.push(b'\n');
                writer.write_all(&out).await?;
            }
            ev = recv(&mut events), if events.is_some() => {
                match ev {
                    Ok(ev) => {
                        let mut out = serde_json::to_vec(&ev)?;
                        out.push(b'\n');
                        writer.write_all(&out).await?;
                    }
                    // Lagged: skip missed events, the next one carries full state.
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
    Ok(())
}

async fn recv(
    rx: &mut Option<broadcast::Receiver<Event>>,
) -> Result<Event, broadcast::error::RecvError> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::{bind, try_lock};

    #[tokio::test]
    async fn bind_yields_none_when_socket_is_live() {
        let path = std::env::temp_dir().join(format!("narvid-live-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let _live = tokio::net::UnixListener::bind(&path).unwrap();
        // A connectable socket means another daemon owns it: not an error.
        assert!(bind(&path).await.unwrap().is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn bind_replaces_stale_socket_file() {
        let path = std::env::temp_dir().join(format!("narvid-stale-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        drop(tokio::net::UnixListener::bind(&path).unwrap()); // file stays
        assert!(bind(&path).await.unwrap().is_some());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lock_is_exclusive_until_released() {
        let path = std::env::temp_dir().join(format!("narvid-test-{}.lock", std::process::id()));
        let first = try_lock(&path).unwrap();
        assert!(first.is_some());
        // Second open file description must be refused while the first lives.
        assert!(try_lock(&path).unwrap().is_none());
        drop(first);
        assert!(try_lock(&path).unwrap().is_some());
        let _ = std::fs::remove_file(&path);
    }
}
