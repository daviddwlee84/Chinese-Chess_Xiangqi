//! Snapshot serialization.
//!
//! Two layers, on purpose:
//!
//! - **JSON** (`to_json` / `from_json`) — canonical, lossless. Used for save
//!   files, network protocol, replays. Serde does the work.
//! - **`.pos` text DSL** (`to_pos_text` / `from_pos_text`) — human-friendly
//!   for hand-written test fixtures and endgame puzzles. Limited to xiangqi
//!   and banqi (two-player variants) — three-kingdoms uses JSON.
//!
//! ## .pos format
//!
//! ```text
//! # Comments start with '#'
//! variant: xiangqi
//! side_to_move: red
//! no_progress_plies: 0       # optional, default 0
//! house: chain,rush          # optional banqi house rules
//! seed: 42                   # optional banqi seed
//! side_assignment: red,black # optional banqi side->color mapping
//!
//! board:
//!   . . . . k . . . .
//!   . . . . . . . . .
//!   . . . r R r . . .
//!   . . . . . . . . .
//!   . . . . . . . . .
//!   . . . . . . . . .
//!   . . . . . . . . .
//!   . . . . . . . . .
//!   . . . . . . . . .
//!   . . . . K . . . .
//! ```
//!
//! - `K A B R N C P` = General/Advisor/Elephant/Chariot/Horse/Cannon/Soldier
//!   (Xiangqi-FEN convention). Uppercase = Red, lowercase = Black.
//! - `.` = empty.
//! - `?X` (e.g. `?K`, `?p`) = face-down piece (engine knows the identity).
//! - Top of grid is the highest rank (Black home for xiangqi); bottom is
//!   rank 0 (Red home).

use std::fmt::Write as _;

use smallvec::SmallVec;

use crate::board::{Board, BoardShape};
use crate::coord::{File, Rank};
use crate::error::CoreError;
use crate::piece::{Piece, PieceKind, PieceOnSquare, Side};
use crate::rules::{DrawPolicy, HouseRules, RuleSet, Variant};
use crate::state::{GameState, GameStatus, SideAssignment, TurnOrder};

impl GameState {
    /// Serialize to canonical JSON (lossless).
    pub fn to_json(&self) -> Result<String, CoreError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| CoreError::BadNotation(format!("json serialize: {e}")))
    }

    /// Deserialize from canonical JSON.
    pub fn from_json(s: &str) -> Result<Self, CoreError> {
        let mut state: GameState = serde_json::from_str(s)
            .map_err(|e| CoreError::BadNotation(format!("json parse: {e}")))?;
        // Pre-v5 snapshots have no `position_hash` field — `#[serde(default)]`
        // gives 0; recompute so the loaded state is search-ready.
        state.recompute_position_hash();
        Ok(state)
    }

    /// Emit human-friendly `.pos` text. Two-player variants only.
    pub fn to_pos_text(&self) -> String {
        emit_pos_text(self)
    }

    /// Parse a `.pos` text fixture into a fresh `GameState`. History is empty.
    pub fn from_pos_text(input: &str) -> Result<Self, CoreError> {
        let mut state = parse_pos_text(input)?;
        state.recompute_position_hash();
        Ok(state)
    }
}

// ============================================================================
// Emitter
// ============================================================================

fn emit_pos_text(state: &GameState) -> String {
    let mut out = String::new();
    writeln!(out, "variant: {}", variant_str(state.rules.variant)).unwrap();
    writeln!(out, "side_to_move: {}", side_str(state.side_to_move)).unwrap();
    if state.no_progress_plies > 0 {
        writeln!(out, "no_progress_plies: {}", state.no_progress_plies).unwrap();
    }
    if !state.rules.house.is_empty() {
        writeln!(out, "house: {}", emit_house(state.rules.house)).unwrap();
    }
    if let Some(seed) = state.rules.banqi_seed {
        writeln!(out, "seed: {seed}").unwrap();
    }
    if let Some(sa) = &state.side_assignment {
        let sides: Vec<&str> = sa.mapping.iter().map(|s| side_str(*s)).collect();
        writeln!(out, "side_assignment: {}", sides.join(",")).unwrap();
    }
    writeln!(out).unwrap();
    writeln!(out, "board:").unwrap();

    let board = &state.board;
    let (w, h) = (board.width(), board.height());
    for display_row in 0..h {
        let rank = h - 1 - display_row;
        write!(out, " ").unwrap();
        for file in 0..w {
            let sq = board.sq(File(file), Rank(rank));
            let glyph = cell_glyph(board.get(sq));
            // 2-char padded so `?K` and `K ` and `. ` all align.
            write!(out, " {:<2}", glyph).unwrap();
        }
        writeln!(out).unwrap();
    }
    out
}

fn cell_glyph(pos: Option<PieceOnSquare>) -> String {
    match pos {
        None => ".".to_string(),
        Some(p) => {
            let c = kind_char(p.piece.kind);
            let cased = if p.piece.side == Side::RED { c } else { c.to_ascii_lowercase() };
            if p.revealed {
                cased.to_string()
            } else {
                format!("?{cased}")
            }
        }
    }
}

#[inline]
fn kind_char(kind: PieceKind) -> char {
    match kind {
        PieceKind::General => 'K',
        PieceKind::Advisor => 'A',
        PieceKind::Elephant => 'B',
        PieceKind::Chariot => 'R',
        PieceKind::Horse => 'N',
        PieceKind::Cannon => 'C',
        PieceKind::Soldier => 'P',
    }
}

#[inline]
fn variant_str(v: Variant) -> &'static str {
    match v {
        Variant::Xiangqi => "xiangqi",
        Variant::Banqi => "banqi",
        Variant::ThreeKingdomBanqi => "three-kingdom-banqi",
    }
}

#[inline]
fn side_str(s: Side) -> &'static str {
    match s {
        Side::RED => "red",
        Side::BLACK => "black",
        Side::GREEN => "green",
        _ => "side?",
    }
}

fn emit_house(h: HouseRules) -> String {
    let mut parts = Vec::new();
    if h.contains(HouseRules::CHAIN_CAPTURE) {
        parts.push("chain");
    }
    if h.contains(HouseRules::DARK_CAPTURE) {
        parts.push("dark");
    }
    if h.contains(HouseRules::CHARIOT_RUSH) {
        parts.push("rush");
    }
    if h.contains(HouseRules::HORSE_DIAGONAL) {
        parts.push("horse-diagonal");
    }
    if h.contains(HouseRules::CANNON_FAST_MOVE) {
        parts.push("cannon-fast");
    }
    if h.contains(HouseRules::DARK_CAPTURE_TRADE) {
        parts.push("dark-trade");
    }
    parts.join(",")
}

// ============================================================================
// Parser
// ============================================================================

fn parse_pos_text(input: &str) -> Result<GameState, CoreError> {
    let mut variant: Option<Variant> = None;
    let mut side_to_move: Option<Side> = None;
    let mut house = HouseRules::empty();
    let mut seed: Option<u64> = None;
    let mut no_progress_plies: u16 = 0;
    let mut side_assignment_raw: Option<Vec<Side>> = None;
    let mut board_rows_raw: Vec<Vec<Option<PieceOnSquare>>> = Vec::new();

    let mut in_board = false;

    for raw in input.lines() {
        let stripped: &str = match raw.find('#') {
            Some(i) => &raw[..i],
            None => raw,
        };
        if stripped.trim().is_empty() {
            // Blank line ends board mode but doesn't error.
            in_board = false;
            continue;
        }
        let leading_ws = stripped.chars().take_while(|c| c.is_whitespace()).count();
        let trimmed = stripped.trim();

        if in_board && leading_ws > 0 {
            board_rows_raw.push(parse_row(trimmed)?);
            continue;
        }

        in_board = false;

        let (key, value) = trimmed
            .split_once(':')
            .ok_or_else(|| CoreError::BadNotation(format!("malformed line: {trimmed}")))?;
        let key = key.trim();
        let value = value.trim();

        match key {
            "variant" => variant = Some(parse_variant_str(value)?),
            "side_to_move" => side_to_move = Some(parse_side_str(value)?),
            "house" => house = parse_house_list(value)?,
            "seed" => {
                seed = Some(
                    value
                        .parse()
                        .map_err(|_| CoreError::BadNotation(format!("bad seed: {value}")))?,
                )
            }
            "no_progress_plies" => {
                no_progress_plies = value.parse().map_err(|_| {
                    CoreError::BadNotation(format!("bad no_progress_plies: {value}"))
                })?
            }
            "side_assignment" => {
                let sides: Result<Vec<_>, _> = value.split(',').map(parse_side_str).collect();
                side_assignment_raw = Some(sides?);
            }
            "board" => {
                if !value.is_empty() {
                    return Err(CoreError::BadNotation(
                        "'board:' must be on its own line; rows follow indented".into(),
                    ));
                }
                in_board = true;
            }
            other => return Err(CoreError::BadNotation(format!("unknown key: {other}"))),
        }
    }

    let variant = variant.ok_or(CoreError::Setup("variant required"))?;
    let side_to_move = side_to_move.ok_or(CoreError::Setup("side_to_move required"))?;

    let rules = RuleSet {
        variant,
        house,
        draw_policy: match variant {
            Variant::Xiangqi => DrawPolicy::XIANGQI,
            _ => DrawPolicy::BANQI,
        },
        banqi_seed: seed,
        xiangqi_allow_self_check: false,
    };

    let shape = match variant {
        Variant::Xiangqi => BoardShape::Xiangqi9x10,
        Variant::Banqi => BoardShape::Banqi4x8,
        Variant::ThreeKingdomBanqi => BoardShape::ThreeKingdom,
    };
    let mut board = Board::new(shape);

    if !board_rows_raw.is_empty() {
        let (w, h) = (board.width() as usize, board.height() as usize);
        if board_rows_raw.len() != h {
            return Err(CoreError::BadNotation(format!(
                "expected {h} rows, got {}",
                board_rows_raw.len()
            )));
        }
        for (display_row, cells) in board_rows_raw.iter().enumerate() {
            if cells.len() != w {
                return Err(CoreError::BadNotation(format!(
                    "row {display_row}: {} cells, expected {w}",
                    cells.len()
                )));
            }
            let rank = (h - 1 - display_row) as u8;
            for (file, cell) in cells.iter().enumerate() {
                let sq = board.sq(File(file as u8), Rank(rank));
                board.set(sq, *cell);
            }
        }
    }

    let mut turn_order = match variant {
        Variant::ThreeKingdomBanqi => TurnOrder::three_player(),
        _ => TurnOrder::two_player(),
    };
    let idx = turn_order
        .seats
        .iter()
        .position(|s| *s == side_to_move)
        .ok_or(CoreError::Setup("side_to_move not in turn order"))? as u8;
    turn_order.current = idx;

    let side_assignment = side_assignment_raw.map(|sides| {
        let mut mapping: SmallVec<[Side; 3]> = SmallVec::new();
        for s in sides {
            mapping.push(s);
        }
        SideAssignment { mapping }
    });

    Ok(GameState {
        rules,
        board,
        side_to_move,
        turn_order,
        history: Vec::new(),
        status: GameStatus::Ongoing,
        side_assignment,
        no_progress_plies,
        chain_lock: None,
        position_hash: 0,
        banqi_first_mover_locked: false,
    })
}

fn parse_row(row_str: &str) -> Result<Vec<Option<PieceOnSquare>>, CoreError> {
    let mut cells = Vec::new();
    let mut chars = row_str.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        if c == '.' {
            cells.push(None);
        } else if c == '?' {
            let next = chars
                .next()
                .ok_or_else(|| CoreError::BadNotation("'?' without piece char".into()))?;
            let (kind, side) = parse_piece_char(next)?;
            cells.push(Some(PieceOnSquare::hidden(Piece::new(side, kind))));
        } else {
            let (kind, side) = parse_piece_char(c)?;
            cells.push(Some(PieceOnSquare::revealed(Piece::new(side, kind))));
        }
    }
    Ok(cells)
}

fn parse_piece_char(c: char) -> Result<(PieceKind, Side), CoreError> {
    let upper = c.to_ascii_uppercase();
    let kind = match upper {
        'K' => PieceKind::General,
        'A' => PieceKind::Advisor,
        'B' => PieceKind::Elephant,
        'R' => PieceKind::Chariot,
        'N' => PieceKind::Horse,
        'C' => PieceKind::Cannon,
        'P' => PieceKind::Soldier,
        _ => return Err(CoreError::BadNotation(format!("invalid piece char: {c}"))),
    };
    let side = if c.is_ascii_uppercase() { Side::RED } else { Side::BLACK };
    Ok((kind, side))
}

fn parse_variant_str(s: &str) -> Result<Variant, CoreError> {
    match s {
        "xiangqi" => Ok(Variant::Xiangqi),
        "banqi" => Ok(Variant::Banqi),
        "three-kingdom-banqi" | "three-kingdoms-banqi" => Ok(Variant::ThreeKingdomBanqi),
        _ => Err(CoreError::BadNotation(format!("unknown variant: {s}"))),
    }
}

fn parse_side_str(s: &str) -> Result<Side, CoreError> {
    match s.trim() {
        "red" | "0" => Ok(Side::RED),
        "black" | "1" => Ok(Side::BLACK),
        "green" | "2" => Ok(Side::GREEN),
        _ => Err(CoreError::BadNotation(format!("unknown side: {s}"))),
    }
}

fn parse_house_list(s: &str) -> Result<HouseRules, CoreError> {
    let mut h = HouseRules::empty();
    if s.is_empty() {
        return Ok(h);
    }
    for tok in s.split(',') {
        h |= match tok.trim() {
            "chain" | "chain-capture" => HouseRules::CHAIN_CAPTURE,
            "dark-chain" | "dark" | "dark-capture" => HouseRules::DARK_CAPTURE,
            "dark-trade" | "trade" => HouseRules::DARK_CAPTURE_TRADE,
            "rush" | "chariot-rush" => HouseRules::CHARIOT_RUSH,
            "horse" | "horse-diagonal" | "diag" => HouseRules::HORSE_DIAGONAL,
            "cannon-fast" | "fast-cannon" => HouseRules::CANNON_FAST_MOVE,
            other => return Err(CoreError::BadNotation(format!("unknown house rule: {other}"))),
        };
    }
    Ok(crate::rules::house::normalize(h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xiangqi_initial_round_trip_pos() {
        let s1 = GameState::new(RuleSet::xiangqi());
        let text = s1.to_pos_text();
        let s2 = GameState::from_pos_text(&text).unwrap();
        assert_eq!(s1.board, s2.board);
        assert_eq!(s1.side_to_move, s2.side_to_move);
        assert_eq!(s1.rules, s2.rules);
    }

    #[test]
    fn json_round_trip() {
        let s1 = GameState::new(RuleSet::xiangqi());
        let json = s1.to_json().unwrap();
        let s2 = GameState::from_json(&json).unwrap();
        assert_eq!(s1, s2);
    }

    #[test]
    fn banqi_face_down_round_trip() {
        let s1 = GameState::new(RuleSet::banqi_with_seed(HouseRules::CHAIN_CAPTURE, 42));
        let text = s1.to_pos_text();
        // Hidden pieces should appear with `?` prefix in the emitted text.
        assert!(text.contains('?'), "banqi initial state should have ?-prefixed pieces");
        let s2 = GameState::from_pos_text(&text).unwrap();
        assert_eq!(s1.board, s2.board);
        assert_eq!(s1.rules.banqi_seed, s2.rules.banqi_seed);
    }

    #[test]
    fn parse_minimal_xiangqi() {
        let text = r#"
variant: xiangqi
side_to_move: red

board:
  . . . . k . . . .
  . . . . . . . . .
  . . . . . . . . .
  . . . . . . . . .
  . . . . . . . . .
  . . . . . . . . .
  . . . . . . . . .
  . . . . . . . . .
  . . . . . . . . .
  . . . . K . . . .
"#;
        let s = GameState::from_pos_text(text).unwrap();
        assert_eq!(s.rules.variant, Variant::Xiangqi);
        assert_eq!(s.side_to_move, Side::RED);
        // Generals at e0 and e9
        let e0 = s.board.sq(File(4), Rank(0));
        let e9 = s.board.sq(File(4), Rank(9));
        assert_eq!(s.board.get(e0).unwrap().piece.kind, PieceKind::General);
        assert_eq!(s.board.get(e0).unwrap().piece.side, Side::RED);
        assert_eq!(s.board.get(e9).unwrap().piece.kind, PieceKind::General);
        assert_eq!(s.board.get(e9).unwrap().piece.side, Side::BLACK);
    }

    #[test]
    fn parse_with_house_rules_and_seed() {
        let text = r#"
variant: banqi
side_to_move: red
house: chain,rush
seed: 7
side_assignment: red,black

board:
  . . . .
  . . . .
  . . . .
  . . K .
  . . . .
  . . . .
  . . . .
  . . . .
"#;
        let s = GameState::from_pos_text(text).unwrap();
        assert!(s.rules.house.contains(HouseRules::CHAIN_CAPTURE));
        assert!(s.rules.house.contains(HouseRules::CHARIOT_RUSH));
        assert_eq!(s.rules.banqi_seed, Some(7));
        assert!(s.side_assignment.is_some());
    }

    #[test]
    fn malformed_input_rejected() {
        assert!(GameState::from_pos_text("").is_err()); // missing variant
        assert!(GameState::from_pos_text("variant: foo\nside_to_move: red\n").is_err());
        assert!(GameState::from_pos_text("variant: xiangqi\nside_to_move: purple\n").is_err());
    }

    #[test]
    fn rank_order_top_is_highest() {
        // Top row has a black king; bottom row has a red king.
        let text = r#"
variant: xiangqi
side_to_move: red

board:
  . . . . k . . . .
  . . . . . . . . .
  . . . . . . . . .
  . . . . . . . . .
  . . . . . . . . .
  . . . . . . . . .
  . . . . . . . . .
  . . . . . . . . .
  . . . . . . . . .
  . . . . K . . . .
"#;
        let s = GameState::from_pos_text(text).unwrap();
        let top = s.board.sq(File(4), Rank(9));
        let bottom = s.board.sq(File(4), Rank(0));
        assert_eq!(s.board.get(top).unwrap().piece.side, Side::BLACK);
        assert_eq!(s.board.get(bottom).unwrap().piece.side, Side::RED);
    }
}
