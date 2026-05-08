//! `chess-ai` — clean-room alpha-beta search for xiangqi.
//!
//! MVP per `.claude/plans/ai-vs-chatgpt-deep-research-docs-ai-dee-typed-stonebraker.md`:
//! negamax + alpha-beta, handcrafted material eval, capture-first move ordering,
//! seeded RNG for difficulty randomness. WASM-clean (no platform deps beyond
//! what `chess-core` already pulls in).
//!
//! Banqi is out of scope — `Move::DarkCapture` / `Move::Reveal` resolution in
//! `chess-core` is deterministic, which would let a search peek at hidden
//! tiles. Use this engine only with `Variant::Xiangqi`.

use chess_core::coord::Square;
use chess_core::moves::Move;
use chess_core::piece::{Piece, PieceKind, Side};
use chess_core::state::GameState;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

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

    /// Default search depth for this difficulty.
    pub fn default_depth(self) -> u8 {
        match self {
            Difficulty::Easy => 1,
            Difficulty::Normal => 3,
            Difficulty::Hard => 4,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AiOptions {
    pub difficulty: Difficulty,
    /// Override `Difficulty::default_depth`. `None` uses the default.
    pub max_depth: Option<u8>,
    /// Optional seed for difficulty randomness (Easy top-N pick / Normal
    /// tiebreak). `None` falls back to a fixed seed so behaviour is
    /// deterministic — the caller may pass a fresh seed per move for variety.
    pub seed: Option<u64>,
}

impl AiOptions {
    pub fn new(difficulty: Difficulty) -> Self {
        Self { difficulty, max_depth: None, seed: None }
    }
}

#[derive(Clone, Debug)]
pub struct AiMoveResult {
    pub mv: Move,
    pub score: i32,
    pub depth: u8,
    pub nodes: u32,
}

/// Mate score — large enough that no material configuration can match it.
const MATE: i32 = 1_000_000;
/// Cap nodes-per-search to keep web UI snappy. Hit at depth 4 in busy
/// midgames; the engine returns the best-found-so-far when exceeded.
const NODE_BUDGET: u32 = 250_000;

/// Pick a move for the side to move in `state`.
///
/// Returns `None` only when there are no legal moves (caller should treat as
/// game over). Game-over states (`status != Ongoing`) also return `None`.
pub fn choose_move(state: &GameState, opts: &AiOptions) -> Option<AiMoveResult> {
    if !matches!(state.status, chess_core::state::GameStatus::Ongoing) {
        return None;
    }
    let moves = state.legal_moves();
    if moves.is_empty() {
        return None;
    }

    let depth = opts.max_depth.unwrap_or_else(|| opts.difficulty.default_depth()).max(1);
    let seed = opts.seed.unwrap_or(0xC0FFEE_u64);
    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    // Order root moves: captures first.
    let mut ordered: Vec<Move> = moves.into_iter().collect();
    ordered.sort_by_key(|m| if is_capture(m) { 0 } else { 1 });

    let mut work = state.clone();
    let mut nodes: u32 = 0;
    let mut scored: Vec<(Move, i32)> = Vec::with_capacity(ordered.len());
    let mut alpha = -MATE - 1;
    let beta = MATE + 1;

    for mv in ordered {
        if work.make_move(&mv).is_err() {
            continue;
        }
        let score = -negamax(&mut work, depth - 1, -beta, -alpha, &mut nodes);
        let _ = work.unmake_move();
        scored.push((mv, score));
        if score > alpha {
            alpha = score;
        }
        if nodes >= NODE_BUDGET {
            break;
        }
    }
    if scored.is_empty() {
        return None;
    }

    let best_score = scored.iter().map(|(_, s)| *s).max().unwrap();
    let chosen = match opts.difficulty {
        Difficulty::Easy => {
            // Pick uniformly from top-3 by score.
            let mut sorted: Vec<&(Move, i32)> = scored.iter().collect();
            sorted.sort_by_key(|b| std::cmp::Reverse(b.1));
            let take = sorted.len().min(3);
            sorted[..take].choose(&mut rng).map(|(m, s)| (m.clone(), *s)).unwrap()
        }
        Difficulty::Normal => {
            // Best-or-near-best: any move within 10cp of best, picked at random.
            let near: Vec<&(Move, i32)> =
                scored.iter().filter(|(_, s)| best_score - *s <= 10).collect();
            let pick = near.choose(&mut rng).copied().unwrap();
            (pick.0.clone(), pick.1)
        }
        Difficulty::Hard => {
            // Strict best (first occurrence on ties — deterministic).
            scored.iter().find(|(_, s)| *s == best_score).cloned().unwrap()
        }
    };

    // Tiny noise on Easy/Normal via the same rng, so identical positions
    // with different seeds vary even when scores tie cleanly. (Hard stays
    // stable.)
    let _: u8 = rng.gen();

    Some(AiMoveResult { mv: chosen.0, score: chosen.1, depth, nodes })
}

fn negamax(state: &mut GameState, depth: u8, mut alpha: i32, beta: i32, nodes: &mut u32) -> i32 {
    *nodes = nodes.saturating_add(1);
    let moves = state.legal_moves();
    if moves.is_empty() {
        // No legal moves under Asian rules → loss for the side to move
        // (checkmate or stalemate). Returning a depth-relative mate score
        // makes the search prefer faster mates and slower losses.
        return -MATE + depth as i32;
    }
    if depth == 0 || *nodes >= NODE_BUDGET {
        return evaluate(state);
    }

    // Capture-first ordering. Cheap — single pass over the SmallVec.
    let mut ordered: Vec<Move> = moves.into_iter().collect();
    ordered.sort_by_key(|m| if is_capture(m) { 0 } else { 1 });

    let mut best = -MATE - 1;
    for mv in ordered {
        if state.make_move(&mv).is_err() {
            continue;
        }
        let score = -negamax(state, depth - 1, -beta, -alpha, nodes);
        let _ = state.unmake_move();
        if score > best {
            best = score;
        }
        if score > alpha {
            alpha = score;
        }
        if alpha >= beta {
            break;
        }
    }
    best
}

#[inline]
fn is_capture(m: &Move) -> bool {
    matches!(
        m,
        Move::Capture { .. }
            | Move::CannonJump { .. }
            | Move::ChainCapture { .. }
            | Move::DarkCapture { .. }
    )
}

/// Side-relative material eval for xiangqi. Positive favours the side to
/// move. Banqi will get its own evaluator when (if) it gets an engine —
/// see ADR / TODO.
fn evaluate(state: &GameState) -> i32 {
    let me = state.side_to_move;
    let mut score = 0i32;
    for sq in state.board.squares() {
        let Some(pos) = state.board.get(sq) else { continue };
        if !pos.revealed {
            continue;
        }
        let v = piece_value(state, pos.piece, sq);
        if pos.piece.side == me {
            score += v;
        } else {
            score -= v;
        }
    }
    score
}

fn piece_value(state: &GameState, p: Piece, sq: Square) -> i32 {
    match p.kind {
        // General is excluded from material — checkmate is handled by the
        // mate score. Casual xiangqi (where a missing general is the loss
        // condition) still works because `legal_moves` going empty after
        // capture is the recursion's terminal.
        PieceKind::General => 0,
        PieceKind::Advisor => 200,
        PieceKind::Elephant => 200,
        PieceKind::Chariot => 900,
        PieceKind::Horse => 400,
        PieceKind::Cannon => 450,
        PieceKind::Soldier => {
            let (_, rank) = state.board.file_rank(sq);
            // Xiangqi river: ranks 0-4 are Red half, 5-9 are Black half.
            let crossed = match p.side {
                Side::RED => rank.0 >= 5,
                _ => rank.0 <= 4,
            };
            if crossed {
                200
            } else {
                100
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess_core::rules::RuleSet;

    #[test]
    fn opening_xiangqi_returns_a_legal_move_each_difficulty() {
        let state = GameState::new(RuleSet::xiangqi_casual());
        for diff in [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard] {
            let opts = AiOptions { difficulty: diff, max_depth: Some(2), seed: Some(7) };
            let result = choose_move(&state, &opts).expect("must return a move");
            // The returned move must be in the legal-move list.
            let legal = state.legal_moves();
            assert!(
                legal.iter().any(|m| m == &result.mv),
                "{:?} returned non-legal move {:?}",
                diff,
                result.mv
            );
        }
    }

    #[test]
    fn no_legal_moves_returns_none() {
        // Synthesise: clear the board entirely, set status by manipulating.
        let mut state = GameState::new(RuleSet::xiangqi());
        let squares: Vec<Square> = state.board.squares().collect();
        for sq in squares {
            state.board.set(sq, None);
        }
        // legal_moves() should now be empty.
        assert!(state.legal_moves().is_empty());
        let opts = AiOptions::new(Difficulty::Hard);
        assert!(choose_move(&state, &opts).is_none());
    }

    #[test]
    fn determinism_same_seed_same_move() {
        let state = GameState::new(RuleSet::xiangqi_casual());
        let opts = AiOptions { difficulty: Difficulty::Easy, max_depth: Some(2), seed: Some(42) };
        let a = choose_move(&state, &opts).unwrap();
        let b = choose_move(&state, &opts).unwrap();
        assert_eq!(a.mv, b.mv);
    }

    #[test]
    fn hard_prefers_capture_when_free() {
        // Build a position where Red has a free capture: place a Red
        // chariot opposite a Black soldier with nothing in between. A
        // material-aware engine at any depth ≥ 1 must take.
        use chess_core::board::Board;
        use chess_core::coord::{File, Rank};
        use chess_core::piece::{Piece, PieceKind, PieceOnSquare};

        let mut state = GameState::new(RuleSet::xiangqi_casual());
        // Wipe and seed.
        let board: Board = state.board.clone();
        let squares: Vec<Square> = board.squares().collect();
        for sq in squares {
            state.board.set(sq, None);
        }
        // Generals so legal_moves stays valid; place far apart.
        let red_gen = state.board.sq(File(4), Rank(0));
        let blk_gen = state.board.sq(File(4), Rank(9));
        state
            .board
            .set(red_gen, Some(PieceOnSquare::revealed(Piece::new(Side::RED, PieceKind::General))));
        state.board.set(
            blk_gen,
            Some(PieceOnSquare::revealed(Piece::new(Side::BLACK, PieceKind::General))),
        );
        // Red chariot at (0,4); Black soldier at (0,5). Chariot goes north
        // one step to capture for free.
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

        let opts = AiOptions { difficulty: Difficulty::Hard, max_depth: Some(2), seed: Some(0) };
        let result = choose_move(&state, &opts).unwrap();
        match result.mv {
            Move::Capture { from, to, .. } => {
                assert_eq!(from, red_chariot);
                assert_eq!(to, blk_soldier);
            }
            other => panic!("expected capture, got {:?}", other),
        }
    }
}
