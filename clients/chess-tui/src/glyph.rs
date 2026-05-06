//! Piece glyph tables. Two render styles: traditional CJK (帥/將 etc.) and
//! ASCII single-letter (uppercase = Red, lowercase = Black). The engine has
//! no concept of glyphs — presentation lives entirely in the client.

use chess_core::piece::{PieceKind, Side};

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Style {
    Cjk,
    Ascii,
}

/// Two-column-wide string for a piece. CJK characters are double-width in a
/// monospace terminal; ASCII glyphs are padded with a trailing space so cells
/// align in either mode.
pub fn glyph(kind: PieceKind, side: Side, style: Style) -> &'static str {
    match style {
        Style::Cjk => cjk(kind, side),
        Style::Ascii => ascii(kind, side),
    }
}

fn cjk(kind: PieceKind, side: Side) -> &'static str {
    match (side, kind) {
        (Side::RED, PieceKind::General) => "帥",
        (Side::RED, PieceKind::Advisor) => "仕",
        (Side::RED, PieceKind::Elephant) => "相",
        (Side::RED, PieceKind::Chariot) => "俥",
        (Side::RED, PieceKind::Horse) => "傌",
        (Side::RED, PieceKind::Cannon) => "炮",
        (Side::RED, PieceKind::Soldier) => "兵",
        (Side::BLACK, PieceKind::General) => "將",
        (Side::BLACK, PieceKind::Advisor) => "士",
        (Side::BLACK, PieceKind::Elephant) => "象",
        (Side::BLACK, PieceKind::Chariot) => "車",
        (Side::BLACK, PieceKind::Horse) => "馬",
        (Side::BLACK, PieceKind::Cannon) => "砲",
        (Side::BLACK, PieceKind::Soldier) => "卒",
        // 三國暗棋 — Green seat. Reuse Black glyphs for now; PR-2 may pick a
        // distinct glyph set (e.g. 君/臣/...) once the variant ships.
        (_, PieceKind::General) => "將",
        (_, PieceKind::Advisor) => "士",
        (_, PieceKind::Elephant) => "象",
        (_, PieceKind::Chariot) => "車",
        (_, PieceKind::Horse) => "馬",
        (_, PieceKind::Cannon) => "砲",
        (_, PieceKind::Soldier) => "卒",
    }
}

fn ascii(kind: PieceKind, side: Side) -> &'static str {
    match (side, kind) {
        (Side::RED, PieceKind::General) => "K ",
        (Side::RED, PieceKind::Advisor) => "A ",
        (Side::RED, PieceKind::Elephant) => "B ",
        (Side::RED, PieceKind::Chariot) => "R ",
        (Side::RED, PieceKind::Horse) => "N ",
        (Side::RED, PieceKind::Cannon) => "C ",
        (Side::RED, PieceKind::Soldier) => "P ",
        (Side::BLACK, PieceKind::General) => "k ",
        (Side::BLACK, PieceKind::Advisor) => "a ",
        (Side::BLACK, PieceKind::Elephant) => "b ",
        (Side::BLACK, PieceKind::Chariot) => "r ",
        (Side::BLACK, PieceKind::Horse) => "n ",
        (Side::BLACK, PieceKind::Cannon) => "c ",
        (Side::BLACK, PieceKind::Soldier) => "p ",
        (_, PieceKind::General) => "K*",
        (_, PieceKind::Advisor) => "A*",
        (_, PieceKind::Elephant) => "B*",
        (_, PieceKind::Chariot) => "R*",
        (_, PieceKind::Horse) => "N*",
        (_, PieceKind::Cannon) => "C*",
        (_, PieceKind::Soldier) => "P*",
    }
}

/// Hidden / face-down piece (banqi pre-flip).
pub fn hidden(style: Style) -> &'static str {
    match style {
        Style::Cjk => "暗",
        Style::Ascii => "? ",
    }
}

/// Empty cell. The xiangqi grid uses `·` (centred dot, double-width compatible)
/// for visual structure; ASCII falls back to a literal period.
pub fn empty(style: Style) -> &'static str {
    match style {
        Style::Cjk => "・",
        Style::Ascii => ". ",
    }
}

/// Human-readable side label for the sidebar. `Side(0)` (engine debug) is
/// useless to a player; this returns "Red (紅)" / "Black (黑)" / "Green (綠)"
/// in CJK style and "Red" / "Black" / "Green" in ASCII style.
pub fn side_name(side: Side, style: Style) -> &'static str {
    match (side, style) {
        (Side::RED, Style::Cjk) => "Red 紅",
        (Side::BLACK, Style::Cjk) => "Black 黑",
        (Side::RED, Style::Ascii) => "Red",
        (Side::BLACK, Style::Ascii) => "Black",
        (_, Style::Cjk) => "Green 綠",
        (_, Style::Ascii) => "Green",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_table_total_for_red_and_black() {
        for kind in PieceKind::ALL {
            for side in [Side::RED, Side::BLACK] {
                let cjk_g = glyph(kind, side, Style::Cjk);
                let ascii_g = glyph(kind, side, Style::Ascii);
                assert!(!cjk_g.is_empty(), "missing CJK glyph for {:?}/{:?}", side, kind);
                assert_eq!(ascii_g.chars().count(), 2, "ASCII glyph not 2 cols: {:?}", ascii_g);
            }
        }
    }

    #[test]
    fn red_and_black_distinct_in_cjk() {
        for kind in PieceKind::ALL {
            assert_ne!(
                glyph(kind, Side::RED, Style::Cjk),
                glyph(kind, Side::BLACK, Style::Cjk),
                "Red and Black share a CJK glyph for {:?}",
                kind
            );
        }
    }

    #[test]
    fn hidden_and_empty_render() {
        assert_eq!(hidden(Style::Ascii), "? ");
        assert_eq!(empty(Style::Ascii), ". ");
        assert!(!hidden(Style::Cjk).is_empty());
        assert!(!empty(Style::Cjk).is_empty());
    }
}
