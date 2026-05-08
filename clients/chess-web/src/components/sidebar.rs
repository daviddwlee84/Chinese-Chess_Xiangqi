//! Sidebar: variant + turn + status + history-length + undo / new-game buttons.
//!
//! `wip=true` (three-kingdom banqi until the engine ships) hides turn /
//! legal-move counts and disables the action buttons — the board overlay
//! tells the user what's happening; the sidebar just stays out of the way.

use chess_core::board::BoardShape;
use chess_core::piece::Side;
use chess_core::rules::Variant;
use chess_core::state::{DrawReason, GameStatus, WinReason};
use chess_core::view::PlayerView;
use leptos::*;

use crate::glyph::{self, Style};
use crate::prefs::Prefs;
use crate::routes::app_href;

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

    // Display the piece-colour to move, not the seat name. After a banqi
    // first-flip the seat (`side_to_move`) and the colour the seat plays
    // (`current_color`) diverge — players think in terms of colour.
    let turn_text = move || {
        let v = view.get();
        glyph::side_name(v.current_color, style).to_string()
    };
    let turn_class = move || {
        let v = view.get();
        match v.current_color {
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

    // Pull FX prefs from context (provided by `app.rs::App`). Both signals
    // default to true on first visit and persist via localStorage.
    let prefs = expect_context::<Prefs>();
    let fx_confetti = prefs.fx_confetti;
    let fx_check_banner = prefs.fx_check_banner;

    let show_check = move || {
        let v = view.get();
        // Check banner is xiangqi-only and gated by the user pref.
        fx_check_banner.get()
            && matches!(v.shape, BoardShape::Xiangqi9x10)
            && v.in_check
            && matches!(v.status, GameStatus::Ongoing)
    };

    view! {
        <aside class="sidebar">
            <h3 class="variant-label">{variant_label}</h3>
            <Show when=move || !wip fallback=|| view! {
                <p class="status drawn">"Engine WIP — board interaction disabled."</p>
            }>
                <Show when=show_check>
                    <div class="check-badge" role="status">"⚠ 將軍 / CHECK"</div>
                </Show>
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
            <FxToggles fx_confetti=fx_confetti fx_check_banner=fx_check_banner/>
            <a class="back-link" href=app_href("/") rel="external">"← Back to picker"</a>
        </aside>
    }
}

#[component]
fn FxToggles(fx_confetti: RwSignal<bool>, fx_check_banner: RwSignal<bool>) -> impl IntoView {
    view! {
        <div class="fx-toggles">
            <label>
                <input
                    type="checkbox"
                    prop:checked=move || fx_confetti.get()
                    on:change=move |ev| fx_confetti.set(event_target_checked(&ev))
                />
                <span>"Victory effects (confetti + banner)"</span>
            </label>
            <label>
                <input
                    type="checkbox"
                    prop:checked=move || fx_check_banner.get()
                    on:change=move |ev| fx_check_banner.set(event_target_checked(&ev))
                />
                <span>"將軍 / CHECK warning"</span>
            </label>
        </div>
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
