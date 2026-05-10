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

use crate::components::move_history::{HistoryEntry, MoveHistory};
use crate::components::pwa::PwaInstallButton;
use crate::eval::EvalSample;
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
    /// Optional move history (local mode supplies this; net mode passes
    /// `None` because `PlayerView` doesn't carry history yet — that's
    /// a future protocol bump).
    #[prop(optional, into)]
    history: Option<Signal<Vec<HistoryEntry>>>,
    /// `Some(signal)` enables an in-game "🧠 Show / Hide AI hint" toggle
    /// button. Owner: the page (so the page also owns the show/hide
    /// state for the actual `<DebugPanel>`). Pass `None` to omit the
    /// button entirely. Distinct from the always-on `?debug=1` panel:
    /// hints are user-driven (default hidden, click to expand /
    /// collapse mid-game), debug is sticky (mounts on page load).
    #[prop(default = None)]
    hint_toggle: Option<RwSignal<bool>>,
    /// `Some(signal)` enables an in-sidebar `紅 % • 黑 %` win-rate
    /// badge fed by the latest [`EvalSample`]. `None` (default) omits
    /// it entirely. Wired by `pages/local.rs` only when the
    /// `?evalbar=1` URL flag is set; net mode currently passes `None`
    /// pending the v6 protocol broadcast (see TODO.md).
    ///
    /// The signal can carry `Some(None)` for "evalbar is on but no
    /// analysis has landed yet" — the badge then renders `紅 — • 黑 —`
    /// so the geometry stays consistent.
    #[prop(default = None, into)]
    eval_badge: Option<Signal<Option<EvalSample>>>,
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
    // default to true on first visit and persist via localStorage. The
    // toggles themselves live on the picker page (collapsed `<details>`)
    // — in-game we only *read* the values to gate FX behaviour.
    let prefs = expect_context::<Prefs>();
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
                {eval_badge.map(|sig| view! { <EvalBadge sample=sig/> })}
                {status_view}
                <p class="muted">{move || format!("{} legal moves", legal_count())}</p>
            </Show>
            <div class="sidebar-actions">
                <button class="btn" on:click=move |_| on_undo.call(()) disabled=move || wip>"Undo"</button>
                <button class="btn" on:click=move |_| on_new_game.call(()) disabled=move || wip>"New game"</button>
            </div>
            {hint_toggle.map(|sig| {
                let label = move || if sig.get() {
                    "🧠 Hide AI hint"
                } else {
                    "🧠 Show AI hint"
                };
                let cls = move || if sig.get() {
                    "btn btn-hint btn-hint--on"
                } else {
                    "btn btn-hint"
                };
                view! {
                    <button
                        class=cls
                        title="Toggle the AI hint panel — runs the bot and shows the top scored moves from your perspective. You stay in control of the game."
                        on:click=move |_| sig.update(|b| *b = !*b)
                    >
                        {label}
                    </button>
                }
            })}
            {history.map(|h| view! { <MoveHistory entries=h/> })}
            <PwaInstallButton/>
            <a class="back-link" href=app_href("/") rel="external">"← Back to picker"</a>
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

/// Compact `紅 52% • 黑 48%` win-rate row driven by the optional
/// `eval_badge` prop. Internal: only rendered when the parent
/// page passes a sample signal (i.e. `?evalbar=1` is on).
#[component]
fn EvalBadge(#[prop(into)] sample: Signal<Option<EvalSample>>) -> impl IntoView {
    let red_text = move || {
        sample.with(|s| match s {
            Some(s) => format!("{}%", (s.red_win_pct * 100.0).round() as i32),
            None => "—".to_string(),
        })
    };
    let black_text = move || {
        sample.with(|s| match s {
            Some(s) => format!("{}%", (s.black_win_pct() * 100.0).round() as i32),
            None => "—".to_string(),
        })
    };
    let red_pct = move || {
        sample.with(|s| match s {
            Some(s) => (s.red_win_pct * 100.0).clamp(0.0, 100.0),
            None => 50.0,
        })
    };
    let red_width = move || format!("{:.1}%", red_pct());
    let black_width = move || format!("{:.1}%", 100.0 - red_pct());
    let cp_title = move || {
        sample.with(|s| match s {
            Some(s) => format!(
                "Bot eval: {:+} cp from {} POV (latest analyzed ply {})",
                s.cp_stm_pov,
                match s.side_to_move_at_pos {
                    Side::RED => "Red 紅",
                    Side::BLACK => "Black 黑",
                    _ => "Green 綠",
                },
                s.ply,
            ),
            None => "Win-rate display: waiting for first analysis…".to_string(),
        })
    };
    view! {
        <div class="eval-badge" title=cp_title>
            <div class="eval-badge__row">
                <span class="eval-badge__side eval-badge__side--red">"紅 " {red_text}</span>
                <span class="eval-badge__sep">"•"</span>
                <span class="eval-badge__side eval-badge__side--black">"黑 " {black_text}</span>
            </div>
            <div class="eval-badge__bar" aria-hidden="true">
                <span class="eval-badge__fill eval-badge__fill--red" style:width=red_width/>
                <span class="eval-badge__fill eval-badge__fill--black" style:width=black_width/>
            </div>
        </div>
    }
}
