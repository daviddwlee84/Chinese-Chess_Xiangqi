//! Full-screen "VICTORY / DEFEAT / DRAW" celebration overlay.
//!
//! The overlay is gated by:
//!   1. the user's `fx_confetti` pref (default ON), and
//!   2. an Ongoing → Won/Drawn status transition.
//!
//! Spectators (and Local pass-and-play, which has no `role`) get neutral
//! "Red Wins / Black Wins / Draw" copy. Net players see the personalised
//! "VICTORY" / "DEFEAT" framing. The overlay auto-dismisses after ~3s
//! via `set_timeout`; the matching sidebar status text stays as a
//! permanent record of the result.
//!
//! The component owns a `prev_status` signal so a remount (e.g. lobby
//! navigation) doesn't re-fire on the persistent end state. New games
//! reset the tracker via the Ongoing → ended check.

use std::time::Duration;

use chess_core::piece::Side;
use chess_core::state::{DrawReason, GameStatus, WinReason};
use chess_core::view::PlayerView;
use leptos::*;

use crate::components::confetti::Confetti;
use crate::state::ClientRole;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum BannerKind {
    Victory,
    Defeat,
    Draw,
    NeutralWin(Side),
}

/// Which half of the board pane this overlay covers in mirror mode.
/// `None` = full-screen overlay (default, non-mirror behaviour).
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum MirrorHalf {
    /// Top half, rotated 180°. Reads upright from the Black seat.
    Top,
    /// Bottom half, upright. Reads from the Red seat.
    Bottom,
}

#[component]
pub fn EndOverlay(
    /// Live `PlayerView` — drives the status transition watcher.
    #[prop(into)]
    view: Signal<PlayerView>,
    /// `None` for Local pass-and-play (uses neutral copy); `Some(role)` for Net.
    #[prop(into)]
    role: Signal<Option<ClientRole>>,
    /// User pref — when false the overlay never mounts.
    #[prop(into)]
    enabled: Signal<bool>,
    /// `Some(half)` enables mirror mode: overlay covers half the board pane,
    /// addresses the seat that half faces (top → Black, bottom → Red), and
    /// the top half is rotated 180° so it reads upright from the far seat.
    #[prop(default = None)]
    half: Option<MirrorHalf>,
) -> impl IntoView {
    let (visible, set_visible) = create_signal(false);
    let (kind_signal, set_kind) = create_signal::<Option<BannerKind>>(None);
    let prev_status = create_rw_signal::<Option<GameStatus>>(None);

    create_effect(move |_| {
        let cur = view.with(|v| v.status);
        let prev = prev_status.get_untracked();
        let was_ongoing = matches!(prev, Some(GameStatus::Ongoing));
        let now_ended = matches!(cur, GameStatus::Won { .. } | GameStatus::Drawn { .. });

        if was_ongoing && now_ended && enabled.get_untracked() {
            // Mirror-mode overlays force a synthetic seat role so each
            // half gets personalised VICTORY/DEFEAT copy regardless of
            // the page's actual `role` (Local pass-and-play is `None`).
            let effective_role = match half {
                Some(MirrorHalf::Top) => Some(ClientRole::Player(Side::BLACK)),
                Some(MirrorHalf::Bottom) => Some(ClientRole::Player(Side::RED)),
                None => role.get_untracked(),
            };
            let resolved_kind = resolve_kind(&cur, effective_role);
            set_kind.set(resolved_kind);
            set_visible.set(true);
            // Auto-dismiss after the confetti animation finishes. Use
            // `set_timeout` rather than CSS animation events so the
            // banner fade and the confetti fade share one source of truth.
            set_timeout(
                move || {
                    set_visible.set(false);
                },
                Duration::from_millis(3200),
            );
        }
        prev_status.set(Some(cur));
    });

    let title = move || match kind_signal.get() {
        Some(BannerKind::Victory) => "VICTORY".to_string(),
        Some(BannerKind::Defeat) => "DEFEAT".to_string(),
        Some(BannerKind::Draw) => "DRAW".to_string(),
        Some(BannerKind::NeutralWin(Side::RED)) => "Red Wins 紅勝".to_string(),
        Some(BannerKind::NeutralWin(Side::BLACK)) => "Black Wins 黑勝".to_string(),
        Some(BannerKind::NeutralWin(_)) => "Green Wins 綠勝".to_string(),
        None => String::new(),
    };

    let title_class = move || match kind_signal.get() {
        Some(BannerKind::Victory) => "endgame-title endgame-title--victory",
        Some(BannerKind::Defeat) => "endgame-title endgame-title--defeat",
        Some(BannerKind::Draw) => "endgame-title endgame-title--draw",
        Some(BannerKind::NeutralWin(Side::RED)) => "endgame-title endgame-title--red",
        Some(BannerKind::NeutralWin(Side::BLACK)) => "endgame-title endgame-title--black",
        Some(BannerKind::NeutralWin(_)) => "endgame-title endgame-title--green",
        None => "endgame-title",
    };

    let reason = move || {
        view.with(|v| match v.status {
            GameStatus::Won { reason, .. } => format!("by {}", win_reason_label(reason)),
            GameStatus::Drawn { reason } => format!("by {}", draw_reason_label(reason)),
            GameStatus::Ongoing => String::new(),
        })
    };

    let overlay_class = match half {
        None => "endgame-overlay",
        Some(MirrorHalf::Top) => "endgame-overlay endgame-overlay--top",
        Some(MirrorHalf::Bottom) => "endgame-overlay endgame-overlay--bottom",
    };

    view! {
        <Show when=move || visible.get()>
            <div class=overlay_class aria-live="polite">
                <div class="endgame-banner">
                    <h1 class=title_class>{title}</h1>
                    <p class="endgame-reason">{reason}</p>
                </div>
                <Confetti/>
            </div>
        </Show>
    }
}

fn resolve_kind(status: &GameStatus, role: Option<ClientRole>) -> Option<BannerKind> {
    match (status, role) {
        (GameStatus::Ongoing, _) => None,
        (GameStatus::Drawn { .. }, _) => Some(BannerKind::Draw),
        (GameStatus::Won { winner, .. }, Some(ClientRole::Player(seat))) => {
            if *winner == seat {
                Some(BannerKind::Victory)
            } else {
                Some(BannerKind::Defeat)
            }
        }
        // Spectator — and Local pass-and-play (`None`) — both fall through
        // to the neutral "Side Wins" framing.
        (GameStatus::Won { winner, .. }, _) => Some(BannerKind::NeutralWin(*winner)),
    }
}

fn win_reason_label(reason: WinReason) -> &'static str {
    match reason {
        WinReason::Checkmate => "checkmate (將死)",
        WinReason::Stalemate => "stalemate (困死)",
        WinReason::Resignation => "resignation",
        WinReason::OnlyOneSideHasPieces => "elimination",
        WinReason::Timeout => "time forfeit",
        WinReason::GeneralCaptured => "general captured (將被吃)",
    }
}

fn draw_reason_label(reason: DrawReason) -> &'static str {
    match reason {
        DrawReason::NoProgress => "no progress (60 plies)",
        DrawReason::Repetition => "repetition",
        DrawReason::Agreed => "agreement",
        DrawReason::InsufficientMaterial => "insufficient material",
    }
}
