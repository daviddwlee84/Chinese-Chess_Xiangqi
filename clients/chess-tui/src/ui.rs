//! ratatui draw layer. Stateless — reads `AppState` and draws.

use chess_core::board::BoardShape;
use chess_core::coord::Square;
use chess_core::piece::{PieceKind, Side};
use chess_core::view::{PlayerView, VisibleCell};

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style as TuiStyle};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{AppState, GameView, PickerEntry, PickerView, RectPx, Screen};
use crate::glyph::{self, Style};
use crate::orient;

const CELL_COLS: u16 = 3;
const RANK_LABEL_COLS: u16 = 3;

pub fn draw(frame: &mut Frame, app: &mut AppState) {
    let area = frame.area();
    // Borrow split: take screen out into a local view kind, then re-borrow app.
    match &app.screen {
        Screen::Picker(p) => draw_picker(frame, area, p),
        Screen::Game(_) => draw_game(frame, area, app),
    }
}

fn draw_picker(frame: &mut Frame, area: Rect, picker: &PickerView) {
    let title = Span::styled("chess-tui", TuiStyle::default().add_modifier(Modifier::BOLD));
    let mut lines = vec![
        Line::from(title),
        Line::from(Span::raw("Choose a variant. Arrow keys + Enter; q to quit.")),
        Line::from(""),
    ];
    for (i, entry) in PickerEntry::ALL.iter().enumerate() {
        let prefix = if i == picker.cursor { "▶ " } else { "  " };
        let style = if i == picker.cursor {
            TuiStyle::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            TuiStyle::default()
        };
        lines.push(Line::from(vec![Span::raw(prefix), Span::styled(entry.label(), style)]));
    }
    let block = Block::default().borders(Borders::ALL).title(" Welcome ");
    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn draw_game(frame: &mut Frame, area: Rect, app: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(36), Constraint::Length(34)])
        .split(area);

    let Screen::Game(g) = &app.screen else {
        return;
    };
    let g_ref: &GameView = g.as_ref();
    let observer = app.observer;
    let style = app.style;
    let use_color = app.use_color;
    let help_open = app.help_open;

    let mut rect = app.board_rect;
    draw_board(frame, chunks[0], observer, style, use_color, g_ref, &mut rect);
    app.board_rect = rect;
    draw_sidebar(frame, chunks[1], g_ref, help_open);
}

fn draw_board(
    frame: &mut Frame,
    area: Rect,
    observer: Side,
    style: Style,
    use_color: bool,
    g: &GameView,
    board_rect: &mut Option<RectPx>,
) {
    let block = Block::default().borders(Borders::ALL).title(" Board ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let view = PlayerView::project(&g.state, g.state.side_to_move);
    let shape = view.shape;
    let (rows, cols) = orient::display_dims(shape);
    let (model_w, _) = shape.dimensions();

    // Build legal-target highlight set when a piece is selected.
    let highlight: std::collections::HashSet<Square> = match g.selected {
        Some(from) => view
            .legal_moves
            .iter()
            .filter(|m| m.origin_square() == from)
            .filter_map(|m| m.to_square())
            .collect(),
        None => std::collections::HashSet::new(),
    };

    let mut lines: Vec<Line> = Vec::with_capacity(rows as usize + 2);

    // Top header: file labels.
    lines.push(file_header_line(observer, shape));

    for display_row in 0..rows {
        let mut spans: Vec<Span> = Vec::with_capacity(cols as usize * 2 + 1);
        spans.push(Span::raw(rank_label(observer, shape, display_row)));
        for display_col in 0..cols {
            let sq = orient::square_at_display(display_row, display_col, observer, shape);
            let (glyph_text, st) = match sq {
                Some(sq) => render_cell(
                    &view,
                    sq,
                    model_w,
                    &highlight,
                    g,
                    style,
                    use_color,
                    display_row,
                    display_col,
                ),
                None => (format!("{:>3}", "·"), TuiStyle::default()),
            };
            spans.push(Span::styled(glyph_text, st));
        }
        lines.push(Line::from(spans));
    }

    // Footer: file labels again.
    lines.push(file_header_line(observer, shape));
    // River reminder for xiangqi — rendered as a separate line below the
    // board so it doesn't disturb the cell-row layout used by mouse hit-test.
    if matches!(shape, BoardShape::Xiangqi9x10) {
        let banner = match style {
            Style::Cjk => "   楚河 ─ 漢界  (between ranks 4–5)",
            Style::Ascii => "   river (between ranks 4-5)",
        };
        lines.push(Line::from(Span::styled(banner, TuiStyle::default().fg(Color::DarkGray))));
    }

    let para = Paragraph::new(lines);
    frame.render_widget(para, inner);

    *board_rect = Some(RectPx {
        x: inner.x,
        y: inner.y,
        cell_cols: CELL_COLS,
        left_pad: RANK_LABEL_COLS,
        top_pad: 1, // header row
    });
}

fn file_header_line(observer: Side, shape: BoardShape) -> Line<'static> {
    let (w, _) = shape.dimensions();
    let (_, cols) = orient::display_dims(shape);
    let mut s = String::new();
    s.push_str("   "); // align with rank labels (3 cols)
    for display_col in 0..cols {
        // What model file corresponds to this display column? Ask the inverse
        // for any row (use 0); banqi transposes so use a model-file resolver.
        let label = file_label_for_col(display_col, w, shape, observer);
        s.push(label);
        s.push_str("  "); // 1-char glyph + 2 spaces ≈ CELL_COLS for ASCII labels
    }
    Line::from(Span::styled(s, TuiStyle::default().fg(Color::DarkGray)))
}

fn file_label_for_col(col: u8, model_w: u8, shape: BoardShape, observer: Side) -> char {
    // Resolve via the inverse: pick row 0 (xiangqi) or row 0 (banqi); compute
    // model file from the resulting Square.
    let sq = orient::square_at_display(0, col, observer, shape);
    let f = match sq {
        Some(sq) => (sq.0 % model_w as u16) as u8,
        None => col, // best-effort fallback
    };
    (b'a' + f) as char
}

fn rank_label(observer: Side, shape: BoardShape, display_row: u8) -> String {
    // Same trick: resolve inverse to find model rank for this display row.
    let sq = orient::square_at_display(display_row, 0, observer, shape);
    let r = match sq {
        Some(sq) => (sq.0 / shape.dimensions().0 as u16) as u8,
        None => display_row,
    };
    format!(" {} ", r)
}

#[allow(clippy::too_many_arguments)]
fn render_cell(
    view: &PlayerView,
    sq: Square,
    _model_w: u8,
    highlight: &std::collections::HashSet<Square>,
    g: &GameView,
    style: Style,
    use_color: bool,
    display_row: u8,
    display_col: u8,
) -> (String, TuiStyle) {
    let cell = &view.cells[sq.0 as usize];
    let is_cursor = (display_row, display_col) == g.cursor;
    let is_selected = g.selected == Some(sq);
    let is_target = highlight.contains(&sq);

    let mut text = match cell {
        VisibleCell::Empty => glyph::empty(style).to_string(),
        VisibleCell::Hidden => glyph::hidden(style).to_string(),
        VisibleCell::Revealed(pos) => {
            glyph::glyph(pos.piece.kind, pos.piece.side, style).to_string()
        }
    };
    // Pad to CELL_COLS terminal columns. CJK glyphs render as 2 cols already;
    // append a space. ASCII glyphs are 2 chars; append a space.
    text.push(' ');

    let mut s = TuiStyle::default();
    if use_color {
        if let VisibleCell::Revealed(pos) = cell {
            s = s.fg(side_color(pos.piece.side));
            if !pos.revealed {
                s = s.add_modifier(Modifier::DIM);
            }
            if pos.piece.kind == PieceKind::General {
                s = s.add_modifier(Modifier::BOLD);
            }
        } else if matches!(cell, VisibleCell::Hidden) {
            s = s.fg(Color::DarkGray);
        }
    }
    if is_target {
        s = s.bg(Color::Rgb(40, 80, 40));
    }
    if is_selected {
        s = s.bg(Color::Rgb(80, 60, 20)).add_modifier(Modifier::BOLD);
    }
    if is_cursor {
        s = s.bg(Color::Rgb(60, 60, 100)).add_modifier(Modifier::REVERSED);
    }
    (text, s)
}

fn side_color(side: Side) -> Color {
    match side {
        Side::RED => Color::Red,
        Side::BLACK => Color::White,
        _ => Color::Green,
    }
}

fn draw_sidebar(frame: &mut Frame, area: Rect, g: &GameView, help_open: bool) {
    let block = Block::default().borders(Borders::ALL).title(" Status ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let view = PlayerView::project(&g.state, g.state.side_to_move);
    let mut lines: Vec<Line> = Vec::new();

    let variant_label = match view.shape {
        BoardShape::Xiangqi9x10 => "Xiangqi (象棋)",
        BoardShape::Banqi4x8 => "Banqi (暗棋)",
        BoardShape::ThreeKingdom => "三國暗棋",
        BoardShape::Custom { .. } => "Custom",
    };
    lines.push(line_label_value("Variant:", variant_label));
    lines.push(line_label_value("Side to move:", &format!("{:?}", view.side_to_move)));
    lines.push(line_label_value("Status:", &format!("{:?}", view.status)));
    lines.push(line_label_value("Legal moves:", &view.legal_moves.len().to_string()));
    lines.push(line_label_value("History:", &g.state.history.len().to_string()));

    let selected_label = match g.selected {
        Some(sq) => chess_core::notation::iccs::encode_square(&g.state.board, sq),
        None => "—".into(),
    };
    lines.push(line_label_value("Selected:", &selected_label));

    lines.push(Line::from(""));
    if let Some(msg) = &g.last_msg {
        lines.push(Line::from(Span::styled(msg.clone(), TuiStyle::default().fg(Color::Yellow))));
    }

    if help_open {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Help",
            TuiStyle::default().add_modifier(Modifier::BOLD),
        )));
        for ln in HELP_LINES {
            lines.push(Line::from(Span::raw(*ln)));
        }
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "? for help, q to quit",
            TuiStyle::default().fg(Color::DarkGray),
        )));
    }

    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(para, inner);
}

fn line_label_value(label: &'static str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(label, TuiStyle::default().fg(Color::DarkGray)),
        Span::raw(" "),
        Span::raw(value.to_string()),
    ])
}

const HELP_LINES: &[&str] = &[
    "h j k l / ←↓↑→  move cursor",
    "Enter / Space   select / commit",
    "Esc             cancel selection",
    "u               undo last move",
    "f               flip (banqi)",
    "Click           select / commit",
    "?               toggle this help",
    "q / Ctrl-C      quit",
];
