//! Xiangqi perft (move-generation node count).
//!
//! Locks in tree-size values so any rule-geometry regression shows up
//! immediately. Values are observed at first run; treat them as snapshots —
//! review carefully if a rule change is intentional.

use chess_core::moves::Move;
use chess_core::rules::RuleSet;
use chess_core::state::GameState;

fn perft(state: &mut GameState, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let moves: Vec<Move> = state.legal_moves().into_iter().collect();
    let mut nodes = 0;
    for m in &moves {
        state.make_move(m).expect("make_move on legal move");
        nodes += perft(state, depth - 1);
        state.unmake_move().expect("unmake_move always succeeds");
    }
    nodes
}

#[test]
fn perft_depth_1_is_44() {
    let mut state = GameState::new(RuleSet::xiangqi());
    assert_eq!(perft(&mut state, 1), 44);
}

#[test]
fn perft_depth_2_locked() {
    let mut state = GameState::new(RuleSet::xiangqi());
    // Locked-in observed value. If this changes, audit the rule change.
    let count = perft(&mut state, 2);
    eprintln!("perft(2) = {count}");
    assert_eq!(count, 1920, "perft(2) regressed");
}

#[test]
#[ignore = "depth-3 is slow without optimization; run with --ignored"]
fn perft_depth_3_locked() {
    let mut state = GameState::new(RuleSet::xiangqi());
    let count = perft(&mut state, 3);
    eprintln!("perft(3) = {count}");
    assert_eq!(count, 79666, "perft(3) regressed");
}
