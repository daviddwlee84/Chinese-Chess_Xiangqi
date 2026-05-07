use chess_core::coord::Square;
use chess_core::piece::Side;
use chess_core::rules::{RuleSet, Variant};
use chess_core::state::GameState;
use chess_core::view::{PlayerView, VisibleCell};
use leptos::*;
use leptos_router::use_params_map;

use crate::components::board::Board;
use crate::components::sidebar::Sidebar;
use crate::routes::parse_variant_slug;
use crate::state::find_move;

#[component]
pub fn LocalPage() -> impl IntoView {
    let params = use_params_map();
    let resolved = move || -> Result<(Variant, RuleSet), String> {
        let slug = params.with(|p| p.get("variant").cloned().unwrap_or_default());
        match parse_variant_slug(&slug) {
            Some(Variant::Xiangqi) => Ok((Variant::Xiangqi, RuleSet::xiangqi_casual())),
            Some(Variant::Banqi) => Err("Banqi support ships in the next commit.".into()),
            Some(Variant::ThreeKingdomBanqi) => {
                Err("Three-kingdom support ships in the next commit.".into())
            }
            None => Err(format!("Unknown variant: {}", slug)),
        }
    };

    move || match resolved() {
        Ok((variant, rules)) => view! { <LocalGame variant=variant rules=rules/> }.into_view(),
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
fn LocalGame(variant: Variant, rules: RuleSet) -> impl IntoView {
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
            _ => {
                selected.set(None);
            }
        }
    });

    let on_undo: Callback<()> = Callback::new(move |_| {
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
            </div>
            <Sidebar
                variant=variant
                view=view
                on_new_game=on_new_game
                on_undo=on_undo
            />
        </section>
    }
}
