//! `chess-web` — Leptos + WASM frontend for chess-core. Scaffold commit:
//! variant picker + routes + page stubs. The board renderer and online
//! play wire up in follow-up commits.
//!
//! Native (`cargo check --workspace`) compiles only the pure-logic modules
//! (`routes`); the leptos UI is wasm32-gated so the workspace target check
//! stays clean. Build for the browser with `trunk serve` (or
//! `cargo build --target wasm32-unknown-unknown -p chess-web`).

pub mod routes;

#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
mod pages;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    leptos::mount_to_body(app::App);
}
