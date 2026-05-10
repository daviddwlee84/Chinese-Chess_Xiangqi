use chess_ai::{AiAnalysis, AiOptions, Difficulty, Randomness, Strategy};
use chess_core::coord::Square;
use chess_core::moves::Move;
use chess_core::piece::Side;
use chess_core::rules::{RuleSet, Variant};
use chess_core::state::{GameState, GameStatus};
use chess_core::view::{PlayerView, VisibleCell};
use leptos::*;
use leptos_router::{use_params_map, use_query_map};

use crate::components::board::Board;
use crate::components::captured::CapturedStrip;
use crate::components::debug_panel::DebugPanel;
use crate::components::end_overlay::EndOverlay;
use crate::components::move_history::HistoryEntry;
use crate::components::sidebar::Sidebar;
use crate::prefs::Prefs;
use crate::routes::{app_href, build_rule_set, parse_local_rules, parse_variant_slug, PlayMode};
use crate::state::{end_chain_move, find_move};

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
    /// `?debug=1` — sticky always-on AI debug panel. The AI move pump
    /// uses `chess_ai::analyze` (full scored root list) instead of
    /// `chess_ai::choose_move`. Pure UI feature — same search work
    /// either way.
    debug: bool,
    /// `?hints=1` — adds a "🧠 Show AI hint" toggle button in the
    /// sidebar. Default state: hidden; the user clicks to expand /
    /// collapse mid-game. Distinct from `debug`: debug = sticky panel
    /// for power users; hints = on-demand panel for players who want
    /// help on their turn.
    hints: bool,
}

#[component]
pub fn LocalPage() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let resolved = move || -> Result<(Variant, RuleSet, bool, Option<VsAiConfig>), String> {
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
                // Decoupled flags:
                // - `debug` (?debug=1) → sticky always-on panel
                // - `hints` (?hints=1) → in-game toggle button, default hidden
                // The picker writes ?hints=1 (user-friendly); ?debug=1
                // remains for power-user URLs. Net mode (play.rs) adds
                // a server-permission gate on top of these.
                debug: parsed.ai_debug,
                hints: parsed.ai_hints,
            })
        } else {
            None
        };
        Ok((variant, rules, wip, ai))
    };

    move || match resolved() {
        Ok((variant, rules, wip, ai)) => {
            view! { <LocalGame variant=variant rules=rules wip=wip ai=ai/> }.into_view()
        }
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
fn LocalGame(variant: Variant, rules: RuleSet, wip: bool, ai: Option<VsAiConfig>) -> impl IntoView {
    let initial_rules = rules.clone();
    let state = create_rw_signal(GameState::new(rules));
    let selected = create_rw_signal::<Option<Square>>(None);
    let ai_thinking = create_rw_signal(false);
    // Most recent AI analysis (only populated when cfg.debug = true).
    // Cleared on undo / new game so the panel doesn't show stale info.
    let ai_analysis = create_rw_signal::<Option<AiAnalysis>>(None);
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
    // Two distinct UX modes share one `<DebugPanel>` mount, but with
    // different semantics:
    //
    // - `?debug=1` (sticky / power-user): panel is always visible.
    //   The AI move pump caches its own POV analysis after picking
    //   each move — the panel shows "why did the bot play this?".
    //
    // - `?hints=1` (on-demand / player-friendly): a "🧠 Show AI hint"
    //   toggle button appears in the sidebar. Default hidden so the
    //   user has to actively ask for help. When opened on the human's
    //   turn, a separate "hint pump" runs `analyze` from the human's
    //   POV — "if the bot were me, what would it play?". Closing the
    //   toggle stops new analyses from being computed (cached one
    //   stays so reopening is instant).
    let debug_enabled = ai.map(|cfg| cfg.debug).unwrap_or(false);
    let hints_enabled = ai.map(|cfg| cfg.hints).unwrap_or(false);
    let hints_open = create_rw_signal::<bool>(false);
    let panel_visible =
        Signal::derive(move || debug_enabled || (hints_enabled && hints_open.get()));
    // Sidebar gets the toggle ONLY when hints (not debug) is the
    // controlling flag — debug already shows the panel always-on, so
    // an extra button would be redundant noise.
    let sidebar_hint_toggle: Option<RwSignal<bool>> =
        if hints_enabled && !debug_enabled { Some(hints_open) } else { None };

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
        ai_analysis.set(None);
        highlighted_pv.set(Vec::new());
        move_epoch.update(|n| *n = n.wrapping_add(1));
    });

    let on_new_game: Callback<()> = Callback::new({
        let initial_rules = initial_rules.clone();
        move |_| {
            state.set(GameState::new(initial_rules.clone()));
            selected.set(None);
            ai_thinking.set(false);
            ai_analysis.set(None);
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
                let analysis = chess_ai::analyze(&snapshot, &opts);
                if move_epoch.get_untracked() != cur_epoch {
                    ai_thinking.set(false);
                    return;
                }
                if let Some(a) = analysis {
                    let chosen_mv = a.chosen.mv.clone();
                    // Cache AI's POV analysis ONLY for ?debug=1 mode
                    // (sticky panel showing "why did the bot pick
                    // this?"). For ?hints=1 mode, the hint pump runs
                    // separately for the human's POV — caching AI's
                    // POV here would clobber the human's hint with
                    // stale debug data on the very next AI turn.
                    if cfg.debug {
                        ai_analysis.set(Some(a));
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

    // ---- Hint pump (player's POV analysis when the toggle is open) ----
    //
    // Distinct from the debug pump above: that one runs `analyze` from
    // the AI's POV at the moment the AI picks a move, caching what the
    // AI considered. THIS pump runs `analyze` from the human's POV —
    // "what would the bot play if it were me?". Fires whenever the
    // toggle is on AND it's the human's turn AND the game is ongoing.
    //
    // Skipped when:
    // - hints disabled (no `?hints=1`)
    // - debug enabled (debug pump already covers analysis; AI's POV is
    //   what debug users want anyway)
    // - panel not visible (saves the ~100-300 ms search cost)
    // - it's the AI's turn (the AI pump will run its own analyze)
    // - AI is mid-think (avoid racing the AI pump)
    if let Some(cfg) = ai {
        if cfg.hints && !cfg.debug {
            create_effect(move |_| {
                // All four reads are tracked so the effect re-fires when
                // ANY of them change. Critical: `ai_thinking` flips
                // false AFTER `state.update()` in the AI pump, so the
                // first reactive fire (from state change) sees thinking
                // still true and bails. We need the second fire (from
                // ai_thinking → false) to actually run analyze.
                let s = state.get();
                let visible = panel_visible.get();
                let thinking = ai_thinking.get();
                if !visible {
                    return;
                }
                if !matches!(s.status, GameStatus::Ongoing) {
                    ai_analysis.set(None);
                    return;
                }
                if s.side_to_move == cfg.ai_side {
                    // AI's turn — wait until AI moves and side flips.
                    return;
                }
                if thinking {
                    // AI mid-think; will re-fire when ai_thinking → false.
                    return;
                }
                // Snapshot the position hash for invalidation. Using
                // `position_hash` (from chess-core v5) lets us cancel
                // stale tasks WITHOUT depending on `move_epoch` —
                // which is bumped by the AI pump's
                // `state.update(...) → move_epoch.update(...)` ordering
                // race (the hint effect would fire from `state.update`
                // BEFORE `move_epoch.update`, capturing the pre-bump
                // epoch and then bailing 40ms later when it doesn't
                // match).
                let snapshot = s;
                let snapshot_hash = snapshot.position_hash;
                let opts = AiOptions {
                    difficulty: cfg.difficulty,
                    max_depth: cfg.depth,
                    seed: Some(snapshot_hash ^ 0xC0FFEE_BABE_u64),
                    strategy: cfg.strategy,
                    randomness: Some(chess_ai::Randomness::STRICT),
                };
                wasm_bindgen_futures::spawn_local(async move {
                    // Yield a frame so the UI repaints (toggle button
                    // flips state visibly) before the search blocks.
                    gloo_timers::future::TimeoutFuture::new(40).await;
                    // Bail if state moved on while we yielded.
                    let current_hash = state.with_untracked(|cur| cur.position_hash);
                    if current_hash != snapshot_hash {
                        return;
                    }
                    let analysis = chess_ai::analyze(&snapshot, &opts);
                    // Bail if state moved on during the search itself.
                    let current_hash = state.with_untracked(|cur| cur.position_hash);
                    if current_hash != snapshot_hash {
                        return;
                    }
                    if let Some(a) = analysis {
                        ai_analysis.set(Some(a));
                    }
                });
            });
        }
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
    let history_signal: Signal<Vec<HistoryEntry>> = Signal::derive(move || {
        state.with(|s| {
            s.history
                .iter()
                .enumerate()
                .map(|(i, rec)| HistoryEntry {
                    ply: i + 1,
                    side: rec.mover,
                    text: chess_core::notation::iccs::encode_move(&s.board, &rec.the_move),
                })
                .collect()
        })
    });

    let highlighted_pv_signal: Signal<Vec<Move>> = highlighted_pv.into();
    let on_debug_hover: Callback<Vec<Move>> =
        Callback::new(move |pv: Vec<Move>| highlighted_pv.set(pv));
    let board_for_debug = state.with_untracked(|s| s.board.clone());
    let analysis_signal: Signal<Option<AiAnalysis>> = ai_analysis.into();

    view! {
        <section class="game-page">
            <div class="board-pane">
                <Board
                    shape=shape
                    observer=observer
                    view=view
                    selected=effective_selected
                    on_click=on_click
                    highlighted_pv=highlighted_pv_signal
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
                <Show when=move || !wip>
                    <EndOverlay view=view role=role_signal enabled=fx_confetti/>
                </Show>
                <Show when=move || !wip>
                    <CapturedStrip view=view/>
                </Show>
            </div>
            <Sidebar
                variant=variant
                view=view
                on_new_game=on_new_game
                on_undo=on_undo
                wip=wip
                history=history_signal
                hint_toggle=sidebar_hint_toggle
            />
            <Show when=move || panel_visible.get()>
                <DebugPanel
                    analysis=analysis_signal
                    board=board_for_debug.clone()
                    on_hover=on_debug_hover
                />
            </Show>
        </section>
    }
}
