use chess_core::coord::Square;
use chess_core::piece::Side;
use chess_core::rules::{RuleSet, Variant};
use chess_core::state::GameState;
use chess_core::view::{PlayerView, VisibleCell};
use leptos::*;
use leptos_router::{use_params_map, use_query_map};

use crate::components::board::Board;
use crate::components::end_overlay::EndOverlay;
use crate::components::sidebar::Sidebar;
use crate::prefs::Prefs;
use crate::routes::{build_rule_set, parse_local_rules, parse_variant_slug};
use crate::state::{end_chain_move, find_move};

#[component]
pub fn LocalPage() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let resolved = move || -> Result<(Variant, RuleSet, bool), String> {
        let slug = params.with(|p| p.get("variant").cloned().unwrap_or_default());
        let variant =
            parse_variant_slug(&slug).ok_or_else(|| format!("Unknown variant: {}", slug))?;
        let parsed = query.with(|q| parse_local_rules(|k| q.get(k).cloned()));
        let rules = build_rule_set(variant, &parsed);
        // Three-kingdom: engine ships an empty 4×8 board. Render it but
        // disable interaction and overlay a "WIP" banner — see
        // `backlog/three-kingdoms-banqi.md` and the gotcha in CLAUDE.md.
        let wip = matches!(variant, Variant::ThreeKingdomBanqi);
        Ok((variant, rules, wip))
    };

    move || match resolved() {
        Ok((variant, rules, wip)) => {
            view! { <LocalGame variant=variant rules=rules wip=wip/> }.into_view()
        }
        Err(msg) => view! {
            <section class="game-page game-page--single">
                <div>
                    <a href="/" class="back-link">"← Back to picker"</a>
                    <h2>"Variant unavailable"</h2>
                    <p class="subtitle">{msg}</p>
                </div>
            </section>
        }
        .into_view(),
    }
}

#[component]
fn LocalGame(variant: Variant, rules: RuleSet, wip: bool) -> impl IntoView {
    let initial_rules = rules.clone();
    let state = create_rw_signal(GameState::new(rules));
    let selected = create_rw_signal::<Option<Square>>(None);

    // Local pass-and-play renders from a fixed Red-side observer (board never
    // flips on turn change). Legal moves come from the projection for the
    // *current* side-to-move so click-to-move uses the right list.
    let observer = Side::RED;
    let shape = state.with_untracked(|s| s.board.shape());

    let view = create_memo(move |_| state.with(|s| PlayerView::project(s, s.side_to_move)));

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

    let on_end_chain: Callback<()> = Callback::new(move |_| {
        if wip {
            return;
        }
        let v = view.get();
        if let Some(mv) = end_chain_move(&v) {
            state.update(|s| {
                s.make_move(&mv).expect("legal end-chain");
                s.refresh_status();
            });
            selected.set(None);
        }
    });

    let on_click: Callback<Square> = Callback::new(move |sq: Square| {
        if wip {
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
            }
        } else {
            selected.set(None);
        }
    });

    let on_undo: Callback<()> = Callback::new(move |_| {
        if wip {
            return;
        }
        state.update(|s| {
            let _ = s.unmake_move();
            s.refresh_status();
        });
        selected.set(None);
    });

    let on_new_game: Callback<()> = Callback::new({
        let initial_rules = initial_rules.clone();
        move |_| {
            state.set(GameState::new(initial_rules.clone()));
            selected.set(None);
        }
    });

    let prefs = expect_context::<Prefs>();
    let fx_confetti: Signal<bool> = prefs.fx_confetti.into();
    // Local pass-and-play: no `ClientRole`, so the overlay falls back to
    // the neutral "Red Wins" / "Black Wins" framing.
    let role_signal: Signal<Option<crate::state::ClientRole>> = Signal::derive(|| None);

    let chain_active = Signal::derive(move || chain_locked_signal.get().is_some());

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
            </div>
            <Sidebar
                variant=variant
                view=view
                on_new_game=on_new_game
                on_undo=on_undo
                wip=wip
            />
        </section>
    }
}
