use chess_ai::{AiAnalysis, AiOptions, Difficulty, Randomness, Strategy};
use chess_core::coord::Square;
use chess_core::moves::Move;
use chess_core::piece::Side;
use chess_core::rules::{RuleSet, Variant};
use chess_core::state::{GameState, GameStatus};
use chess_core::view::{PlayerView, VisibleCell};
use leptos::*;
use leptos_router::{use_params_map, use_query_map};

use crate::components::board::{Board, ThreatOverlay};
use crate::components::captured::{CapturedSlot, CapturedStrip};
use crate::components::debug_panel::DebugPanel;
use crate::components::end_overlay::{EndOverlay, MirrorHalf};
use crate::components::eval_bar::EvalBar;
use crate::components::eval_chart::EvalChart;
use crate::components::move_history::HistoryEntry;
use crate::components::sidebar::Sidebar;
use crate::eval::EvalSample;
use crate::prefs::{Prefs, ThreatMode};
use crate::routes::{app_href, build_rule_set, parse_local_rules, parse_variant_slug, PlayMode};
use crate::state::{end_chain_move, find_move, hover_threat_squares};
use crate::time::perf_now_ms;

/// Resolved local-page configuration. `Some(VsAiConfig)` only when the URL
/// asked for vs-AI on a supported variant (xiangqi).
#[derive(Clone, Copy, Debug)]
struct VsAiConfig {
    ai_side: Side,
    difficulty: Difficulty,
    strategy: Strategy,
    /// `None` = use difficulty default; `Some(_)` = explicit override.
    randomness: Option<Randomness>,
    /// `None` = use difficulty default depth; `Some(N)` = explicit override.
    depth: Option<u8>,
    /// `None` = use the engine's auto-scaled budget (v5: depth-scaled,
    /// v1-v4: flat NODE_BUDGET); `Some(N)` = explicit override.
    /// Set via the picker's Custom-difficulty inputs and round-tripped
    /// through the URL `&budget=N` token.
    node_budget: Option<u32>,
}

/// AI insight panel configuration — independent of `VsAiConfig` so
/// pass-and-play (PvP, two humans) can also surface hint analyses.
///
/// `debug` and `hints` are decoupled URL flags:
///
/// - **Debug** (`?debug=1`): sticky always-on panel showing analysis
///   for the current side-to-move. The label says "🔍 AI Debug" so
///   the user knows they're peeking at the engine.
/// - **Hint** (`?hints=1`): on-demand toggle button (default hidden)
///   in the sidebar. Same analysis under the hood, but the panel
///   labels itself "🧠 AI Hint" — phrased for a player asking the
///   bot for advice on their move.
/// - **Evalbar** (`?evalbar=1`): vertical SVG eval bar next to the
///   board + sidebar `紅 % • 黑 %` badge + post-game trend chart.
///   Reuses the same hint pump that powers `?hints=1` for
///   per-ply sampling — when this flag is on, the hint pump runs
///   every turn even if `?hints=1` itself is not set.
///
/// When BOTH debug and hints are set, debug wins on layout (sticky
/// panel, no toggle button) but the title shows both badges so the
/// user knows they're seeing whatever the side-to-move would consider.
/// Evalbar is independent of both — it always mounts when set.
#[derive(Clone, Copy, Debug, Default)]
struct InsightConfig {
    debug: bool,
    hints: bool,
    evalbar: bool,
}

#[component]
pub fn LocalPage() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let resolved = move || -> Result<
        (Variant, RuleSet, bool, Option<VsAiConfig>, InsightConfig, bool),
        String,
    > {
            let slug = params.with(|p| p.get("variant").cloned().unwrap_or_default());
            let variant =
                parse_variant_slug(&slug).ok_or_else(|| format!("Unknown variant: {}", slug))?;
            let parsed = query.with(|q| parse_local_rules(|k| q.get(k).cloned()));
            let rules = build_rule_set(variant, &parsed);
            // Three-kingdom: engine ships an empty 4×8 board. Render it but
            // disable interaction and overlay a "WIP" banner — see
            // `backlog/three-kingdoms-banqi.md` and the gotcha in CLAUDE.md.
            let wip = matches!(variant, Variant::ThreeKingdomBanqi);
            // vs-AI is xiangqi-only; banqi/three-kingdom silently fall back to
            // pass-and-play (the picker hides the toggle there too — see plan).
            let ai = if parsed.mode == PlayMode::VsAi && matches!(variant, Variant::Xiangqi) {
                Some(VsAiConfig {
                    ai_side: parsed.ai_side,
                    difficulty: parsed.ai_difficulty,
                    strategy: parsed.ai_strategy,
                    randomness: parsed.ai_variation,
                    depth: parsed.ai_depth,
                    node_budget: parsed.ai_node_budget,
                })
            } else {
                None
            };
            // AI insight panel works for both vs-AI AND pass-and-play —
            // it's just an on-demand bot analysis, no AI opponent required.
            // Xiangqi-only because the chess-ai engine is xiangqi-scoped.
            let insight = if matches!(variant, Variant::Xiangqi) {
                InsightConfig {
                    debug: parsed.ai_debug,
                    hints: parsed.ai_hints,
                    evalbar: parsed.ai_evalbar,
                }
            } else {
                InsightConfig::default()
            };
            // Mirror is xiangqi pass-and-play only (the picker only
            // exposes the checkbox there). Vs-AI / banqi / three-kingdom
            // silently drop the flag.
            let mirror = matches!(variant, Variant::Xiangqi)
                && parsed.mode == PlayMode::Pvp
                && parsed.mirror;
            Ok((variant, rules, wip, ai, insight, mirror))
        };

    move || match resolved() {
        Ok((variant, rules, wip, ai, insight, mirror)) => view! {
            <LocalGame variant=variant rules=rules wip=wip ai=ai insight=insight mirror=mirror/>
        }
        .into_view(),
        Err(msg) => view! {
            <section class="game-page game-page--single">
                <div>
                    <a href=app_href("/") rel="external" class="back-link">"← Back to picker"</a>
                    <h2>"Variant unavailable"</h2>
                    <p class="subtitle">{msg}</p>
                </div>
            </section>
        }
        .into_view(),
    }
}

#[component]
fn LocalGame(
    variant: Variant,
    rules: RuleSet,
    wip: bool,
    ai: Option<VsAiConfig>,
    insight: InsightConfig,
    /// Pass-and-play only: rotate Black piece glyphs 180° and split the
    /// captured-pieces strip to opposite edges of the board so two
    /// players sitting on opposite sides of the device each read their
    /// own pieces upright. Resolved upstream — already gated to xiangqi
    /// + Pvp mode by the caller.
    mirror: bool,
) -> impl IntoView {
    let initial_rules = rules.clone();
    let state = create_rw_signal(GameState::new(rules));
    let selected = create_rw_signal::<Option<Square>>(None);
    let ai_thinking = create_rw_signal(false);
    // Two SEPARATE analysis caches — they MUST NOT share storage,
    // otherwise the freshest writer (typically the hint pump after the
    // AI moves) clobbers the other and the user can't compare AI's POV
    // vs. their own POV. The panel chooses which cache to render based
    // on `hint_view_active` below.
    //
    // - `debug_analysis`: cached AI's POV after each AI move. Sticky
    //   between turns; only the AI pump writes it. Empty in PvP (no AI
    //   pump runs there).
    // - `hint_analysis`: live re-analysis from the side-to-move's POV.
    //   The hint pump rewrites it on every relevant state change.
    let debug_analysis = create_rw_signal::<Option<AiAnalysis>>(None);
    let hint_analysis = create_rw_signal::<Option<AiAnalysis>>(None);
    // Per-ply win-rate samples for the eval bar / sidebar badge / end-game
    // chart. Populated by an effect (see below) that watches both
    // analysis caches and pushes a fresh `EvalSample` whenever an
    // analysis lands AND the history grew. Only meaningful when
    // `evalbar_enabled` (the URL flag); when off, the vector stays
    // empty and the components don't mount. Reset on undo / new game
    // alongside the analysis caches so the chart starts fresh.
    let eval_samples = create_rw_signal::<Vec<EvalSample>>(Vec::new());
    // Hover-driven PV chain for the AI debug panel: when the user
    // hovers a row, board renders a faded chain of from→to overlays.
    // Empty Vec = no highlight.
    let highlighted_pv = create_rw_signal::<Vec<Move>>(Vec::new());
    // Bumped on every state change (player move, AI move, undo, new game).
    // The AI task captures the epoch when it starts and discards its result
    // if the epoch has changed by the time it returns — prevents stale AI
    // moves landing after the player undid or restarted.
    let move_epoch = create_rw_signal::<u32>(0);

    // ---- Debug vs Hint panel state ----
    //
    // `?debug=1` and `?hints=1` are independent URL flags. They drive
    // a single `<DebugPanel>` mount but with different content per the
    // matrix below (effective_debug = debug_enabled && ai.is_some()
    // because PvP has no AI to debug — flag is silently ignored there):
    //
    // | flags        | mode  | panel visible        | content        | button             |
    // |--------------|-------|----------------------|----------------|--------------------|
    // | hints        | any   | when button ON       | hint_analysis  | 🧠 Show / Hide     |
    // | debug        | vs-AI | always               | debug_analysis | hidden             |
    // | debug        | PvP   | never                | —              | hidden             |
    // | hints+debug  | vs-AI | always               | toggled below  | 🧠 Show / Hide     |
    // | hints+debug  | PvP   | when button ON       | hint_analysis  | 🧠 Show / Hide     |
    //
    // In hints+debug+vs-AI: button OFF → debug_analysis (cached, sticky
    // — user can study it as long as they want); button ON → switch to
    // hint_analysis (live, the user's own POV). Critical UX point: the
    // AI debug content was previously "稍縱即逝" (gone in a flash) when
    // the hint pump immediately overwrote it after each AI move. Two
    // separate caches + button-driven view selection fixes that — the
    // user can flip between the two views without losing either.
    let debug_enabled = insight.debug;
    let hints_enabled = insight.hints;
    let evalbar_enabled = insight.evalbar;
    let effective_debug = debug_enabled && ai.is_some();
    let hint_view_active = create_rw_signal::<bool>(false);
    let panel_visible =
        Signal::derive(move || effective_debug || (hints_enabled && hint_view_active.get()));
    // Sidebar gets the toggle whenever hints is enabled. In debug-only
    // mode it's hidden (the panel is sticky, no toggle needed). In
    // hints-only mode it shows/hides the panel. In hints+debug+vs-AI
    // mode it switches the panel CONTENT between debug and hint views.
    let sidebar_hint_toggle: Option<RwSignal<bool>> =
        if hints_enabled { Some(hint_view_active) } else { None };
    // Title reflects which content is currently being displayed, not
    // which flags are set. Cleaner than the previous "🔍 + 🧠" combined
    // header which was ambiguous about what the user was actually seeing.
    let panel_title = Signal::derive(move || {
        if hints_enabled && hint_view_active.get() {
            "🧠 AI Hint".to_string()
        } else {
            "🔍 AI Debug".to_string()
        }
    });
    // Subtitle clarifies whose POV the analysis is from. For hint
    // content this matches side-to-move (hint pump only runs on
    // human's turn). For debug content it's the AI's last move POV —
    // which equals the *previous* side-to-move; shown as "Last AI:
    // Red 紅" to be unambiguous.
    let panel_subtitle = Signal::derive(move || {
        if hints_enabled && hint_view_active.get() {
            let stm = state.with(|s| s.side_to_move);
            match stm {
                Side::RED => "Red 紅 to move".to_string(),
                Side::BLACK => "Black 黑 to move".to_string(),
                _ => "Green 綠 to move".to_string(),
            }
        } else if let Some(cfg) = ai {
            // Debug view shows the AI's own POV.
            match cfg.ai_side {
                Side::RED => "AI POV: Red 紅".to_string(),
                Side::BLACK => "AI POV: Black 黑".to_string(),
                _ => "AI POV: Green 綠".to_string(),
            }
        } else {
            String::new()
        }
    });
    // The signal the panel actually reads. Mirrors `panel_title` /
    // `panel_subtitle` logic so all three stay in sync.
    let analysis_signal: Signal<Option<AiAnalysis>> = Signal::derive(move || {
        if hints_enabled && hint_view_active.get() {
            hint_analysis.get()
        } else {
            debug_analysis.get()
        }
    });

    // Render from the human player's POV when in vs-AI mode so the player's
    // pieces sit on the bottom; in pass-and-play the board is fixed Red-side.
    let observer = match ai {
        Some(cfg) => cfg.ai_side.opposite(),
        None => Side::RED,
    };
    let shape = state.with_untracked(|s| s.board.shape());

    let view = create_memo(move |_| state.with(|s| PlayerView::project(s, s.side_to_move)));

    // Clear any held selection when the seat-to-move flips. Catches the
    // chain-mode tail where the last hop ends the chain and switches
    // sides — the previous player's `selected` is meaningless for the
    // next one. Also a safety net for any future click path that
    // forgets its own `selected.set(None)`.
    let side_signal = Signal::derive(move || view.with(|v| v.side_to_move));
    create_effect(move |prev: Option<Side>| {
        let cur = side_signal.get();
        if prev.is_some_and(|p| p != cur) {
            selected.set(None);
        }
        cur
    });

    // The chain-locked piece (if any) renders with the same affordance as
    // a manually-selected piece, so the user sees it visually highlighted.
    let chain_locked_signal: Signal<Option<Square>> =
        Signal::derive(move || view.with(|v| v.chain_lock));

    let effective_selected: Signal<Option<Square>> = Signal::derive(move || {
        // In chain mode the engine dictates which piece moves; surface
        // chain_lock as the selected square so the legal-target dots
        // render around it.
        chain_locked_signal.get().or_else(|| selected.get())
    });

    // ---- Threat-highlight overlay (Display setting) ----
    //
    // Hover state for the orthogonal "what-if" preview: when the user
    // hovers (or selects, in the absence of pointer hover) one of
    // their own pieces, we ring any *other* piece that becomes newly
    // vulnerable if the hovered piece moves away. We start with
    // `effective_selected` as the trigger source (selection is a
    // superset of hover semantically — the user committed enough to
    // single this piece out) so the feature works on touch / keyboard
    // too. A future enhancement can wire actual `pointerover` events
    // for desktop-only mouse refinement; selection-driven covers the
    // common case without needing per-cell pointer plumbing.
    let prefs_threat = expect_context::<Prefs>();
    let fx_threat_mode = prefs_threat.fx_threat_mode;
    let fx_threat_hover = prefs_threat.fx_threat_hover;
    let threat_overlay: Signal<ThreatOverlay> = Signal::derive(move || {
        let v = view.get();
        let mode = fx_threat_mode.get();
        // Mode A/B/C → static_squares (red) and mate_squares (magenta)
        // populated from the engine-computed `view.threats`.
        let (static_squares, mate_squares): (Vec<Square>, Vec<Square>) = match mode {
            ThreatMode::Off => (Vec::new(), Vec::new()),
            ThreatMode::Attacked => (v.threats.attacked.clone(), Vec::new()),
            ThreatMode::NetLoss => (v.threats.net_loss.clone(), Vec::new()),
            ThreatMode::MateThreat => (Vec::new(), v.threats.mate_threats.clone()),
        };
        // Hover preview is independent of the mode selector; it ONLY
        // fires when the user has a piece selected AND the toggle is
        // on AND the selected piece belongs to the observer.
        let hover_squares = if fx_threat_hover.get() {
            if let Some(sq) = effective_selected.get() {
                hover_threat_squares(&v, observer, sq)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        ThreatOverlay { static_squares, mate_squares, hover_squares }
    });

    // Whether the human can act right now. In vs-AI mode the human is
    // locked out while it's AI's turn or while the AI is computing.
    let player_can_act = Signal::derive(move || {
        if wip {
            return false;
        }
        match ai {
            None => true,
            Some(cfg) => {
                if ai_thinking.get() {
                    return false;
                }
                view.with(|v| v.side_to_move != cfg.ai_side)
            }
        }
    });

    let on_end_chain: Callback<()> = Callback::new(move |_| {
        if !player_can_act.get_untracked() {
            return;
        }
        let v = view.get();
        if let Some(mv) = end_chain_move(&v) {
            state.update(|s| {
                s.make_move(&mv).expect("legal end-chain");
                s.refresh_status();
            });
            selected.set(None);
            move_epoch.update(|n| *n = n.wrapping_add(1));
        }
    });

    let on_click: Callback<Square> = Callback::new(move |sq: Square| {
        if !player_can_act.get_untracked() {
            return;
        }
        let v = view.get();

        // Chain mode is engine-driven: when chain_lock is set, only the
        // locked piece can move (captures only). Clicking the locked
        // piece itself is the explicit "end chain" gesture.
        if let Some(locked) = v.chain_lock {
            if sq == locked {
                if let Some(mv) = end_chain_move(&v) {
                    state.update(|s| {
                        s.make_move(&mv).expect("legal end-chain");
                        s.refresh_status();
                    });
                    move_epoch.update(|n| *n = n.wrapping_add(1));
                }
                selected.set(None);
                return;
            }
            if let Some(mv) = find_move(&v, locked, sq) {
                state.update(|s| {
                    s.make_move(&mv).expect("legal chain capture");
                    s.refresh_status();
                });
                selected.set(None);
                move_epoch.update(|n| *n = n.wrapping_add(1));
                return;
            }
            // Click somewhere else during chain mode: ignore (the user
            // must end the chain or capture). Selection is irrelevant.
            return;
        }

        let cur = selected.get();

        if cur == Some(sq) {
            selected.set(None);
            return;
        }

        if let Some(from) = cur {
            if let Some(mv) = find_move(&v, from, sq) {
                state.update(|s| {
                    s.make_move(&mv).expect("legal move from view");
                    s.refresh_status();
                });
                selected.set(None);
                move_epoch.update(|n| *n = n.wrapping_add(1));
                return;
            }
        }

        let cell = v.cells[sq.0 as usize];
        // Selectable iff the engine emits a legal move from this square — this
        // works regardless of `side_assignment` remapping (banqi after the
        // first flip), since `legal_moves` is already filtered by piece-color.
        let is_selectable_revealed = matches!(cell, VisibleCell::Revealed(_))
            && v.legal_moves.iter().any(|m| m.origin_square() == sq);
        if is_selectable_revealed {
            selected.set(Some(sq));
        } else if matches!(cell, VisibleCell::Hidden) {
            // Banqi flip — engine emits `Reveal { at }` whose origin and
            // target both equal `sq`. Apply directly.
            if let Some(mv) = find_move(&v, sq, sq) {
                state.update(|s| {
                    s.make_move(&mv).expect("legal reveal");
                    s.refresh_status();
                });
                selected.set(None);
                move_epoch.update(|n| *n = n.wrapping_add(1));
            }
        } else {
            selected.set(None);
        }
    });

    let on_undo: Callback<()> = Callback::new(move |_| {
        if wip {
            return;
        }
        // In vs-AI mode, undo two plies so the player is back on move.
        // (Undoing only one would put the AI on the move and the AI effect
        // would just re-play immediately — defeating the purpose.) If
        // there's only one ply on the stack — typical when the AI moved
        // first because the player picked Black — fall back to a single
        // undo, which returns to the empty initial state.
        let plies = match ai {
            Some(_) => 2,
            None => 1,
        };
        state.update(|s| {
            for _ in 0..plies {
                if s.unmake_move().is_err() {
                    break;
                }
            }
            s.refresh_status();
        });
        selected.set(None);
        ai_thinking.set(false);
        debug_analysis.set(None);
        hint_analysis.set(None);
        eval_samples.set(Vec::new());
        highlighted_pv.set(Vec::new());
        move_epoch.update(|n| *n = n.wrapping_add(1));
    });

    let on_new_game: Callback<()> = Callback::new({
        let initial_rules = initial_rules.clone();
        move |_| {
            state.set(GameState::new(initial_rules.clone()));
            selected.set(None);
            ai_thinking.set(false);
            debug_analysis.set(None);
            hint_analysis.set(None);
            eval_samples.set(Vec::new());
            highlighted_pv.set(Vec::new());
            move_epoch.update(|n| *n = n.wrapping_add(1));
        }
    });

    // AI move pump. Fires whenever the side-to-move flips to the AI in a
    // vs-AI game. The async task captures the current epoch and the
    // engine snapshot; if the player undoes/restarts mid-think the epoch
    // mismatch causes us to drop the result on the floor.
    if let Some(cfg) = ai {
        create_effect(move |_| {
            // Only `move_epoch` is tracked. `view` is read untracked so the
            // effect doesn't re-fire on the intermediate `state.update(...)`
            // before `move_epoch.update(...)` lands — that ordering would
            // otherwise let the effect capture a stale epoch and immediately
            // bail out on the post-task epoch check. Click handlers bump the
            // epoch *after* every state change, so by the time this fires,
            // both signals are in sync.
            let cur_epoch = move_epoch.get();
            let v = view.get_untracked();
            if !matches!(v.status, GameStatus::Ongoing) {
                return;
            }
            if v.side_to_move != cfg.ai_side {
                return;
            }
            if ai_thinking.get_untracked() {
                return;
            }
            ai_thinking.set(true);
            let snapshot = state.get_untracked();
            let opts = AiOptions {
                difficulty: cfg.difficulty,
                max_depth: cfg.depth,
                seed: Some(cur_epoch as u64 ^ 0xA5A5_5A5A_u64),
                strategy: cfg.strategy,
                randomness: cfg.randomness,
                node_budget: cfg.node_budget,
            };
            wasm_bindgen_futures::spawn_local(async move {
                // One animation frame's worth of yield so the "AI thinking…"
                // banner paints before the search blocks the main thread.
                gloo_timers::future::TimeoutFuture::new(80).await;
                if move_epoch.get_untracked() != cur_epoch {
                    ai_thinking.set(false);
                    return;
                }
                // Always go through `analyze` so the debug panel can
                // surface the full scored list. The cost is the same —
                // `choose_move` internally calls `analyze` and discards
                // everything but the chosen move. When `cfg.debug` is
                // off and `cfg.hints` is off we just don't mount the
                // `<DebugPanel>`. When hints is on we still cache the
                // analysis so opening the toggle mid-game has data
                // ready immediately.
                //
                // Time the search via `performance.now()` and patch
                // the resulting wall-clock into `analysis.elapsed_ms`
                // for the debug panel. Done here (caller-side) rather
                // than inside chess-ai because `std::time::Instant`
                // panics on `wasm32-unknown-unknown` — the platform
                // boundary owns the clock.
                let analysis = {
                    let start = perf_now_ms();
                    let mut a = chess_ai::analyze(&snapshot, &opts);
                    let elapsed = perf_now_ms().saturating_sub(start);
                    if let Some(ref mut a) = a {
                        a.elapsed_ms = Some(elapsed);
                    }
                    a
                };
                if move_epoch.get_untracked() != cur_epoch {
                    ai_thinking.set(false);
                    return;
                }
                if let Some(a) = analysis {
                    let chosen_mv = a.chosen.mv.clone();
                    // Cache AI's POV analysis ONLY for ?debug=1 mode.
                    // Goes into `debug_analysis` (separate from
                    // `hint_analysis`) so the hint pump's later writes
                    // don't clobber what the user might want to study
                    // on the sticky debug panel. The two caches are
                    // toggled into the panel by `hint_view_active`.
                    if effective_debug {
                        debug_analysis.set(Some(a));
                    }
                    state.update(|s| {
                        if s.make_move(&chosen_mv).is_ok() {
                            s.refresh_status();
                        }
                    });
                    move_epoch.update(|n| *n = n.wrapping_add(1));
                }
                ai_thinking.set(false);
            });
        });
    }

    // ---- Hint pump (side-to-move's POV analysis) ----
    //
    // Distinct from the AI move pump above: that one runs `analyze`
    // from the AI's POV at the moment the AI picks a move, caching
    // what the AI considered. THIS pump runs `analyze` from the
    // **side-to-move's** POV — "what would the bot play if it were
    // me?". Works for both vs-AI and pass-and-play (PvP, two humans).
    //
    // Crucially this pump runs WHENEVER hints are enabled OR the
    // win-rate eval bar is enabled — both consume the same analysis
    // (hints surface it as the move list; evalbar surfaces only the
    // top-move's score). Keeping a single pump for both flags avoids
    // duplicate searches.
    //
    // Skipped when:
    // - both hints AND evalbar disabled
    // - vs-AI mode AND it's the AI's turn (the AI pump will run its
    //   own analyze for the AI's POV; spawning a duplicate would race
    //   and the result wouldn't be the side-to-move's POV anyway)
    // - AI is mid-think (avoid racing the AI pump)
    if hints_enabled || evalbar_enabled {
        // Difficulty/strategy/randomness for the hint search. For PvP we
        // pick sensible defaults (Hard + STRICT) since there's no AI
        // config to inherit. For vs-AI we mirror the AI's own knobs so
        // the hint quality matches what the user is up against.
        let hint_difficulty = ai.map(|c| c.difficulty).unwrap_or(Difficulty::Hard);
        let hint_strategy = ai.map(|c| c.strategy).unwrap_or_else(Strategy::default);
        let hint_depth = ai.and_then(|c| c.depth);
        let hint_node_budget = ai.and_then(|c| c.node_budget);
        let hint_ai_side = ai.map(|c| c.ai_side);

        create_effect(move |_| {
            // All three reads are tracked so the effect re-fires when
            // ANY of them change. Critical: `ai_thinking` flips false
            // AFTER `state.update()` in the AI pump, so the first
            // reactive fire (from state change) sees thinking still
            // true and bails. We need the second fire (from
            // ai_thinking → false) to actually run analyze.
            //
            // We do NOT track `panel_visible` / `hint_view_active`
            // here — the hint cache should stay fresh whether or not
            // the user has the panel open, so flipping the toggle is
            // instant. Cost is one extra ~100-300 ms search per turn
            // when hints are enabled but the toggle is off; acceptable
            // given we already pay that cost in vs-AI mode for the AI
            // move itself.
            let s = state.get();
            let thinking = ai_thinking.get();
            if !matches!(s.status, GameStatus::Ongoing) {
                hint_analysis.set(None);
                return;
            }
            // vs-AI mode: skip when it's the AI's turn — the AI pump
            // is already searching the same position from a different
            // POV (the AI's own side, not the side-to-move).
            if let Some(ai_side) = hint_ai_side {
                if s.side_to_move == ai_side {
                    return;
                }
            }
            if thinking {
                // AI mid-think; will re-fire when ai_thinking → false.
                return;
            }
            // Snapshot the position hash for invalidation. Using
            // `position_hash` (from chess-core v5) lets us cancel
            // stale tasks WITHOUT depending on `move_epoch` — which
            // is bumped AFTER `state.update(...)` in the AI pump,
            // racing any effect that captured the pre-bump value.
            let snapshot = s;
            let snapshot_hash = snapshot.position_hash;
            let opts = AiOptions {
                difficulty: hint_difficulty,
                max_depth: hint_depth,
                seed: Some(snapshot_hash ^ 0xC0FFEE_BABE_u64),
                strategy: hint_strategy,
                randomness: Some(chess_ai::Randomness::STRICT),
                node_budget: hint_node_budget,
            };
            wasm_bindgen_futures::spawn_local(async move {
                // Yield a frame so the UI repaints before the search
                // blocks the main thread.
                gloo_timers::future::TimeoutFuture::new(40).await;
                // Bail if state moved on while we yielded.
                let current_hash = state.with_untracked(|cur| cur.position_hash);
                if current_hash != snapshot_hash {
                    return;
                }
                // Time the search via performance.now() — see the
                // matching pattern in the AI move pump above for why
                // we measure caller-side rather than inside chess-ai.
                let analysis = {
                    let start = perf_now_ms();
                    let mut a = chess_ai::analyze(&snapshot, &opts);
                    let elapsed = perf_now_ms().saturating_sub(start);
                    if let Some(ref mut a) = a {
                        a.elapsed_ms = Some(elapsed);
                    }
                    a
                };
                // Bail if state moved on during the search itself.
                let current_hash = state.with_untracked(|cur| cur.position_hash);
                if current_hash != snapshot_hash {
                    return;
                }
                if let Some(a) = analysis {
                    hint_analysis.set(Some(a));
                }
            });
        });
    }

    // ---- Eval-sample sync effect ----
    //
    // Watches both analysis caches + the history length. When a fresh
    // analysis lands AND its position is the *current* state AND there
    // are fewer recorded samples than plies played, push a new
    // [`EvalSample`]. The `position_hash` check (via the snapshot the
    // analysis was computed against, indirectly verified by checking
    // that history.len() matches what we'd expect for this sample)
    // protects against pushing samples for stale positions while
    // search effects race.
    //
    // Sample at `samples[N]` is the AI's eval of the position **after**
    // ply N was played (N=0 = initial, N=1 = after first move, …).
    // We deduplicate by ply: if samples already contains an entry for
    // history.len(), we skip — handles the common case where both AI
    // pump and hint pump produce an analysis for the same position
    // (in vs-AI mode, only one runs per turn; this guard is defensive).
    if evalbar_enabled {
        create_effect(move |_| {
            // Track all three so the effect re-runs whenever any update.
            let history_len = state.with(|s| s.history.len());
            let stm = state.with(|s| s.side_to_move);
            // Read analyses untracked? No — we want to fire on either
            // landing. Tracked reads.
            let dbg = debug_analysis.get();
            let hnt = hint_analysis.get();
            // Pick whichever analysis exists (prefer hint — it's the
            // side-to-move's POV, which is the most recent for the
            // current position; debug_analysis can be stale by one
            // ply between the AI moving and the hint pump catching up).
            let analysis = hnt.or(dbg);
            let Some(a) = analysis else { return };
            // Already have a sample for this position?
            let next_ply = history_len;
            let already_present = eval_samples.with(|v| v.iter().any(|s| s.ply == next_ply));
            if already_present {
                return;
            }
            // Push the new sample. The cp from `analyze` is
            // side-to-move-relative; `EvalSample::new` does the
            // Red-POV normalisation.
            let sample = EvalSample::new(next_ply, stm, a.chosen.score);
            eval_samples.update(|v| v.push(sample));
        });
    }

    let prefs = expect_context::<Prefs>();
    let fx_confetti: Signal<bool> = prefs.fx_confetti.into();
    // Local pass-and-play: no `ClientRole`, so the overlay falls back to
    // the neutral "Red Wins" / "Black Wins" framing.
    let role_signal: Signal<Option<crate::state::ClientRole>> = Signal::derive(|| None);

    let chain_active = Signal::derive(move || chain_locked_signal.get().is_some());

    // Derive a pre-encoded history list for the sidebar's MoveHistory
    // panel. ICCS notation is short and grep-friendly for debugging
    // (user takes a screenshot, pastes the move list — much easier than
    // describing positions). Recomputes on every state change because
    // it depends on `state` directly, not just `view`.
    //
    // When the win-rate display is on (`?evalbar=1`), each row also
    // gets an `eval_delta_pct` field showing the win-% change the move
    // produced from the mover's POV. Computed by pairing
    // `eval_samples[ply-1]` (before) with `eval_samples[ply]` (after),
    // both Red-POV; flipped if the mover is Black so positive always
    // means "this move improved my position". `None` when either
    // sample is missing (typically the very first move before the
    // initial-position sample lands).
    let history_signal: Signal<Vec<HistoryEntry>> = Signal::derive(move || {
        let samples = eval_samples.get();
        state.with(|s| {
            s.history
                .iter()
                .enumerate()
                .map(|(i, rec)| {
                    let ply = i + 1;
                    let before = samples.iter().find(|smp| smp.ply == ply - 1);
                    let after = samples.iter().find(|smp| smp.ply == ply);
                    let delta = before.zip(after).map(|(b, a)| {
                        let red_delta = a.red_win_pct - b.red_win_pct;
                        match rec.mover {
                            Side::RED => red_delta,
                            _ => -red_delta,
                        }
                    });
                    HistoryEntry {
                        ply,
                        side: rec.mover,
                        text: chess_core::notation::iccs::encode_move(&s.board, &rec.the_move),
                        eval_delta_pct: delta,
                    }
                })
                .collect()
        })
    });

    let highlighted_pv_signal: Signal<Vec<Move>> = highlighted_pv.into();
    let on_debug_hover: Callback<Vec<Move>> =
        Callback::new(move |pv: Vec<Move>| highlighted_pv.set(pv));
    let board_for_debug = state.with_untracked(|s| s.board.clone());
    // `analysis_signal` is the derived signal declared above that
    // toggles between `debug_analysis` (sticky cache) and
    // `hint_analysis` (live cache) based on `hint_view_active`.

    let mirror_signal: Signal<bool> = Signal::derive(move || mirror);

    // Eval-display derived signals — exposed to <EvalBar>, the
    // sidebar's badge, and <EvalChart>. When `evalbar_enabled` is
    // false, `eval_samples` stays empty so all three components see
    // empty / None and either hide themselves or render placeholders.
    let current_eval: Signal<Option<EvalSample>> =
        Signal::derive(move || eval_samples.with(|v| v.last().copied()));
    let eval_samples_signal: Signal<Vec<EvalSample>> = eval_samples.into();
    let evalbar_show = move || evalbar_enabled;
    let chart_show =
        Signal::derive(move || evalbar_enabled && eval_samples.with(|v| !v.is_empty()));

    let game_over = move || !matches!(state.with(|s| s.status), GameStatus::Ongoing);
    let on_resign = move |side: Side| {
        state.update(|s| {
            s.resign(side);
            s.refresh_status();
        });
    };
    let game_page_class = move || {
        if evalbar_show() {
            "game-page game-page--with-evalbar"
        } else {
            "game-page"
        }
    };
    view! {
        <section class=game_page_class>
            <div class=move || if mirror { "board-pane board-pane--mirror" } else { "board-pane" }>
                <Show when=move || mirror && !wip>
                    <button
                        class="resign-btn resign-btn--top"
                        title="Black resigns"
                        disabled=game_over
                        on:click=move |_| on_resign(Side::BLACK)
                    >
                        "投降 Resign"
                    </button>
                    <CapturedStrip view=view placement={CapturedSlot::MirroredAbove}/>
                </Show>
                <Board
                    shape=shape
                    observer=observer
                    view=view
                    selected=effective_selected
                    on_click=on_click
                    highlighted_pv=highlighted_pv_signal
                    mirror_black=mirror_signal
                    threats=threat_overlay
                />
                <Show when=move || chain_active.get()>
                    <div class="chain-banner">
                        <span>"連吃 — 繼續吃 or "</span>
                        <button class="btn btn-ghost btn-sm" on:click=move |_| on_end_chain.call(())>
                            "End chain"
                        </button>
                    </div>
                </Show>
                <Show when=move || ai_thinking.get()>
                    <div class="chain-banner ai-thinking-banner">
                        <span>"⏳ AI thinking…"</span>
                    </div>
                </Show>
                <Show when=move || wip>
                    <div class="wip-overlay">
                        <div class="wip-banner">
                            <h3>"Three-Kingdom Banqi 三國暗棋"</h3>
                            <p>
                                "Engine still WIP — pieces, rules, and 3-seat turn order land in PR-2. "
                                "See "
                                <code>"backlog/three-kingdoms-banqi.md"</code>
                                "."
                            </p>
                        </div>
                    </div>
                </Show>
                <Show when=move || !wip && !mirror>
                    <EndOverlay view=view role=role_signal enabled=fx_confetti/>
                </Show>
                <Show when=move || !wip && mirror>
                    <EndOverlay view=view role=role_signal enabled=fx_confetti half=Some(MirrorHalf::Top)/>
                    <EndOverlay view=view role=role_signal enabled=fx_confetti half=Some(MirrorHalf::Bottom)/>
                </Show>
                <Show when=move || !wip && !mirror>
                    <CapturedStrip view=view/>
                </Show>
                <Show when=move || !wip && mirror>
                    <CapturedStrip view=view placement={CapturedSlot::MirroredBelow}/>
                    <button
                        class="resign-btn resign-btn--bottom"
                        title="Red resigns"
                        disabled=game_over
                        on:click=move |_| on_resign(Side::RED)
                    >
                        "投降 Resign"
                    </button>
                </Show>
            </div>
            <Show when=evalbar_show>
                <EvalBar sample=current_eval/>
            </Show>
            <Sidebar
                variant=variant
                view=view
                on_new_game=on_new_game
                on_undo=on_undo
                wip=wip
                history=history_signal
                hint_toggle=sidebar_hint_toggle
                eval_badge=current_eval
            />
            <Show when=move || panel_visible.get()>
                <DebugPanel
                    analysis=analysis_signal
                    board=board_for_debug.clone()
                    on_hover=on_debug_hover
                    title=panel_title
                    subtitle=panel_subtitle
                />
            </Show>
            <Show when=move || chart_show.get()>
                <EvalChart samples=eval_samples_signal/>
            </Show>
        </section>
    }
}
