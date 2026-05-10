//! Below-board "captured pieces" strip ("graveyard").
//!
//! Renders as two horizontal rows directly under the Board: each side's
//! dead pieces shown as small disc-icons styled like the board pieces
//! (red glyph on a paler tile-back). A tiny toggle in the header flips
//! the sort order between chronological (default) and rank. The signal
//! lives on `Prefs` so the choice persists in `localStorage`.
//!
//! Mirror-mode split: in pass-and-play mirror mode the two players sit
//! on opposite sides of the device. Each player's captured-pieces shelf
//! ("trophies") sits directly in front of them — the strip splits into
//! two slot positions ([`CapturedSlot::MirroredAbove`] /
//! [`CapturedSlot::MirroredBelow`]) and the page mounts one of each
//! flanking the board. The above-strip is rotated 180° via CSS so the
//! glyphs read upright when viewed from the far seat.

use chess_core::piece::Piece;
use chess_core::view::PlayerView;
use leptos::*;

use crate::glyph::{self, Style};
use crate::prefs::Prefs;
use crate::state::{split_and_sort_captured, CapturedSort};

/// Which row(s) this `<CapturedStrip>` instance renders.
///
/// - `Combined` (default — non-mirror layout) renders both rows under
///   the board with a shared header + sort toggle.
/// - `MirroredAbove` renders only Red captured pieces (Black's
///   trophies), positioned ABOVE the board and flipped 180° so the
///   Black player on the opposite seat reads them upright.
/// - `MirroredBelow` renders only Black captured pieces (Red's
///   trophies), positioned BELOW the board with the shared header
///   (sort toggle stays on this strip — Red has access to it; Black's
///   inverted strip omits the header to avoid duplicating the toggle).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CapturedSlot {
    #[default]
    Combined,
    MirroredAbove,
    MirroredBelow,
}

#[component]
pub fn CapturedStrip(
    #[prop(into)] view: Signal<PlayerView>,
    #[prop(optional)] placement: CapturedSlot,
) -> impl IntoView {
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

    match placement {
        CapturedSlot::Combined => view! {
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
                    <CapturedRow side_class="black" label="黑" pieces=Signal::derive(black_pieces) style=style mirrored=false/>
                    <CapturedRow side_class="red"   label="紅" pieces=Signal::derive(red_pieces)   style=style mirrored=false/>
                </section>
            </Show>
        }
        .into_view(),
        CapturedSlot::MirroredAbove => view! {
            <Show when=any_captured>
                <section class="captured-strip captured-strip--mirrored-above" aria-label="Captured Red pieces (Black's trophies)">
                    // No header on the top strip — sort toggle lives on the bottom strip
                    // so it isn't duplicated. Black player can still see/sort via the
                    // bottom (Red's) toggle indirectly since sort affects both rows.
                    <CapturedRow side_class="red" label="紅" pieces=Signal::derive(red_pieces) style=style mirrored=true/>
                </section>
            </Show>
        }
        .into_view(),
        CapturedSlot::MirroredBelow => view! {
            <Show when=any_captured>
                <section class="captured-strip captured-strip--mirrored-below" aria-label="Captured Black pieces (Red's trophies)">
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
                    <CapturedRow side_class="black" label="黑" pieces=Signal::derive(black_pieces) style=style mirrored=false/>
                </section>
            </Show>
        }
        .into_view(),
    }
}

#[component]
fn CapturedRow(
    side_class: &'static str,
    label: &'static str,
    #[prop(into)] pieces: Signal<Vec<Piece>>,
    style: Style,
    mirrored: bool,
) -> impl IntoView {
    let row_class = if mirrored {
        format!("captured-row captured-row--mirrored {side_class}")
    } else {
        format!("captured-row {side_class}")
    };
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
