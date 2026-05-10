//! SVG board renderer. Variant-agnostic: takes a `PlayerView` + observer +
//! a click callback, and lays cells on the intersection grid that the TUI
//! also uses (see `crate::orient`).

use chess_core::board::BoardShape;
use chess_core::coord::Square;
use chess_core::moves::Move;
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
/// Extra space reserved OUTSIDE the existing PAD for ICCS-style coord
/// labels (a..i / 0..9). Lives in negative-coord territory so we don't
/// shrink the playing area or change piece geometry. Without this, an
/// edge-rank piece (e.g. 俥 on a0) overlaps the label — PAD=30 vs
/// piece radius=24 only leaves 6 px of clearance, not enough to fit a
/// readable label outside the disc.
const LABEL_MARGIN: i32 = 18;

#[component]
pub fn Board(
    shape: BoardShape,
    observer: Side,
    #[prop(into)] view: Signal<PlayerView>,
    #[prop(into)] selected: Signal<Option<Square>>,
    #[prop(into)] on_click: Callback<Square>,
    /// Optional debug-overlay highlight for an arbitrary move chain
    /// (no commitment — purely visual). Used by the AI debug panel:
    /// element 0 is the AI's chosen move, elements 1..N are the
    /// principal variation continuation. Renders as a sequence of
    /// from→to arrows with the first move bright and later moves
    /// progressively faded so the user can trace the AI's predicted
    /// line. Empty Vec = no highlight.
    #[prop(optional, into)]
    highlighted_pv: Option<Signal<Vec<Move>>>,
    /// Pass-and-play mirror mode: when `true`, Black-side piece glyphs
    /// are rendered rotated 180° so a player sitting opposite the device
    /// reads their pieces upright. Coordinates / hit-testing unchanged.
    #[prop(optional, into)]
    mirror_black: Option<Signal<bool>>,
) -> impl IntoView {
    let (rows, cols) = display_dims(shape);
    let view_w = (cols as i32 - 1) * CELL + PAD * 2;
    let view_h = (rows as i32 - 1) * CELL + PAD * 2;
    // Extend the viewBox into negative space on all sides for coord
    // labels. Pieces still live at their original positive-coord
    // centers — only the SVG canvas grows. Bg rect grows to match so
    // the labels render on the wood-tone board frame, not on the dark
    // page background.
    let total_w = view_w + LABEL_MARGIN * 2;
    let total_h = view_h + LABEL_MARGIN * 2;
    let viewbox = format!("{} {} {} {}", -LABEL_MARGIN, -LABEL_MARGIN, total_w, total_h);
    let style = Style::Cjk;
    let highlighted_pv: Signal<Vec<Move>> =
        highlighted_pv.unwrap_or_else(|| Signal::derive(Vec::new));
    let mirror_black: Signal<bool> = mirror_black.unwrap_or_else(|| Signal::derive(|| false));

    view! {
        <svg class="board" viewBox=viewbox preserveAspectRatio="xMidYMid meet">
            <rect
                class="board-bg"
                x=-LABEL_MARGIN
                y=-LABEL_MARGIN
                width=total_w
                height=total_h
            />
            <g class="grid-layer">{grid_lines(shape, rows, cols)}</g>
            <g class="coord-layer">
                {move || coord_labels(shape, rows, cols, observer, mirror_black.get())}
            </g>
            <g class="river-layer">{river_text(shape, cols)}</g>
            <g class="palace-layer">{palace_diagonals(shape)}</g>
            <g class="overlay-layer">
                {move || move_dots_view(&view.get(), selected.get(), observer, shape)}
            </g>
            <g class="pieces-layer">
                {move || pieces_view(&view.get(), observer, shape, style, mirror_black.get())}
            </g>
            <g class="overlay-top-layer">
                {move || chain_lock_marker(view.get().chain_lock, observer, shape)}
                {move || selection_marker(selected.get(), observer, shape)}
                {move || debug_pv_marker(highlighted_pv.get(), observer, shape)}
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

/// Algebraic-style coordinate labels around the board: file letters
/// (`a..i`) along the top + bottom edges, rank digits (`0..9`) along
/// the left + right edges. Same notation as the ICCS-encoded moves
/// shown in the AI hint / debug panel and the move-history sidebar
/// (e.g. `h2g2` = move from h2 to g2). Without these labels users
/// can't visually map the table entries to board squares.
///
/// Observer-aware: looks up file/rank via `square_at_display` then
/// asks the board for the chess-coord of that square. This way the
/// labels respect Red-at-bottom vs Black-at-bottom orientation
/// automatically (Red sees `a..i` left-to-right, Black sees `i..a`).
///
/// Labels live in the LABEL_MARGIN strip OUTSIDE the original PAD
/// area (negative coords for top/left, beyond view_w/view_h for
/// bottom/right). The previous version put them at PAD-16, which
/// landed *inside* the disc of any edge-rank/edge-file piece (PAD=30
/// vs piece radius=24 → only 6 px clearance) so the corner labels
/// were invisible whenever a 俥/車 sat on a0/i0/a9/i9. Negative-coord
/// placement keeps them clear of all piece geometry.
///
/// Xiangqi only — banqi has no algebraic-style notation in this codebase.
fn coord_labels(shape: BoardShape, rows: u8, cols: u8, observer: Side, mirror_black: bool) -> View {
    if !matches!(shape, BoardShape::Xiangqi9x10) {
        return ().into_view();
    }
    let board = chess_core::board::Board::new(shape);
    let mut out: Vec<View> = Vec::with_capacity((cols as usize + rows as usize) * 2);

    // In mirror mode, top file labels and right rank labels are rotated
    // 180° in place so the player on the opposite seat reads them
    // upright (matches the same rotation we apply to Black's piece
    // glyphs). Bottom + left labels stay normal for the Red player.
    let mirror_attr = |x: i32, y: i32| -> String { format!("rotate(180 {} {})", x, y) };

    // File labels (a..i) — read the file from the bottom-edge cell of
    // each column (any row would do; bottom-edge avoids needing to
    // worry about non-rectangular shapes), then mirror at the top edge.
    let bottom_row = rows - 1;
    let view_h = (rows as i32 - 1) * CELL + PAD * 2;
    // Halfway up the top margin (negative coords): well clear of
    // top-edge piece discs (whose top edge is at y = PAD - TILE_RADIUS = 6).
    let label_y_top = -LABEL_MARGIN / 2;
    // Halfway down the bottom margin (beyond view_h): clear of
    // bottom-edge piece discs (whose bottom edge is at y = view_h - PAD + TILE_RADIUS).
    let label_y_bot = view_h + LABEL_MARGIN / 2;
    for col in 0..cols {
        let Some(sq) = square_at_display(bottom_row, col, observer, shape) else { continue };
        let (f, _) = board.file_rank(sq);
        let ch = (b'a' + f.0) as char;
        let x = PAD + col as i32 * CELL;
        let top_transform = if mirror_black { mirror_attr(x, label_y_top) } else { String::new() };
        out.push(
            view! {
                <text class="board-coord" x=x y=label_y_top transform=top_transform
                    text-anchor="middle" dominant-baseline="central">
                    {ch.to_string()}
                </text>
            }
            .into_view(),
        );
        out.push(
            view! {
                <text class="board-coord" x=x y=label_y_bot text-anchor="middle" dominant-baseline="central">
                    {ch.to_string()}
                </text>
            }
            .into_view(),
        );
    }

    // Rank labels (0..9) — read the rank from the left-edge cell of
    // each row, then mirror at the right edge.
    let view_w = (cols as i32 - 1) * CELL + PAD * 2;
    let label_x_left = -LABEL_MARGIN / 2; // ≈ -9
    let label_x_right = view_w + LABEL_MARGIN / 2; // ≈ view_w + 9
    for row in 0..rows {
        let Some(sq) = square_at_display(row, 0, observer, shape) else { continue };
        let (_, r) = board.file_rank(sq);
        let digit = r.0;
        let y = PAD + row as i32 * CELL;
        let right_transform =
            if mirror_black { mirror_attr(label_x_right, y) } else { String::new() };
        out.push(
            view! {
                <text class="board-coord" x=label_x_left y=y text-anchor="middle" dominant-baseline="central">
                    {digit.to_string()}
                </text>
            }
            .into_view(),
        );
        out.push(
            view! {
                <text class="board-coord" x=label_x_right y=y transform=right_transform
                    text-anchor="middle" dominant-baseline="central">
                    {digit.to_string()}
                </text>
            }
            .into_view(),
        );
    }
    out.into_view()
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

fn pieces_view(
    view: &PlayerView,
    observer: Side,
    shape: BoardShape,
    style: Style,
    mirror_black: bool,
) -> View {
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
                    // Mirror: rotate Black-side glyphs 180° around the
                    // piece center so the opposite-seated player reads
                    // them upright. Only the inner glyph rotates — the
                    // disc is symmetric so no visual difference.
                    let transform = if mirror_black && p.piece.side == Side::BLACK {
                        format!("translate({}, {}) rotate(180)", cx, cy)
                    } else {
                        format!("translate({}, {})", cx, cy)
                    };
                    out.push(
                        view! {
                            <g class=format!("tile piece {}", side_class) transform=transform>
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

/// Debug overlay: render a sequence of `from→to` arrows for an entire
/// principal variation, with the first move bright (the AI's chosen
/// move) and later moves progressively faded (the predicted line).
/// Used by the AI debug panel's hover-to-highlight. Empty Vec renders
/// nothing.
fn debug_pv_marker(pv: Vec<Move>, observer: Side, shape: BoardShape) -> View {
    if pv.is_empty() {
        return ().into_view();
    }
    let (rows, cols) = display_dims(shape);
    let mut out: Vec<View> = Vec::with_capacity(pv.len() * 3);
    let total = pv.len();
    for (i, mv) in pv.iter().enumerate() {
        // Opacity fades from 1.0 (chosen) to ~0.35 (deepest PV move).
        let alpha = 1.0 - (i as f32 / total.max(2) as f32) * 0.65;
        let alpha_str = format!("{:.2}", alpha);
        // Step 0 (the AI's chosen move) gets the "from" / "to" classes;
        // PV continuations get the "pv" variant which is dashed-thinner.
        let from_class = if i == 0 { "debug-from" } else { "debug-pv-from" };
        let to_class = if i == 0 { "debug-to" } else { "debug-pv-to" };
        let line_class = if i == 0 { "debug-line" } else { "debug-pv-line" };

        let from_sq = mv.origin_square();
        let to_sq = mv.to_square();
        let mut from_xy: Option<(i32, i32)> = None;
        let mut to_xy: Option<(i32, i32)> = None;
        for row in 0..rows {
            for col in 0..cols {
                if square_at_display(row, col, observer, shape) == Some(from_sq) {
                    from_xy = Some(intersection(row, col));
                }
                if let Some(t) = to_sq {
                    if square_at_display(row, col, observer, shape) == Some(t) {
                        to_xy = Some(intersection(row, col));
                    }
                }
            }
        }
        let Some((fx, fy)) = from_xy else { continue };
        out.push(
            view! { <circle class=from_class cx=fx cy=fy r="32" opacity=alpha_str.clone()/> }
                .into_view(),
        );
        if let Some((tx, ty)) = to_xy {
            out.push(
                view! {
                    <line class=line_class x1=fx y1=fy x2=tx y2=ty opacity=alpha_str.clone()/>
                }
                .into_view(),
            );
            out.push(
                view! { <circle class=to_class cx=tx cy=ty r="32" opacity=alpha_str/> }.into_view(),
            );
        }
    }
    out.into_view()
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
            if !targets.contains(&sq) {
                continue;
            }
            let (cx, cy) = intersection(row, col);
            // Empty target → small dot. Capturable target (revealed
            // enemy piece OR face-down tile under DARK_CAPTURE) →
            // larger ring framing the piece, so the player sees at a
            // glance which enemies / hidden tiles can be taken — not
            // just where to slide to. Same data the TUI surfaces with
            // its green-highlighted attackable squares.
            let cell = view.cells[sq.0 as usize];
            let class = match cell {
                VisibleCell::Empty => "move-dot",
                _ => "move-target",
            };
            let r = if class == "move-dot" { "9" } else { "26" };
            out.push(view! { <circle class=class cx=cx cy=cy r=r/> }.into_view());
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
                        on:pointerup=move |_| on_click.call(sq)
                    />
                }
                .into_view(),
            );
        }
    }
    out.into_view()
}
