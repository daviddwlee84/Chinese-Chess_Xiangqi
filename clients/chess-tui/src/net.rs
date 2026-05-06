//! Sync `tungstenite` ws client driven from a background OS thread, with
//! `std::sync::mpsc` channels to/from the (sync) TUI loop. We deliberately
//! keep tokio out of the TUI binary — `crossterm::event::poll` is sync, and
//! mixing in a runtime would force a refactor.
//!
//! The thread does a non-blocking poll cycle: try one ws read, drain pending
//! outbound `ClientMsg`s, sleep ~20ms. Every parsed `ServerMsg` (or fatal
//! error) is shipped to the TUI as a `NetEvent`.

use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{anyhow, Result};
use chess_net::{ClientMsg, ServerMsg};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

const POLL_INTERVAL: Duration = Duration::from_millis(20);
const CONNECT_ATTEMPTS: u32 = 8;
const CONNECT_BACKOFF: Duration = Duration::from_millis(200);

#[derive(Debug)]
pub enum NetEvent {
    Connected,
    Server(Box<ServerMsg>),
    Disconnected(String),
}

pub struct NetClient {
    pub cmd_tx: Sender<ClientMsg>,
    pub evt_rx: Receiver<NetEvent>,
    _handle: JoinHandle<()>,
}

impl NetClient {
    /// Spawns the worker thread and returns immediately. The caller polls
    /// `evt_rx` each frame (use `try_recv`, never `recv` — the TUI loop must
    /// not block) and pushes outbound moves via `cmd_tx`.
    pub fn spawn(url: String) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<ClientMsg>();
        let (evt_tx, evt_rx) = mpsc::channel::<NetEvent>();
        let handle = thread::spawn(move || run(url, cmd_rx, evt_tx));
        Self { cmd_tx, evt_rx, _handle: handle }
    }
}

fn run(url: String, cmd_rx: Receiver<ClientMsg>, evt_tx: Sender<NetEvent>) {
    let mut ws = match connect_with_retry(&url) {
        Ok(ws) => ws,
        Err(e) => {
            let _ = evt_tx.send(NetEvent::Disconnected(format!("connect failed: {e}")));
            return;
        }
    };
    if let Err(e) = set_nonblocking(&mut ws) {
        let _ = evt_tx.send(NetEvent::Disconnected(format!("set_nonblocking: {e}")));
        return;
    }
    let _ = evt_tx.send(NetEvent::Connected);

    loop {
        // Inbound: try to read one frame.
        match ws.read() {
            Ok(Message::Text(t)) => match serde_json::from_str::<ServerMsg>(t.as_str()) {
                Ok(msg) => {
                    if evt_tx.send(NetEvent::Server(Box::new(msg))).is_err() {
                        return; // TUI dropped the receiver
                    }
                }
                Err(e) => {
                    let _ = evt_tx.send(NetEvent::Disconnected(format!("decode ServerMsg: {e}")));
                    return;
                }
            },
            Ok(Message::Close(_)) => {
                let _ = evt_tx.send(NetEvent::Disconnected("server closed".into()));
                return;
            }
            Ok(_) => {} // ping/pong/binary — ignore, tungstenite handles ping/pong itself
            Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => {
                let _ = evt_tx.send(NetEvent::Disconnected(format!("read: {e}")));
                return;
            }
        }

        // Outbound: drain whatever's queued.
        loop {
            match cmd_rx.try_recv() {
                Ok(msg) => {
                    let json = match serde_json::to_string(&msg) {
                        Ok(j) => j,
                        Err(e) => {
                            let _ = evt_tx.send(NetEvent::Disconnected(format!("encode: {e}")));
                            return;
                        }
                    };
                    if let Err(e) = ws.send(Message::Text(json)) {
                        let _ = evt_tx.send(NetEvent::Disconnected(format!("send: {e}")));
                        return;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let _ = ws.close(None);
                    return;
                }
            }
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn connect_with_retry(url: &str) -> Result<WebSocket<MaybeTlsStream<TcpStream>>> {
    let mut last_err: Option<tungstenite::Error> = None;
    for _ in 0..CONNECT_ATTEMPTS {
        match tungstenite::connect(url) {
            Ok((ws, _resp)) => return Ok(ws),
            Err(e) => {
                last_err = Some(e);
                thread::sleep(CONNECT_BACKOFF);
            }
        }
    }
    Err(anyhow!(
        "could not reach {url} after {} attempts: {}",
        CONNECT_ATTEMPTS,
        last_err.map(|e| e.to_string()).unwrap_or_default()
    ))
}

fn set_nonblocking(ws: &mut WebSocket<MaybeTlsStream<TcpStream>>) -> std::io::Result<()> {
    match ws.get_mut() {
        MaybeTlsStream::Plain(tcp) => tcp.set_nonblocking(true),
        // Other MaybeTlsStream variants (rustls/native-tls) only appear when
        // those features are enabled; we don't enable them.
        _ => Ok(()),
    }
}
