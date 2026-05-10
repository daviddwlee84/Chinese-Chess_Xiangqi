//! Lichess-style vertical eval bar — narrow SVG strip that lives next
//! to the board, fills from the **bottom** with red and from the
//! **top** with black, and shows the current Red-side win % at the
//! boundary.
//!
//! Mounted by `pages/local.rs` as a sibling to `<Board>` inside
//! `.board-pane`, to the right of the board SVG. CSS in `style.css`
//! places it as a flex column item; the SVG itself uses
//! `preserveAspectRatio="none"` so its height stretches to match the
//! board.
//!
//! The bar reads a single `Signal<Option<EvalSample>>` (the
//! most-recent sample). Empty state (no analysis yet, or evalbar flag
//! off) renders a neutral 50/50 bar with a muted "—" label so the UI
//! geometry stays consistent regardless of whether data has arrived.

use leptos::*;

use crate::eval::EvalSample;

/// Render the vertical eval bar.
///
/// `sample` is the most-recent [`EvalSample`]; when `None` (no
/// analysis yet) the bar shows a neutral 50/50 split with no marker
/// text so the placeholder doesn't lie about the position.
#[component]
pub fn EvalBar(
    /// The latest sample. `None` before any AI/hint analyze has run,
    /// or when `?evalbar=1` is off.
    #[prop(into)]
    sample: Signal<Option<EvalSample>>,
) -> impl IntoView {
    // Y in viewBox terms: 0 = top (black), 100 = bottom (red).
    // Red fills from bottom up; black fills from top down.
    // `boundary_y` = position of the dividing line.
    //
    // Convention pinned: chart Y axis is "Red wins %", so 100 % red
    // → boundary at y=0 (red fills the entire bar). 0 % red → boundary
    // at y=100 (black fills the entire bar).
    let boundary_y = move || {
        sample.with(|s| match s {
            Some(s) => 100.0 - (s.red_win_pct * 100.0),
            None => 50.0, // neutral 50/50
        })
    };

    let red_pct_text = move || {
        sample.with(|s| match s {
            Some(s) => format!("{}%", (s.red_win_pct * 100.0).round() as i32),
            None => "—".to_string(),
        })
    };

    let black_pct_text = move || {
        sample.with(|s| match s {
            Some(s) => format!("{}%", (s.black_win_pct() * 100.0).round() as i32),
            None => "—".to_string(),
        })
    };

    let cp_text = move || {
        sample.with(|s| match s {
            Some(s) => format!("{:+} cp (Red POV)", red_pov_cp(s)),
            None => "no analysis yet".to_string(),
        })
    };

    view! {
        <aside class="eval-bar" aria-label="AI win-rate bar (Red bottom, Black top)" title=cp_text>
            <svg
                class="eval-bar__svg"
                viewBox="0 0 10 100"
                preserveAspectRatio="none"
                aria-hidden="true"
            >
                // Black background (full height) — red rect grows on top.
                <rect x="0" y="0" width="10" height="100" class="eval-bar__bg-black"/>
                // Red fill from bottom up — height = 100 - boundary_y.
                <rect
                    x="0"
                    width="10"
                    y=move || boundary_y().to_string()
                    height=move || (100.0 - boundary_y()).to_string()
                    class="eval-bar__fill-red"
                />
                // Boundary line for visual separation.
                <line
                    x1="0"
                    x2="10"
                    y1=move || boundary_y().to_string()
                    y2=move || boundary_y().to_string()
                    class="eval-bar__boundary"
                />
            </svg>
            // Top label = Black's %, bottom label = Red's %. Matches
            // the bar's spatial convention (black on top, red on bottom).
            <span class="eval-bar__label eval-bar__label--top">{black_pct_text}</span>
            <span class="eval-bar__label eval-bar__label--bot">{red_pct_text}</span>
        </aside>
    }
}

/// Project the side-relative cp back to Red's POV for the tooltip.
/// (We store `cp_stm_pov` rather than red-POV cp so the raw value the
/// engine produced is preserved for debug; conversion happens here.)
fn red_pov_cp(s: &EvalSample) -> i32 {
    use chess_core::piece::Side;
    match s.side_to_move_at_pos {
        Side::RED => s.cp_stm_pov,
        _ => -s.cp_stm_pov,
    }
}
