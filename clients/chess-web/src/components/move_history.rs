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
#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub ply: usize,
    pub side: Side,
    pub text: String,
    /// Optional win-% delta from the mover's POV that this move
    /// produced. Positive = the move improved the mover's position
    /// (in win-% terms); negative = the move worsened it. `None` when
    /// the producer doesn't have the paired before/after samples
    /// available (e.g. `?evalbar=1` is off, or the analysis hasn't
    /// landed yet for one of the two endpoints).
    ///
    /// Rendered as a small `+5%` / `-12%` annotation in the row.
    /// Lichess-style move-quality glyphs (`?!` / `??` / `!`) are not
    /// used — they're contentious in xiangqi where the bot's eval
    /// isn't deeply calibrated for the variant.
    pub eval_delta_pct: Option<f32>,
}

impl PartialEq for HistoryEntry {
    fn eq(&self, other: &Self) -> bool {
        self.ply == other.ply && self.side == other.side && self.text == other.text
    }
}

impl Eq for HistoryEntry {}

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
    // Eval delta annotation: shown only when the producer (pages/local.rs)
    // had paired before/after samples available — i.e. `?evalbar=1`
    // was on AND the analysis had landed for both endpoints by the
    // time this row was built.
    let delta_view = entry.eval_delta_pct.map(|d| {
        let pct = (d * 100.0).round() as i32;
        let (cls, text) = if pct > 0 {
            ("move-history__delta move-history__delta--up", format!("+{}%", pct))
        } else if pct < 0 {
            ("move-history__delta move-history__delta--down", format!("{}%", pct))
        } else {
            ("move-history__delta move-history__delta--flat", "±0%".to_string())
        };
        view! { <span class=cls title="Win-% change for this mover (bot's view)">{text}</span> }
    });
    view! {
        <li class=class>
            <span class="move-history__num">{num_text}</span>
            <span class="move-history__side">{label}</span>
            <span class="move-history__text">{text}</span>
            {delta_view}
        </li>
    }
}
