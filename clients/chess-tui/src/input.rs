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
            KeyCode::Char('t') => Action::ChatStart,
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
