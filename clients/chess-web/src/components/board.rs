//! SVG board renderer. Variant-agnostic: takes a `PlayerView` + observer +
//! a click callback, and lays cells on the intersection grid that the TUI
//! also uses (see `crate::orient`).

use chess_core::board::BoardShape;
use chess_core::coord::Square;
use chess_core::piece::Side;
use chess_core::view::{PlayerView, VisibleCell};
use leptos::*;

use crate::glyph::{self, Style};
use crate::orient::{display_dims, square_at_display};
use crate::state::legal_targets;

pub const CELL: i32 = 60;
pub const PAD: i32 = 30;
const RIVER_TOP_ROW: u8 = 4;
const RIVER_BOT_ROW: u8 = 5;
const TILE_RADIUS: i32 = 24;

#[component]
pub fn Board(
    shape: BoardShape,
    observer: Side,
    #[prop(into)] view: Signal<PlayerView>,
    #[prop(into)] selected: Signal<Option<Square>>,
    #[prop(into)] on_click: Callback<Square>,
) -> impl IntoView {
    let (rows, cols) = display_dims(shape);
    let view_w = (cols as i32 - 1) * CELL + PAD * 2;
    let view_h = (rows as i32 - 1) * CELL + PAD * 2;
    let viewbox = format!("0 0 {} {}", view_w, view_h);
    let style = Style::Cjk;

    view! {
        <svg class="board" viewBox=viewbox preserveAspectRatio="xMidYMid meet">
            <rect class="board-bg" x="0" y="0" width="100%" height="100%"/>
            <g class="grid-layer">{grid_lines(shape, rows, cols)}</g>
            <g class="river-layer">{river_text(shape, cols)}</g>
            <g class="palace-layer">{palace_diagonals(shape)}</g>
            <g class="overlay-layer">
                {move || chain_lock_marker(view.get().chain_lock, observer, shape)}
                {move || selection_marker(selected.get(), observer, shape)}
                {move || move_dots_view(&view.get(), selected.get(), observer, shape)}
            </g>
            <g class="pieces-layer">
                {move || pieces_view(&view.get(), observer, shape, style)}
            </g>
            <g class="cells-layer">
                {hit_cells(rows, cols, observer, shape, on_click)}
            </g>
        </svg>
    }
}

#[inline]
fn intersection(row: u8, col: u8) -> (i32, i32) {
    (PAD + col as i32 * CELL, PAD + row as i32 * CELL)
}

fn grid_lines(shape: BoardShape, rows: u8, cols: u8) -> View {
    let is_xiangqi = matches!(shape, BoardShape::Xiangqi9x10);
    let mut out: Vec<View> = Vec::with_capacity(rows as usize + cols as usize * 2);
    for r in 0..rows {
        let y = PAD + r as i32 * CELL;
        let x1 = PAD;
        let x2 = PAD + (cols as i32 - 1) * CELL;
        out.push(view! { <line class="grid" x1=x1 y1=y x2=x2 y2=y/> }.into_view());
    }
    for c in 0..cols {
        let x = PAD + c as i32 * CELL;
        let is_border = c == 0 || c == cols - 1;
        let y_top = PAD;
        let y_bot = PAD + (rows as i32 - 1) * CELL;
        if is_xiangqi && !is_border {
            let y_river_top = PAD + RIVER_TOP_ROW as i32 * CELL;
            let y_river_bot = PAD + RIVER_BOT_ROW as i32 * CELL;
            out.push(
                view! {
                    <line class="grid" x1=x y1=y_top x2=x y2=y_river_top/>
                    <line class="grid" x1=x y1=y_river_bot x2=x y2=y_bot/>
                }
                .into_view(),
            );
        } else {
            out.push(view! { <line class="grid" x1=x y1=y_top x2=x y2=y_bot/> }.into_view());
        }
    }
    out.into_view()
}

fn river_text(shape: BoardShape, cols: u8) -> View {
    if !matches!(shape, BoardShape::Xiangqi9x10) {
        return ().into_view();
    }
    let cx = PAD + (cols as i32 - 1) * CELL / 2;
    let cy = PAD + (RIVER_TOP_ROW as i32 * CELL + RIVER_BOT_ROW as i32 * CELL) / 2;
    view! {
        <text class="river-text" x=cx y=cy text-anchor="middle" dominant-baseline="central">
            "楚 河 漢 界"
        </text>
    }
    .into_view()
}

fn palace_diagonals(shape: BoardShape) -> View {
    if !matches!(shape, BoardShape::Xiangqi9x10) {
        return ().into_view();
    }
    // Files 3..5; top palace = display rows 0..2; bottom palace = rows 7..9.
    // Symmetric for both observers because the palace box is centered on the file axis.
    let segments = [
        (intersection(0, 3), intersection(2, 5)),
        (intersection(0, 5), intersection(2, 3)),
        (intersection(7, 3), intersection(9, 5)),
        (intersection(7, 5), intersection(9, 3)),
    ];
    segments
        .into_iter()
        .map(|(a, b)| view! { <line class="palace" x1=a.0 y1=a.1 x2=b.0 y2=b.1/> }.into_view())
        .collect::<Vec<View>>()
        .into_view()
}

fn pieces_view(view: &PlayerView, observer: Side, shape: BoardShape, style: Style) -> View {
    let (rows, cols) = display_dims(shape);
    let mut out: Vec<View> = Vec::new();
    for row in 0..rows {
        for col in 0..cols {
            let Some(sq) = square_at_display(row, col, observer, shape) else { continue };
            let cell = view.cells[sq.0 as usize];
            let (cx, cy) = intersection(row, col);
            match cell {
                VisibleCell::Empty => {}
                VisibleCell::Hidden => {
                    out.push(
                        view! {
                            <g class="tile hidden" transform=format!("translate({}, {})", cx, cy)>
                                <circle class="tile-back" r=TILE_RADIUS cx="0" cy="0"/>
                            </g>
                        }
                        .into_view(),
                    );
                }
                VisibleCell::Revealed(p) => {
                    let side_class = match p.piece.side {
                        Side::RED => "red",
                        Side::BLACK => "black",
                        _ => "green",
                    };
                    out.push(
                        view! {
                            <g class=format!("tile piece {}", side_class) transform=format!("translate({}, {})", cx, cy)>
                                <circle class="tile-disc" r=TILE_RADIUS cx="0" cy="0"/>
                                <text class="tile-glyph" text-anchor="middle" dominant-baseline="central">
                                    {glyph::glyph(p.piece.kind, p.piece.side, style)}
                                </text>
                            </g>
                        }
                        .into_view(),
                    );
                }
            }
        }
    }
    out.into_view()
}

fn selection_marker(selected: Option<Square>, observer: Side, shape: BoardShape) -> View {
    let Some(sq) = selected else { return ().into_view() };
    let (rows, cols) = display_dims(shape);
    for row in 0..rows {
        for col in 0..cols {
            if square_at_display(row, col, observer, shape) == Some(sq) {
                let (cx, cy) = intersection(row, col);
                return view! { <circle class="selection" cx=cx cy=cy r="28"/> }.into_view();
            }
        }
    }
    ().into_view()
}

fn chain_lock_marker(locked: Option<Square>, observer: Side, shape: BoardShape) -> View {
    let Some(sq) = locked else { return ().into_view() };
    let (rows, cols) = display_dims(shape);
    for row in 0..rows {
        for col in 0..cols {
            if square_at_display(row, col, observer, shape) == Some(sq) {
                let (cx, cy) = intersection(row, col);
                return view! { <circle class="chain-lock" cx=cx cy=cy r="30"/> }.into_view();
            }
        }
    }
    ().into_view()
}

fn move_dots_view(
    view: &PlayerView,
    selected: Option<Square>,
    observer: Side,
    shape: BoardShape,
) -> View {
    let Some(from) = selected else { return ().into_view() };
    let targets = legal_targets(view, from);
    if targets.is_empty() {
        return ().into_view();
    }
    let (rows, cols) = display_dims(shape);
    let mut out: Vec<View> = Vec::new();
    for row in 0..rows {
        for col in 0..cols {
            let Some(sq) = square_at_display(row, col, observer, shape) else { continue };
            if targets.contains(&sq) {
                let (cx, cy) = intersection(row, col);
                // Distinct dot for "move to my own square" (banqi reveal) — same
                // visual treatment for now; refined in commit 3.
                out.push(view! { <circle class="move-dot" cx=cx cy=cy r="9"/> }.into_view());
            }
        }
    }
    out.into_view()
}

fn hit_cells(
    rows: u8,
    cols: u8,
    observer: Side,
    shape: BoardShape,
    on_click: Callback<Square>,
) -> View {
    let mut out: Vec<View> = Vec::with_capacity(rows as usize * cols as usize);
    for row in 0..rows {
        for col in 0..cols {
            let Some(sq) = square_at_display(row, col, observer, shape) else { continue };
            let x = col as i32 * CELL;
            let y = row as i32 * CELL;
            out.push(
                view! {
                    <rect
                        class="cell-hit"
                        x=x y=y width=CELL height=CELL
                        on:click=move |_| on_click.call(sq)
                    />
                }
                .into_view(),
            );
        }
    }
    out.into_view()
}
