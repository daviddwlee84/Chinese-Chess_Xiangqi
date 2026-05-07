//! Below-board "captured pieces" strip ("graveyard").
//!
//! Renders as two horizontal rows directly under the Board: each side's
//! dead pieces shown as small disc-icons styled like the board pieces
//! (red glyph on a paler tile-back). A tiny toggle in the header flips
//! the sort order between chronological (default) and rank. The signal
//! lives on `Prefs` so the choice persists in `localStorage`.

use chess_core::piece::Piece;
use chess_core::view::PlayerView;
use leptos::*;

use crate::glyph::{self, Style};
use crate::prefs::Prefs;
use crate::state::{split_and_sort_captured, CapturedSort};

#[component]
pub fn CapturedStrip(#[prop(into)] view: Signal<PlayerView>) -> impl IntoView {
    let prefs = expect_context::<Prefs>();
    let sort_signal = prefs.captured_sort;
    let style = Style::Cjk;

    let red_pieces = move || {
        let v = view.get();
        let (red, _black) = split_and_sort_captured(&v.captured, sort_signal.get());
        red
    };
    let black_pieces = move || {
        let v = view.get();
        let (_red, black) = split_and_sort_captured(&v.captured, sort_signal.get());
        black
    };

    // Hide the strip entirely until at least one piece has died — keeps
    // the board area uncluttered for opening positions.
    let any_captured = move || !view.get().captured.is_empty();

    let toggle_label = move || match sort_signal.get() {
        CapturedSort::Time => "⏱ time",
        CapturedSort::Rank => "📊 rank",
    };
    let on_toggle = move |_| sort_signal.update(|s| *s = s.toggled());

    view! {
        <Show when=any_captured>
            <section class="captured-strip" aria-label="Captured pieces">
                <div class="captured-header">
                    <h4>"Captured 死亡"</h4>
                    <button
                        class="captured-toggle btn"
                        on:click=on_toggle
                        title="Toggle sort: time / rank"
                    >
                        {toggle_label}
                    </button>
                </div>
                <CapturedRow side_class="black" label="黑" pieces=Signal::derive(black_pieces) style=style/>
                <CapturedRow side_class="red"   label="紅" pieces=Signal::derive(red_pieces)   style=style/>
            </section>
        </Show>
    }
}

#[component]
fn CapturedRow(
    side_class: &'static str,
    label: &'static str,
    #[prop(into)] pieces: Signal<Vec<Piece>>,
    style: Style,
) -> impl IntoView {
    let row_class = format!("captured-row {side_class}");
    let glyphs = move || {
        let ps = pieces.get();
        if ps.is_empty() {
            view! { <span class="captured-empty">"—"</span> }.into_view()
        } else {
            ps.into_iter()
                .map(|p| {
                    let g = glyph::glyph(p.kind, p.side, style).to_string();
                    view! {
                        <span class="captured-piece" aria-label=format!("{:?}", p.kind)>
                            <span class="captured-piece-glyph">{g}</span>
                        </span>
                    }
                })
                .collect_view()
        }
    };
    view! {
        <div class=row_class>
            <span class="captured-label">{label}</span>
            <div class="captured-pieces">{glyphs}</div>
        </div>
    }
}
