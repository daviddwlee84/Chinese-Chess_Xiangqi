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
use crate::state::find_move;

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

    let on_click: Callback<Square> = Callback::new(move |sq: Square| {
        if wip {
            return;
        }
        let v = view.get();
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
        match cell {
            VisibleCell::Revealed(p) if p.piece.side == v.side_to_move => {
                selected.set(Some(sq));
            }
            VisibleCell::Hidden => {
                // Banqi flip — engine emits `Reveal { at }` whose origin and
                // target both equal `sq`. Apply directly.
                if let Some(mv) = find_move(&v, sq, sq) {
                    state.update(|s| {
                        s.make_move(&mv).expect("legal reveal");
                        s.refresh_status();
                    });
                    selected.set(None);
                }
            }
            _ => {
                selected.set(None);
            }
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

    view! {
        <section class="game-page">
            <div class="board-pane">
                <Board
                    shape=shape
                    observer=observer
                    view=view
                    selected=selected
                    on_click=on_click
                />
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
