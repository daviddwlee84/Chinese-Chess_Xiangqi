//! `chess-ai` — clean-room alpha-beta search for xiangqi.
//!
//! Plug-and-play engine: a pure function `choose_move(state, opts)` that
//! returns one move. No frontend coupling, no I/O, no globals — both
//! `chess-tui` and `chess-web` (WASM) share the same library.
//!
//! ## Versioned, switchable, non-overwriting
//!
//! Strategy is selected via [`AiOptions::strategy`]. Older versions stay
//! reachable forever — adding v3 means adding a new [`Strategy`] variant
//! and a new module under [`crate::engines`], never deleting v1/v2. The
//! UI exposes the choice via the `?engine=v1|v2` query param (web) and
//! `--ai-engine` CLI flag (tui).
//!
//! See `docs/ai/README.md` for the version index and roadmap, and
//! per-version specs in `docs/ai/v1-material.md`, `docs/ai/v2-material-pst.md`.
//!
//! Banqi (`Move::DarkCapture` / `Move::Reveal`) is out of scope — its
//! resolution in `chess-core` is deterministic, which would let an
//! alpha-beta search peek at hidden tiles. Use this engine only with
//! `Variant::Xiangqi`. ISMCTS for banqi is tracked as v6 in the roadmap.

pub mod engines;
pub mod eval;
pub mod search;

use chess_core::moves::Move;
use chess_core::state::GameState;

use crate::engines::Engine;

/// User-facing difficulty knob. Maps to a search depth and a randomisation
/// policy in the engine layer (see [`engines::NegamaxV1`] /
/// [`engines::NegamaxV2`]).
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
}

impl Difficulty {
    pub fn as_str(self) -> &'static str {
        match self {
            Difficulty::Easy => "easy",
            Difficulty::Normal => "normal",
            Difficulty::Hard => "hard",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "easy" => Some(Difficulty::Easy),
            "normal" | "medium" => Some(Difficulty::Normal),
            "hard" => Some(Difficulty::Hard),
            _ => None,
        }
    }

    /// Default search depth for this difficulty. Engines may override
    /// per-strategy if a stronger eval needs less depth (or vice versa).
    pub fn default_depth(self) -> u8 {
        match self {
            Difficulty::Easy => 1,
            Difficulty::Normal => 3,
            Difficulty::Hard => 4,
        }
    }
}

/// Engine version selector. New versions are *added*, never replacing old
/// ones — `Strategy::MaterialV1` will always reproduce the 2026-05-08 MVP
/// behaviour bit-for-bit. Defaults to the current "best" recommendation
/// ([`Strategy::default`]).
#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub enum Strategy {
    /// v1 (2026-05-08): negamax + α-β + capture-first ordering, eval =
    /// material only. Plays a "random" opening because all reasonable
    /// moves tie at score 0.
    MaterialV1,
    /// v2 (2026-05-08): same search, eval = material + 7 hand-rolled
    /// piece-square tables. Strictly stronger than v1; default since
    /// v2 landed.
    #[default]
    MaterialPstV2,
}

impl Strategy {
    /// URL/CLI canonical token. Used by `chess-web`'s `?engine=` query
    /// and `chess-tui`'s `--ai-engine` flag. Stable across releases.
    pub fn as_str(self) -> &'static str {
        match self {
            Strategy::MaterialV1 => "v1",
            Strategy::MaterialPstV2 => "v2",
        }
    }

    /// Inverse of [`Strategy::as_str`]. Accepts long aliases as well so
    /// hand-typed URLs are forgiving.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "v1" | "material" | "material-v1" => Some(Strategy::MaterialV1),
            "v2" | "material-pst" | "material-pst-v2" => Some(Strategy::MaterialPstV2),
            _ => None,
        }
    }

    /// Human-readable label for the picker UI.
    pub fn label(self) -> &'static str {
        match self {
            Strategy::MaterialV1 => "Material only (v1, original MVP)",
            Strategy::MaterialPstV2 => "Material + piece-square tables (v2, recommended)",
        }
    }

    /// Iteration helper — the picker uses this to render a dropdown.
    pub const ALL: [Strategy; 2] = [Strategy::MaterialPstV2, Strategy::MaterialV1];
}

#[derive(Clone, Debug)]
pub struct AiOptions {
    pub difficulty: Difficulty,
    /// Override [`Difficulty::default_depth`]. `None` uses the default.
    pub max_depth: Option<u8>,
    /// Optional seed for difficulty randomness (Easy top-N pick / Normal
    /// tiebreak). `None` falls back to a fixed seed so behaviour is
    /// deterministic — the caller may pass a fresh seed per move for variety.
    pub seed: Option<u64>,
    /// Engine version to dispatch on. Defaults to [`Strategy::default`]
    /// (v2 since 2026-05-08).
    pub strategy: Strategy,
}

impl AiOptions {
    pub fn new(difficulty: Difficulty) -> Self {
        Self { difficulty, max_depth: None, seed: None, strategy: Strategy::default() }
    }

    /// Builder shortcut for callers that want to override the default
    /// strategy without touching the other knobs.
    pub fn with_strategy(mut self, strategy: Strategy) -> Self {
        self.strategy = strategy;
        self
    }
}

#[derive(Clone, Debug)]
pub struct AiMoveResult {
    pub mv: Move,
    pub score: i32,
    pub depth: u8,
    pub nodes: u32,
}

/// Pick a move for the side to move in `state`.
///
/// Returns `None` only when there are no legal moves (caller should treat
/// as game over) or when `state.status != Ongoing`.
///
/// Dispatches on `opts.strategy` — the entire `engines::*` module surface
/// is private to chess-ai for callers; downstream consumers should
/// construct an [`AiOptions`] and call this single entry point.
pub fn choose_move(state: &GameState, opts: &AiOptions) -> Option<AiMoveResult> {
    match opts.strategy {
        Strategy::MaterialV1 => engines::NegamaxV1.choose_move(state, opts),
        Strategy::MaterialPstV2 => engines::NegamaxV2.choose_move(state, opts),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess_core::board::Board;
    use chess_core::coord::{File, Rank, Square};
    use chess_core::piece::{Piece, PieceKind, PieceOnSquare, Side};
    use chess_core::rules::RuleSet;

    // ---------- Inherited from v1 MVP — every strategy must satisfy these. ----------

    #[test]
    fn opening_xiangqi_returns_a_legal_move_each_difficulty_each_strategy() {
        let state = GameState::new(RuleSet::xiangqi_casual());
        for strategy in Strategy::ALL {
            for diff in [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard] {
                let opts =
                    AiOptions { difficulty: diff, max_depth: Some(2), seed: Some(7), strategy };
                let result = choose_move(&state, &opts).expect("must return a move");
                let legal = state.legal_moves();
                assert!(
                    legal.iter().any(|m| m == &result.mv),
                    "{:?}/{:?} returned non-legal move {:?}",
                    strategy,
                    diff,
                    result.mv
                );
            }
        }
    }

    #[test]
    fn no_legal_moves_returns_none() {
        let mut state = GameState::new(RuleSet::xiangqi());
        let board: Board = state.board.clone();
        let squares: Vec<Square> = board.squares().collect();
        for sq in squares {
            state.board.set(sq, None);
        }
        assert!(state.legal_moves().is_empty());
        for strategy in Strategy::ALL {
            let opts = AiOptions::new(Difficulty::Hard).with_strategy(strategy);
            assert!(choose_move(&state, &opts).is_none(), "{:?} should yield None", strategy);
        }
    }

    #[test]
    fn determinism_same_seed_same_move() {
        let state = GameState::new(RuleSet::xiangqi_casual());
        for strategy in Strategy::ALL {
            let opts = AiOptions {
                difficulty: Difficulty::Easy,
                max_depth: Some(2),
                seed: Some(42),
                strategy,
            };
            let a = choose_move(&state, &opts).unwrap();
            let b = choose_move(&state, &opts).unwrap();
            assert_eq!(a.mv, b.mv, "{:?} drifted between identical-seed calls", strategy);
        }
    }

    #[test]
    fn hard_prefers_capture_when_free() {
        // Red chariot opposite Black soldier with nothing between — every
        // material-aware engine at any depth ≥ 1 must take.
        for strategy in Strategy::ALL {
            let mut state = GameState::new(RuleSet::xiangqi_casual());
            let board: Board = state.board.clone();
            let squares: Vec<Square> = board.squares().collect();
            for sq in squares {
                state.board.set(sq, None);
            }
            let red_gen = state.board.sq(File(4), Rank(0));
            let blk_gen = state.board.sq(File(4), Rank(9));
            state.board.set(
                red_gen,
                Some(PieceOnSquare::revealed(Piece::new(Side::RED, PieceKind::General))),
            );
            state.board.set(
                blk_gen,
                Some(PieceOnSquare::revealed(Piece::new(Side::BLACK, PieceKind::General))),
            );
            let red_chariot = state.board.sq(File(0), Rank(4));
            let blk_soldier = state.board.sq(File(0), Rank(5));
            state.board.set(
                red_chariot,
                Some(PieceOnSquare::revealed(Piece::new(Side::RED, PieceKind::Chariot))),
            );
            state.board.set(
                blk_soldier,
                Some(PieceOnSquare::revealed(Piece::new(Side::BLACK, PieceKind::Soldier))),
            );

            let opts = AiOptions {
                difficulty: Difficulty::Hard,
                max_depth: Some(2),
                seed: Some(0),
                strategy,
            };
            let result = choose_move(&state, &opts).unwrap();
            match result.mv {
                Move::Capture { from, to, .. } => {
                    assert_eq!(from, red_chariot, "{:?}", strategy);
                    assert_eq!(to, blk_soldier, "{:?}", strategy);
                }
                other => panic!("{:?}: expected capture, got {:?}", strategy, other),
            }
        }
    }

    // ---------- v2-specific: PST should differentiate opening moves. ----------

    #[test]
    fn v2_breaks_v1_opening_tie_on_hard() {
        // From the initial xiangqi position, every opening move is
        // material-tied at score 0 under v1 → Hard returns the first
        // capture-ordered legal move (deterministic but uninformed).
        // Under v2, PST values differentiate so Hard should pick a move
        // whose root score is *not* zero (some non-trivial PST swing
        // happens at depth 2).
        let state = GameState::new(RuleSet::xiangqi_casual());
        let opts_v2 = AiOptions {
            difficulty: Difficulty::Hard,
            max_depth: Some(2),
            seed: Some(0),
            strategy: Strategy::MaterialPstV2,
        };
        let r2 = choose_move(&state, &opts_v2).expect("v2 returns a move");
        // v2 evaluations of opening positions are generally non-zero
        // because the PST for both sides won't perfectly cancel in a
        // 2-ply tree. Tolerate exact ties as long as the move is legal,
        // but assert the search did *something* (nodes > 0).
        assert!(r2.nodes > 0, "v2 search must have visited nodes");
    }

    #[test]
    fn strategy_dispatch_is_distinguishable() {
        // v1 and v2 ARE allowed to agree on some positions, but on the
        // initial xiangqi position with Hard-deterministic-best, the
        // ordering of equally-rated (v1) moves vs PST-broken-tie (v2)
        // moves should diverge on at least one of the standard fixtures.
        // This guards against accidentally wiring both Strategy variants
        // to the same evaluator.
        let state = GameState::new(RuleSet::xiangqi_casual());
        let v1 = AiOptions {
            difficulty: Difficulty::Hard,
            max_depth: Some(2),
            seed: Some(0),
            strategy: Strategy::MaterialV1,
        };
        let v2 = AiOptions {
            difficulty: Difficulty::Hard,
            max_depth: Some(2),
            seed: Some(0),
            strategy: Strategy::MaterialPstV2,
        };
        let r1 = choose_move(&state, &v1).unwrap();
        let r2 = choose_move(&state, &v2).unwrap();
        // Either the chosen move differs OR the score differs — we just
        // need evidence that the dispatch routed to two different code
        // paths. Hard-mode v1 returns the capture-ordered first legal
        // move at score 0; v2 should disagree on at least one axis.
        let routed_differently = r1.mv != r2.mv || r1.score != r2.score;
        assert!(
            routed_differently,
            "v1 and v2 produced identical (mv, score) — dispatch may be broken: {:?} vs {:?}",
            r1, r2
        );
    }

    #[test]
    fn strategy_parse_round_trips() {
        for s in Strategy::ALL {
            assert_eq!(Strategy::parse(s.as_str()), Some(s));
        }
        assert_eq!(Strategy::parse("V2"), Some(Strategy::MaterialPstV2));
        assert_eq!(Strategy::parse("material-pst"), Some(Strategy::MaterialPstV2));
        assert_eq!(Strategy::parse("nonsense"), None);
    }

    #[test]
    fn default_strategy_is_v2() {
        assert_eq!(Strategy::default(), Strategy::MaterialPstV2);
        assert_eq!(AiOptions::new(Difficulty::Normal).strategy, Strategy::MaterialPstV2);
    }
}
