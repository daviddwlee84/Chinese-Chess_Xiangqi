//! User-facing FX preferences, persisted in `localStorage`.
//!
//! Switches (all default ON or to a recommended value):
//! * `fx_confetti` — show end-of-game confetti + VICTORY/DEFEAT/DRAW overlay.
//! * `fx_check_banner` — show the "將軍 / CHECK" badge in the sidebar when
//!   the side-to-move is in check (xiangqi only).
//! * `fx_threat_mode` — board-overlay highlight for pieces under threat.
//!   See `ThreatMode` for the four levels (Off / Attacked / NetLoss /
//!   MateThreat); default `NetLoss` ('被捉' — visually clean enough to
//!   leave on without burying the board in red rings).
//! * `fx_threat_hover` — secondary toggle: when a player's own piece is
//!   hovered/selected, recompute and show what the opponent could
//!   capture if that piece *didn't move* (a 'what-if I leave this here?'
//!   helper). Default OFF — orthogonal to the mode selector above.
//! * `fx_last_move` — soft blue ring around the from/to squares of the
//!   most recent move so you can see what the opponent just did
//!   without having to scrutinise the board diff. Default ON
//!   (high-value, low-noise — only ever rings two squares at a time).
//!
//! Storage keys are stable so a user toggling once carries the choice
//! across sessions. Missing values read as the recommended default
//! (true / `NetLoss` / false / true). We persist `"1"` / `"0"` rather
//! than booleans so future debugging via DevTools is obvious.
//!
//! All `web_sys` calls are wrapped in `Option`/`Result` chains so a
//! sandboxed context with no `window` (or storage disabled) silently
//! falls back to in-memory defaults — no panics.
//!
//! This module is wasm32-only; the workspace native check skips it.

use leptos::*;

use crate::state::CapturedSort;

const KEY_CONFETTI: &str = "chess.fx.confetti";
const KEY_CHECK_BANNER: &str = "chess.fx.checkBanner";
const KEY_CAPTURED_SORT: &str = "chess.ui.capturedSort";
const KEY_THREAT_MODE: &str = "chess.fx.threatMode";
const KEY_THREAT_HOVER: &str = "chess.fx.threatHover";
const KEY_LAST_MOVE: &str = "chess.fx.lastMove";

/// Which "threat highlight" overlay to draw on the board. Off by
/// historical default (this feature is new); the picker UI defaults
/// fresh users to `NetLoss` because that's the one that's visually
/// clean enough to leave on without burying the board in red rings.
///
/// See `crate::components::board::threat_marker` for the SVG render
/// path and `chess_core::view::ThreatInfo` for the underlying data.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ThreatMode {
    /// No overlay — feature off.
    Off,
    /// Highlight every piece an opponent could capture in one ply
    /// (mode A, '被攻擊'). May be visually busy; useful when learning
    /// or doing tactics drills.
    Attacked,
    /// Highlight only pieces whose Static Exchange Evaluation predicts
    /// material loss (mode B, '被捉'). Recommended default.
    NetLoss,
    /// Highlight the opponent piece(s) participating in a
    /// checkmate-in-1 threat (mode C, '叫殺'). Xiangqi only — the
    /// underlying engine helper returns empty for banqi.
    MateThreat,
}

impl Default for ThreatMode {
    fn default() -> Self {
        Self::NetLoss
    }
}

impl ThreatMode {
    /// Stable storage / URL serialization. Kept short and lowercased
    /// so the localStorage values stay grep-able in DevTools.
    pub fn as_str(self) -> &'static str {
        match self {
            ThreatMode::Off => "off",
            ThreatMode::Attacked => "attacked",
            ThreatMode::NetLoss => "netLoss",
            ThreatMode::MateThreat => "mateThreat",
        }
    }

    /// Inverse of `as_str`; unknown / legacy values fall back to the
    /// recommended default (`NetLoss`) so future renames don't break
    /// stored prefs.
    pub fn from_str(s: &str) -> Self {
        match s {
            "off" => ThreatMode::Off,
            "attacked" => ThreatMode::Attacked,
            "mateThreat" => ThreatMode::MateThreat,
            _ => ThreatMode::NetLoss,
        }
    }
}

#[derive(Copy, Clone)]
pub struct Prefs {
    pub fx_confetti: RwSignal<bool>,
    pub fx_check_banner: RwSignal<bool>,
    /// Sort order for the sidebar captured-pieces panel. Persists
    /// across sessions; toggled via the panel header button.
    pub captured_sort: RwSignal<CapturedSort>,
    /// Which threat-highlight mode to render on the board. See
    /// `ThreatMode` for the four options. Persists across sessions.
    pub fx_threat_mode: RwSignal<ThreatMode>,
    /// 'What-if' hover preview: when ON and the user hovers/selects
    /// one of their own pieces, recompute and overlay the opponent
    /// pieces that could capture targets the hovered piece is
    /// currently defending. Orthogonal to `fx_threat_mode` — they
    /// stack visually if both are active.
    pub fx_threat_hover: RwSignal<bool>,
    /// "Highlight latest move" overlay: rings the from/to squares
    /// of the most recent move so the user can see what the
    /// opponent just played without scrutinising the board diff.
    /// Default ON — only ever rings two squares at a time, very low
    /// visual cost. Reads `view.last_move` (engine-projected;
    /// `EndChain` filtered out).
    pub fx_last_move: RwSignal<bool>,
}

impl Prefs {
    /// Hydrate from `localStorage` and arm `create_effect` watchers that
    /// persist any future change. Designed to be called once at app
    /// boot and `provide_context`-shared with all routes.
    pub fn hydrate() -> Self {
        let fx_confetti = create_rw_signal(read_bool(KEY_CONFETTI, true));
        let fx_check_banner = create_rw_signal(read_bool(KEY_CHECK_BANNER, true));
        let captured_sort = create_rw_signal(read_captured_sort());
        let fx_threat_mode = create_rw_signal(read_threat_mode());
        let fx_threat_hover = create_rw_signal(read_bool(KEY_THREAT_HOVER, false));
        let fx_last_move = create_rw_signal(read_bool(KEY_LAST_MOVE, true));

        // Persist on change. The closures only run inside the browser, so
        // calling `localStorage` on every flip is safe and cheap.
        create_effect(move |_| {
            let v = fx_confetti.get();
            write_bool(KEY_CONFETTI, v);
        });
        create_effect(move |_| {
            let v = fx_check_banner.get();
            write_bool(KEY_CHECK_BANNER, v);
        });
        create_effect(move |_| {
            let v = captured_sort.get();
            write_captured_sort(v);
        });
        create_effect(move |_| {
            let v = fx_threat_mode.get();
            write_threat_mode(v);
        });
        create_effect(move |_| {
            let v = fx_threat_hover.get();
            write_bool(KEY_THREAT_HOVER, v);
        });
        create_effect(move |_| {
            let v = fx_last_move.get();
            write_bool(KEY_LAST_MOVE, v);
        });

        Self {
            fx_confetti,
            fx_check_banner,
            captured_sort,
            fx_threat_mode,
            fx_threat_hover,
            fx_last_move,
        }
    }
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

fn read_bool(key: &str, default_value: bool) -> bool {
    let Some(storage) = local_storage() else { return default_value };
    match storage.get_item(key) {
        Ok(Some(s)) => s != "0",
        _ => default_value,
    }
}

fn write_bool(key: &str, value: bool) {
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(key, if value { "1" } else { "0" });
    }
}

fn read_captured_sort() -> CapturedSort {
    let Some(storage) = local_storage() else { return CapturedSort::default() };
    match storage.get_item(KEY_CAPTURED_SORT) {
        Ok(Some(s)) if s == "rank" => CapturedSort::Rank,
        _ => CapturedSort::Time,
    }
}

fn write_captured_sort(value: CapturedSort) {
    if let Some(storage) = local_storage() {
        let s = match value {
            CapturedSort::Time => "time",
            CapturedSort::Rank => "rank",
        };
        let _ = storage.set_item(KEY_CAPTURED_SORT, s);
    }
}

fn read_threat_mode() -> ThreatMode {
    let Some(storage) = local_storage() else { return ThreatMode::default() };
    match storage.get_item(KEY_THREAT_MODE) {
        Ok(Some(s)) => ThreatMode::from_str(&s),
        _ => ThreatMode::default(),
    }
}

fn write_threat_mode(value: ThreatMode) {
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(KEY_THREAT_MODE, value.as_str());
    }
}
