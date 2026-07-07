//! Unix-socket server: newline-delimited JSON, request→response, plus pushed
//! `state` events to connections that sent `subscribe`.

use std::sync::Arc;

use anyhow::{Context, Result};
use narvi_core::proto::{Command, Event, Request, Response};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, broadcast};

use crate::state::Daemon;

/// Bind the listener, refusing to clobber a live daemon's socket.
pub async fn bind(path: &std::path::Path) -> Result<UnixListener> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if path.exists() {
        if UnixStream::connect(path).await.is_ok() {
            anyhow::bail!("narvid already running on {}", path.display());
        }
        std::fs::remove_file(path)?; // stale socket from a dead daemon
    }
    UnixListener::bind(path).with_context(|| format!("bind {}", path.display()))
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
