//! ICCS notation: file `a..i` × rank `0..9` → `h2e2` style.
//!
//! Encoder is straightforward; decoder takes a state because string-form
//! moves are ambiguous between Step/Capture/CannonJump (the geometry is
//! identical from outside) — we resolve by intersecting with `legal_moves`.

use crate::board::Board;
use crate::coord::{File, Rank, Square};
use crate::error::CoreError;
use crate::moves::Move;
use crate::state::GameState;

/// Encode a single square as `<file><rank>` e.g. `h2`.
pub fn encode_square(board: &Board, sq: Square) -> String {
    let (f, r) = board.file_rank(sq);
    format!("{}{}", file_char(f.0), r.0)
}

fn file_char(f: u8) -> char {
    (b'a' + f) as char
}

fn parse_square(board: &Board, s: &str) -> Result<Square, CoreError> {
    let mut chars = s.chars();
    let f = chars.next().ok_or_else(|| CoreError::BadNotation(s.into()))?;
    let r = chars.next().ok_or_else(|| CoreError::BadNotation(s.into()))?;
    if chars.next().is_some() {
        return Err(CoreError::BadNotation(s.into()));
    }
    if !f.is_ascii_lowercase() {
        return Err(CoreError::BadNotation(s.into()));
    }
    let file_idx = (f as u8) - b'a';
    let rank_idx = r.to_digit(10).ok_or_else(|| CoreError::BadNotation(s.into()))? as u8;
    if file_idx >= board.width() || rank_idx >= board.height() {
        return Err(CoreError::BadNotation(format!("{s} out of board bounds")));
    }
    Ok(board.sq(File(file_idx), Rank(rank_idx)))
}

/// Encode a move as ICCS. `Reveal` becomes `flip <sq>`. `ChainCapture`
/// chains squares with `x` separators.
pub fn encode_move(board: &Board, m: &Move) -> String {
    match m {
        Move::Reveal { at, .. } => format!("flip {}", encode_square(board, *at)),
        Move::Step { from, to } => {
            format!("{}{}", encode_square(board, *from), encode_square(board, *to))
        }
        Move::Capture { from, to, .. } | Move::CannonJump { from, to, .. } => {
            format!("{}x{}", encode_square(board, *from), encode_square(board, *to))
        }
        Move::ChainCapture { from, path } => {
            let mut s = encode_square(board, *from);
            for hop in path {
                s.push('x');
                s.push_str(&encode_square(board, hop.to));
            }
            s
        }
    }
}

/// Decode a move from ICCS, resolving Step/Capture ambiguity by matching
/// against the current legal-move list. Forms accepted:
/// - `h2e2`               — step
/// - `h2xh9`              — capture (single)
/// - `a3xb3xc3`           — chain capture (any number of x-separated squares)
/// - `flip a3` / `flip a3` — reveal
pub fn decode_move(state: &GameState, input: &str) -> Result<Move, CoreError> {
    let trimmed = input.trim();
    if let Some(rest) = trimmed.strip_prefix("flip ") {
        let at = parse_square(&state.board, rest.trim())?;
        return Ok(Move::Reveal { at, revealed: None });
    }
    let board = &state.board;

    // Split into squares: handle both 'h2e2' (no separator) and 'h2xe2'.
    let parts: Vec<Square> = if trimmed.contains('x') {
        trimmed.split('x').map(|p| parse_square(board, p.trim())).collect::<Result<Vec<_>, _>>()?
    } else if trimmed.len() == 4 {
        vec![parse_square(board, &trimmed[..2])?, parse_square(board, &trimmed[2..])?]
    } else {
        return Err(CoreError::BadNotation(input.into()));
    };

    if parts.len() < 2 {
        return Err(CoreError::BadNotation(input.into()));
    }

    let from = parts[0];
    let legal = state.legal_moves();

    if parts.len() == 2 {
        // Step / Capture / CannonJump — find unique match by (from, to).
        let to = parts[1];
        let candidates: Vec<&Move> = legal
            .iter()
            .filter(|m| m.origin_square() == from && m.to_square() == Some(to))
            .collect();
        match candidates.as_slice() {
            [one] => Ok((*one).clone()),
            [] => Err(CoreError::Illegal("no legal move matches notation")),
            _multi => {
                // Multiple matches (e.g. Step vs ChainCapture with len 2). Prefer
                // the simplest: Step > Capture > CannonJump > ChainCapture.
                let pick = candidates
                    .iter()
                    .find(|m| matches!(m, Move::Step { .. }))
                    .or_else(|| candidates.iter().find(|m| matches!(m, Move::Capture { .. })))
                    .or_else(|| candidates.iter().find(|m| matches!(m, Move::CannonJump { .. })))
                    .or_else(|| candidates.iter().find(|m| matches!(m, Move::ChainCapture { .. })));
                pick.map(|m| (*m).clone()).ok_or(CoreError::Illegal("ambiguous"))
            }
        }
    } else {
        // ChainCapture — find legal move whose path matches.
        let chain_squares = &parts[1..];
        let candidate = legal.iter().find(|m| match m {
            Move::ChainCapture { from: f, path } => {
                *f == from
                    && path.len() == chain_squares.len()
                    && path.iter().zip(chain_squares.iter()).all(|(h, sq)| h.to == *sq)
            }
            _ => false,
        });
        candidate.cloned().ok_or(CoreError::Illegal("no chain capture matches notation"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::RuleSet;

    #[test]
    fn round_trip_simple_step() {
        let state = GameState::new(RuleSet::xiangqi());
        // Pick a legal Step and round-trip it.
        let m = state.legal_moves().into_iter().find(|m| matches!(m, Move::Step { .. })).unwrap();
        let s = encode_move(&state.board, &m);
        let decoded = decode_move(&state, &s).unwrap();
        assert_eq!(m, decoded);
    }

    #[test]
    fn parse_h2e2_in_xiangqi() {
        let state = GameState::new(RuleSet::xiangqi());
        // h2 is a Red cannon. h2e2 moves it 6 squares west — the standard 炮二平五 (well, 炮 to e2).
        let m = decode_move(&state, "h2e2").unwrap();
        if let Move::Step { from, to } = m {
            let (f1, r1) = state.board.file_rank(from);
            let (f2, r2) = state.board.file_rank(to);
            assert_eq!((f1.0, r1.0, f2.0, r2.0), (7, 2, 4, 2));
        } else {
            panic!("expected Step, got {m:?}");
        }
    }

    #[test]
    fn flip_decodes_to_reveal() {
        let state = GameState::new(crate::rules::RuleSet::banqi_with_seed(
            crate::rules::HouseRules::empty(),
            3,
        ));
        let m = decode_move(&state, "flip a0").unwrap();
        assert!(matches!(m, Move::Reveal { revealed: None, .. }));
    }

    #[test]
    fn bad_notation_rejected() {
        let state = GameState::new(RuleSet::xiangqi());
        assert!(decode_move(&state, "").is_err());
        assert!(decode_move(&state, "zz").is_err());
        assert!(decode_move(&state, "z9z9").is_err());
    }
}
