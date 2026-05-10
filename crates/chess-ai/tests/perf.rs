//! Performance smoke tests for `chess_ai::choose_move`.
//!
//! `#[ignore]` so they don't run in normal CI / `cargo test`. Invoke with:
//!
//! ```sh
//! cargo test -p chess-ai --release perf -- --ignored --nocapture
//! ```
//!
//! Output is human-readable (one row per Strategy×Difficulty×fixture) and
//! gets pasted into `docs/ai/perf.md` after each significant change to the
//! search or evaluator. There are no hard pass/fail thresholds — these
//! tests exist to catch order-of-magnitude regressions, not microsecond
//! drift. Asserting on wall-clock would make CI flaky across machines.
//!
//! Each fixture is run 3 times and the median wall-clock is reported.
//! Nodes are deterministic given (state, depth, evaluator), so we report
//! the value from the first run.

use std::time::Instant;

use chess_ai::{AiOptions, Difficulty, Randomness, Strategy};
use chess_core::board::Board;
use chess_core::coord::{File, Rank, Square};
use chess_core::piece::{Piece, PieceKind, PieceOnSquare, Side};
use chess_core::rules::RuleSet;
use chess_core::state::GameState;

/// Each (label, factory). Factories return a fresh GameState — important
/// because `choose_move` doesn't mutate but the bench loop does many
/// passes and we want each pass deterministic.
type Fixture = (&'static str, fn() -> GameState);

fn fixtures() -> Vec<Fixture> {
    vec![
        ("opening (initial xiangqi)", initial_xiangqi),
        ("midgame (4 pieces removed)", midgame_xiangqi),
        ("sparse endgame (5 pieces total)", endgame_xiangqi),
    ]
}

fn initial_xiangqi() -> GameState {
    GameState::new(RuleSet::xiangqi_casual())
}

/// Take the initial position and remove a few pieces from each side to
/// simulate a midgame: more open lines = wider branching factor.
fn midgame_xiangqi() -> GameState {
    let mut state = GameState::new(RuleSet::xiangqi_casual());
    // Remove both Red soldiers on the flanks and both Black ditto.
    let to_clear = [(File(0), Rank(3)), (File(8), Rank(3)), (File(0), Rank(6)), (File(8), Rank(6))];
    for (f, r) in to_clear {
        let sq = state.board.sq(f, r);
        state.board.set(sq, None);
    }
    state
}

/// 5-piece sparse endgame: two generals + a chariot per side + one extra
/// soldier. Branching factor ~12-15.
fn endgame_xiangqi() -> GameState {
    let mut state = GameState::new(RuleSet::xiangqi_casual());
    let board: Board = state.board.clone();
    let squares: Vec<Square> = board.squares().collect();
    for sq in squares {
        state.board.set(sq, None);
    }
    let red_gen = state.board.sq(File(4), Rank(0));
    let blk_gen = state.board.sq(File(4), Rank(9));
    let red_chariot = state.board.sq(File(0), Rank(0));
    let blk_chariot = state.board.sq(File(8), Rank(9));
    let red_soldier = state.board.sq(File(4), Rank(5));
    state
        .board
        .set(red_gen, Some(PieceOnSquare::revealed(Piece::new(Side::RED, PieceKind::General))));
    state
        .board
        .set(blk_gen, Some(PieceOnSquare::revealed(Piece::new(Side::BLACK, PieceKind::General))));
    state
        .board
        .set(red_chariot, Some(PieceOnSquare::revealed(Piece::new(Side::RED, PieceKind::Chariot))));
    state.board.set(
        blk_chariot,
        Some(PieceOnSquare::revealed(Piece::new(Side::BLACK, PieceKind::Chariot))),
    );
    state
        .board
        .set(red_soldier, Some(PieceOnSquare::revealed(Piece::new(Side::RED, PieceKind::Soldier))));
    state
}

fn median(mut v: Vec<u128>) -> u128 {
    v.sort_unstable();
    v[v.len() / 2]
}

#[test]
#[ignore]
fn perf_table() {
    println!();
    println!("# chess-ai performance — release profile");
    println!();
    println!("Run via: `cargo test -p chess-ai --release perf -- --ignored --nocapture`");
    println!();
    println!("| Fixture | Strategy | Difficulty | Depth | Nodes | Median ms (3 runs) | nodes/ms |");
    println!("|---------|----------|------------|-------|-------|--------------------|---------|");

    let strategies = [
        ("v1", Strategy::MaterialV1),
        ("v2", Strategy::MaterialPstV2),
        ("v3", Strategy::MaterialKingSafetyPstV3),
        ("v4", Strategy::QuiescenceMvvLvaV4),
        ("v5", Strategy::IterativeDeepeningTtV5),
    ];
    let difficulties =
        [("Easy", Difficulty::Easy), ("Normal", Difficulty::Normal), ("Hard", Difficulty::Hard)];

    for (fix_label, factory) in fixtures() {
        for (strat_label, strategy) in strategies {
            for (diff_label, difficulty) in difficulties {
                // Pin Randomness::STRICT so we measure the search cost
                // alone, not RNG-driven variation.
                let opts = AiOptions {
                    difficulty,
                    max_depth: None,
                    seed: Some(0),
                    strategy,
                    randomness: Some(Randomness::STRICT),
                    node_budget: None,
                };
                let mut nodes = 0u32;
                let mut depth = 0u8;
                let mut times: Vec<u128> = Vec::with_capacity(3);
                for _ in 0..3 {
                    let state = factory();
                    let t0 = Instant::now();
                    let result = chess_ai::choose_move(&state, &opts).expect("must return a move");
                    times.push(t0.elapsed().as_micros());
                    nodes = result.nodes;
                    depth = result.depth;
                }
                let med_us = median(times);
                let med_ms = med_us as f64 / 1000.0;
                let nodes_per_ms =
                    if med_us > 0 { (nodes as f64) * 1000.0 / (med_us as f64) } else { 0.0 };
                println!(
                    "| {} | {} | {} | {} | {} | {:.2} | {:.0} |",
                    fix_label, strat_label, diff_label, depth, nodes, med_ms, nodes_per_ms,
                );
            }
        }
    }
    println!();
    println!("**Notes**:");
    println!("- All runs use `Randomness::STRICT` so the measurement is search cost only.");
    println!("- `Nodes` is the count visited up to the 250 000 budget (search bails to best-so-far when hit).");
    println!("- `Difficulty::default_depth` maps Easy→1, Normal→3, Hard→4.");
    println!("- Branching factor for opening xiangqi: ~40 moves; with α-β + capture-first the effective branch ≈ b^(3d/4).");
}
