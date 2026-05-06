//! App state + dispatch. The AppState holds a `GameState`, UI cursor, and a
//! short-lived flash message; actions from `input.rs` mutate it.

use chess_core::coord::Square;
use chess_core::moves::Move;
use chess_core::piece::Side;
use chess_core::rules::{HouseRules, RuleSet};
use chess_core::state::GameState;
use chess_core::view::PlayerView;

use crate::glyph::Style;
use crate::input::Action;
use crate::orient;

/// Variant + preset choices in the picker, in display order.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum PickerEntry {
    Xiangqi,
    XiangqiCasual,
    BanqiPurist,
    BanqiTaiwan,
    BanqiAggressive,
    Quit,
}

impl PickerEntry {
    pub const ALL: [PickerEntry; 6] = [
        PickerEntry::Xiangqi,
        PickerEntry::XiangqiCasual,
        PickerEntry::BanqiPurist,
        PickerEntry::BanqiTaiwan,
        PickerEntry::BanqiAggressive,
        PickerEntry::Quit,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PickerEntry::Xiangqi => "Xiangqi (象棋)",
            PickerEntry::XiangqiCasual => "Xiangqi (casual — allow self-check, lose by capture)",
            PickerEntry::BanqiPurist => "Banqi (暗棋) — purist",
            PickerEntry::BanqiTaiwan => "Banqi (暗棋) — Taiwan house rules",
            PickerEntry::BanqiAggressive => "Banqi (暗棋) — aggressive house rules",
            PickerEntry::Quit => "Quit",
        }
    }

    pub fn rules(self) -> Option<RuleSet> {
        match self {
            PickerEntry::Xiangqi => Some(RuleSet::xiangqi()),
            PickerEntry::XiangqiCasual => Some(RuleSet::xiangqi_casual()),
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

pub enum Screen {
    Picker(PickerView),
    Game(Box<GameView>),
}

pub struct AppState {
    pub screen: Screen,
    pub style: Style,
    pub use_color: bool,
    pub observer: Side,
    pub help_open: bool,
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
                    "Welcome. Cursor: arrows or hjkl. Enter to select/move. ? for help.".into(),
                ),
            })),
            style,
            use_color,
            observer,
            help_open: false,
            board_rect: None,
            should_quit: false,
        }
    }

    pub fn dispatch(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::Quit => self.should_quit = true,
            Action::HelpToggle => self.help_open = !self.help_open,
            Action::PickerUp | Action::PickerDown | Action::PickerSelect => {
                self.dispatch_picker(action);
            }
            _ => self.dispatch_game(action),
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
}

/// Convert terminal click coords to (display_row, display_col) within board.
fn hit_test(rect: RectPx, term_col: u16, term_row: u16, rows: u8, cols: u8) -> Option<(u8, u8)> {
    if term_col < rect.x + rect.left_pad || term_row < rect.y + rect.top_pad {
        return None;
    }
    let col_off = term_col - rect.x - rect.left_pad;
    let row_off = term_row - rect.y - rect.top_pad;
    if rect.cell_cols == 0 {
        return None;
    }
    let cell_col = col_off / rect.cell_cols;
    let cell_row = row_off;
    if cell_row >= rows as u16 || cell_col >= cols as u16 {
        return None;
    }
    Some((cell_row as u8, cell_col as u8))
}
