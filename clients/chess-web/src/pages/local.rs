use chess_ai::{AiOptions, Difficulty, Randomness, Strategy};
use chess_core::coord::Square;
use chess_core::piece::Side;
use chess_core::rules::{RuleSet, Variant};
use chess_core::state::{GameState, GameStatus};
use chess_core::view::{PlayerView, VisibleCell};
use leptos::*;
use leptos_router::{use_params_map, use_query_map};

use crate::components::board::Board;
use crate::components::captured::CapturedStrip;
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
    // Bumped on every state change (player move, AI move, undo, new game).
    // The AI task captures the epoch when it starts and discards its result
    // if the epoch has changed by the time it returns — prevents stale AI
    // moves landing after the player undid or restarted.
    let move_epoch = create_rw_signal::<u32>(0);

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
        move_epoch.update(|n| *n = n.wrapping_add(1));
    });

    let on_new_game: Callback<()> = Callback::new({
        let initial_rules = initial_rules.clone();
        move |_| {
            state.set(GameState::new(initial_rules.clone()));
            selected.set(None);
            ai_thinking.set(false);
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
                max_depth: None,
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
                let chosen = chess_ai::choose_move(&snapshot, &opts);
                if move_epoch.get_untracked() != cur_epoch {
                    ai_thinking.set(false);
                    return;
                }
                if let Some(result) = chosen {
                    state.update(|s| {
                        if s.make_move(&result.mv).is_ok() {
                            s.refresh_status();
                        }
                    });
                    move_epoch.update(|n| *n = n.wrapping_add(1));
                }
                ai_thinking.set(false);
            });
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

    view! {
        <section class="game-page">
            <div class="board-pane">
                <Board
                    shape=shape
                    observer=observer
                    view=view
                    selected=effective_selected
                    on_click=on_click
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
            />
        </section>
    }
}
