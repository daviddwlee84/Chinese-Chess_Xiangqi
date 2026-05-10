//! AI debug panel — shows the engine's full scored root-move list +
//! search metadata, with hover-to-highlight (full PV chain) on the board.
//!
//! Mounted by `pages/local.rs` (and `pages/play.rs` for net debug)
//! only when `?debug=1` is in the URL. Consumes a
//! `Signal<Option<AiAnalysis>>` produced by the AI move pump (the
//! pump now calls `chess_ai::analyze` instead of `choose_move` when
//! debug is on, then takes `chosen` for the actual move). Sets a
//! `highlighted_pv: RwSignal<Vec<Move>>` on hover so the board can
//! render the full predicted line as a chain of from→to arrows.

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
    /// Set to the row's full PV (chosen move + opponent's predicted
    /// replies) while the user hovers a row, empty `Vec` otherwise.
    /// The board component reads this to render a chain of from→to
    /// arrows fading from bright (chosen) to dim (deepest PV move).
    #[prop(into)]
    on_hover: Callback<Vec<Move>>,
    /// Header label — typically "🧠 AI Hint" (`?hints=1`) or "🔍 AI
    /// Debug" (`?debug=1`). Reactive so the page can flip it based on
    /// which mode is currently driving the panel.
    #[prop(into, default = Signal::derive(|| "🔍 AI Debug".to_string()))]
    title: Signal<String>,
    /// Optional subtitle line under the header, typically describing
    /// **whose POV** the analysis is from (e.g. "Red 紅 to move"). Empty
    /// string omits the subtitle. Reactive.
    #[prop(into, default = Signal::derive(|| String::new()))]
    subtitle: Signal<String>,
) -> impl IntoView {
    let board = store_value(board);
    let has_subtitle = move || !subtitle.with(|s| s.is_empty());

    view! {
        <section class="debug-panel" aria-label="AI debug panel">
            <h4 class="debug-panel__title">{move || title.get()}</h4>
            <Show when=has_subtitle>
                <p class="debug-panel__subtitle">{move || subtitle.get()}</p>
            </Show>
            <Show
                when=move || analysis.with(|a| a.is_some())
                fallback=|| view! { <p class="muted debug-panel__empty">"Waiting for analysis…"</p> }
            >
                <DebugMeta analysis=analysis/>
                <DebugTable analysis=analysis on_hover=on_hover board=board/>
            </Show>
        </section>
    }
}

#[component]
fn DebugMeta(#[prop(into)] analysis: Signal<Option<AiAnalysis>>) -> impl IntoView {
    // The "Depth" cell now reports BOTH reached and target depths so
    // users who set Search depth = 10 can see immediately when the
    // node-budget cap truncated the search to a smaller reached depth.
    // Format:
    //   - "4"            when reached == target  (no truncation)
    //   - "4 / 10 (cap)" when reached <  target  (budget bit; tooltip explains)
    // The previous version just showed "4" with no indication that the
    // user's request had been silently clipped — see
    // `pitfalls/ai-search-depth-setting-shows-depth-4.md`.
    let depth_text = move || {
        analysis.with(|a| match a.as_ref() {
            None => "0".to_string(),
            Some(x) if x.depth >= x.target_depth => x.depth.to_string(),
            Some(x) => format!("{} / {} (cap)", x.depth, x.target_depth),
        })
    };
    let depth_title = move || {
        analysis.with(|a| match a.as_ref() {
            Some(x) if x.budget_hit => format!(
                "Iterative deepening reached depth {} of {} requested before the per-search node \
                 budget (250k) was exhausted. Lower Search depth or accept the truncation.",
                x.depth, x.target_depth,
            ),
            _ => "Reached search depth (== requested target).".to_string(),
        })
    };
    let nodes = move || analysis.with(|a| a.as_ref().map(|x| x.nodes).unwrap_or(0));
    // "Time" row is omitted when the caller didn't measure
    // (`elapsed_ms == None`). Format helper lives in `crate::time` so
    // the chess-web → chess-tui consistency check (same numbers in
    // both UIs) has one source of truth for the formatting.
    let elapsed_text = move || {
        analysis.with(|a| a.as_ref().and_then(|x| x.elapsed_ms).map(crate::time::format_elapsed_ms))
    };
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
            <div><dt>"Depth"</dt><dd title=depth_title>{depth_text}</dd></div>
            <div><dt>"Nodes"</dt><dd>{move || nodes().to_string()}</dd></div>
            // "Time" only appears when caller measured (chess-web's
            // pages do via `crate::time::perf_now_ms`; net mode also
            // measures). Hidden when None — keeps the meta block from
            // showing a misleading "0 ms" placeholder.
            <Show when=move || elapsed_text().is_some()>
                <div>
                    <dt>"Time"</dt>
                    <dd title="Wall-clock duration of the search; rises with target depth + node budget.">
                        {move || elapsed_text().unwrap_or_default()}
                    </dd>
                </div>
            </Show>
            <div><dt>"Randomness"</dt><dd>{randomness_label}</dd></div>
            <div><dt>"Best score"</dt><dd>{move || format!("{:+} cp", chosen_score())}</dd></div>
            <div><dt>"Moves scored"</dt><dd>{move || move_count().to_string()}</dd></div>
        </dl>
    }
}

#[component]
fn DebugTable(
    #[prop(into)] analysis: Signal<Option<AiAnalysis>>,
    #[prop(into)] on_hover: Callback<Vec<Move>>,
    board: StoredValue<Board>,
) -> impl IntoView {
    let chosen_mv =
        Signal::derive(move || analysis.with(|a| a.as_ref().map(|x| x.chosen.mv.clone())));

    view! {
        <table class="debug-panel__table">
            <thead>
                <tr><th>"#"</th><th>"Move (PV)"</th><th>"Score (cp)"</th></tr>
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
    #[prop(into)] on_hover: Callback<Vec<Move>>,
    board: StoredValue<Board>,
) -> impl IntoView {
    // Build the full PV chain (chosen move + continuation) for the hover.
    let full_chain: Vec<Move> =
        std::iter::once(sm.mv.clone()).chain(sm.pv.iter().cloned()).collect();
    let full_chain_for_enter = full_chain.clone();
    let mv_for_check = sm.mv.clone();
    let mv_text = board.with_value(|b| iccs::encode_move(b, &sm.mv));
    // Render PV continuation as " → m1 → m2 → m3" inline. Truncate at
    // 3 plies in the table cell to keep rows compact; the board chain
    // overlay shows the full PV when hovered.
    let pv_text = if sm.pv.is_empty() {
        String::new()
    } else {
        let strs: Vec<String> =
            board.with_value(|b| sm.pv.iter().take(3).map(|m| iccs::encode_move(b, m)).collect());
        let suffix =
            if sm.pv.len() > 3 { format!(" …(+{})", sm.pv.len() - 3) } else { String::new() };
        format!(" → {}{}", strs.join(" → "), suffix)
    };
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
            on:mouseenter=move |_| on_hover.call(full_chain_for_enter.clone())
            on:mouseleave=move |_| on_hover.call(Vec::new())
        >
            <td class="debug-panel__rank">{rank + 1}</td>
            <td class="debug-panel__mv">
                <span class="debug-panel__mv-main">{mv_text}{star_text}</span>
                <span class="debug-panel__mv-pv">{pv_text}</span>
            </td>
            <td class="debug-panel__score">{format!("{:+}", score)}</td>
        </tr>
    }
}
