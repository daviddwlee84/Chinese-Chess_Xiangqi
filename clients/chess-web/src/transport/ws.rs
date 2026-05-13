//! WebSocket transport for chess-net protocol.
//!
//! One read task fans server frames into the [`Session::incoming`] signal;
//! one write task drains the per-handle mpsc into the WebSocket sink.
//!
//! No reconnect — see `backlog/web-ws-reconnect.md` for the planned
//! exponential-backoff retry. On disconnect or error the
//! [`Session::state`] signal flips to `Closed` / `Error` and the page
//! surfaces a "reload" toast.
//!
//! As of Phase 2 of `backlog/webrtc-lan-pairing.md` this lives under
//! `transport/` and exports a [`WsTransport`] implementing the shared
//! [`super::Transport`] trait, so the play / lobby pages don't care
//! whether they're talking to chess-net or (eventually) a WebRTC peer.

use std::rc::Rc;

use chess_net::protocol::{ClientMsg, ServerMsg};
use futures::channel::mpsc::{unbounded, UnboundedSender};
use futures::{SinkExt, StreamExt};
use gloo_net::websocket::{futures::WebSocket, Message};
use leptos::*;

use super::{ConnState, Incoming, Session, Transport};

/// `Transport` impl backed by a single `gloo-net` WebSocket. Holds only
/// the outbound mpsc sender; the read pump lives in a `spawn_local` task
/// owned by the [`connect`] call's lifetime.
pub struct WsTransport {
    sender: UnboundedSender<ClientMsg>,
}

impl Transport for WsTransport {
    fn send(&self, msg: ClientMsg) -> bool {
        self.sender.unbounded_send(msg).is_ok()
    }
}

/// Open a WS connection. Spawns the read + write pumps and returns a
/// `Session` whose `handle` will succeed at `send(...)` until the
/// underlying socket closes.
pub fn connect(url: String) -> Session {
    // `incoming` is a queue (push from read pump, page drains via
    // tick signal). See `transport::Incoming` doc for the rationale.
    let incoming = Incoming::new();
    let (state, set_state) = create_signal(ConnState::Connecting);
    let (out_tx, mut out_rx) = unbounded::<ClientMsg>();

    match WebSocket::open(&url) {
        Ok(ws) => {
            let (mut tx, mut rx) = ws.split();
            set_state.set(ConnState::Open);

            // Read pump — server frames → queue.
            let incoming_for_read = incoming.clone();
            wasm_bindgen_futures::spawn_local(async move {
                while let Some(msg) = rx.next().await {
                    match msg {
                        Ok(Message::Text(t)) => {
                            if let Ok(m) = serde_json::from_str::<ServerMsg>(&t) {
                                incoming_for_read.push(m);
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

    Session { handle: Rc::new(WsTransport { sender: out_tx }), incoming, state }
}
