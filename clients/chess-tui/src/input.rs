//! Keyboard / mouse events → `Action`. Pure mapping; no state mutation.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

/// Where the user's focus is right now. Drives the keymap branch in
/// [`from_key`]. `Game` covers both local hot-seat (`Screen::Game`) and
/// online (`Screen::Net`) play.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum InputMode {
    Picker,
    Lobby,
    Text,
    Game,
    /// Tree-style "Custom rules…" sub-screen: cursor + Space-toggle, plus
    /// digit-only text input on the Seed row.
    CustomRules,
}

/// Flavor of the coordinate-input prompt opened from `Game` mode.
/// `Instant` = pure text buffer, no board feedback.
/// `Live` = each keystroke re-parses; valid square prefixes update the
/// `selected` highlight, and a complete `from+to` move also moves the
/// cursor onto the destination square as a preview.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum CoordKind {
    Instant,
    Live,
}

#[derive(Clone, Debug)]
pub enum Action {
    None,
    Quit,
    /// Confirm an open quit-confirm dialog (user pressed y/Y).
    ConfirmYes,
    /// Cancel an open quit-confirm dialog (user pressed anything else).
    ConfirmNo,
    HelpToggle,
    RulesToggle,
    NewGame,
    /// "Back one screen" — Esc semantics outside game/picker. The
    /// dispatcher decides what to back into per screen.
    Back,
    /// Toggle the captured-pieces panel sort order (Time ↔ Rank).
    /// Bound to `g` ("graveyard") in `Game` mode; Picker / Lobby ignore.
    CapturedSortToggle,
    // Picker / Lobby (list-cursor)
    PickerUp,
    PickerDown,
    PickerSelect,
    // Lobby
    LobbyCreate,
    LobbyRefresh,
    /// Spectate the highlighted room (`?role=spectator`).
    LobbyWatch,
    // Text input (HostPrompt / CreateRoom)
    TextInput(char),
    TextBackspace,
    FocusNext,
    FocusPrev,
    /// Submit a text-input form (Enter while in `Text` mode).
    Submit,
    // Game
    CursorUp,
    CursorDown,
    CursorLeft,
    CursorRight,
    SelectOrCommit,
    Cancel,
    Undo,
    Flip,
    /// Open the chat input editor (Net mode, players only).
    ChatStart,
    /// Open the coordinate-input prompt (Game / Net, players only). The
    /// kind picks instant-vs-live preview behaviour.
    CoordStart(CoordKind),
    /// Mouse click in terminal coords. The app resolves it against the board
    /// rect captured by the most recent draw().
    Click {
        term_col: u16,
        term_row: u16,
    },
}

pub fn from_key(ev: KeyEvent, mode: InputMode, quit_confirm_open: bool) -> Action {
    // Quit-confirm dialog hijacks input: y/Y confirms, anything else cancels.
    if quit_confirm_open {
        return match ev.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => Action::ConfirmYes,
            _ => Action::ConfirmNo,
        };
    }
    if ev.modifiers.contains(KeyModifiers::CONTROL) && matches!(ev.code, KeyCode::Char('c')) {
        return Action::Quit;
    }
    match mode {
        InputMode::Picker => match ev.code {
            KeyCode::Up | KeyCode::Char('k') => Action::PickerUp,
            KeyCode::Down | KeyCode::Char('j') => Action::PickerDown,
            KeyCode::Enter | KeyCode::Char(' ') => Action::PickerSelect,
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Char('?') => Action::HelpToggle,
            KeyCode::Esc => Action::Quit,
            _ => Action::None,
        },
        InputMode::Lobby => match ev.code {
            KeyCode::Up | KeyCode::Char('k') => Action::PickerUp,
            KeyCode::Down | KeyCode::Char('j') => Action::PickerDown,
            KeyCode::Enter | KeyCode::Char(' ') => Action::PickerSelect,
            KeyCode::Char('c') => Action::LobbyCreate,
            KeyCode::Char('r') => Action::LobbyRefresh,
            KeyCode::Char('w') => Action::LobbyWatch,
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Char('?') => Action::HelpToggle,
            KeyCode::Esc => Action::Back,
            _ => Action::None,
        },
        InputMode::Text => match ev.code {
            KeyCode::Esc => Action::Back,
            KeyCode::Enter => Action::Submit,
            KeyCode::Backspace => Action::TextBackspace,
            KeyCode::Tab => Action::FocusNext,
            KeyCode::BackTab => Action::FocusPrev,
            KeyCode::Char(c) => Action::TextInput(c),
            _ => Action::None,
        },
        InputMode::CustomRules => match ev.code {
            KeyCode::Up | KeyCode::Char('k') => Action::PickerUp,
            KeyCode::Down | KeyCode::Char('j') => Action::PickerDown,
            KeyCode::Enter | KeyCode::Char(' ') => Action::PickerSelect,
            KeyCode::Backspace => Action::TextBackspace,
            KeyCode::Char(c) if c.is_ascii_digit() => Action::TextInput(c),
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Char('?') => Action::HelpToggle,
            KeyCode::Esc => Action::Back,
            _ => Action::None,
        },
        InputMode::Game => match ev.code {
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Char('?') => Action::HelpToggle,
            KeyCode::Char('r') => Action::RulesToggle,
            KeyCode::Up | KeyCode::Char('k') => Action::CursorUp,
            KeyCode::Down | KeyCode::Char('j') => Action::CursorDown,
            KeyCode::Left | KeyCode::Char('h') => Action::CursorLeft,
            KeyCode::Right | KeyCode::Char('l') => Action::CursorRight,
            KeyCode::Enter | KeyCode::Char(' ') => Action::SelectOrCommit,
            KeyCode::Esc => Action::Cancel,
            KeyCode::Char('u') => Action::Undo,
            KeyCode::Char('f') => Action::Flip,
            KeyCode::Char('n') => Action::NewGame,
            KeyCode::Char('g') => Action::CapturedSortToggle,
            KeyCode::Char('t') => Action::ChatStart,
            KeyCode::Char(':') => Action::CoordStart(CoordKind::Instant),
            KeyCode::Char('m') => Action::CoordStart(CoordKind::Live),
            _ => Action::None,
        },
    }
}

pub fn from_mouse(ev: MouseEvent) -> Action {
    match ev.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            Action::Click { term_col: ev.column, term_row: ev.row }
        }
        _ => Action::None,
    }
}
