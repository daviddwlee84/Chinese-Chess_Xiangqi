//! Move history sidebar panel.
//!
//! Renders a scrollable list of plies in ICCS notation, side-coloured
//! by mover, with the move number column. Designed for debugging and
//! casual review — not a full PGN replay UI (history scrubber lives in
//! P3 backlog, see `TODO.md`).
//!
//! Decoupled from chess-core: takes a pre-encoded `Vec<HistoryEntry>`
//! so the parent (typically `pages/local.rs`) decides how to format
//! each move (ICCS, WXF, raw coords, etc.). Net mode currently has no
//! history because `PlayerView` doesn't carry one — when a future
//! protocol bump adds it, this component already accepts the right
//! shape.

use chess_core::piece::Side;
use leptos::*;

/// Pre-encoded history entry: 1-based ply number + mover side + display string.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct HistoryEntry {
    pub ply: usize,
    pub side: Side,
    pub text: String,
}

#[component]
pub fn MoveHistory(
    /// Each ply in order; element 0 is the first move played.
    #[prop(into)]
    entries: Signal<Vec<HistoryEntry>>,
) -> impl IntoView {
    let is_empty = move || entries.with(|e| e.is_empty());

    view! {
        <section class="move-history" aria-label="Move history">
            <h4 class="move-history__title">"History 棋譜"</h4>
            <Show
                when=move || !is_empty()
                fallback=|| view! { <p class="muted move-history__empty">"No moves yet."</p> }
            >
                <ol class="move-history__list">
                    <For
                        each=move || entries.get()
                        key=|entry| (entry.ply, entry.text.clone())
                        children=move |entry| view! { <HistoryRow entry=entry/> }
                    />
                </ol>
            </Show>
        </section>
    }
}

#[component]
fn HistoryRow(entry: HistoryEntry) -> impl IntoView {
    let class = match entry.side {
        Side::RED => "move-history__entry move-history__entry--red",
        Side::BLACK => "move-history__entry move-history__entry--black",
        _ => "move-history__entry",
    };
    let label = match entry.side {
        Side::RED => "紅",
        Side::BLACK => "黑",
        _ => "綠",
    };
    let num_text = format!("{:>3}.", entry.ply);
    let text = entry.text;
    view! {
        <li class=class>
            <span class="move-history__num">{num_text}</span>
            <span class="move-history__side">{label}</span>
            <span class="move-history__text">{text}</span>
        </li>
    }
}
