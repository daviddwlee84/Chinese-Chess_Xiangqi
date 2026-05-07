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

use crate::app::{
    self, AppState, CapturedSort, CreateRoomField, CreateRoomView, GameView, HostPromptView,
    LobbyView, NetRole, NetView, PickerEntry, PickerView, RectPx, Screen,
};
use crate::banner::{self, BannerKind, NeutralSide};
use crate::confetti::ConfettiAnim;
use crate::glyph::{self, Style};
use crate::orient;
use crate::text_input;

const CELL_COLS: u16 = 4;
const CELL_ROWS: u16 = 2;
const RANK_LABEL_COLS: u16 = 3;

pub fn draw(frame: &mut Frame, app: &mut AppState) {
    let area = frame.area();
    let app_style = app.style;
    match &app.screen {
        Screen::Picker(p) => draw_picker(frame, area, p, app_style),
        Screen::Game(_) => draw_game(frame, area, app),
        Screen::Net(_) => draw_net(frame, area, app),
        Screen::HostPrompt(h) => draw_host_prompt(frame, area, h, app.help_open),
        Screen::Lobby(l) => draw_lobby(frame, area, l, app.help_open),
        Screen::CreateRoom(c) => draw_create_room(frame, area, c),
    }

    if app.rules_open {
        draw_rules_overlay(frame, area, app.style);
    }
    if app.quit_confirm_open {
        draw_quit_confirm_overlay(frame, area);
    }
}

fn draw_picker(frame: &mut Frame, area: Rect, picker: &PickerView, style: Style) {
    let mut lines: Vec<Line> = Vec::new();

    // Big "CHESS TUI" ASCII banner — only when the terminal is wide enough.
    // Narrow terminals (less than the banner width + a little padding) fall
    // back to the original single-line title so the picker still fits.
    let title_art = banner::art(BannerKind::AppTitle, style);
    let banner_w = banner::max_width(title_art);
    if (area.width as usize) >= banner_w + 4 {
        for row in title_art {
            lines.push(Line::from(Span::styled(
                row.to_string(),
                TuiStyle::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "chess-tui",
            TuiStyle::default().add_modifier(Modifier::BOLD),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::raw("Choose a variant. Arrow keys + Enter; q to quit.")));
    lines.push(Line::from(""));
    for (i, entry) in PickerEntry::ALL.iter().enumerate() {
        let prefix = if i == picker.cursor { "▶ " } else { "  " };
        let row_style = if i == picker.cursor {
            TuiStyle::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            TuiStyle::default()
        };
        lines.push(Line::from(vec![Span::raw(prefix), Span::styled(entry.label(), row_style)]));
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
    let show_check_banner = app.show_check_banner;

    let view = PlayerView::project(&g_ref.state, g_ref.state.side_to_move);
    let mut rect = app.board_rect;
    draw_board(
        frame,
        chunks[0],
        observer,
        style,
        use_color,
        &view,
        g_ref.cursor,
        g_ref.selected,
        &mut rect,
    );
    app.board_rect = rect;
    let captured_sort = app.captured_sort;
    draw_sidebar(
        frame,
        chunks[1],
        g_ref,
        &view,
        style,
        help_open,
        show_check_banner,
        captured_sort,
    );

    // Confetti + endgame banner. The board area is `chunks[0]`. We do this
    // after the sidebar render so the layer paints on top of the board only,
    // and after we've taken the `&app.screen` immutable borrow above.
    let board_area = chunks[0];
    let endgame_kind = endgame_kind_local(&view.status, observer);
    render_confetti_and_banner(frame, board_area, app, endgame_kind, style, use_color);
}

fn draw_net(frame: &mut Frame, area: Rect, app: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(40), Constraint::Length(36)])
        .split(area);

    let Screen::Net(n) = &app.screen else {
        return;
    };
    let n_ref: &NetView = n.as_ref();
    let style = app.style;
    let use_color = app.use_color;
    let help_open = app.help_open;
    let show_check_banner = app.show_check_banner;
    let observer = n_ref.role.map(|r| r.observer()).unwrap_or(app.observer);

    match n_ref.last_view.as_ref() {
        Some(view) => {
            let view_status = view.status;
            let role = n_ref.role;
            let mut rect = app.board_rect;
            draw_board(
                frame,
                chunks[0],
                observer,
                style,
                use_color,
                view,
                n_ref.cursor,
                n_ref.selected,
                &mut rect,
            );
            app.board_rect = rect;
            let captured_sort = app.captured_sort;
            draw_sidebar_net(
                frame,
                chunks[1],
                n_ref,
                view,
                style,
                help_open,
                show_check_banner,
                captured_sort,
            );

            let board_area = chunks[0];
            let endgame_kind = endgame_kind_net(&view_status, role);
            render_confetti_and_banner(frame, board_area, app, endgame_kind, style, use_color);
        }
        None => {
            draw_connecting_placeholder(frame, chunks[0], n_ref);
            draw_sidebar_net_idle(frame, chunks[1], n_ref);
        }
    }
}

fn draw_connecting_placeholder(frame: &mut Frame, area: Rect, n: &NetView) {
    let block = Block::default().borders(Borders::ALL).title(" Net ");
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled("  Connecting…", TuiStyle::default().add_modifier(Modifier::BOLD))),
        Line::from(Span::styled(format!("  → {}", n.url), TuiStyle::default().fg(Color::Gray))),
        Line::from(""),
        Line::from(Span::styled(
            n.last_msg.clone().unwrap_or_else(|| "  (waiting for server hello)".into()),
            TuiStyle::default().fg(Color::Yellow),
        )),
    ];
    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn draw_sidebar_net_idle(frame: &mut Frame, area: Rect, n: &NetView) {
    let block = Block::default().borders(Borders::ALL).title(" Status ");
    let lines = vec![
        Line::from(Span::styled("Connecting…", TuiStyle::default().add_modifier(Modifier::BOLD))),
        Line::from(format!("URL: {}", n.url)),
        Line::from(""),
        Line::from(Span::styled("Press q to give up.", TuiStyle::default().fg(Color::DarkGray))),
    ];
    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn draw_host_prompt(frame: &mut Frame, area: Rect, h: &HostPromptView, help_open: bool) {
    let title =
        Span::styled("Connect to chess-net", TuiStyle::default().add_modifier(Modifier::BOLD));
    let mut lines = vec![
        Line::from(title),
        Line::from(""),
        Line::from(Span::raw("Server URL (e.g. ws://127.0.0.1:7878):")),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("{}_", h.buf), TuiStyle::default().fg(Color::Yellow)),
        ]),
        Line::from(""),
    ];
    if let Some(err) = h.error.as_deref() {
        lines
            .push(Line::from(Span::styled(format!("× {err}"), TuiStyle::default().fg(Color::Red))));
        lines.push(Line::from(""));
    }
    if help_open {
        for ln in HELP_LINES_HOST_PROMPT {
            lines.push(Line::from(Span::styled(*ln, TuiStyle::default().fg(Color::DarkGray))));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "Enter to connect, Esc to go back, ? for help.",
            TuiStyle::default().fg(Color::DarkGray),
        )));
    }
    let block = Block::default().borders(Borders::ALL).title(" Online ");
    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn draw_lobby(frame: &mut Frame, area: Rect, l: &LobbyView, help_open: bool) {
    // Two columns: room list (left) + status/help (right). When the user is
    // mid-password-prompt, overlay a small modal.
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(40), Constraint::Length(36)])
        .split(area);

    let header_style = TuiStyle::default().fg(Color::DarkGray);
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(l.rooms.len() + 4);
    lines.push(Line::from(Span::styled(
        format!("Lobby — {}", l.host),
        TuiStyle::default().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        format!("{:<20} {:<14} {:<6} {:<6} {:<8}", "Room", "Variant", "Seats", "Watch", "Status"),
        header_style,
    )));
    if l.rooms.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no rooms yet — press 'c' to create one)",
            TuiStyle::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, r) in l.rooms.iter().enumerate() {
            let prefix = if i == l.cursor { "▶ " } else { "  " };
            let lock = if r.has_password { "🔒" } else { "  " };
            let status = match r.status {
                chess_net::RoomStatus::Lobby => "lobby",
                chess_net::RoomStatus::Playing => "playing",
                chess_net::RoomStatus::Finished => "finished",
            };
            let cell_style = if i == l.cursor {
                TuiStyle::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                TuiStyle::default()
            };
            let spec_label =
                if r.spectators == 0 { "—".to_string() } else { format!("{}👁", r.spectators) };
            lines.push(Line::from(vec![
                Span::raw(prefix),
                Span::raw(lock),
                Span::raw(" "),
                Span::styled(
                    format!(
                        "{:<18} {:<14} {}/2   {:<6} {:<8}",
                        r.id, r.variant, r.seats, spec_label, status
                    ),
                    cell_style,
                ),
            ]));
        }
    }

    let block = Block::default().borders(Borders::ALL).title(" Lobby ");
    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, chunks[0]);

    draw_lobby_sidebar(frame, chunks[1], l, help_open);

    if let Some(pj) = l.pending_join.as_ref() {
        let modal = Rect {
            x: area.x + area.width / 4,
            y: area.y + area.height / 3,
            width: (area.width / 2).max(40),
            height: 7,
        };
        frame.render_widget(Clear, modal);
        let lines = vec![
            Line::from(Span::styled(
                format!("Join '{}' — locked", pj.room_id),
                TuiStyle::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::raw("Password: "),
                Span::styled(
                    format!("{}_", text_input::mask(&pj.password_buf)),
                    TuiStyle::default().fg(Color::Yellow),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Enter to join, Esc to cancel.",
                TuiStyle::default().fg(Color::DarkGray),
            )),
        ];
        let block = Block::default().borders(Borders::ALL).title(" Password ");
        frame.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), modal);
    }
}

fn draw_lobby_sidebar(frame: &mut Frame, area: Rect, l: &LobbyView, help_open: bool) {
    let block = Block::default().borders(Borders::ALL).title(" Status ");
    let mut lines = vec![Line::from(vec![
        Span::raw("Connection: "),
        Span::styled(
            if l.connected { "ws live" } else { "connecting…" },
            TuiStyle::default().fg(if l.connected { Color::Green } else { Color::Yellow }),
        ),
    ])];
    lines.push(Line::from(format!("Rooms: {}", l.rooms.len())));
    lines.push(Line::from(""));
    if let Some(msg) = l.last_msg.as_deref() {
        lines.push(Line::from(Span::styled(msg.to_string(), TuiStyle::default().fg(Color::Cyan))));
        lines.push(Line::from(""));
    }
    if help_open {
        for ln in HELP_LINES_LOBBY {
            lines.push(Line::from(Span::styled(*ln, TuiStyle::default().fg(Color::DarkGray))));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "Enter=join, w=watch, c=create, r=refresh, Esc=back, ?=help, q=quit.",
            TuiStyle::default().fg(Color::DarkGray),
        )));
    }
    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn draw_create_room(frame: &mut Frame, area: Rect, c: &CreateRoomView) {
    let title = Span::styled("Create room", TuiStyle::default().add_modifier(Modifier::BOLD));
    let id_active = matches!(c.focus, CreateRoomField::Id);
    let pw_active = matches!(c.focus, CreateRoomField::Password);
    let submit_active = matches!(c.focus, CreateRoomField::Submit);
    let mut lines = vec![
        Line::from(title),
        Line::from(format!("Server: {}", c.host)),
        Line::from(""),
        Line::from(Span::raw("Room id (1–32 chars, [a-zA-Z0-9_-]):")),
        Line::from(vec![
            Span::raw(if id_active { "▶ " } else { "  " }),
            Span::styled(
                if id_active { format!("{}_", c.id_buf) } else { c.id_buf.clone() },
                if id_active {
                    TuiStyle::default().fg(Color::Yellow)
                } else {
                    TuiStyle::default().fg(Color::Gray)
                },
            ),
        ]),
        Line::from(""),
        Line::from(Span::raw("Password (optional, leave blank for open room):")),
        Line::from(vec![
            Span::raw(if pw_active { "▶ " } else { "  " }),
            Span::styled(
                if pw_active {
                    format!("{}_", text_input::mask(&c.password_buf))
                } else {
                    text_input::mask(&c.password_buf)
                },
                if pw_active {
                    TuiStyle::default().fg(Color::Yellow)
                } else {
                    TuiStyle::default().fg(Color::Gray)
                },
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw(if submit_active { "▶ " } else { "  " }),
            Span::styled(
                "[ Create & join ]",
                if submit_active {
                    TuiStyle::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    TuiStyle::default().fg(Color::Gray)
                },
            ),
        ]),
    ];
    if let Some(err) = c.error.as_deref() {
        lines.push(Line::from(""));
        lines
            .push(Line::from(Span::styled(format!("× {err}"), TuiStyle::default().fg(Color::Red))));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Tab/Shift-Tab: switch field. Enter on submit creates the room. Esc: back.",
        TuiStyle::default().fg(Color::DarkGray),
    )));
    let block = Block::default().borders(Borders::ALL).title(" New room ");
    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

const HELP_LINES_HOST_PROMPT: &[&str] = &[
    "Type a server URL like  ws://192.168.1.5:7878",
    "Schemes: ws://  wss://  http(s):// auto-converts.",
    "Enter      open the lobby for that host",
    "Esc        back to the variant picker",
    "?          toggle this help",
    "Ctrl-C     quit",
];

const HELP_LINES_LOBBY: &[&str] = &[
    "j k / ↓ ↑   move cursor",
    "Enter       join the highlighted room (prompts for password if locked)",
    "w           watch the highlighted room as a spectator (read-only)",
    "c           create a new room",
    "r           force-refresh room list (server pushes automatically)",
    "Esc         back to host prompt",
    "?           toggle this help",
    "q / Ctrl-C  quit",
];

#[allow(clippy::too_many_arguments)]
fn draw_board(
    frame: &mut Frame,
    area: Rect,
    observer: Side,
    style: Style,
    use_color: bool,
    view: &PlayerView,
    cursor: (u8, u8),
    selected: Option<Square>,
    board_rect: &mut Option<RectPx>,
) {
    let block = Block::default().borders(Borders::ALL).title(" Board ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let shape = view.shape;
    let (rows, cols) = orient::display_dims(shape);
    let model_w = shape.dimensions().0;

    // Build legal-target highlight set when a piece is selected.
    let highlight: std::collections::HashSet<Square> = match selected {
        Some(from) => view
            .legal_moves
            .iter()
            .filter(|m| m.origin_square() == from)
            .filter_map(|m| m.to_square())
            .collect(),
        None => std::collections::HashSet::new(),
    };

    let border_style = TuiStyle::default().fg(Color::DarkGray);
    let mut lines: Vec<Line> = Vec::with_capacity(rows as usize * 2 + 2);

    // File header (top)
    lines.push(file_header_line(observer, shape));

    let river_after = if matches!(shape, BoardShape::Xiangqi9x10) { Some(4u8) } else { None };

    for display_row in 0..rows {
        lines.push(rank_row(
            view,
            observer,
            shape,
            display_row,
            cols,
            model_w,
            &highlight,
            cursor,
            selected,
            style,
            use_color,
            border_style,
        ));

        // Between row (skip after last rank)
        if display_row + 1 == rows {
            // no between row after the final rank
        } else if river_after == Some(display_row) {
            lines.push(river_line(cols, style));
        } else {
            lines.push(between_row(observer, shape, display_row, cols, style, border_style));
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
        // file header (1)
        top_pad: 1,
        // rank label (3)
        left_pad: RANK_LABEL_COLS,
    });
}

#[allow(clippy::too_many_arguments)]
fn rank_row<'a>(
    view: &'a PlayerView,
    observer: Side,
    shape: BoardShape,
    display_row: u8,
    cols: u8,
    model_w: u8,
    highlight: &std::collections::HashSet<Square>,
    cursor: (u8, u8),
    selected: Option<Square>,
    style: Style,
    use_color: bool,
    border_style: TuiStyle,
) -> Line<'a> {
    let mut spans: Vec<Span> = Vec::with_capacity(cols as usize * 2 + 1);
    spans.push(Span::raw(rank_label(observer, shape, display_row)));

    for display_col in 0..cols {
        let sq = orient::square_at_display(display_row, display_col, observer, shape);
        let (content, st, is_piece_or_glyph) = match sq {
            Some(sq) => intersection_or_piece(
                view,
                sq,
                model_w,
                shape,
                highlight,
                cursor,
                selected,
                style,
                use_color,
                display_row,
                display_col,
            ),
            None => (intersection_glyph(false, style).to_string(), TuiStyle::default(), false),
        };

        spans.push(Span::styled(content, st));

        // Connector to next cell or trailing pad on last cell
        if display_col + 1 < cols {
            let connector = horizontal_connector(is_piece_or_glyph, style);
            spans.push(Span::styled(connector, border_style));
        } else if !is_piece_or_glyph {
            // Last cell with 1-col content: pad to 2 cols so all rows have
            // a consistent total width.
            spans.push(Span::raw(" "));
        }
    }

    Line::from(spans)
}

/// Per-cell content for a rank row.
/// Returns `(content_str, content_style, content_is_glyph_width_2)`.
/// `content_is_glyph_width_2` is true for revealed pieces and hidden banqi
/// pieces (which use a 2-col glyph); false for empty intersections (1 col).
#[allow(clippy::too_many_arguments)]
fn intersection_or_piece(
    view: &PlayerView,
    sq: Square,
    model_w: u8,
    shape: BoardShape,
    highlight: &std::collections::HashSet<Square>,
    cursor: (u8, u8),
    selected: Option<Square>,
    style: Style,
    use_color: bool,
    display_row: u8,
    display_col: u8,
) -> (String, TuiStyle, bool) {
    let cell = &view.cells[sq.0 as usize];
    let is_cursor = (display_row, display_col) == cursor;
    let is_selected = selected == Some(sq);
    let is_target = highlight.contains(&sq);

    let (text, is_glyph_w2) = match cell {
        VisibleCell::Empty => {
            let palace_center = is_palace_center(sq, model_w, shape);
            (intersection_glyph(palace_center, style).to_string(), false)
        }
        VisibleCell::Hidden => (glyph::hidden(style).to_string(), true),
        VisibleCell::Revealed(pos) => {
            (glyph::glyph(pos.piece.kind, pos.piece.side, style).to_string(), true)
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
        } else if is_palace_center(sq, model_w, shape) {
            s = s.fg(Color::Yellow);
        } else {
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
    (text, s, is_glyph_w2)
}

fn is_palace_center(sq: Square, model_w: u8, shape: BoardShape) -> bool {
    if !matches!(shape, BoardShape::Xiangqi9x10) {
        return false;
    }
    let f = (sq.0 % model_w as u16) as u8;
    let r = (sq.0 / model_w as u16) as u8;
    f == 4 && (r == 1 || r == 8)
}

fn intersection_glyph(palace_center: bool, style: Style) -> &'static str {
    match (palace_center, style) {
        (true, Style::Cjk) => "╳",
        (true, Style::Ascii) => "X",
        (false, Style::Cjk) => "┼",
        (false, Style::Ascii) => "+",
    }
}

fn horizontal_connector(content_is_w2: bool, style: Style) -> &'static str {
    let dash: char = match style {
        Style::Cjk => '─',
        Style::Ascii => '-',
    };
    if content_is_w2 {
        // 2-col content (piece) leaves 2 cols for connector.
        match dash {
            '─' => "──",
            _ => "--",
        }
    } else {
        // 1-col content (intersection) leaves 3 cols for connector.
        match dash {
            '─' => "───",
            _ => "---",
        }
    }
}

fn between_row(
    observer: Side,
    shape: BoardShape,
    display_row_above: u8,
    cols: u8,
    style: Style,
    border_style: TuiStyle,
) -> Line<'static> {
    let vbar: char = match style {
        Style::Cjk => '│',
        Style::Ascii => '|',
    };
    let diagonals = palace_diagonals(display_row_above, observer, shape, style);

    let mut s = String::from("   "); // rank label
    for cell_idx in 0..cols {
        s.push(vbar);
        if cell_idx + 1 < cols {
            // 3-space gap, possibly with a diagonal char in the middle
            // for palace between-rows. cell_idx is the index of the LEFT
            // cell of the gap. Diagonals appear between cells (3, 4) and (4, 5).
            let diag_char = match (diagonals, cell_idx) {
                (Some((left_diag, _)), 3) => Some(left_diag),
                (Some((_, right_diag)), 4) => Some(right_diag),
                _ => None,
            };
            match diag_char {
                Some(c) => {
                    s.push(' ');
                    s.push(c);
                    s.push(' ');
                }
                None => s.push_str("   "),
            }
        }
    }
    // Trailing space so the row width matches rank rows (34 cols of content
    // + 3 of rank label).
    s.push(' ');
    Line::from(Span::styled(s, border_style))
}

/// For a between-row sitting between display rows N and N+1, returns the
/// (left, right) diagonal characters if this between-row crosses a palace.
/// Otherwise None.
fn palace_diagonals(
    display_row_above: u8,
    observer: Side,
    shape: BoardShape,
    style: Style,
) -> Option<(char, char)> {
    if !matches!(shape, BoardShape::Xiangqi9x10) {
        return None;
    }
    let top_sq = orient::square_at_display(display_row_above, 4, observer, shape)?;
    let bot_sq = orient::square_at_display(display_row_above + 1, 4, observer, shape)?;
    let top_rank = (top_sq.0 / 9) as u8;
    let bot_rank = (bot_sq.0 / 9) as u8;
    let (low, high) = if top_rank < bot_rank { (top_rank, bot_rank) } else { (bot_rank, top_rank) };

    let center_rank = match (low, high) {
        (0, 1) | (1, 2) => 1u8,
        (7, 8) | (8, 9) => 8u8,
        _ => return None,
    };

    let top_dist = (top_rank as i32 - center_rank as i32).abs();
    let bot_dist = (bot_rank as i32 - center_rank as i32).abs();
    let converging = bot_dist < top_dist;

    let (left_cjk, right_cjk) = if converging { ('╲', '╱') } else { ('╱', '╲') };
    let (left, right) = match style {
        Style::Cjk => (left_cjk, right_cjk),
        Style::Ascii => (if converging { '\\' } else { '/' }, if converging { '/' } else { '\\' }),
    };
    Some((left, right))
}

fn river_line(cols: u8, style: Style) -> Line<'static> {
    // The river replaces the between-row entirely; there are no vertical
    // bars connecting through it. Total width must match a rank row:
    //   rank label (3) + grid (cols * 4 - 2) + trailing pad to align.
    let target_width = (cols as usize) * 4 - 2;
    let banner = match style {
        Style::Cjk => "〇 楚 河 ── 漢 界 〇",
        Style::Ascii => "~ river -- river ~",
    };
    let mut s = String::from("   "); // rank label
    let banner_w = visual_width(banner);
    let pad = target_width.saturating_sub(banner_w);
    let left_pad = pad / 2;
    let right_pad = pad - left_pad;
    for _ in 0..left_pad {
        s.push(' ');
    }
    s.push_str(banner);
    for _ in 0..right_pad {
        s.push(' ');
    }
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
    // Each cell takes 4 cols (label at col 0, then 3 spaces). Last cell
    // has just the label + 1 trailing space to match rank-row width.
    let mut s = String::from("   "); // rank label
    for display_col in 0..cols {
        let label = file_label_for_col(display_col, w, shape, observer);
        s.push(label);
        if display_col + 1 < cols {
            s.push_str("   ");
        } else {
            s.push(' '); // trailing pad
        }
    }
    Line::from(Span::styled(s, TuiStyle::default().fg(Color::DarkGray)))
}

fn file_label_for_col(col: u8, model_w: u8, shape: BoardShape, observer: Side) -> char {
    let sq = orient::square_at_display(0, col, observer, shape);
    match (shape, sq) {
        // Banqi displays transposed (8 cols × 4 rows). Display columns
        // therefore correspond to model RANKS (0–7), and rows to files (a–d).
        (BoardShape::Banqi4x8, Some(sq)) => {
            let r = (sq.0 / model_w as u16) as u8;
            (b'0' + r) as char
        }
        (_, Some(sq)) => {
            let f = (sq.0 % model_w as u16) as u8;
            (b'a' + f) as char
        }
        _ => '?',
    }
}

fn rank_label(observer: Side, shape: BoardShape, display_row: u8) -> String {
    let sq = orient::square_at_display(display_row, 0, observer, shape);
    let model_w = shape.dimensions().0;
    match (shape, sq) {
        (BoardShape::Banqi4x8, Some(sq)) => {
            // Banqi rows = model files; show as a–d.
            let f = (sq.0 % model_w as u16) as u8;
            format!(" {} ", (b'a' + f) as char)
        }
        (_, Some(sq)) => {
            let r = (sq.0 / model_w as u16) as u8;
            format!(" {} ", r)
        }
        _ => "   ".to_string(),
    }
}

fn side_color(side: Side) -> Color {
    match side {
        Side::RED => Color::Red,
        Side::BLACK => Color::White,
        _ => Color::Green,
    }
}

/// Append the "Captured pieces" block to `lines`. Two rows
/// (Red / Black) of glyph spans coloured by side. Compact layout
/// designed for the 36-col sidebar; long lists wrap via the parent
/// `Paragraph::wrap`. The header echoes the active sort label so the
/// user can see at a glance whether they're in time- or rank-mode.
fn push_captured_lines(
    lines: &mut Vec<Line<'static>>,
    captured: &[chess_core::piece::Piece],
    sort: CapturedSort,
    style: Style,
) {
    let header = format!("Captured ({}):", sort.label());
    lines.push(Line::from(Span::styled(header, TuiStyle::default().fg(Color::DarkGray))));

    let (red, black) = app::split_and_sort_captured(captured, sort);
    lines.push(captured_row(" R 紅:", &red, Color::Red, style));
    lines.push(captured_row(" B 黑:", &black, Color::Gray, style));
}

fn captured_row(
    label: &'static str,
    pieces: &[chess_core::piece::Piece],
    color: Color,
    style: Style,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(pieces.len() * 2 + 2);
    spans.push(Span::styled(label, TuiStyle::default().fg(Color::DarkGray)));
    if pieces.is_empty() {
        spans.push(Span::styled(" —", TuiStyle::default().fg(Color::DarkGray)));
    } else {
        for p in pieces {
            spans.push(Span::raw(" "));
            let g = glyph::glyph(p.kind, p.side, style);
            spans.push(Span::styled(
                g.to_string(),
                TuiStyle::default().fg(color).add_modifier(Modifier::BOLD),
            ));
        }
    }
    Line::from(spans)
}

#[allow(clippy::too_many_arguments)]
fn draw_sidebar(
    frame: &mut Frame,
    area: Rect,
    g: &GameView,
    view: &PlayerView,
    style: Style,
    help_open: bool,
    show_check_banner: bool,
    captured_sort: CapturedSort,
) {
    let block = Block::default().borders(Borders::ALL).title(" Status ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

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
            // Casual is the TUI default; strict is the alternative.
            if g.state.rules.xiangqi_allow_self_check {
                "Xiangqi (象棋)"
            } else {
                "Xiangqi (象棋, strict)"
            }
        }
        BoardShape::Banqi4x8 => "Banqi (暗棋)",
        BoardShape::ThreeKingdom => "三國暗棋",
        BoardShape::Custom { .. } => "Custom",
    };
    lines.push(line_label_value("Variant:", variant_label));

    // Side-to-move only makes sense while the game is ongoing; once it's
    // Won/Drawn the banner above already shows the winner. Display the
    // piece-colour (`current_color`), which diverges from `side_to_move`
    // (the seat) after a banqi first-flip.
    if matches!(view.status, GameStatus::Ongoing) {
        let stm_color = side_color(view.current_color);
        lines.push(Line::from(vec![
            Span::styled("Side to move:", TuiStyle::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(
                glyph::side_name(view.current_color, style),
                TuiStyle::default().fg(stm_color).add_modifier(Modifier::BOLD),
            ),
        ]));

        if show_check_banner
            && matches!(view.shape, BoardShape::Xiangqi9x10)
            && g.state.is_in_check(view.side_to_move)
        {
            lines.push(Line::from(Span::styled(
                "  ⚠ CHECK 將軍 — your general is under attack",
                TuiStyle::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )));
        }
    }

    let (status_text, status_color) = format_status_short(view.status, style);
    lines.push(Line::from(vec![
        Span::styled("Status:", TuiStyle::default().fg(Color::DarkGray)),
        Span::raw(" "),
        Span::styled(status_text, TuiStyle::default().fg(status_color)),
    ]));
    lines.push(line_label_value("Legal moves:", &view.legal_moves.len().to_string()));
    lines.push(line_label_value("History:", &g.state.history.len().to_string()));

    let selected_label = match g.selected {
        Some(sq) => chess_core::notation::iccs::encode_square(&g.state.board, sq),
        None => "—".into(),
    };
    lines.push(line_label_value("Selected:", &selected_label));

    push_captured_lines(&mut lines, &view.captured, captured_sort, style);

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
        if let Some(ci) = &g.coord_input {
            lines.push(Line::from(vec![
                Span::styled("> ", TuiStyle::default().fg(Color::Yellow)),
                Span::styled(format!("{}_", ci.buf), TuiStyle::default().fg(Color::Yellow)),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                "?=help, r=rules, : / m=coord, n=new, q=quit",
                TuiStyle::default().fg(Color::DarkGray),
            )));
        }
    }

    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(para, inner);
}

#[allow(clippy::too_many_arguments)]
fn draw_sidebar_net(
    frame: &mut Frame,
    area: Rect,
    n: &NetView,
    view: &PlayerView,
    style: Style,
    help_open: bool,
    show_check_banner: bool,
    captured_sort: CapturedSort,
) {
    let block = Block::default().borders(Borders::ALL).title(" Status (net) ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split the sidebar into "meta info" (top) and "chat region" (bottom).
    // The chat region keeps a scrolling log + a single input row when the
    // user is composing. Meta height is dynamic (grows to fit) and chat
    // gets the rest.
    let chat_input_active = n.chat_input.is_some();
    let chat_h = inner.height.saturating_sub(MIN_META_ROWS).max(MIN_CHAT_ROWS);
    let meta_h = inner.height.saturating_sub(chat_h);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(meta_h), Constraint::Length(chat_h)])
        .split(inner);

    let mut lines: Vec<Line> = Vec::new();

    if let Some(banner) = game_over_banner(&view.status, style) {
        for l in banner {
            lines.push(l);
        }
        lines.push(Line::from(""));
    }

    let variant_label = match (view.shape, n.rules.as_ref()) {
        (BoardShape::Xiangqi9x10, Some(r)) => {
            if r.xiangqi_allow_self_check {
                "Xiangqi (象棋)"
            } else {
                "Xiangqi (象棋, strict)"
            }
        }
        (BoardShape::Xiangqi9x10, None) => "Xiangqi (象棋)",
        (BoardShape::Banqi4x8, _) => "Banqi (暗棋)",
        (BoardShape::ThreeKingdom, _) => "三國暗棋",
        (BoardShape::Custom { .. }, _) => "Custom",
    };
    lines.push(line_label_value("Variant:", variant_label));

    match n.role {
        Some(NetRole::Player(side)) => {
            let c = side_color(side);
            lines.push(Line::from(vec![
                Span::styled("You:", TuiStyle::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(
                    glyph::side_name(side, style),
                    TuiStyle::default().fg(c).add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        Some(NetRole::Spectator) => {
            lines.push(Line::from(vec![
                Span::styled("You:", TuiStyle::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(
                    "Spectator (read-only)",
                    TuiStyle::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        None => {}
    }

    if matches!(view.status, GameStatus::Ongoing) {
        // "Your turn?" depends on the observer's SEAT (side_to_move) but
        // the displayed colour is the piece-colour the seat plays
        // (`current_color`). For banqi the two diverge after the first
        // flip; for xiangqi they're identical.
        let stm_color = side_color(view.current_color);
        let observer_side = n.role.and_then(|r| match r {
            NetRole::Player(s) => Some(s),
            NetRole::Spectator => None,
        });
        let label = match observer_side {
            Some(obs) if obs == view.side_to_move => "Your turn:",
            Some(_) => "Opponent:",
            None => "To move:",
        };
        lines.push(Line::from(vec![
            Span::styled(label, TuiStyle::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(
                glyph::side_name(view.current_color, style),
                TuiStyle::default().fg(stm_color).add_modifier(Modifier::BOLD),
            ),
        ]));

        // Check banner: `view.in_check` is set by the server in v4+ from the
        // observer's POV (or Red's POV for spectators). So Players read it
        // as "my own general"; Spectators read it as "Red's general".
        if show_check_banner && matches!(view.shape, BoardShape::Xiangqi9x10) && view.in_check {
            let msg = match n.role {
                Some(NetRole::Player(_)) => "  ⚠ CHECK 將軍 — your general is under attack",
                Some(NetRole::Spectator) => "  ⚠ CHECK 將軍 — Red's general is under attack",
                None => "  ⚠ CHECK 將軍",
            };
            lines.push(Line::from(Span::styled(
                msg,
                TuiStyle::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )));
        }
    }

    let (status_text, status_color) = format_status_short(view.status, style);
    lines.push(Line::from(vec![
        Span::styled("Status:", TuiStyle::default().fg(Color::DarkGray)),
        Span::raw(" "),
        Span::styled(status_text, TuiStyle::default().fg(status_color)),
    ]));
    lines.push(line_label_value("Legal moves:", &view.legal_moves.len().to_string()));

    let selected_label = match n.selected {
        Some(sq) => format!("sq {}", sq.0),
        None => "—".into(),
    };
    lines.push(line_label_value("Selected:", &selected_label));

    push_captured_lines(&mut lines, &view.captured, captured_sort, style);

    lines.push(line_label_value("Server:", &n.url));
    lines.push(line_label_value("Connection:", if n.connected { "live" } else { "disconnected" }));

    lines.push(Line::from(""));
    if let Some(msg) = &n.last_msg {
        lines.push(Line::from(Span::styled(msg.clone(), TuiStyle::default().fg(Color::Yellow))));
    }

    if help_open {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Help",
            TuiStyle::default().add_modifier(Modifier::BOLD),
        )));
        for ln in HELP_LINES_NET {
            lines.push(Line::from(Span::raw(*ln)));
        }
    } else {
        lines.push(Line::from(""));
        if let Some(ci) = &n.coord_input {
            lines.push(Line::from(vec![
                Span::styled("> ", TuiStyle::default().fg(Color::Yellow)),
                Span::styled(format!("{}_", ci.buf), TuiStyle::default().fg(Color::Yellow)),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                "?=help, r=rules, t=chat, : / m=coord, q=quit",
                TuiStyle::default().fg(Color::DarkGray),
            )));
        }
    }

    let meta_para = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(meta_para, chunks[0]);

    draw_chat_pane(frame, chunks[1], n, chat_input_active);
}

const MIN_META_ROWS: u16 = 17;
const MIN_CHAT_ROWS: u16 = 6;

fn draw_chat_pane(frame: &mut Frame, area: Rect, n: &NetView, input_active: bool) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(TuiStyle::default().fg(Color::DarkGray))
        .title(" Chat ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 {
        return;
    }

    // Reserve the bottom row for the input prompt (or hint when not typing).
    let log_h = inner.height.saturating_sub(1);
    let log_area = Rect { x: inner.x, y: inner.y, width: inner.width, height: log_h };
    let input_area = Rect { x: inner.x, y: inner.y + log_h, width: inner.width, height: 1 };

    let visible = log_h as usize;
    // Show the most recent N lines (cap by visible height) so the buffer
    // stays scrolled to the bottom on every render.
    let start = n.chat.len().saturating_sub(visible);
    let mut log_lines: Vec<Line> = Vec::with_capacity(visible);
    if n.chat.is_empty() {
        log_lines.push(Line::from(Span::styled(
            "(no messages yet)",
            TuiStyle::default().fg(Color::DarkGray),
        )));
    } else {
        for line in n.chat.iter().skip(start) {
            log_lines.push(format_chat_line(line));
        }
    }
    let log_para = Paragraph::new(log_lines).wrap(Wrap { trim: false });
    frame.render_widget(log_para, log_area);

    let input_line = if input_active {
        let buf = n.chat_input.as_deref().unwrap_or("");
        Line::from(vec![
            Span::styled("> ", TuiStyle::default().fg(Color::Yellow)),
            Span::styled(format!("{}_", buf), TuiStyle::default().fg(Color::Yellow)),
        ])
    } else {
        let hint = match n.role {
            Some(NetRole::Spectator) => "(spectator — read-only)",
            Some(NetRole::Player(_)) => "Press 't' to chat.",
            None => "",
        };
        Line::from(Span::styled(hint, TuiStyle::default().fg(Color::DarkGray)))
    };
    frame.render_widget(Paragraph::new(input_line).wrap(Wrap { trim: false }), input_area);
}

fn format_chat_line(line: &chess_net::ChatLine) -> Line<'static> {
    let from_label = match line.from {
        Side::RED => "Red:",
        Side::BLACK => "Black:",
        _ => "Green:",
    };
    let from_color = side_color(line.from);
    Line::from(vec![
        Span::styled(
            from_label.to_string(),
            TuiStyle::default().fg(from_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::raw(line.text.clone()),
    ])
}

const HELP_LINES_NET: &[&str] = &[
    "h j k l / ←↓↑→  move cursor",
    "Enter / Space   select / commit (sends Move to server)",
    "Esc             cancel selection",
    ":               coord input (instant): type ICCS (h2e2 / flip a0), Enter sends",
    "m               coord input (live preview): same, with selected/cursor preview",
    "f               flip (banqi)",
    "n               request rematch (after game over; needs both sides)",
    "t               open chat input (players only; Enter sends, Esc cancels)",
    "g               toggle captured-pieces sort (time / rank)",
    "r               toggle rules overlay",
    "?               toggle this help",
    "Click           select / commit",
    "q / Ctrl-C      quit (closes the connection)",
    "(undo not supported online yet)",
];

/// Map a Local game's outcome to the kind of big banner to draw. Local
/// (hot-seat) play gets neutral "RED WINS / BLACK WINS / DRAW" copy
/// because both seats share the same screen — calling either side
/// "VICTORY" or "DEFEAT" would be misleading. Returns `None` while the
/// game is still ongoing or for unsupported variants.
fn endgame_kind_local(status: &GameStatus, _observer: Side) -> Option<BannerKind> {
    match status {
        GameStatus::Ongoing => None,
        GameStatus::Won { winner, .. } => Some(BannerKind::Outcome(neutral_side_for(*winner))),
        GameStatus::Drawn { .. } => Some(BannerKind::Draw),
    }
}

/// Map a Net game's outcome. Players see VICTORY/DEFEAT keyed off their
/// own seat; spectators always see the neutral RED WINS / BLACK WINS
/// copy. Returns `None` while ongoing or before the role handshake.
fn endgame_kind_net(status: &GameStatus, role: Option<NetRole>) -> Option<BannerKind> {
    match (status, role) {
        (GameStatus::Ongoing, _) => None,
        (GameStatus::Drawn { .. }, _) => Some(BannerKind::Draw),
        (GameStatus::Won { winner, .. }, Some(NetRole::Player(seat))) => {
            if *winner == seat {
                Some(BannerKind::Victory)
            } else {
                Some(BannerKind::Defeat)
            }
        }
        (GameStatus::Won { winner, .. }, Some(NetRole::Spectator) | None) => {
            Some(BannerKind::Outcome(neutral_side_for(*winner)))
        }
    }
}

fn neutral_side_for(side: Side) -> NeutralSide {
    match side {
        Side::RED => NeutralSide::Red,
        Side::BLACK => NeutralSide::Black,
        _ => NeutralSide::Green,
    }
}

/// Render a confetti burst + centered ASCII banner over the board area
/// during the post-game "look at this" window.
///
/// The trigger lives elsewhere (`AppState::note_status_transition`); this
/// function only consumes the `confetti_pending` flag and the active
/// `confetti_anim`. While an animation is alive we also render the
/// matching big banner; once the animation expires both go away and the
/// sidebar's existing `game_over_banner` is left as the persistent cue.
///
/// Skipped entirely when the user disabled FX (`--no-confetti`) — the
/// pending flag is never armed in that case, so this function is a no-op.
fn render_confetti_and_banner(
    frame: &mut Frame,
    board_area: Rect,
    app: &mut AppState,
    endgame_kind: Option<BannerKind>,
    style: Style,
    use_color: bool,
) {
    // Spawn a fresh burst if the trigger fired since the last frame. The
    // sub-rect must be inside the board widget so particles land where the
    // user is already looking.
    if app.confetti_pending && app.confetti_anim.is_none() {
        app.confetti_anim = Some(ConfettiAnim::spawn(board_area));
        app.confetti_pending = false;
    }

    // Draw the big banner first (before particles overwrite cells), but
    // only while the animation is alive so it auto-clears with the burst.
    let anim_alive = app.confetti_anim.is_some();
    if anim_alive {
        if let Some(kind) = endgame_kind {
            render_big_banner(frame, board_area, kind, style, use_color);
        }
    }

    // Step + render particles. `step` advances physics and drops offscreen
    // particles; `render` paints into the frame's buffer cells. When `done`,
    // clear the slot so the next frame stops animating.
    if let Some(anim) = app.confetti_anim.as_mut() {
        let done = anim.step(board_area);
        anim.render(frame.buffer_mut(), board_area, use_color);
        if done {
            app.confetti_anim = None;
        }
    }
}

/// Centre the ASCII art in `area`. If `area` is too narrow to fit the
/// banner with reasonable padding, we silently skip the banner (confetti
/// alone still plays). The kind's color is chosen for emotional read:
/// yellow for VICTORY (excitement), red for DEFEAT (loss), magenta for
/// DRAW (neutral but not muted), red for CHECK.
fn render_big_banner(
    frame: &mut Frame,
    area: Rect,
    kind: BannerKind,
    style: Style,
    use_color: bool,
) {
    let rows = banner::art(kind, style);
    let banner_w = banner::max_width(rows) as u16;
    let banner_h = rows.len() as u16;
    if area.width < banner_w + 2 || area.height < banner_h + 2 {
        return;
    }
    let x = area.x + (area.width.saturating_sub(banner_w)) / 2;
    let y = area.y + (area.height.saturating_sub(banner_h)) / 2;
    let sub = Rect { x, y, width: banner_w, height: banner_h };

    let color = if !use_color {
        Color::Reset
    } else {
        match kind {
            BannerKind::Victory => Color::Yellow,
            BannerKind::Defeat => Color::Red,
            BannerKind::Draw => Color::Magenta,
            BannerKind::Outcome(_) => Color::Yellow,
            BannerKind::AppTitle => Color::Yellow,
        }
    };

    let lines: Vec<Line> = rows
        .iter()
        .map(|r| {
            Line::from(Span::styled(
                r.to_string(),
                TuiStyle::default().fg(color).add_modifier(Modifier::BOLD),
            ))
        })
        .collect();

    // Clear the cells under the banner so the board glyphs don't bleed
    // through. The Clear widget wipes to default style, then Paragraph
    // writes the banner text.
    frame.render_widget(Clear, sub);
    frame.render_widget(Paragraph::new(lines), sub);
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

/// Compact, human-readable rendering of `GameStatus` for the sidebar's
/// `Status:` line. Returns `(text, color)`. Engine `Debug` repr leaks
/// `Side(0)` etc. and reads as noise to a player.
fn format_status_short(status: GameStatus, style: Style) -> (String, Color) {
    match status {
        GameStatus::Ongoing => ("Ongoing".to_string(), Color::Gray),
        GameStatus::Won { winner, reason } => (
            format!("{} wins — {}", glyph::side_name(winner, style), win_reason_label(reason)),
            side_color(winner),
        ),
        GameStatus::Drawn { reason } => (format!("Draw — {:?}", reason), Color::Gray),
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

fn draw_quit_confirm_overlay(frame: &mut Frame, area: Rect) {
    // Small modal centered over the screen.
    let w = 48u16;
    let h = 7u16;
    let pad_x = area.width.saturating_sub(w) / 2;
    let pad_y = area.height.saturating_sub(h) / 2;
    let overlay = Rect {
        x: area.x + pad_x,
        y: area.y + pad_y,
        width: area.width.min(w),
        height: area.height.min(h),
    };
    frame.render_widget(Clear, overlay);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Quit? ")
        .border_style(TuiStyle::default().fg(Color::Yellow));
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  End the in-progress game?",
            TuiStyle::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  Press y to quit, anything else to keep playing.",
            TuiStyle::default().fg(Color::Gray),
        )),
    ];
    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, overlay);
}

const HELP_LINES: &[&str] = &[
    "h j k l / ←↓↑→  move cursor",
    "Enter / Space   select / commit",
    "Esc             cancel selection",
    ":               coord input (instant): type ICCS (h2e2 / flip a0), Enter commits",
    "m               coord input (live preview): same, with selected/cursor preview",
    "u               undo last move",
    "f               flip (banqi)",
    "n               new game (back to picker)",
    "g               toggle captured-pieces sort (time / rank)",
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
