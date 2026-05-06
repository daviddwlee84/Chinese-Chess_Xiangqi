//! Keyboard / mouse events → `Action`. Pure mapping; no state mutation.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

#[derive(Clone, Debug)]
pub enum Action {
    None,
    Quit,
    HelpToggle,
    RulesToggle,
    NewGame,
    // Picker
    PickerUp,
    PickerDown,
    PickerSelect,
    // Game
    CursorUp,
    CursorDown,
    CursorLeft,
    CursorRight,
    SelectOrCommit,
    Cancel,
    Undo,
    Flip,
    /// Mouse click in terminal coords. The app resolves it against the board
    /// rect captured by the most recent draw().
    Click {
        term_col: u16,
        term_row: u16,
    },
}

pub fn from_key(ev: KeyEvent, in_picker: bool) -> Action {
    if ev.modifiers.contains(KeyModifiers::CONTROL) && matches!(ev.code, KeyCode::Char('c')) {
        return Action::Quit;
    }
    match ev.code {
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Char('?') => Action::HelpToggle,
        KeyCode::Char('r') if !in_picker => Action::RulesToggle,
        _ if in_picker => match ev.code {
            KeyCode::Up | KeyCode::Char('k') => Action::PickerUp,
            KeyCode::Down | KeyCode::Char('j') => Action::PickerDown,
            KeyCode::Enter | KeyCode::Char(' ') => Action::PickerSelect,
            KeyCode::Esc => Action::Quit,
            _ => Action::None,
        },
        _ => match ev.code {
            KeyCode::Up | KeyCode::Char('k') => Action::CursorUp,
            KeyCode::Down | KeyCode::Char('j') => Action::CursorDown,
            KeyCode::Left | KeyCode::Char('h') => Action::CursorLeft,
            KeyCode::Right | KeyCode::Char('l') => Action::CursorRight,
            KeyCode::Enter | KeyCode::Char(' ') => Action::SelectOrCommit,
            KeyCode::Esc => Action::Cancel,
            KeyCode::Char('u') => Action::Undo,
            KeyCode::Char('f') => Action::Flip,
            KeyCode::Char('n') => Action::NewGame,
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
