//! `chess-web` — Leptos + WASM frontend for chess-core.
//!
//! Native (`cargo check --workspace`) compiles only the pure-logic modules
//! (orient, glyph, routes, state); the leptos UI and the WS layer are
//! wasm32-gated so the workspace target check stays clean. Build for the
//! browser with `trunk serve` (or `cargo build --target wasm32-unknown-unknown
//! -p chess-web`).

pub mod eval;
pub mod glyph;
pub mod net_diag;
pub mod orient;
pub mod routes;
pub mod state;
pub mod time;

#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
mod beforeunload;
#[cfg(target_arch = "wasm32")]
mod camera;
#[cfg(target_arch = "wasm32")]
mod components;
#[cfg(target_arch = "wasm32")]
mod config;
#[cfg(target_arch = "wasm32")]
mod host_room;
#[cfg(target_arch = "wasm32")]
mod pages;
#[cfg(target_arch = "wasm32")]
mod prefs;
#[cfg(target_arch = "wasm32")]
mod qr_decode;
#[cfg(target_arch = "wasm32")]
mod spike;
#[cfg(target_arch = "wasm32")]
mod transport;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    leptos::mount_to_body(app::App);
}
