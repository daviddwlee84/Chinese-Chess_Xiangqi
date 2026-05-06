//! ratatui draw layer. Stateless — reads `AppState` and draws.

use chess_core::board::BoardShape;
use chess_core::coord::Square;
use chess_core::piece::{PieceKind, Side};
use chess_core::state::{GameStatus, WinReason};
use chess_core::view::{PlayerView, VisibleCell};

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style as TuiStyle};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{AppState, GameView, PickerEntry, PickerView, RectPx, Screen};
use crate::glyph::{self, Style};
use crate::orient;

const CELL_COLS: u16 = 3;
const CELL_ROWS: u16 = 2;
const RANK_LABEL_COLS: u16 = 3;

pub fn draw(frame: &mut Frame, app: &mut AppState) {
    let area = frame.area();
    match &app.screen {
        Screen::Picker(p) => draw_picker(frame, area, p),
        Screen::Game(_) => draw_game(frame, area, app),
    }

    if app.rules_open {
        draw_rules_overlay(frame, area, app.style);
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
        .constraints([Constraint::Min(40), Constraint::Length(36)])
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
    draw_sidebar(frame, chunks[1], g_ref, style, help_open);
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

    let border = TuiStyle::default().fg(Color::DarkGray);
    let mut lines: Vec<Line> = Vec::with_capacity(rows as usize * 2 + 4);

    // File header (top)
    lines.push(file_header_line(observer, shape));
    // Top border of grid
    lines.push(Line::from(Span::styled(border_row(BorderKind::Top, cols), border)));

    let river_after = if matches!(shape, BoardShape::Xiangqi9x10) { Some(4u8) } else { None };

    for display_row in 0..rows {
        // Rank-content row: │piece│piece│...│
        let mut spans: Vec<Span> = Vec::with_capacity(cols as usize * 2 + 2);
        spans.push(Span::raw(rank_label(observer, shape, display_row)));
        spans.push(Span::styled("│", border));
        for display_col in 0..cols {
            let sq = orient::square_at_display(display_row, display_col, observer, shape);
            let (text, st) = match sq {
                Some(sq) => render_cell(
                    &view,
                    sq,
                    &highlight,
                    g,
                    style,
                    use_color,
                    display_row,
                    display_col,
                ),
                None => (glyph::empty(style).to_string(), TuiStyle::default()),
            };
            spans.push(Span::styled(text, st));
            spans.push(Span::styled("│", border));
        }
        lines.push(Line::from(spans));

        // Mid border or river break or bottom border
        if display_row + 1 == rows {
            lines.push(Line::from(Span::styled(border_row(BorderKind::Bottom, cols), border)));
        } else if river_after == Some(display_row) {
            lines.push(river_line(cols, style));
        } else {
            lines.push(Line::from(Span::styled(border_row(BorderKind::Mid, cols), border)));
        }
    }

    // File header (bottom)
    lines.push(file_header_line(observer, shape));

    let para = Paragraph::new(lines);
    frame.render_widget(para, inner);

    *board_rect = Some(RectPx {
        x: inner.x,
        y: inner.y,
        cell_cols: CELL_COLS,
        cell_rows: CELL_ROWS,
        // file header (1) + top border (1)
        top_pad: 2,
        // rank label (3) + leading │ (1)
        left_pad: RANK_LABEL_COLS + 1,
        river_after_display_row: river_after,
    });
}

#[derive(Copy, Clone)]
enum BorderKind {
    Top,
    Mid,
    Bottom,
}

fn border_row(kind: BorderKind, cols: u8) -> String {
    let (left, sep, right) = match kind {
        BorderKind::Top => ('┌', '┬', '┐'),
        BorderKind::Mid => ('├', '┼', '┤'),
        BorderKind::Bottom => ('└', '┴', '┘'),
    };
    let mut s = String::from("   "); // align with rank label
    s.push(left);
    for i in 0..cols {
        s.push_str("──");
        if i + 1 < cols {
            s.push(sep);
        } else {
            s.push(right);
        }
    }
    s
}

fn river_line(cols: u8, style: Style) -> Line<'static> {
    // Inner content fits between │ markers at the column boundaries.
    // Total line width = 1 (left │) + 3*cols (cells) + (cols - 1) wait, no
    // we keep the column-boundary │'s and put text between. To keep mouse
    // hit-test simple, we render the river as a single continuous decorated
    // band and rely on `river_after_display_row` to skip over it.
    let total_cols_inner = (cols as usize) * 3 - 1; // 3 chars per cell minus the trailing │
    let banner = match style {
        Style::Cjk => "  楚 河 ── 漢 界  ",
        Style::Ascii => "  ----- river ----- ",
    };
    // pad/truncate to total_cols_inner display width; CJK is 2 cols/glyph,
    // so a literal char count is approximate. Use a generous overall row.
    let mut s = String::from("   "); // rank label space
    s.push('│');
    let banner_pad = total_cols_inner.saturating_sub(visual_width(banner));
    let left_pad = banner_pad / 2;
    let right_pad = banner_pad - left_pad;
    for _ in 0..left_pad {
        s.push(' ');
    }
    s.push_str(banner);
    for _ in 0..right_pad {
        s.push(' ');
    }
    s.push('│');
    Line::from(Span::styled(s, TuiStyle::default().fg(Color::Cyan).add_modifier(Modifier::DIM)))
}

/// Approximate display width: CJK chars count as 2 cols, others as 1.
fn visual_width(s: &str) -> usize {
    s.chars()
        .map(|c| {
            let cp = c as u32;
            // East Asian Wide ranges (rough). Covers CJK Unified, Hangul,
            // box-drawing isn't wide. This is "good enough" — a real fix
            // would pull in unicode-width.
            if (0x1100..=0x115F).contains(&cp)
                || (0x2E80..=0x303E).contains(&cp)
                || (0x3041..=0x33FF).contains(&cp)
                || (0x3400..=0x4DBF).contains(&cp)
                || (0x4E00..=0x9FFF).contains(&cp)
                || (0xA000..=0xA4CF).contains(&cp)
                || (0xAC00..=0xD7A3).contains(&cp)
                || (0xF900..=0xFAFF).contains(&cp)
                || (0xFE30..=0xFE4F).contains(&cp)
                || (0xFF00..=0xFF60).contains(&cp)
                || (0xFFE0..=0xFFE6).contains(&cp)
            {
                2
            } else {
                1
            }
        })
        .sum()
}

fn file_header_line(observer: Side, shape: BoardShape) -> Line<'static> {
    let (w, _) = shape.dimensions();
    let (_, cols) = orient::display_dims(shape);
    // Each cell is 3 cols: " label "  with the label centered.
    let mut s = String::new();
    s.push_str("    "); // 3 cols rank label + 1 col left │
    for display_col in 0..cols {
        let label = file_label_for_col(display_col, w, shape, observer);
        s.push(label);
        s.push_str("  "); // pad to 3 cols
    }
    Line::from(Span::styled(s, TuiStyle::default().fg(Color::DarkGray)))
}

fn file_label_for_col(col: u8, model_w: u8, shape: BoardShape, observer: Side) -> char {
    let sq = orient::square_at_display(0, col, observer, shape);
    let f = match sq {
        Some(sq) => (sq.0 % model_w as u16) as u8,
        None => col,
    };
    (b'a' + f) as char
}

fn rank_label(observer: Side, shape: BoardShape, display_row: u8) -> String {
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

    let text = match cell {
        VisibleCell::Empty => glyph::empty(style).to_string(),
        VisibleCell::Hidden => glyph::hidden(style).to_string(),
        VisibleCell::Revealed(pos) => {
            glyph::glyph(pos.piece.kind, pos.piece.side, style).to_string()
        }
    };

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

fn draw_sidebar(frame: &mut Frame, area: Rect, g: &GameView, style: Style, help_open: bool) {
    let block = Block::default().borders(Borders::ALL).title(" Status ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let view = PlayerView::project(&g.state, g.state.side_to_move);
    let mut lines: Vec<Line> = Vec::new();

    // Game-over banner (takes priority).
    if let Some(banner) = game_over_banner(&view.status, style) {
        for l in banner {
            lines.push(l);
        }
        lines.push(Line::from(""));
    }

    let variant_label = match view.shape {
        BoardShape::Xiangqi9x10 => {
            if g.state.rules.xiangqi_allow_self_check {
                "Xiangqi (象棋, casual)"
            } else {
                "Xiangqi (象棋)"
            }
        }
        BoardShape::Banqi4x8 => "Banqi (暗棋)",
        BoardShape::ThreeKingdom => "三國暗棋",
        BoardShape::Custom { .. } => "Custom",
    };
    lines.push(line_label_value("Variant:", variant_label));

    let stm_color = side_color(view.side_to_move);
    lines.push(Line::from(vec![
        Span::styled("Side to move:", TuiStyle::default().fg(Color::DarkGray)),
        Span::raw(" "),
        Span::styled(
            glyph::side_name(view.side_to_move, style),
            TuiStyle::default().fg(stm_color).add_modifier(Modifier::BOLD),
        ),
    ]));

    if matches!(view.shape, BoardShape::Xiangqi9x10)
        && g.state.is_in_check(view.side_to_move)
        && matches!(view.status, GameStatus::Ongoing)
    {
        lines.push(Line::from(Span::styled(
            "  ⚠ CHECK 將軍 — your general is under attack",
            TuiStyle::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
    }

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
            "?=help, r=rules, n=new, q=quit",
            TuiStyle::default().fg(Color::DarkGray),
        )));
    }

    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(para, inner);
}

fn game_over_banner(status: &GameStatus, style: Style) -> Option<Vec<Line<'static>>> {
    match status {
        GameStatus::Ongoing => None,
        GameStatus::Won { winner, reason } => {
            let header = Line::from(Span::styled(
                "★  GAME OVER  ★",
                TuiStyle::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));
            let winner_line = Line::from(vec![
                Span::raw("Winner: "),
                Span::styled(
                    glyph::side_name(*winner, style),
                    TuiStyle::default().fg(side_color(*winner)).add_modifier(Modifier::BOLD),
                ),
            ]);
            let reason_line = Line::from(vec![
                Span::raw("Reason: "),
                Span::styled(
                    win_reason_label(*reason),
                    TuiStyle::default().add_modifier(Modifier::BOLD),
                ),
            ]);
            let footer = Line::from(Span::styled(
                "Press 'n' for new game, 'u' to take back, 'q' to quit.",
                TuiStyle::default().fg(Color::Cyan),
            ));
            Some(vec![header, winner_line, reason_line, footer])
        }
        GameStatus::Drawn { reason } => {
            let header = Line::from(Span::styled(
                "—  DRAW  —",
                TuiStyle::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));
            let reason_line = Line::from(vec![
                Span::raw("Reason: "),
                Span::styled(
                    format!("{:?}", reason),
                    TuiStyle::default().add_modifier(Modifier::BOLD),
                ),
            ]);
            let footer = Line::from(Span::styled(
                "Press 'n' for new game, 'u' to take back, 'q' to quit.",
                TuiStyle::default().fg(Color::Cyan),
            ));
            Some(vec![header, reason_line, footer])
        }
    }
}

fn win_reason_label(reason: WinReason) -> &'static str {
    match reason {
        WinReason::Checkmate => "Checkmate (將死)",
        WinReason::Stalemate => "Stalemate (困死) — no legal moves",
        WinReason::Resignation => "Resignation",
        WinReason::OnlyOneSideHasPieces => "Opponent eliminated",
        WinReason::Timeout => "Time forfeit",
        WinReason::GeneralCaptured => "General captured (將被吃)",
    }
}

fn line_label_value(label: &'static str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(label, TuiStyle::default().fg(Color::DarkGray)),
        Span::raw(" "),
        Span::raw(value.to_string()),
    ])
}

fn draw_rules_overlay(frame: &mut Frame, area: Rect, _style: Style) {
    // Center an overlay roughly 70 cols × 24 rows over the screen.
    let pad_x = area.width.saturating_sub(72) / 2;
    let pad_y = area.height.saturating_sub(26) / 2;
    let overlay = Rect {
        x: area.x + pad_x,
        y: area.y + pad_y,
        width: area.width.min(72),
        height: area.height.min(26),
    };
    frame.render_widget(Clear, overlay);
    let block =
        Block::default().borders(Borders::ALL).title(" Rules / 規則 — press r or Esc to close ");
    let lines: Vec<Line> = RULES_LINES
        .iter()
        .map(|(s, accent)| {
            let style = if *accent {
                TuiStyle::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                TuiStyle::default()
            };
            Line::from(Span::styled(*s, style))
        })
        .collect();
    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, overlay);
}

const HELP_LINES: &[&str] = &[
    "h j k l / ←↓↑→  move cursor",
    "Enter / Space   select / commit",
    "Esc             cancel selection",
    "u               undo last move",
    "f               flip (banqi)",
    "n               new game (back to picker)",
    "r               toggle rules overlay",
    "?               toggle this help",
    "Click           select / commit",
    "q / Ctrl-C      quit",
];

const RULES_LINES: &[(&str, bool)] = &[
    ("Xiangqi 象棋", true),
    ("", false),
    ("• Red (帥仕相俥傌炮兵) moves first; Black (將士象車馬砲卒) replies.", false),
    ("• General/Advisor stay inside the 3×3 palace (files d–f).", false),
    ("• Elephants stay on their own side of the river (ranks 0–4 / 5–9).", false),
    ("• Horses are blocked by 馬腿 — the orthogonal step in the move's direction.", false),
    ("• Cannon non-capturing slide is like a chariot; capture requires jumping", false),
    ("  exactly one piece (the screen 砲架) in a straight line.", false),
    ("• Soldiers move forward one square; after crossing the river they can also", false),
    ("  move sideways (but never backward).", false),
    ("• Generals cannot face each other on a clear file (飛將 rule).", false),
    ("• Standard: any move that leaves your general capturable is illegal.", false),
    ("  Casual mode (--allow-self-check / picker entry) lifts that filter;", false),
    ("  the game ends when the general is physically captured.", false),
    ("", false),
    ("Banqi 暗棋", true),
    ("", false),
    ("• 4×8 half-board, 32 pieces face-down. Pick a hidden piece to flip;", false),
    ("  the FIRST flip locks the colors (flipper plays the revealed color).", false),
    ("• Capture by piece rank: General > Advisor > Elephant > Chariot >", false),
    ("  Horse > Cannon > Soldier. Soldier captures General (民推翻王).", false),
    ("• Cannon captures by jumping over exactly one piece (any rank).", false),
    ("• House rules (toggleable presets): chain-capture (連吃), chariot-rush", false),
    ("  (車衝). Other house rules are P1 TODO.", false),
    ("", false),
    ("Press r or Esc to close. Default keys: hjkl/arrows · Enter · u · f · n · q", true),
];
