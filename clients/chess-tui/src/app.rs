//! App state + dispatch. The AppState holds a `GameState`, UI cursor, and a
//! short-lived flash message; actions from `input.rs` mutate it.

use chess_core::coord::Square;
use chess_core::moves::Move;
use chess_core::piece::Side;
use chess_core::rules::{HouseRules, RuleSet};
use chess_core::state::{GameState, GameStatus};
use chess_core::view::{PlayerView, VisibleCell};
use chess_net::{ClientMsg, RoomSummary, ServerMsg};

use crate::glyph::Style;
use crate::input::{Action, InputMode};
use crate::net::{NetClient, NetEvent};
use crate::orient;
use crate::text_input;
use crate::url::{normalize_host_url, urlencode, valid_room_id};

/// Variant + preset choices in the picker, in display order.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum PickerEntry {
    Xiangqi,
    XiangqiStrict,
    BanqiPurist,
    BanqiTaiwan,
    BanqiAggressive,
    /// Open the host prompt → lobby browser → online play flow.
    ConnectToServer,
    Quit,
}

impl PickerEntry {
    pub const ALL: [PickerEntry; 7] = [
        PickerEntry::Xiangqi,
        PickerEntry::XiangqiStrict,
        PickerEntry::BanqiPurist,
        PickerEntry::BanqiTaiwan,
        PickerEntry::BanqiAggressive,
        PickerEntry::ConnectToServer,
        PickerEntry::Quit,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PickerEntry::Xiangqi => "Xiangqi (象棋)",
            PickerEntry::XiangqiStrict => "Xiangqi (象棋, strict — must defend check)",
            PickerEntry::BanqiPurist => "Banqi (暗棋) — purist",
            PickerEntry::BanqiTaiwan => "Banqi (暗棋) — Taiwan house rules",
            PickerEntry::BanqiAggressive => "Banqi (暗棋) — aggressive house rules",
            PickerEntry::ConnectToServer => "Connect to server… (online)",
            PickerEntry::Quit => "Quit",
        }
    }

    pub fn rules(self) -> Option<RuleSet> {
        match self {
            // Default xiangqi is casual: more permissive, you lose by general
            // capture. Strict (standard rules) is one row down.
            PickerEntry::Xiangqi => Some(RuleSet::xiangqi_casual()),
            PickerEntry::XiangqiStrict => Some(RuleSet::xiangqi()),
            PickerEntry::BanqiPurist => Some(RuleSet::banqi(HouseRules::empty())),
            PickerEntry::BanqiTaiwan => Some(RuleSet::banqi(chess_core::rules::PRESET_TAIWAN)),
            PickerEntry::BanqiAggressive => {
                Some(RuleSet::banqi(chess_core::rules::PRESET_AGGRESSIVE))
            }
            PickerEntry::ConnectToServer | PickerEntry::Quit => None,
        }
    }
}

pub struct PickerView {
    pub cursor: usize,
}

pub struct GameView {
    pub state: GameState,
    pub cursor: (u8, u8),
    pub selected: Option<Square>,
    pub last_msg: Option<String>,
}

pub struct NetView {
    pub client: NetClient,
    pub url: String,
    pub last_view: Option<PlayerView>,
    pub rules: Option<RuleSet>,
    /// Server-assigned side. `None` until `Hello` arrives.
    pub observer: Option<Side>,
    pub cursor: (u8, u8),
    pub selected: Option<Square>,
    pub last_msg: Option<String>,
    /// True between Connected and Disconnected events. Used by the sidebar.
    pub connected: bool,
}

/// Free-text "ws://host:port" prompt entered before the lobby.
pub struct HostPromptView {
    pub buf: String,
    pub error: Option<String>,
}

/// Live room browser. Reads from a separate `NetClient` connected to the
/// server's `/lobby` endpoint; joining a room spawns a fresh `NetClient` to
/// `/ws/<id>?password=…` and transitions to `Screen::Net`.
pub struct LobbyView {
    pub client: NetClient,
    /// Original `ws://host:port` (no path). The lobby ws is `host/lobby`;
    /// joining builds `host/ws/<id>` from the same prefix.
    pub host: String,
    pub rooms: Vec<RoomSummary>,
    pub cursor: usize,
    pub last_msg: Option<String>,
    pub connected: bool,
    /// When `Some`, the user picked a password-locked room and we're
    /// reading the password into this buffer before issuing the join.
    pub pending_join: Option<PendingJoin>,
}

pub struct PendingJoin {
    pub room_id: String,
    pub password_buf: String,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum CreateRoomField {
    Id,
    Password,
    Submit,
}

/// Form for creating a new room (server auto-creates on first join).
pub struct CreateRoomView {
    pub host: String,
    pub id_buf: String,
    pub password_buf: String,
    pub focus: CreateRoomField,
    pub error: Option<String>,
}

pub enum Screen {
    Picker(PickerView),
    Game(Box<GameView>),
    Net(Box<NetView>),
    HostPrompt(HostPromptView),
    Lobby(Box<LobbyView>),
    CreateRoom(CreateRoomView),
}

pub struct AppState {
    pub screen: Screen,
    pub style: Style,
    pub use_color: bool,
    pub observer: Side,
    pub help_open: bool,
    pub rules_open: bool,
    /// True while the y/N quit-confirm dialog is shown. Set when the user
    /// presses 'q' / Ctrl-C during an in-progress game (status `Ongoing` and
    /// at least one move played). Picker / game-over `q` skip the prompt.
    pub quit_confirm_open: bool,
    /// Rect of the board widget last drawn (terminal coords). Used for
    /// mouse-click hit-testing. ui.rs writes this each frame.
    pub board_rect: Option<RectPx>,
    pub should_quit: bool,
}

/// Minimal Rect copy so app.rs doesn't depend on ratatui types directly.
#[derive(Copy, Clone, Debug)]
pub struct RectPx {
    pub x: u16,
    pub y: u16,
    /// Width in terminal cols of one cell (glyph + padding).
    pub cell_cols: u16,
    /// Height in terminal rows of one cell (rank row + between row).
    pub cell_rows: u16,
    /// Offset (cols) of the first cell's start from rect.x.
    pub left_pad: u16,
    /// Offset (rows) of the first row from rect.y.
    pub top_pad: u16,
}

impl AppState {
    pub fn new_picker(style: Style, use_color: bool, observer: Side) -> Self {
        Self {
            screen: Screen::Picker(PickerView { cursor: 0 }),
            style,
            use_color,
            observer,
            help_open: false,
            rules_open: false,
            quit_confirm_open: false,
            board_rect: None,
            should_quit: false,
        }
    }

    pub fn new_game(rules: RuleSet, style: Style, use_color: bool, observer: Side) -> Self {
        let state = GameState::new(rules);
        let shape = state.board.shape();
        let (rows, cols) = orient::display_dims(shape);
        let cursor = (rows / 2, cols / 2);
        Self {
            screen: Screen::Game(Box::new(GameView {
                state,
                cursor,
                selected: None,
                last_msg: Some(
                    "Welcome. Arrows/hjkl move cursor. Enter selects. r=rules, ?=help, n=new, q=quit."
                        .into(),
                ),
            })),
            style,
            use_color,
            observer,
            help_open: false,
            rules_open: false,
            quit_confirm_open: false,
            board_rect: None,
            should_quit: false,
        }
    }

    pub fn new_net(url: String, style: Style, use_color: bool) -> Self {
        let client = NetClient::spawn(url.clone());
        Self {
            screen: Screen::Net(Box::new(NetView {
                client,
                url,
                last_view: None,
                rules: None,
                observer: None,
                cursor: (0, 0),
                selected: None,
                last_msg: Some("Connecting…".into()),
                connected: false,
            })),
            style,
            use_color,
            // Pre-Hello, we render as Red until the server tells us our seat.
            observer: Side::RED,
            help_open: false,
            rules_open: false,
            quit_confirm_open: false,
            board_rect: None,
            should_quit: false,
        }
    }

    /// Skip the picker and land on the host-prompt screen so the user can
    /// type a server URL. Used by the `--lobby` flag with no host argument
    /// — currently main always passes a URL, so this is the safety net for
    /// future entrypoints (e.g. picker → "Connect to server…").
    pub fn new_host_prompt(style: Style, use_color: bool, observer: Side) -> Self {
        Self {
            screen: Screen::HostPrompt(HostPromptView { buf: "ws://".into(), error: None }),
            style,
            use_color,
            observer,
            help_open: false,
            rules_open: false,
            quit_confirm_open: false,
            board_rect: None,
            should_quit: false,
        }
    }

    /// Open the lobby browser against `host` (e.g. `"ws://127.0.0.1:7878"`).
    pub fn new_lobby(host: String, style: Style, use_color: bool, observer: Side) -> Self {
        let client = NetClient::spawn(format!("{host}/lobby"));
        Self {
            screen: Screen::Lobby(Box::new(LobbyView {
                client,
                host,
                rooms: Vec::new(),
                cursor: 0,
                last_msg: Some("Connecting to lobby…".into()),
                connected: false,
                pending_join: None,
            })),
            style,
            use_color,
            observer,
            help_open: false,
            rules_open: false,
            quit_confirm_open: false,
            board_rect: None,
            should_quit: false,
        }
    }

    /// Drain ws events from the worker thread(s) and apply them to the
    /// active `NetView` / `LobbyView`. Called once per main-loop tick
    /// (no-op outside Net / Lobby modes).
    pub fn tick_net(&mut self) {
        match &mut self.screen {
            Screen::Net(n) => {
                while let Ok(evt) = n.client.evt_rx.try_recv() {
                    apply_net_event(n, evt);
                }
            }
            Screen::Lobby(l) => {
                while let Ok(evt) = l.client.evt_rx.try_recv() {
                    apply_lobby_event(l, evt);
                }
            }
            _ => {}
        }
    }

    /// Compute the input mode for the current screen so main.rs can drive
    /// `from_key` without poking at private state.
    pub fn input_mode(&self) -> InputMode {
        match &self.screen {
            Screen::Picker(_) => InputMode::Picker,
            Screen::Lobby(l) => {
                if l.pending_join.is_some() {
                    InputMode::Text
                } else {
                    InputMode::Lobby
                }
            }
            Screen::HostPrompt(_) | Screen::CreateRoom(_) => InputMode::Text,
            Screen::Game(_) | Screen::Net(_) => InputMode::Game,
        }
    }

    pub fn dispatch(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::ConfirmYes => {
                self.quit_confirm_open = false;
                self.should_quit = true;
            }
            Action::ConfirmNo => self.quit_confirm_open = false,
            Action::Quit => {
                if self.is_game_in_progress() {
                    self.quit_confirm_open = true;
                } else {
                    self.should_quit = true;
                }
            }
            Action::HelpToggle => self.help_open = !self.help_open,
            Action::RulesToggle => self.rules_open = !self.rules_open,
            Action::NewGame => {
                if matches!(self.screen, Screen::Net(_)) {
                    // In Net mode, 'n' requests a rematch via the server
                    // instead of dropping the connection. Game must be over.
                    if let Screen::Net(n) = &mut self.screen {
                        let status = n.last_view.as_ref().map(|v| v.status);
                        match status {
                            Some(GameStatus::Won { .. }) | Some(GameStatus::Drawn { .. }) => {
                                let _ = n.client.cmd_tx.send(ClientMsg::Rematch);
                                n.last_msg =
                                    Some("Rematch requested. Waiting for opponent…".into());
                            }
                            Some(GameStatus::Ongoing) => {
                                n.last_msg = Some(
                                    "'n' requests a rematch only after the game is over.".into(),
                                );
                            }
                            None => {
                                n.last_msg = Some("Not connected yet.".into());
                            }
                        }
                    }
                } else {
                    let style = self.style;
                    let use_color = self.use_color;
                    let observer = self.observer;
                    *self = AppState::new_picker(style, use_color, observer);
                }
            }
            Action::Back => self.dispatch_back(),
            Action::PickerUp | Action::PickerDown | Action::PickerSelect => match &self.screen {
                Screen::Picker(_) => self.dispatch_picker(action),
                Screen::Lobby(_) => self.dispatch_lobby(action),
                _ => {}
            },
            Action::LobbyCreate | Action::LobbyRefresh => {
                if matches!(self.screen, Screen::Lobby(_)) {
                    self.dispatch_lobby(action);
                }
            }
            Action::TextInput(_)
            | Action::TextBackspace
            | Action::FocusNext
            | Action::FocusPrev
            | Action::Submit => self.dispatch_text(action),
            _ => match &self.screen {
                Screen::Net(_) => self.dispatch_net(action),
                Screen::Game(_) => self.dispatch_game(action),
                Screen::Picker(_)
                | Screen::HostPrompt(_)
                | Screen::Lobby(_)
                | Screen::CreateRoom(_) => {}
            },
        }
    }

    fn dispatch_back(&mut self) {
        let style = self.style;
        let use_color = self.use_color;
        let observer = self.observer;
        match &mut self.screen {
            Screen::HostPrompt(_) => {
                *self = AppState::new_picker(style, use_color, observer);
            }
            Screen::Lobby(l) => {
                if l.pending_join.is_some() {
                    l.pending_join = None;
                    l.last_msg = None;
                    return;
                }
                *self = AppState::new_picker(style, use_color, observer);
            }
            Screen::CreateRoom(c) => {
                let host = c.host.clone();
                *self = AppState::new_lobby(host, style, use_color, observer);
            }
            _ => {}
        }
    }

    fn is_game_in_progress(&self) -> bool {
        match &self.screen {
            Screen::Game(g) => {
                matches!(g.state.status, GameStatus::Ongoing) && !g.state.history.is_empty()
            }
            Screen::Net(n) => match &n.last_view {
                Some(view) => matches!(view.status, GameStatus::Ongoing) && n.connected,
                None => false,
            },
            Screen::Picker(_)
            | Screen::HostPrompt(_)
            | Screen::Lobby(_)
            | Screen::CreateRoom(_) => false,
        }
    }

    fn dispatch_picker(&mut self, action: Action) {
        let Screen::Picker(p) = &mut self.screen else {
            return;
        };
        let n = PickerEntry::ALL.len();
        match action {
            Action::PickerUp => p.cursor = (p.cursor + n - 1) % n,
            Action::PickerDown => p.cursor = (p.cursor + 1) % n,
            Action::PickerSelect => {
                let entry = PickerEntry::ALL[p.cursor];
                let observer = self.observer;
                let style = self.style;
                let use_color = self.use_color;
                match entry {
                    PickerEntry::ConnectToServer => {
                        *self = AppState::new_host_prompt(style, use_color, observer);
                    }
                    PickerEntry::Quit => self.should_quit = true,
                    other => {
                        if let Some(rules) = other.rules() {
                            *self = AppState::new_game(rules, style, use_color, observer);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn dispatch_lobby(&mut self, action: Action) {
        let Screen::Lobby(l) = &mut self.screen else {
            return;
        };
        // Pending password prompt eats list-cursor inputs.
        if l.pending_join.is_some() {
            return;
        }
        match action {
            Action::PickerUp => {
                if !l.rooms.is_empty() {
                    l.cursor = (l.cursor + l.rooms.len() - 1) % l.rooms.len();
                }
            }
            Action::PickerDown => {
                if !l.rooms.is_empty() {
                    l.cursor = (l.cursor + 1) % l.rooms.len();
                }
            }
            Action::PickerSelect => {
                if l.rooms.is_empty() {
                    l.last_msg =
                        Some("No rooms yet. Press 'c' to create one, or 'r' to refresh.".into());
                    return;
                }
                let cursor = l.cursor.min(l.rooms.len() - 1);
                let room = l.rooms[cursor].clone();
                if room.seats >= 2 {
                    l.last_msg = Some(format!("Room '{}' is full (2/2).", room.id));
                    return;
                }
                if room.has_password {
                    l.pending_join =
                        Some(PendingJoin { room_id: room.id, password_buf: String::new() });
                    l.last_msg = Some("Type password, Enter to join, Esc to cancel.".into());
                    return;
                }
                let host = l.host.clone();
                let style = self.style;
                let use_color = self.use_color;
                *self = AppState::new_net(format!("{host}/ws/{}", room.id), style, use_color);
            }
            Action::LobbyCreate => {
                let host = l.host.clone();
                self.screen = Screen::CreateRoom(CreateRoomView {
                    host,
                    id_buf: String::new(),
                    password_buf: String::new(),
                    focus: CreateRoomField::Id,
                    error: None,
                });
            }
            Action::LobbyRefresh => {
                let _ = l.client.cmd_tx.send(ClientMsg::ListRooms);
                l.last_msg = Some("Refresh requested.".into());
            }
            _ => {}
        }
    }

    fn dispatch_text(&mut self, action: Action) {
        match &mut self.screen {
            Screen::HostPrompt(h) => match action {
                Action::TextInput(c) => text_input::push_char(&mut h.buf, c, 128),
                Action::TextBackspace => text_input::backspace(&mut h.buf),
                Action::Submit => {
                    let raw = h.buf.trim().to_string();
                    let host = match normalize_host_url(&raw) {
                        Ok(u) => u,
                        Err(e) => {
                            h.error = Some(e);
                            return;
                        }
                    };
                    let style = self.style;
                    let use_color = self.use_color;
                    let observer = self.observer;
                    *self = AppState::new_lobby(host, style, use_color, observer);
                }
                _ => {}
            },
            Screen::CreateRoom(c) => match action {
                Action::TextInput(ch) => match c.focus {
                    CreateRoomField::Id => text_input::push_char(&mut c.id_buf, ch, 32),
                    CreateRoomField::Password => text_input::push_char(&mut c.password_buf, ch, 64),
                    CreateRoomField::Submit => {
                        if matches!(ch, ' ' | '\n') {
                            self.dispatch_text(Action::Submit);
                        }
                    }
                },
                Action::TextBackspace => match c.focus {
                    CreateRoomField::Id => text_input::backspace(&mut c.id_buf),
                    CreateRoomField::Password => text_input::backspace(&mut c.password_buf),
                    _ => {}
                },
                Action::FocusNext => {
                    c.focus = match c.focus {
                        CreateRoomField::Id => CreateRoomField::Password,
                        CreateRoomField::Password => CreateRoomField::Submit,
                        CreateRoomField::Submit => CreateRoomField::Id,
                    };
                }
                Action::FocusPrev => {
                    c.focus = match c.focus {
                        CreateRoomField::Id => CreateRoomField::Submit,
                        CreateRoomField::Password => CreateRoomField::Id,
                        CreateRoomField::Submit => CreateRoomField::Password,
                    };
                }
                Action::Submit => {
                    let id = c.id_buf.trim().to_string();
                    if !valid_room_id(&id) {
                        c.error = Some("Room id must be 1–32 chars of [a-zA-Z0-9_-].".into());
                        return;
                    }
                    let host = c.host.clone();
                    let password =
                        if c.password_buf.is_empty() { None } else { Some(c.password_buf.clone()) };
                    let url = match password {
                        Some(pw) => format!("{host}/ws/{id}?password={}", urlencode(&pw)),
                        None => format!("{host}/ws/{id}"),
                    };
                    let style = self.style;
                    let use_color = self.use_color;
                    *self = AppState::new_net(url, style, use_color);
                }
                _ => {}
            },
            Screen::Lobby(l) => {
                let Some(pj) = l.pending_join.as_mut() else {
                    return;
                };
                match action {
                    Action::TextInput(c) => text_input::push_char(&mut pj.password_buf, c, 64),
                    Action::TextBackspace => text_input::backspace(&mut pj.password_buf),
                    Action::Submit => {
                        let host = l.host.clone();
                        let pj_owned = l.pending_join.take().unwrap();
                        let url = format!(
                            "{host}/ws/{}?password={}",
                            pj_owned.room_id,
                            urlencode(&pj_owned.password_buf)
                        );
                        let style = self.style;
                        let use_color = self.use_color;
                        *self = AppState::new_net(url, style, use_color);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn dispatch_game(&mut self, action: Action) {
        let Screen::Game(g) = &mut self.screen else {
            return;
        };
        let shape = g.state.board.shape();
        let (rows, cols) = orient::display_dims(shape);
        match action {
            Action::CursorUp => {
                if g.cursor.0 > 0 {
                    g.cursor.0 -= 1;
                }
            }
            Action::CursorDown => {
                if g.cursor.0 + 1 < rows {
                    g.cursor.0 += 1;
                }
            }
            Action::CursorLeft => {
                if g.cursor.1 > 0 {
                    g.cursor.1 -= 1;
                }
            }
            Action::CursorRight => {
                if g.cursor.1 + 1 < cols {
                    g.cursor.1 += 1;
                }
            }
            Action::Cancel => {
                g.selected = None;
                g.last_msg = None;
            }
            Action::SelectOrCommit => {
                let observer = self.observer;
                Self::handle_select_or_commit(g, observer);
            }
            Action::Undo => match g.state.unmake_move() {
                Ok(()) => {
                    g.state.refresh_status();
                    g.selected = None;
                    g.last_msg = Some(format!("Undone. {:?} to move.", g.state.side_to_move));
                }
                Err(e) => {
                    g.last_msg = Some(format!("Cannot undo: {e}"));
                }
            },
            Action::Flip => {
                if !matches!(g.state.status, chess_core::state::GameStatus::Ongoing) {
                    g.last_msg = Some(
                        "Game over. Press 'n' for new game, 'u' to take back, 'q' to quit.".into(),
                    );
                    return;
                }
                let observer = self.observer;
                let Some(sq) = orient::square_at_display(g.cursor.0, g.cursor.1, observer, shape)
                else {
                    g.last_msg = Some("Cursor not on a playable square.".into());
                    return;
                };
                let m = Move::Reveal { at: sq, revealed: None };
                Self::apply_move(g, m);
            }
            Action::Click { term_col, term_row } => {
                if let Some(rect) = self.board_rect {
                    let observer = self.observer;
                    if let Some((row, col)) = hit_test(rect, term_col, term_row, rows, cols) {
                        g.cursor = (row, col);
                        Self::handle_select_or_commit(g, observer);
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_select_or_commit(g: &mut GameView, observer: Side) {
        if !matches!(g.state.status, chess_core::state::GameStatus::Ongoing) {
            g.last_msg =
                Some("Game over. Press 'n' for new game, 'u' to take back, 'q' to quit.".into());
            return;
        }
        let shape = g.state.board.shape();
        let Some(sq) = orient::square_at_display(g.cursor.0, g.cursor.1, observer, shape) else {
            g.last_msg = Some("Cursor not on a playable square.".into());
            return;
        };

        let view = PlayerView::project(&g.state, g.state.side_to_move);

        match g.selected {
            None => {
                // Try to select a piece. Allowed if there's a legal move from this square.
                let any = view.legal_moves.iter().any(|m| m.origin_square() == sq);
                if any {
                    g.selected = Some(sq);
                    g.last_msg = None;
                } else {
                    // Maybe it's a banqi hidden cell — suggest 'f' to flip.
                    match g.state.board.get(sq) {
                        Some(p) if !p.revealed => {
                            g.last_msg = Some("Hidden piece. Press 'f' to flip.".into());
                        }
                        _ => g.last_msg = Some("No legal move from that square.".into()),
                    }
                }
            }
            Some(from) if from == sq => {
                g.selected = None;
                g.last_msg = None;
            }
            Some(from) => {
                // Find a legal move from `from` to `sq`. Prefer Capture / CannonJump
                // over a chain ending at sq (for banqi); take the first match.
                let candidate = view
                    .legal_moves
                    .iter()
                    .find(|m| m.origin_square() == from && m.to_square() == Some(sq))
                    .cloned();
                match candidate {
                    Some(m) => Self::apply_move(g, m),
                    None => {
                        g.last_msg = Some("Illegal move.".into());
                        g.selected = None;
                    }
                }
            }
        }
    }

    fn apply_move(g: &mut GameView, m: Move) {
        match g.state.make_move(&m) {
            Ok(()) => {
                g.state.refresh_status();
                g.selected = None;
                g.last_msg = Some(format!(
                    "Played: {}",
                    chess_core::notation::iccs::encode_move(&g.state.board, &m)
                ));
            }
            Err(e) => {
                g.last_msg = Some(format!("Engine rejected move: {e}"));
                g.selected = None;
            }
        }
    }

    fn dispatch_net(&mut self, action: Action) {
        let Screen::Net(n) = &mut self.screen else {
            return;
        };
        // Pre-Hello: no view yet, nothing to dispatch on.
        let Some(view) = n.last_view.as_ref() else {
            return;
        };
        let shape = view.shape;
        let (rows, cols) = orient::display_dims(shape);
        let observer = n.observer.unwrap_or(Side::RED);
        match action {
            Action::CursorUp => {
                if n.cursor.0 > 0 {
                    n.cursor.0 -= 1;
                }
            }
            Action::CursorDown => {
                if n.cursor.0 + 1 < rows {
                    n.cursor.0 += 1;
                }
            }
            Action::CursorLeft => {
                if n.cursor.1 > 0 {
                    n.cursor.1 -= 1;
                }
            }
            Action::CursorRight => {
                if n.cursor.1 + 1 < cols {
                    n.cursor.1 += 1;
                }
            }
            Action::Cancel => {
                n.selected = None;
                n.last_msg = None;
            }
            Action::SelectOrCommit => {
                let outcome = compute_select_outcome(n, observer);
                apply_select_outcome(n, outcome);
            }
            Action::Flip => {
                if !matches!(view.status, GameStatus::Ongoing) {
                    n.last_msg = Some("Game over.".into());
                    return;
                }
                if view.side_to_move != observer {
                    n.last_msg = Some("Not your turn.".into());
                    return;
                }
                let Some(sq) = orient::square_at_display(n.cursor.0, n.cursor.1, observer, shape)
                else {
                    n.last_msg = Some("Cursor not on a playable square.".into());
                    return;
                };
                let _ = n
                    .client
                    .cmd_tx
                    .send(ClientMsg::Move { mv: Move::Reveal { at: sq, revealed: None } });
                n.last_msg = Some("Reveal sent.".into());
            }
            Action::Undo => {
                n.last_msg = Some("Undo not supported in online mode yet.".into());
            }
            Action::Click { term_col, term_row } => {
                if let Some(rect) = self.board_rect {
                    if let Some((row, col)) = hit_test(rect, term_col, term_row, rows, cols) {
                        let n = match &mut self.screen {
                            Screen::Net(b) => b,
                            _ => return,
                        };
                        n.cursor = (row, col);
                        let outcome = compute_select_outcome(n, observer);
                        apply_select_outcome(n, outcome);
                    }
                }
            }
            _ => {}
        }
    }
}

enum SelectOutcome {
    Ignore,
    Msg(String),
    ClearAndMsg(String),
    Select(Square),
    Deselect,
    Commit(Move),
}

fn compute_select_outcome(n: &NetView, observer: Side) -> SelectOutcome {
    let Some(view) = n.last_view.as_ref() else {
        return SelectOutcome::Ignore;
    };
    if !matches!(view.status, GameStatus::Ongoing) {
        return SelectOutcome::Msg("Game over.".into());
    }
    let shape = view.shape;
    let Some(sq) = orient::square_at_display(n.cursor.0, n.cursor.1, observer, shape) else {
        return SelectOutcome::Msg("Cursor not on a playable square.".into());
    };
    if view.side_to_move != observer {
        return SelectOutcome::Msg("Not your turn.".into());
    }
    match n.selected {
        None => {
            if view.legal_moves.iter().any(|m| m.origin_square() == sq) {
                SelectOutcome::Select(sq)
            } else if matches!(view.cells[sq.0 as usize], VisibleCell::Hidden) {
                SelectOutcome::Msg("Hidden piece. Press 'f' to flip.".into())
            } else {
                SelectOutcome::Msg("No legal move from that square.".into())
            }
        }
        Some(from) if from == sq => SelectOutcome::Deselect,
        Some(from) => {
            let candidate = view
                .legal_moves
                .iter()
                .find(|m| m.origin_square() == from && m.to_square() == Some(sq))
                .cloned();
            match candidate {
                Some(mv) => SelectOutcome::Commit(mv),
                None => SelectOutcome::ClearAndMsg("Illegal move.".into()),
            }
        }
    }
}

fn apply_select_outcome(n: &mut NetView, outcome: SelectOutcome) {
    match outcome {
        SelectOutcome::Ignore => {}
        SelectOutcome::Msg(m) => n.last_msg = Some(m),
        SelectOutcome::ClearAndMsg(m) => {
            n.selected = None;
            n.last_msg = Some(m);
        }
        SelectOutcome::Select(sq) => {
            n.selected = Some(sq);
            n.last_msg = None;
        }
        SelectOutcome::Deselect => {
            n.selected = None;
            n.last_msg = None;
        }
        SelectOutcome::Commit(mv) => {
            let _ = n.client.cmd_tx.send(ClientMsg::Move { mv });
            n.selected = None;
            n.last_msg = Some("Sent.".into());
        }
    }
}

fn apply_net_event(n: &mut NetView, evt: NetEvent) {
    match evt {
        NetEvent::Connected => {
            n.connected = true;
            n.last_msg = Some("Connected. Waiting for hello…".into());
        }
        NetEvent::Server(boxed) => match *boxed {
            ServerMsg::Hello { observer, rules, view, .. } => {
                let was_seated = n.observer.is_some();
                n.observer = Some(observer);
                n.rules = Some(rules);
                let shape = view.shape;
                let (rows, cols) = orient::display_dims(shape);
                n.cursor = (rows / 2, cols / 2);
                n.selected = None;
                n.last_view = Some(view);
                n.connected = true;
                // First Hello = "Joined as X". Subsequent Hello = rematch reset.
                n.last_msg = Some(if was_seated {
                    "Rematch — new game.".into()
                } else {
                    format!("Joined as {}.", side_label(observer))
                });
            }
            ServerMsg::Update { view } => {
                n.last_view = Some(view);
                // Don't clobber a freshly-set "Sent." — but stale msgs should clear.
                if n.last_msg.as_deref() == Some("Sent.") {
                    n.last_msg = None;
                }
            }
            ServerMsg::Error { message } => {
                n.last_msg = Some(message);
            }
            ServerMsg::Rooms { .. } => {
                // Game socket should never receive Rooms — surface so a
                // server bug is debuggable rather than silently dropped.
                n.last_msg = Some("(unexpected lobby payload on game socket)".into());
            }
        },
        NetEvent::Disconnected(reason) => {
            n.connected = false;
            n.last_msg = Some(format!("Disconnected: {reason}"));
        }
    }
}

fn side_label(side: Side) -> &'static str {
    if side == Side::RED {
        "Red 紅"
    } else if side == Side::BLACK {
        "Black 黑"
    } else {
        "Green 綠"
    }
}

fn apply_lobby_event(l: &mut LobbyView, evt: NetEvent) {
    match evt {
        NetEvent::Connected => {
            l.connected = true;
            l.last_msg = Some("Lobby connected.".into());
        }
        NetEvent::Server(boxed) => match *boxed {
            ServerMsg::Rooms { rooms } => {
                let prev_id = l.rooms.get(l.cursor).map(|r| r.id.clone());
                l.rooms = rooms;
                l.rooms.sort_by(|a, b| a.id.cmp(&b.id));
                // Try to keep the cursor on the same room id; otherwise clamp.
                if let Some(id) = prev_id {
                    if let Some(idx) = l.rooms.iter().position(|r| r.id == id) {
                        l.cursor = idx;
                    } else if l.cursor >= l.rooms.len() && !l.rooms.is_empty() {
                        l.cursor = l.rooms.len() - 1;
                    } else if l.rooms.is_empty() {
                        l.cursor = 0;
                    }
                } else if l.cursor >= l.rooms.len() && !l.rooms.is_empty() {
                    l.cursor = l.rooms.len() - 1;
                }
            }
            ServerMsg::Error { message } => {
                l.last_msg = Some(message);
            }
            ServerMsg::Hello { .. } | ServerMsg::Update { .. } => {
                // Game-socket payloads should never arrive on a lobby ws.
                // If they do (server bug), surface for debugging.
                l.last_msg = Some("(unexpected game payload on lobby socket)".into());
            }
        },
        NetEvent::Disconnected(reason) => {
            l.connected = false;
            l.last_msg = Some(format!("Lobby disconnected: {reason}"));
        }
    }
}

/// Convert terminal click coords to (display_row, display_col) within board.
///
/// Cells are `cell_rows × cell_cols` terminal cells. With the intersection
/// layout, `cell_rows = 2` (rank row + between row) so clicks on either of
/// those rows resolve to the same display cell — including the river row,
/// which simply replaces the between-row at index 4 without changing the
/// row layout.
fn hit_test(rect: RectPx, term_col: u16, term_row: u16, rows: u8, cols: u8) -> Option<(u8, u8)> {
    if term_col < rect.x + rect.left_pad || term_row < rect.y + rect.top_pad {
        return None;
    }
    let col_off = term_col - rect.x - rect.left_pad;
    let row_off = term_row - rect.y - rect.top_pad;
    if rect.cell_cols == 0 || rect.cell_rows == 0 {
        return None;
    }
    let cell_col = col_off / rect.cell_cols;
    let cell_row = row_off / rect.cell_rows;
    if cell_row >= rows as u16 || cell_col >= cols as u16 {
        return None;
    }
    Some((cell_row as u8, cell_col as u8))
}
