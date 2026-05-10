//! ratatui draw layer. Stateless — reads `AppState` and draws.

use chess_core::board::BoardShape;
use chess_core::coord::Square;
use chess_core::moves::Move;
use chess_core::piece::{PieceKind, Side};
use chess_core::state::{GameStatus, WinReason};
use chess_core::view::{PlayerView, VisibleCell};

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style as TuiStyle};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{
    self, AppState, BanqiPreset, CapturedSort, CreateRoomField, CreateRoomView, CustomRulesView,
    CustomVariant, GameView, HostPromptView, LobbyView, NetRole, NetView, PickerEntry, PickerView,
    RectPx, Screen, CUSTOM_BANQI_FLAGS,
};
use crate::banner::{self, BannerKind, NeutralSide};
use crate::confetti::ConfettiAnim;
use crate::glyph::{self, Style};
use crate::orient;
use crate::text_input;
use chess_core::rules::{RuleSet, Variant};

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
        Screen::CustomRules(c) => draw_custom_rules(frame, area, c),
    }

    if app.rules_open {
        let active_rules = active_rules(&app.screen);
        draw_rules_overlay(frame, area, active_rules);
    }
    if app.quit_confirm_open {
        draw_quit_confirm_overlay(frame, area);
    }
    if app.resign_confirm_open {
        draw_resign_confirm_overlay(frame, area);
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
    // Engine 連吃 chain_lock takes precedence as the "effective selection"
    // so the locked piece + its legal next-hops light up automatically —
    // the user doesn't have to re-click their attacker between hops.
    let effective_selected = view.chain_lock.or(g_ref.selected);
    let mut rect = app.board_rect;
    // PV chain to highlight on the board (debug overlay). Empty unless
    // debug panel is open AND there's an analysis to read from. The
    // cursor index selects which scored move's PV to render — index 0
    // (the AI's chosen move) by default, ',' / '.' navigate.
    let highlighted_pv: Vec<Move> = if app.debug_open {
        g_ref
            .last_analysis
            .as_ref()
            .and_then(|a| {
                let cur = app.debug_cursor.min(a.scored.len().saturating_sub(1));
                a.scored.get(cur).map(|sm| {
                    let mut chain = Vec::with_capacity(sm.pv.len() + 1);
                    chain.push(sm.mv.clone());
                    chain.extend(sm.pv.iter().cloned());
                    chain
                })
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    // Threat overlay (Display setting). Pulls from the engine-cached
    // `view.threats` for the static buckets; computes the optional
    // hover-on-select bucket on demand from `g_ref.state` (which we
    // have direct access to in Game mode — no PlayerView round-trip
    // needed). Net mode runs through `compute_threat_sets_view`,
    // which can only consult the projected view.
    let threats = compute_threat_sets_local(
        &view,
        observer,
        app.threat_mode,
        app.threat_on_select,
        effective_selected,
        &g_ref.state,
    );
    draw_board(
        frame,
        chunks[0],
        observer,
        style,
        use_color,
        &view,
        g_ref.cursor,
        effective_selected,
        &mut rect,
        &highlighted_pv,
        &threats,
    );
    app.board_rect = rect;
    let captured_sort = app.captured_sort;
    let history_open = app.history_open;
    let debug_open = app.debug_open;
    let debug_cursor = app.debug_cursor;
    draw_sidebar(
        frame,
        chunks[1],
        g_ref,
        &view,
        style,
        help_open,
        show_check_banner,
        captured_sort,
        history_open,
        debug_open,
        debug_cursor,
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
            // Engine 連吃 chain_lock takes precedence as the "effective
            // selection" — chain-locked piece + next-hops auto-highlighted.
            let effective_selected = view.chain_lock.or(n_ref.selected);
            let mut rect = app.board_rect;
            // Net mode: no AI here, no debug PV to highlight. Pass empty.
            let empty_pv: Vec<Move> = Vec::new();
            // Net mode threat overlay: no GameState handy (we only
            // see the projected view), so the hover-on-select bucket
            // is best-effort via `compute_threat_sets_view` (xiangqi
            // only — banqi reconstruction would leak hidden info).
            let threats = compute_threat_sets_view(
                view,
                observer,
                app.threat_mode,
                app.threat_on_select,
                effective_selected,
            );
            draw_board(
                frame,
                chunks[0],
                observer,
                style,
                use_color,
                view,
                n_ref.cursor,
                effective_selected,
                &mut rect,
                &empty_pv,
                &threats,
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

/// Build a `ThreatSets` for Game-mode rendering. Has direct access
/// to `state` (the authoritative `GameState`), so the optional
/// hover-on-select bucket runs on a clone of `state` with the
/// selected piece removed — same delta semantic as the web's
/// `hover_threat_squares`.
///
/// Static / mate buckets come straight from `view.threats`, which is
/// already pre-projected for the observer.
fn compute_threat_sets_local(
    view: &PlayerView,
    observer: Side,
    mode: crate::app::ThreatMode,
    hover_on: bool,
    selected: Option<Square>,
    state: &chess_core::state::GameState,
) -> ThreatSets {
    use crate::app::ThreatMode;
    let (static_squares, mate_squares) = match mode {
        ThreatMode::Off => (Vec::new(), Vec::new()),
        ThreatMode::Attacked => (view.threats.attacked.clone(), Vec::new()),
        ThreatMode::NetLoss => (view.threats.net_loss.clone(), Vec::new()),
        ThreatMode::MateThreat => (Vec::new(), view.threats.mate_threats.clone()),
    };
    let hover_squares = if hover_on {
        if let Some(sel) = selected {
            hover_threat_squares_from_state(state, observer, sel)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    ThreatSets {
        static_squares: static_squares.into_iter().collect(),
        mate_squares: mate_squares.into_iter().collect(),
        hover_squares: hover_squares.into_iter().collect(),
    }
}

/// Net-mode counterpart to `compute_threat_sets_local`. Has only the
/// projected `PlayerView` available, so the hover-on-select bucket
/// is best-effort — and only attempted on xiangqi (banqi state
/// reconstruction would leak hidden info; see the web's
/// `state::reconstruct_xiangqi_state_for_analysis` for the same
/// constraint).
fn compute_threat_sets_view(
    view: &PlayerView,
    observer: Side,
    mode: crate::app::ThreatMode,
    hover_on: bool,
    selected: Option<Square>,
) -> ThreatSets {
    use crate::app::ThreatMode;
    let (static_squares, mate_squares) = match mode {
        ThreatMode::Off => (Vec::new(), Vec::new()),
        ThreatMode::Attacked => (view.threats.attacked.clone(), Vec::new()),
        ThreatMode::NetLoss => (view.threats.net_loss.clone(), Vec::new()),
        ThreatMode::MateThreat => (Vec::new(), view.threats.mate_threats.clone()),
    };
    let hover_squares = if hover_on && matches!(view.shape, BoardShape::Xiangqi9x10) {
        if let Some(sel) = selected {
            hover_threat_squares_from_view(view, observer, sel)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    ThreatSets {
        static_squares: static_squares.into_iter().collect(),
        mate_squares: mate_squares.into_iter().collect(),
        hover_squares: hover_squares.into_iter().collect(),
    }
}

/// Newly-vulnerable observer pieces if the piece at `selected` were
/// removed from the board. Direct GameState path used by Game mode.
/// See the web's `crate::state::hover_threat_squares` for the same
/// semantic on the projected-view path.
fn hover_threat_squares_from_state(
    state: &chess_core::state::GameState,
    observer: Side,
    selected: Square,
) -> Vec<Square> {
    use chess_core::state::GameStatus;
    if !matches!(state.status, GameStatus::Ongoing) {
        return Vec::new();
    }
    let Some(pos) = state.board.get(selected) else { return Vec::new() };
    if pos.piece.side != observer {
        return Vec::new();
    }
    let before: std::collections::HashSet<Square> =
        state.attacked_pieces(observer).into_iter().collect();
    let mut without = state.clone();
    without.board.set(selected, None);
    without
        .attacked_pieces(observer)
        .into_iter()
        .filter(|sq| !before.contains(sq) && *sq != selected)
        .collect()
}

/// Net-mode hover preview: rebuild a casual xiangqi `GameState` from
/// the view, then run the same delta as `hover_threat_squares_from_state`.
/// Banqi reconstruction is impossible without leaking hidden info, so
/// this returns empty for any non-xiangqi shape.
fn hover_threat_squares_from_view(
    view: &PlayerView,
    observer: Side,
    selected: Square,
) -> Vec<Square> {
    use chess_core::board::Board;
    use chess_core::rules::RuleSet;
    use chess_core::state::{GameState, GameStatus};
    if !matches!(view.shape, BoardShape::Xiangqi9x10) {
        return Vec::new();
    }
    if !matches!(view.status, GameStatus::Ongoing) {
        return Vec::new();
    }
    let mut state = GameState::new(RuleSet::xiangqi_casual());
    let mut board = Board::new(view.shape);
    for (idx, cell) in view.cells.iter().enumerate() {
        let sq = Square(idx as u16);
        let pos = match cell {
            VisibleCell::Empty => None,
            VisibleCell::Hidden => return Vec::new(),
            VisibleCell::Revealed(p) => Some(*p),
        };
        board.set(sq, pos);
    }
    state.board = board;
    state.side_to_move = view.side_to_move;
    state.status = view.status;
    state.chain_lock = view.chain_lock;
    hover_threat_squares_from_state(&state, observer, selected)
}

#[allow(clippy::too_many_arguments)]
// Optional `highlighted_pv`: PV chain to highlight on the board (debug
// overlay). Element 0 is the AI's chosen move, 1..N is the predicted
// continuation. Empty = no highlight. Rendered with bg color tint on
// origin and destination of each step.
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
    highlighted_pv: &[Move],
    threats: &ThreatSets,
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

    // Build debug-overlay highlight sets from `highlighted_pv`.
    // `debug_from` collects all origin squares (one per PV move),
    // `debug_to` all destination squares. Both empty when not debugging.
    let mut debug_from: std::collections::HashSet<Square> = std::collections::HashSet::new();
    let mut debug_to: std::collections::HashSet<Square> = std::collections::HashSet::new();
    for mv in highlighted_pv {
        debug_from.insert(mv.origin_square());
        if let Some(t) = mv.to_square() {
            debug_to.insert(t);
        }
    }

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
            &debug_from,
            &debug_to,
            threats,
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
    debug_from: &std::collections::HashSet<Square>,
    debug_to: &std::collections::HashSet<Square>,
    threats: &ThreatSets,
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
                debug_from,
                debug_to,
                threats,
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

/// Threat-highlight buckets passed down to `intersection_or_piece` so
/// each cell can paint a tinted background when its square shows up
/// in any of the three lists. Mirrors
/// `crate::components::board::ThreatOverlay` on the web side; same
/// semantic split (static / mate / hover) — see that struct's docs.
///
/// Passed as `&ThreatSets` rather than three loose params so adding
/// a fourth bucket later only touches the struct, not every
/// signature in the render path.
#[derive(Default)]
pub(crate) struct ThreatSets {
    pub static_squares: std::collections::HashSet<Square>,
    pub mate_squares: std::collections::HashSet<Square>,
    pub hover_squares: std::collections::HashSet<Square>,
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
    debug_from: &std::collections::HashSet<Square>,
    debug_to: &std::collections::HashSet<Square>,
    threats: &ThreatSets,
) -> (String, TuiStyle, bool) {
    let cell = &view.cells[sq.0 as usize];
    let is_cursor = (display_row, display_col) == cursor;
    let is_selected = selected == Some(sq);
    let is_target = highlight.contains(&sq);
    // Engine 連吃 chain mode: the locked piece gets a distinct
    // orange-ish highlight so the user sees that it's the engine
    // forcing this selection (not their own click) and can recall
    // that Esc / Enter on it ends the chain.
    let is_chain_locked = view.chain_lock == Some(sq);
    // Threat-highlight buckets — applied BEFORE selection / cursor so
    // user-driven highlights still take visual priority. Mate (mode C)
    // wins over static (mode A/B) when both fire on the same square,
    // hover loses to both (it's an extra hint, not a primary signal).
    let is_threat_static = threats.static_squares.contains(&sq);
    let is_threat_mate = threats.mate_squares.contains(&sq);
    let is_threat_hover = threats.hover_squares.contains(&sq);

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
    // Debug overlay highlights — applied before selection/cursor so
    // those still take visual priority. `from` = origin of a PV move,
    // `to` = destination. Different bg colors so the user can read
    // direction at a glance.
    if debug_from.contains(&sq) {
        s = s.bg(Color::Rgb(80, 70, 30));
    }
    if debug_to.contains(&sq) {
        s = s.bg(Color::Rgb(30, 70, 100));
    }
    // Threat tints — also applied before selection/cursor so the
    // user's interactive choices visually win. Stack order chosen so
    // mate (the strongest signal — strict 叫殺) overrides static
    // (mode A/B), and hover (the orthogonal toggle, weakest) is
    // applied first so a square promoted into the static / mate set
    // visually wins. All three use red-family backgrounds; mate
    // shifts toward magenta to mirror the web `.threat-mark--mate`
    // class.
    if is_threat_hover {
        s = s.bg(Color::Rgb(60, 20, 20));
    }
    if is_threat_static {
        s = s.bg(Color::Rgb(95, 25, 25));
    }
    if is_threat_mate {
        s = s.bg(Color::Rgb(110, 25, 90));
    }
    if is_selected {
        // Brown for a regular user-driven selection, brighter orange
        // when the same square is also the chain-lock (engine-driven).
        let bg = if is_chain_locked { Color::Rgb(140, 80, 20) } else { Color::Rgb(80, 60, 20) };
        s = s.bg(bg).add_modifier(Modifier::BOLD);
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

/// Render the move-history panel: header + last `MAX_HISTORY_LINES`
/// plies in ICCS notation, side-coloured. Newest at the bottom (matches
/// reading order). Toggled via `H` and gated by `app.history_open`.
///
/// Cap to a window so a 200-ply game doesn't blow up the sidebar; older
/// plies are dropped (the user can grow the panel by reading the help
/// or, in a future round, scrolling).
fn push_history_lines(lines: &mut Vec<Line<'static>>, g: &GameView, _style: Style) {
    use chess_core::piece::Side;
    const MAX_HISTORY_LINES: usize = 24;

    let total = g.state.history.len();
    let header = format!("History 棋譜 ({} plies):", total);
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(header, TuiStyle::default().fg(Color::DarkGray))));

    if total == 0 {
        lines.push(Line::from(Span::styled(
            "  (no moves yet)",
            TuiStyle::default().fg(Color::DarkGray),
        )));
        return;
    }

    let start = total.saturating_sub(MAX_HISTORY_LINES);
    if start > 0 {
        lines.push(Line::from(Span::styled(
            format!("  … {} earlier plies omitted", start),
            TuiStyle::default().fg(Color::DarkGray),
        )));
    }
    for (i, rec) in g.state.history.iter().enumerate().skip(start) {
        let n = i + 1;
        let side_label = match rec.mover {
            Side::RED => "紅",
            Side::BLACK => "黑",
            _ => "綠",
        };
        let side_color = match rec.mover {
            Side::RED => Color::Red,
            Side::BLACK => Color::Gray,
            _ => Color::Green,
        };
        let text = chess_core::notation::iccs::encode_move(&g.state.board, &rec.the_move);
        lines.push(Line::from(vec![
            Span::styled(format!(" {:>3}.", n), TuiStyle::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(
                side_label,
                TuiStyle::default().fg(side_color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::raw(text),
        ]));
    }
}

/// Format wall-clock elapsed time into the `, time=X ms` /
/// `, time=Y.Z s` suffix used inside the AI Debug header line. Same
/// conversion thresholds as chess-web's `crate::time::format_elapsed_ms`
/// (integer ms below 1 s; one decimal of seconds above) so the two
/// frontends agree on what a 1.4 s search looks like.
fn format_elapsed_ms_for_header(ms: u32) -> String {
    if ms < 1000 {
        format!(", time={} ms", ms)
    } else {
        format!(", time={:.1} s", ms as f64 / 1000.0)
    }
}

/// Render the AI debug panel: scored root moves with the cursor row
/// highlighted (its PV is also overlaid on the board via
/// `draw_board`'s `highlighted_pv`). Window of `MAX_DEBUG_LINES`
/// rows centred on the cursor so even with 40+ moves the panel stays
/// compact.
fn push_debug_lines(lines: &mut Vec<Line<'static>>, g: &GameView, cursor: usize) {
    use chess_core::piece::Side;
    const MAX_DEBUG_LINES: usize = 14;

    lines.push(Line::from(""));
    let Some(analysis) = g.last_analysis.as_ref() else {
        lines.push(Line::from(Span::styled(
            "AI Debug: waiting for AI's first move…",
            TuiStyle::default().fg(Color::DarkGray),
        )));
        return;
    };

    // Honest depth display: when the iterative-deepening loop bailed
    // before completing the requested target (NODE_BUDGET cap), show
    // both reached and target so the user knows their --ai-depth=N was
    // truncated. See `pitfalls/ai-search-depth-setting-shows-depth-4.md`.
    let depth_str = if analysis.budget_hit && analysis.target_depth > analysis.depth {
        format!("{} / {} (cap)", analysis.depth, analysis.target_depth)
    } else {
        analysis.depth.to_string()
    };
    // Wall-clock time, when the caller measured it. chess-tui's
    // `ai_reply` (clients/chess-tui/src/app.rs) wraps the analyze()
    // call in `std::time::Instant`. None on screens/cases where the
    // caller didn't time (no row added).
    let time_str = analysis.elapsed_ms.map(format_elapsed_ms_for_header).unwrap_or_default();
    let header = format!(
        "AI Debug ({}, depth {}, {} nodes, {} moves{}):",
        analysis.strategy.as_str(),
        depth_str,
        analysis.nodes,
        analysis.scored.len(),
        time_str,
    );
    lines.push(Line::from(Span::styled(header, TuiStyle::default().fg(Color::Rgb(245, 166, 35)))));
    lines.push(Line::from(Span::styled(
        "  ',' / '.' to navigate; PV overlays board",
        TuiStyle::default().fg(Color::DarkGray),
    )));

    let chosen_mv = &analysis.chosen.mv;
    let total = analysis.scored.len();
    if total == 0 {
        return;
    }
    let cur = cursor.min(total - 1);
    // Window: try to centre cursor with a small bias toward the top.
    let half = MAX_DEBUG_LINES / 2;
    let start = cur.saturating_sub(half);
    let end = (start + MAX_DEBUG_LINES).min(total);

    if start > 0 {
        lines.push(Line::from(Span::styled(
            format!("  … {} above", start),
            TuiStyle::default().fg(Color::DarkGray),
        )));
    }

    for i in start..end {
        let sm = &analysis.scored[i];
        let mv_text = chess_core::notation::iccs::encode_move(&g.state.board, &sm.mv);
        let pv_text: String = if sm.pv.is_empty() {
            String::new()
        } else {
            let strs: Vec<String> = sm
                .pv
                .iter()
                .take(2)
                .map(|m| chess_core::notation::iccs::encode_move(&g.state.board, m))
                .collect();
            let suffix =
                if sm.pv.len() > 2 { format!(" …+{}", sm.pv.len() - 2) } else { String::new() };
            format!(" → {}{}", strs.join(" → "), suffix)
        };
        let is_chosen = &sm.mv == chosen_mv;
        let is_cursor = i == cur;
        let mut row_color = match sm.mv.origin_square() {
            _ if sm.score < -1000 => Color::Red,
            _ if sm.score > 200 => Color::Green,
            _ => Color::White,
        };
        let _ = &mut row_color;
        let prefix = if is_cursor { ">" } else { " " };
        let star = if is_chosen { "★" } else { " " };
        let text =
            format!("{}{:>3}. {} {} {:+}{}", prefix, i + 1, star, mv_text, sm.score, pv_text);
        let style = if is_cursor {
            TuiStyle::default()
                .bg(Color::Rgb(60, 50, 30))
                .fg(row_color)
                .add_modifier(Modifier::BOLD)
        } else if sm.score < -1000 {
            TuiStyle::default().fg(Color::Red)
        } else if sm.score > 200 {
            TuiStyle::default().fg(Color::Green)
        } else {
            TuiStyle::default().fg(Color::Gray)
        };
        let _ = Side::RED; // suppress unused-import warning if all branches don't reach
        lines.push(Line::from(Span::styled(text, style)));
    }

    if end < total {
        lines.push(Line::from(Span::styled(
            format!("  … {} below", total - end),
            TuiStyle::default().fg(Color::DarkGray),
        )));
    }
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
    history_open: bool,
    debug_open: bool,
    debug_cursor: usize,
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

    if let Some(lock) = view.chain_lock {
        let sq_label = chess_core::notation::iccs::encode_square(&g.state.board, lock);
        lines.push(Line::from(Span::styled(
            format!("⛓ 連吃 chain on {sq_label}"),
            TuiStyle::default().fg(Color::Rgb(245, 166, 35)).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "  Enter / click locked = end chain · Esc = end · capture target = continue",
            TuiStyle::default().fg(Color::Rgb(245, 166, 35)),
        )));
    }

    push_captured_lines(&mut lines, &view.captured, captured_sort, style);

    if history_open {
        push_history_lines(&mut lines, g, style);
    }

    if debug_open {
        push_debug_lines(&mut lines, g, debug_cursor);
    }

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
            // Banqi adds an `f=flip` hint (and an inline 暗 reminder
            // when the cursor is on a face-down tile) so newcomers
            // don't get stuck pressing Enter on 暗 wondering why
            // nothing happens.
            let is_banqi = matches!(view.shape, BoardShape::Banqi4x8);
            let hint = if is_banqi {
                "?=help, f=flip 暗, s=resign, r=rules, : / m=coord, g=captured, H=history, D=debug, n=new, q=quit"
            } else {
                "?=help, s=resign, r=rules, : / m=coord, g=captured, H=history, D=debug, n=new, q=quit"
            };
            lines.push(Line::from(Span::styled(hint, TuiStyle::default().fg(Color::DarkGray))));

            if is_banqi {
                let cursor_sq =
                    orient::square_at_display(g.cursor.0, g.cursor.1, view.observer, view.shape);
                if cursor_sq
                    .is_some_and(|sq| matches!(view.cells[sq.0 as usize], VisibleCell::Hidden))
                {
                    lines.push(Line::from(Span::styled(
                        "  ↑ press f to flip this 暗 tile",
                        TuiStyle::default().fg(Color::Yellow),
                    )));
                }
            }
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

    if let Some(lock) = view.chain_lock {
        lines.push(Line::from(Span::styled(
            format!("⛓ 連吃 chain on sq {}", lock.0),
            TuiStyle::default().fg(Color::Rgb(245, 166, 35)).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "  Enter / click locked = end · Esc = end · target = continue",
            TuiStyle::default().fg(Color::Rgb(245, 166, 35)),
        )));
    }

    push_captured_lines(&mut lines, &view.captured, captured_sort, style);

    lines.push(line_label_value("Server:", &n.url));
    lines.push(line_label_value("Connection:", if n.connected { "live" } else { "disconnected" }));
    // Surface the room's hints sanction so opponents see when AI hints
    // are sanctioned. Matches the web client's "🧠 hints enabled"
    // banner. None until welcome arrives.
    if let Some(allowed) = n.hints_allowed {
        lines
            .push(line_label_value("AI hints:", if allowed { "enabled (🧠)" } else { "disabled" }));
    }

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
            let is_banqi = matches!(view.shape, BoardShape::Banqi4x8);
            let hint = if is_banqi {
                "?=help, f=flip 暗, s=resign, r=rules, t=chat, : / m=coord, g=captured, q=quit"
            } else {
                "?=help, s=resign, r=rules, t=chat, : / m=coord, g=captured, q=quit"
            };
            lines.push(Line::from(Span::styled(hint, TuiStyle::default().fg(Color::DarkGray))));

            if is_banqi {
                let observer = n.role.map(|r| r.observer()).unwrap_or(Side::RED);
                let cursor_sq =
                    orient::square_at_display(n.cursor.0, n.cursor.1, observer, view.shape);
                if cursor_sq
                    .is_some_and(|sq| matches!(view.cells[sq.0 as usize], VisibleCell::Hidden))
                {
                    lines.push(Line::from(Span::styled(
                        "  ↑ press f to flip this 暗 tile",
                        TuiStyle::default().fg(Color::Yellow),
                    )));
                }
            }
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
    "s               resign (投降) — concede to opponent (players only)",
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

/// Resolve the live `RuleSet` for the active screen, if any. Used to
/// drive the rules-overlay's "current rules" prelude. Returns `None` for
/// screens without an active game (Picker, HostPrompt, Lobby, …).
fn active_rules(screen: &Screen) -> Option<&RuleSet> {
    match screen {
        Screen::Game(g) => Some(&g.state.rules),
        Screen::Net(n) => n.rules.as_ref(),
        Screen::Picker(_)
        | Screen::HostPrompt(_)
        | Screen::Lobby(_)
        | Screen::CreateRoom(_)
        | Screen::CustomRules(_) => None,
    }
}

fn draw_rules_overlay(frame: &mut Frame, area: Rect, rules: Option<&RuleSet>) {
    // Center an overlay roughly 70 cols × 30 rows over the screen.
    let pad_x = area.width.saturating_sub(72) / 2;
    let pad_y = area.height.saturating_sub(32) / 2;
    let overlay = Rect {
        x: area.x + pad_x,
        y: area.y + pad_y,
        width: area.width.min(72),
        height: area.height.min(32),
    };
    frame.render_widget(Clear, overlay);
    let block =
        Block::default().borders(Borders::ALL).title(" Rules / 規則 — press r or Esc to close ");

    let mut lines: Vec<Line> = Vec::new();

    // Per-game prelude: show which house rules / mode are active for this
    // session so the user knows what they're playing without having to
    // remember the picker or CLI args.
    if let Some(r) = rules {
        for ln in current_rules_lines(r) {
            lines.push(ln);
        }
        lines.push(Line::from(""));
    }

    for (s, accent) in RULES_LINES {
        let st = if *accent {
            TuiStyle::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            TuiStyle::default()
        };
        lines.push(Line::from(Span::styled(*s, st)));
    }
    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, overlay);
}

/// Build the "current rules" prelude block for the overlay. Xiangqi gets a
/// strict/casual line; banqi gets a 6-row [x]/[ ] checkbox of the active
/// flags plus a Seed line when deterministic.
fn current_rules_lines(rules: &RuleSet) -> Vec<Line<'static>> {
    let header_style = TuiStyle::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let dim = TuiStyle::default().fg(Color::DarkGray);
    let mut out: Vec<Line<'static>> = Vec::new();
    match rules.variant {
        Variant::Xiangqi => {
            out.push(Line::from(Span::styled("Xiangqi 象棋 — current rules", header_style)));
            let mode = if rules.xiangqi_allow_self_check {
                "Casual (allow self-check — lose by general capture)"
            } else {
                "Standard (self-check filter on — must defend)"
            };
            out.push(Line::from(vec![Span::raw("  • "), Span::raw(mode.to_string())]));
        }
        Variant::Banqi => {
            out.push(Line::from(Span::styled("Banqi 暗棋 — current rules", header_style)));
            for (flag, token, subtitle) in CUSTOM_BANQI_FLAGS {
                let on = rules.house.contains(flag);
                let mark = if on { "[x]" } else { "[ ]" };
                let row_style = if on { TuiStyle::default() } else { dim };
                out.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(mark.to_string(), row_style),
                    Span::raw(" "),
                    Span::styled(format!("{token:<11}"), row_style),
                    Span::styled(subtitle.to_string(), row_style),
                ]));
            }
            let seed_line = match rules.banqi_seed {
                Some(s) => format!("  Seed: {s} (deterministic)"),
                None => "  Seed: — (random)".to_string(),
            };
            out.push(Line::from(Span::styled(seed_line, dim)));
        }
        Variant::ThreeKingdomBanqi => {
            out.push(Line::from(Span::styled("三國暗棋 — current rules", header_style)));
            out.push(Line::from(Span::styled(
                "  (move-gen still landing — see TODO.md)".to_string(),
                dim,
            )));
        }
    }
    out
}

fn draw_custom_rules(frame: &mut Frame, area: Rect, view: &CustomRulesView) {
    let mut lines: Vec<Line> = Vec::new();
    let header_style = TuiStyle::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let dim = TuiStyle::default().fg(Color::DarkGray);

    let title = match view.variant {
        CustomVariant::Xiangqi => "Custom Xiangqi rules",
        CustomVariant::Banqi => "Custom Banqi rules",
    };
    lines.push(Line::from(Span::styled(title.to_string(), header_style)));
    lines.push(Line::from(""));

    let items = view.items();
    let cur = view.cursor.min(items.len().saturating_sub(1));
    let mut item_idx = 0usize;

    match view.variant {
        CustomVariant::Xiangqi => {
            lines
                .push(Line::from(Span::styled("Mode (Enter / Space to choose):".to_string(), dim)));
            // Two radio rows.
            for (i, (label, strict)) in
                [("Casual (allow self-check)", false), ("Standard (must defend check)", true)]
                    .iter()
                    .enumerate()
            {
                let selected = view.xiangqi_strict == *strict;
                let prefix = if item_idx + i == cur { "▶ " } else { "  " };
                let glyph = if selected { "(•)" } else { "( )" };
                let row_style = if item_idx + i == cur {
                    TuiStyle::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    TuiStyle::default()
                };
                lines.push(Line::from(vec![
                    Span::raw(prefix),
                    Span::styled(glyph.to_string(), row_style),
                    Span::raw(" "),
                    Span::styled((*label).to_string(), row_style),
                ]));
            }
            item_idx += 2;
        }
        CustomVariant::Banqi => {
            lines.push(Line::from(Span::styled(
                "Preset (Enter / Space to choose):".to_string(),
                dim,
            )));
            for (i, p) in [
                BanqiPreset::Purist,
                BanqiPreset::Taiwan,
                BanqiPreset::Aggressive,
                BanqiPreset::Custom,
            ]
            .iter()
            .enumerate()
            {
                let selected = view.banqi_preset == *p;
                let prefix = if item_idx + i == cur { "▶ " } else { "  " };
                let glyph = if selected { "(•)" } else { "( )" };
                let row_style = if item_idx + i == cur {
                    TuiStyle::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    TuiStyle::default()
                };
                lines.push(Line::from(vec![
                    Span::raw(prefix),
                    Span::styled(glyph.to_string(), row_style),
                    Span::raw(" "),
                    Span::styled(p.label().to_string(), row_style),
                ]));
            }
            item_idx += 4;

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("House rules (Space to toggle):".to_string(), dim)));
            for (i, (flag, token, subtitle)) in CUSTOM_BANQI_FLAGS.iter().enumerate() {
                let on = view.banqi_flags.contains(*flag);
                let prefix = if item_idx + i == cur { "▶ " } else { "  " };
                let glyph = if on { "[x]" } else { "[ ]" };
                let row_style = if item_idx + i == cur {
                    TuiStyle::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else if on {
                    TuiStyle::default()
                } else {
                    dim
                };
                lines.push(Line::from(vec![
                    Span::raw(prefix),
                    Span::styled(glyph.to_string(), row_style),
                    Span::raw(" "),
                    Span::styled(format!("{:<11}", *token), row_style),
                    Span::styled((*subtitle).to_string(), row_style),
                ]));
            }
            item_idx += CUSTOM_BANQI_FLAGS.len();

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Shuffle seed (digits only, blank = random):".to_string(),
                dim,
            )));
            let prefix = if item_idx == cur { "▶ " } else { "  " };
            let row_style = if item_idx == cur {
                TuiStyle::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                TuiStyle::default()
            };
            let buf_display = if item_idx == cur {
                format!("[ {}_ ]", view.banqi_seed)
            } else if view.banqi_seed.is_empty() {
                "[          ]".to_string()
            } else {
                format!("[ {} ]", view.banqi_seed)
            };
            lines.push(Line::from(vec![
                Span::raw(prefix),
                Span::styled("Seed: ", row_style),
                Span::styled(buf_display, row_style),
            ]));
            item_idx += 1;
        }
    }

    // Start button — last item.
    lines.push(Line::from(""));
    let prefix = if item_idx == cur { "▶ " } else { "  " };
    let btn_style = if item_idx == cur {
        TuiStyle::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        TuiStyle::default().fg(Color::Gray)
    };
    lines.push(Line::from(vec![
        Span::raw(prefix),
        Span::styled("[ Start ]".to_string(), btn_style),
    ]));

    if let Some(msg) = view.last_msg.as_deref() {
        lines.push(Line::from(""));
        lines
            .push(Line::from(Span::styled(format!("× {msg}"), TuiStyle::default().fg(Color::Red))));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "[Enter/Space] activate · [↑↓ / jk] move · digits = seed · [Esc] back · [q] quit",
        dim,
    )));

    let block = Block::default().borders(Borders::ALL).title(" Custom rules ");
    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, area);
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

fn draw_resign_confirm_overlay(frame: &mut Frame, area: Rect) {
    let w = 52u16;
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
        .title(" Resign? 投降? ")
        .border_style(TuiStyle::default().fg(Color::Red));
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Concede the game to your opponent?",
            TuiStyle::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  Press y to resign, anything else to keep playing.",
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
    "s               resign (投降) — concede to opponent",
    "n               new game (back to picker)",
    "g               toggle captured-pieces sort (time / rank)",
    "H               toggle move-history panel (sidebar)",
    "D               toggle AI debug panel (vs-AI only)",
    ", / .           AI debug: prev / next scored move (PV on board)",
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
