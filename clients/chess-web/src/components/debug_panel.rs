//! AI debug panel — shows the engine's full scored root-move list +
//! search metadata, with hover-to-highlight on the board.
//!
//! Mounted by `pages/local.rs` only when `?debug=1` is in the URL.
//! Consumes a `Signal<Option<AiAnalysis>>` produced by the AI move
//! pump (the pump now calls `chess_ai::analyze` instead of
//! `choose_move` when debug is on, then takes `chosen` for the actual
//! move). Sets a `highlighted_move: RwSignal<Option<Move>>` on hover
//! so the board can render the move's from/to overlay.
//!
//! No PV (principal variation) yet — the search returns one score per
//! root move but doesn't track best-line. Adding PV is moderate effort
//! and slated for a future iteration; for v1 of the debug panel,
//! single-ply hover highlights are already a big debugging win.

use chess_ai::{AiAnalysis, ScoredMove};
use chess_core::board::Board;
use chess_core::moves::Move;
use chess_core::notation::iccs;
use leptos::*;

#[component]
pub fn DebugPanel(
    /// Most recent analysis (`None` before the AI has thought yet, or
    /// during a player's turn in vs-AI mode where the AI doesn't run).
    #[prop(into)]
    analysis: Signal<Option<AiAnalysis>>,
    /// Board reference for ICCS encoding. Captured by closures —
    /// shouldn't change during a game (the board shape stays constant).
    board: Board,
    /// Set to `Some(move)` while the user hovers a row, `None` otherwise.
    /// The board component reads this to render a temporary highlight.
    #[prop(into)]
    on_hover: Callback<Option<Move>>,
) -> impl IntoView {
    let board = store_value(board);

    view! {
        <section class="debug-panel" aria-label="AI debug panel">
            <h4 class="debug-panel__title">"AI Debug 🔍"</h4>
            <Show
                when=move || analysis.with(|a| a.is_some())
                fallback=|| view! { <p class="muted debug-panel__empty">"Waiting for AI move…"</p> }
            >
                <DebugMeta analysis=analysis/>
                <DebugTable analysis=analysis on_hover=on_hover board=board/>
            </Show>
        </section>
    }
}

#[component]
fn DebugMeta(#[prop(into)] analysis: Signal<Option<AiAnalysis>>) -> impl IntoView {
    let depth = move || analysis.with(|a| a.as_ref().map(|x| x.depth).unwrap_or(0));
    let nodes = move || analysis.with(|a| a.as_ref().map(|x| x.nodes).unwrap_or(0));
    let strategy = move || {
        analysis.with(|a| a.as_ref().map(|x| x.strategy.as_str()).unwrap_or("—").to_string())
    };
    let randomness_label = move || {
        analysis.with(|a| {
            a.as_ref()
                .map(|x| {
                    let r = x.randomness;
                    match r.preset_name() {
                        Some(name) => format!("{} (top-{} ±{}cp)", name, r.top_k, r.cp_window),
                        None => format!("custom (top-{} ±{}cp)", r.top_k, r.cp_window),
                    }
                })
                .unwrap_or_else(|| "—".to_string())
        })
    };
    let chosen_score = move || analysis.with(|a| a.as_ref().map(|x| x.chosen.score).unwrap_or(0));
    let move_count = move || analysis.with(|a| a.as_ref().map(|x| x.scored.len()).unwrap_or(0));

    view! {
        <dl class="debug-panel__meta">
            <div><dt>"Engine"</dt><dd>{strategy}</dd></div>
            <div><dt>"Depth"</dt><dd>{move || depth().to_string()}</dd></div>
            <div><dt>"Nodes"</dt><dd>{move || nodes().to_string()}</dd></div>
            <div><dt>"Randomness"</dt><dd>{randomness_label}</dd></div>
            <div><dt>"Best score"</dt><dd>{move || format!("{:+} cp", chosen_score())}</dd></div>
            <div><dt>"Moves scored"</dt><dd>{move || move_count().to_string()}</dd></div>
        </dl>
    }
}

#[component]
fn DebugTable(
    #[prop(into)] analysis: Signal<Option<AiAnalysis>>,
    #[prop(into)] on_hover: Callback<Option<Move>>,
    board: StoredValue<Board>,
) -> impl IntoView {
    let chosen_mv =
        Signal::derive(move || analysis.with(|a| a.as_ref().map(|x| x.chosen.mv.clone())));

    view! {
        <table class="debug-panel__table">
            <thead>
                <tr><th>"#"</th><th>"Move"</th><th>"Score (cp)"</th></tr>
            </thead>
            <tbody>
                <For
                    each=move || {
                        analysis.with(|a| {
                            a.as_ref()
                                .map(|x| x.scored.iter().enumerate().map(|(i, sm)| (i, sm.clone())).collect::<Vec<_>>())
                                .unwrap_or_default()
                        })
                    }
                    key=|(i, sm)| (*i, sm.score, format!("{:?}", sm.mv))
                    children=move |(i, sm)| view! {
                        <DebugRow
                            rank=i
                            sm=sm
                            chosen_mv=chosen_mv
                            on_hover=on_hover
                            board=board
                        />
                    }
                />
            </tbody>
        </table>
    }
}

#[component]
fn DebugRow(
    rank: usize,
    sm: ScoredMove,
    #[prop(into)] chosen_mv: Signal<Option<Move>>,
    #[prop(into)] on_hover: Callback<Option<Move>>,
    board: StoredValue<Board>,
) -> impl IntoView {
    let mv_for_hover = sm.mv.clone();
    let mv_for_leave = sm.mv.clone();
    let mv_for_check = sm.mv.clone();
    let text = board.with_value(|b| iccs::encode_move(b, &sm.mv));
    let score = sm.score;
    let is_chosen = Signal::derive(move || {
        chosen_mv.with(|c| match c {
            Some(m) => m == &mv_for_check,
            None => false,
        })
    });
    let row_class = move || {
        let mut classes = String::from("debug-panel__row");
        if is_chosen.get() {
            classes.push_str(" debug-panel__row--chosen");
        }
        if score < -1000 {
            classes.push_str(" debug-panel__row--blunder");
        } else if score > 200 {
            classes.push_str(" debug-panel__row--good");
        }
        classes
    };
    let star_text = move || if is_chosen.get() { " ★" } else { "" };

    view! {
        <tr
            class=row_class
            on:mouseenter=move |_| on_hover.call(Some(mv_for_hover.clone()))
            on:mouseleave=move |_| {
                let _ = &mv_for_leave; // capture
                on_hover.call(None);
            }
        >
            <td class="debug-panel__rank">{rank + 1}</td>
            <td class="debug-panel__mv">{text}{star_text}</td>
            <td class="debug-panel__score">{format!("{:+}", score)}</td>
        </tr>
    }
}
