//! App state + dispatch. The AppState holds a `GameState`, UI cursor, and a
//! short-lived flash message; actions from `input.rs` mutate it.

use chess_core::coord::Square;
use chess_core::moves::Move;
use chess_core::piece::Side;
use chess_core::rules::{HouseRules, RuleSet};
use chess_core::state::{GameState, GameStatus};
use chess_core::view::{PlayerView, VisibleCell};
use chess_net::{ClientMsg, ServerMsg};

use crate::glyph::Style;
use crate::input::Action;
use crate::net::{NetClient, NetEvent};
use crate::orient;

/// Variant + preset choices in the picker, in display order.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum PickerEntry {
    Xiangqi,
    XiangqiStrict,
    BanqiPurist,
    BanqiTaiwan,
    BanqiAggressive,
    Quit,
}

impl PickerEntry {
    pub const ALL: [PickerEntry; 6] = [
        PickerEntry::Xiangqi,
        PickerEntry::XiangqiStrict,
        PickerEntry::BanqiPurist,
        PickerEntry::BanqiTaiwan,
        PickerEntry::BanqiAggressive,
        PickerEntry::Quit,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PickerEntry::Xiangqi => "Xiangqi (象棋)",
            PickerEntry::XiangqiStrict => "Xiangqi (象棋, strict — must defend check)",
            PickerEntry::BanqiPurist => "Banqi (暗棋) — purist",
            PickerEntry::BanqiTaiwan => "Banqi (暗棋) — Taiwan house rules",
            PickerEntry::BanqiAggressive => "Banqi (暗棋) — aggressive house rules",
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
            PickerEntry::Quit => None,
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

pub enum Screen {
    Picker(PickerView),
    Game(Box<GameView>),
    Net(Box<NetView>),
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

    /// Drain ws events from the worker thread and apply them to the
    /// `NetView`. Called once per main-loop tick (no-op outside Net mode).
    pub fn tick_net(&mut self) {
        let Screen::Net(n) = &mut self.screen else {
            return;
        };
        while let Ok(evt) = n.client.evt_rx.try_recv() {
            apply_net_event(n, evt);
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
                let style = self.style;
                let use_color = self.use_color;
                let observer = self.observer;
                *self = AppState::new_picker(style, use_color, observer);
            }
            Action::PickerUp | Action::PickerDown | Action::PickerSelect => {
                self.dispatch_picker(action);
            }
            _ => match &self.screen {
                Screen::Net(_) => self.dispatch_net(action),
                Screen::Game(_) => self.dispatch_game(action),
                Screen::Picker(_) => {}
            },
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
            Screen::Picker(_) => false,
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
                match entry.rules() {
                    Some(rules) => {
                        let observer = self.observer;
                        let style = self.style;
                        let use_color = self.use_color;
                        *self = AppState::new_game(rules, style, use_color, observer);
                    }
                    None => self.should_quit = true,
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
                n.observer = Some(observer);
                n.rules = Some(rules);
                let shape = view.shape;
                let (rows, cols) = orient::display_dims(shape);
                n.cursor = (rows / 2, cols / 2);
                n.last_view = Some(view);
                n.connected = true;
                n.last_msg = Some(format!("Joined as {}.", side_label(observer)));
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
