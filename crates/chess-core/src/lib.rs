//! `chess-core` — pure game logic for Chinese chess variants.
//!
//! This crate is the engine: boards, pieces, rules, and game state.
//! It has no IO, no rendering, no platform deps — compiles cleanly to WASM.

#![warn(missing_debug_implementations, rust_2018_idioms)]

pub mod board;
pub mod coord;
pub mod error;
pub mod moves;
pub mod notation;
pub mod piece;
pub mod prelude;
pub mod replay;
pub mod rules;
pub mod setup;
pub mod snapshot;
pub mod state;
pub mod view;
