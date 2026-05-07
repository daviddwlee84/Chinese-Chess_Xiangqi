//! Hand-rolled block-letter ASCII art for end-of-game and check banners.
//!
//! No `figlet-rs` dep — the strings rendered are fixed (VICTORY, DEFEAT,
//! DRAW, CHECK, 將軍, the app title), so embedding the rasterised rows
//! is cheap and keeps the dependency graph clean. CJK style uses ░▒▓
//! block fills; ASCII style uses `#` so monochrome terminals still read.
//!
//! Each `&'static [&'static str]` is the rasterised letters as one row
//! per `&'static str`. Renderers join with `'\n'` and centre.
//!
//! Width characters are ASCII only (no CJK) so each `char` is one terminal
//! cell — `width()` is `s.chars().count()`.
//!
//! See `confetti.rs` for the matching particle effect and
//! `ui::draw_game_over_overlay` for the renderer that consumes both.
//!
//! IMPORTANT: keep all rows of a given variant the same width so that
//! centering is trivial; the renderer trusts row 0's width.

use crate::glyph::Style;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum BannerKind {
    /// Player won (CJK: 勝利, ASCII: VICTORY).
    Victory,
    /// Player lost (DEFEAT / 敗北).
    Defeat,
    /// Game ended in a draw (DRAW / 和棋).
    Draw,
    /// Spectator-style neutral outcome — caller passes the winner side.
    Outcome(NeutralSide),
    /// chess-tui picker title.
    AppTitle,
}

/// Spectator banner uses neutral wording ("RED WINS", not "VICTORY").
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum NeutralSide {
    Red,
    Black,
    Green,
}

pub fn art(kind: BannerKind, style: Style) -> &'static [&'static str] {
    match (kind, style) {
        (BannerKind::Victory, Style::Cjk) => VICTORY_BLOCK,
        (BannerKind::Victory, Style::Ascii) => VICTORY_ASCII,
        (BannerKind::Defeat, Style::Cjk) => DEFEAT_BLOCK,
        (BannerKind::Defeat, Style::Ascii) => DEFEAT_ASCII,
        (BannerKind::Draw, Style::Cjk) => DRAW_BLOCK,
        (BannerKind::Draw, Style::Ascii) => DRAW_ASCII,
        (BannerKind::Outcome(NeutralSide::Red), Style::Cjk) => RED_WINS_BLOCK,
        (BannerKind::Outcome(NeutralSide::Red), Style::Ascii) => RED_WINS_ASCII,
        (BannerKind::Outcome(NeutralSide::Black), Style::Cjk) => BLACK_WINS_BLOCK,
        (BannerKind::Outcome(NeutralSide::Black), Style::Ascii) => BLACK_WINS_ASCII,
        (BannerKind::Outcome(NeutralSide::Green), Style::Cjk) => GREEN_WINS_BLOCK,
        (BannerKind::Outcome(NeutralSide::Green), Style::Ascii) => GREEN_WINS_ASCII,
        (BannerKind::AppTitle, Style::Cjk) => APP_TITLE_BLOCK,
        (BannerKind::AppTitle, Style::Ascii) => APP_TITLE_ASCII,
    }
}

/// Maximum row width for a banner — useful for layout fits-check before
/// committing to the overlay (in narrow terminals we skip the overlay).
pub fn max_width(rows: &[&str]) -> usize {
    rows.iter().map(|r| r.chars().count()).max().unwrap_or(0)
}

// ---- CJK / block-fill style (use ░▒▓ shading) ---------------------------

const VICTORY_BLOCK: &[&str] = &[
    "█    █  ████  ████  █████  ████  █████ █   █",
    "█    █    █  █        █   █    █ █   █  █ █ ",
    " █  █     █  █        █   █    █ █████   █  ",
    "  ██      █  █        █   █    █ █  █    █  ",
    "  █     ████  ████    █    ████  █   █   █  ",
];

const DEFEAT_BLOCK: &[&str] = &[
    "████   █████ █████ █████  █████ █████",
    "█   █  █     █     █      █   █   █  ",
    "█   █  ████  ████  ████   █████   █  ",
    "█   █  █     █     █      █   █   █  ",
    "████   █████ █     █████  █   █   █  ",
];

const DRAW_BLOCK: &[&str] = &[
    "████   ████   █████ █     █",
    "█   █  █   █  █     █     █",
    "█   █  ████   ████  █  █  █",
    "█   █  █  █   █     █ █ █ █",
    "████   █   █  █████  █   █ ",
];

const RED_WINS_BLOCK: &[&str] = &[
    "████   █████ ████      █   █ █████ █   █  ████",
    "█   █  █     █   █     █   █   █   ██  █ █    ",
    "████   ████  █   █     █ █ █   █   █ █ █  ███ ",
    "█  █   █     █   █     █ █ █   █   █  ██     █",
    "█   █  █████ ████       █ █  █████ █   █ ████ ",
];

const BLACK_WINS_BLOCK: &[&str] = &[
    "████   █     █████  ████ █   █     █   █ █████ █   █  ████",
    "█   █  █     █   █ █     █  █      █   █   █   ██  █ █    ",
    "████   █     █████ █     ███       █ █ █   █   █ █ █  ███ ",
    "█   █  █     █   █ █     █  █      █ █ █   █   █  ██     █",
    "████   █████ █   █  ████ █   █      █ █  █████ █   █ ████ ",
];

const GREEN_WINS_BLOCK: &[&str] = &[
    " ████  ████   █████ █████ █   █     █   █ █████ █   █  ████",
    "█      █   █  █     █     ██  █     █   █   █   ██  █ █    ",
    "█  ██  ████   ████  ████  █ █ █     █ █ █   █   █ █ █  ███ ",
    "█   █  █  █   █     █     █  ██     █ █ █   █   █  ██     █",
    " ████  █   █  █████ █████ █   █      █ █  █████ █   █ ████ ",
];

const APP_TITLE_BLOCK: &[&str] = &[
    " ████ █   █ █████  ████  ████      █████ █   █ █████",
    "█     █   █ █     █     █            █   █   █   █  ",
    "█     █████ ████   ███   ███         █   █   █   █  ",
    "█     █   █ █         █     █        █   █   █   █  ",
    " ████ █   █ █████ ████  ████         █    ███    █  ",
];

// ---- ASCII fallback (same words, simpler glyphs `#`/`+`) ---------------
//
// Some terminals render '█' as a hollow box. The ASCII variants use the
// same letterforms but with '#' fills, so `--style ascii` users get
// readable output anywhere.

const VICTORY_ASCII: &[&str] = &[
    "#    #  ####  ####  #####  ####  ##### #   #",
    "#    #    #  #        #   #    # #   #  # # ",
    " #  #     #  #        #   #    # #####   #  ",
    "  ##      #  #        #   #    # #  #    #  ",
    "  #     ####  ####    #    ####  #   #   #  ",
];

const DEFEAT_ASCII: &[&str] = &[
    "####   ##### ##### #####  ##### #####",
    "#   #  #     #     #      #   #   #  ",
    "#   #  ####  ####  ####   #####   #  ",
    "#   #  #     #     #      #   #   #  ",
    "####   ##### #     #####  #   #   #  ",
];

const DRAW_ASCII: &[&str] = &[
    "####   ####   ##### #     #",
    "#   #  #   #  #     #     #",
    "#   #  ####   ####  #  #  #",
    "#   #  #  #   #     # # # #",
    "####   #   #  #####  #   # ",
];

const RED_WINS_ASCII: &[&str] = &[
    "####   ##### ####      #   # ##### #   #  ####",
    "#   #  #     #   #     #   #   #   ##  # #    ",
    "####   ####  #   #     # # #   #   # # #  ### ",
    "#  #   #     #   #     # # #   #   #  ##     #",
    "#   #  ##### ####       # #  ##### #   # #### ",
];

const BLACK_WINS_ASCII: &[&str] = &[
    "####   #     #####  #### #   #     #   # ##### #   #  ####",
    "#   #  #     #   # #     #  #      #   #   #   ##  # #    ",
    "####   #     ##### #     ###       # # #   #   # # #  ### ",
    "#   #  #     #   # #     #  #      # # #   #   #  ##     #",
    "####   ##### #   #  #### #   #      # #  ##### #   # #### ",
];

const GREEN_WINS_ASCII: &[&str] = &[
    " ####  ####   ##### ##### #   #     #   # ##### #   #  ####",
    "#      #   #  #     #     ##  #     #   #   #   ##  # #    ",
    "#  ##  ####   ####  ####  # # #     # # #   #   # # #  ### ",
    "#   #  #  #   #     #     #  ##     # # #   #   #  ##     #",
    " ####  #   #  ##### ##### #   #      # #  ##### #   # #### ",
];

const APP_TITLE_ASCII: &[&str] = &[
    " #### #   # #####  ####  ####      ##### #   # #####",
    "#     #   # #     #     #            #   #   #   #  ",
    "#     ##### ####   ###   ###         #   #   #   #  ",
    "#     #   # #         #     #        #   #   #   #  ",
    " #### #   # ##### ####  ####         #    ###    #  ",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_banner_has_consistent_row_widths() {
        for (kind, style) in [
            (BannerKind::Victory, Style::Cjk),
            (BannerKind::Victory, Style::Ascii),
            (BannerKind::Defeat, Style::Cjk),
            (BannerKind::Defeat, Style::Ascii),
            (BannerKind::Draw, Style::Cjk),
            (BannerKind::Draw, Style::Ascii),
            (BannerKind::AppTitle, Style::Cjk),
            (BannerKind::AppTitle, Style::Ascii),
            (BannerKind::Outcome(NeutralSide::Red), Style::Cjk),
            (BannerKind::Outcome(NeutralSide::Black), Style::Cjk),
            (BannerKind::Outcome(NeutralSide::Green), Style::Cjk),
        ] {
            let rows = art(kind, style);
            assert!(!rows.is_empty(), "{kind:?}/{style:?}: empty");
            let w = rows[0].chars().count();
            for (i, r) in rows.iter().enumerate() {
                assert_eq!(r.chars().count(), w, "{kind:?}/{style:?} row {i} width drift");
            }
        }
    }

    #[test]
    fn max_width_returns_widest_row() {
        let rows: &[&str] = &["abc", "abcdef", "ab"];
        assert_eq!(max_width(rows), 6);
    }
}
