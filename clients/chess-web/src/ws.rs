//! WebSocket pump for chess-net protocol. One read task fans server frames
//! into a `ServerMsg` signal; one write task drains an mpsc of `ClientMsg`s
//! to the server.
//!
//! No reconnect in PR-1 — see `backlog/web-ws-reconnect.md` for the planned
//! exponential-backoff retry. On disconnect or error, the conn-state signal
//! flips to `Closed`/`Error` and the page surfaces a "reload" toast.

use chess_net::protocol::{ClientMsg, ServerMsg};
use futures::channel::mpsc::{unbounded, UnboundedSender};
use futures::{SinkExt, StreamExt};
use gloo_net::websocket::{futures::WebSocket, Message};
use leptos::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnState {
    Connecting,
    Open,
    Closed,
    Error,
}

#[derive(Clone)]
pub struct WsHandle {
    sender: UnboundedSender<ClientMsg>,
}

impl WsHandle {
    /// Send a `ClientMsg`. Returns false if the write pump has shut down.
    pub fn send(&self, msg: ClientMsg) -> bool {
        self.sender.unbounded_send(msg).is_ok()
    }
}

/// Open a WS connection. Returns:
///   * a handle to send `ClientMsg`s,
///   * a signal that latches the most recent `ServerMsg`,
///   * a connection-state signal.
pub fn connect(url: String) -> (WsHandle, ReadSignal<Option<ServerMsg>>, ReadSignal<ConnState>) {
    let (incoming, set_incoming) = create_signal::<Option<ServerMsg>>(None);
    let (state, set_state) = create_signal(ConnState::Connecting);
    let (out_tx, mut out_rx) = unbounded::<ClientMsg>();

    match WebSocket::open(&url) {
        Ok(ws) => {
            let (mut tx, mut rx) = ws.split();
            set_state.set(ConnState::Open);

            // Read pump — server frames → signal.
            wasm_bindgen_futures::spawn_local(async move {
                while let Some(msg) = rx.next().await {
                    match msg {
                        Ok(Message::Text(t)) => {
                            if let Ok(m) = serde_json::from_str::<ServerMsg>(&t) {
                                set_incoming.set(Some(m));
                            }
                        }
                        Ok(Message::Bytes(_)) => {}
                        Err(_) => {
                            set_state.set(ConnState::Error);
                            return;
                        }
                    }
                }
                set_state.set(ConnState::Closed);
            });

            // Write pump — drain mpsc → server.
            wasm_bindgen_futures::spawn_local(async move {
                while let Some(cm) = out_rx.next().await {
                    let body = match serde_json::to_string(&cm) {
                        Ok(b) => b,
                        Err(_) => continue,
                    };
                    if tx.send(Message::Text(body)).await.is_err() {
                        break;
                    }
                }
            });
        }
        Err(_) => {
            set_state.set(ConnState::Error);
        }
    }

    (WsHandle { sender: out_tx }, incoming, state)
}
