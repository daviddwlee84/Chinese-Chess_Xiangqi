//! Sidebar: variant + turn + status + history-length + undo / new-game buttons.
//!
//! `wip=true` (three-kingdom banqi until the engine ships) hides turn /
//! legal-move counts and disables the action buttons — the board overlay
//! tells the user what's happening; the sidebar just stays out of the way.

use chess_core::piece::Side;
use chess_core::rules::Variant;
use chess_core::state::{DrawReason, GameStatus, WinReason};
use chess_core::view::PlayerView;
use leptos::*;

use crate::glyph::{self, Style};

#[component]
pub fn Sidebar(
    variant: Variant,
    #[prop(into)] view: Signal<PlayerView>,
    #[prop(into)] on_new_game: Callback<()>,
    #[prop(into)] on_undo: Callback<()>,
    #[prop(default = false)] wip: bool,
) -> impl IntoView {
    let style = Style::Cjk;
    let variant_label = match variant {
        Variant::Xiangqi => "Xiangqi 象棋",
        Variant::Banqi => "Banqi 暗棋",
        Variant::ThreeKingdomBanqi => "Three-Kingdom 三國暗棋",
    };

    let turn_text = move || {
        let v = view.get();
        glyph::side_name(v.side_to_move, style).to_string()
    };
    let turn_class = move || {
        let v = view.get();
        match v.side_to_move {
            Side::RED => "turn-red",
            Side::BLACK => "turn-black",
            _ => "turn-green",
        }
    };

    let status_view = move || -> View {
        let v = view.get();
        match v.status {
            GameStatus::Ongoing => view! { <p class="status ongoing">"Ongoing"</p> }.into_view(),
            GameStatus::Won { winner, reason } => {
                let side = glyph::side_name(winner, style);
                let reason = win_reason_label(reason);
                view! {
                    <p class="status won">
                        {format!("{} wins — {}", side, reason)}
                    </p>
                }
                .into_view()
            }
            GameStatus::Drawn { reason } => view! {
                <p class="status drawn">
                    {format!("Drawn — {}", draw_reason_label(reason))}
                </p>
            }
            .into_view(),
        }
    };

    let legal_count = move || view.with(|v| v.legal_moves.len());

    view! {
        <aside class="sidebar">
            <h3 class="variant-label">{variant_label}</h3>
            <Show when=move || !wip fallback=|| view! {
                <p class="status drawn">"Engine WIP — board interaction disabled."</p>
            }>
                <div class="turn-row">
                    <span class="turn-prefix">"Turn: "</span>
                    <span class=turn_class>{turn_text}</span>
                </div>
                {status_view}
                <p class="muted">{move || format!("{} legal moves", legal_count())}</p>
            </Show>
            <div class="sidebar-actions">
                <button class="btn" on:click=move |_| on_undo.call(()) disabled=move || wip>"Undo"</button>
                <button class="btn" on:click=move |_| on_new_game.call(()) disabled=move || wip>"New game"</button>
            </div>
            <a class="back-link" href="/">"← Back to picker"</a>
        </aside>
    }
}

fn win_reason_label(reason: WinReason) -> &'static str {
    match reason {
        WinReason::Checkmate => "checkmate 將死",
        WinReason::Stalemate => "stalemate 困死",
        WinReason::Resignation => "resignation",
        WinReason::OnlyOneSideHasPieces => "only one side has pieces",
        WinReason::Timeout => "timeout",
        WinReason::GeneralCaptured => "general captured",
    }
}

fn draw_reason_label(reason: DrawReason) -> &'static str {
    match reason {
        DrawReason::NoProgress => "no progress",
        DrawReason::Repetition => "repetition",
        DrawReason::Agreed => "agreed",
        DrawReason::InsufficientMaterial => "insufficient material",
    }
}
