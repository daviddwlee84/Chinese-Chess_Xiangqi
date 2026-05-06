//! End-to-end replay test from the public API surface:
//! play a real game → build Replay → JSON → reload → replay forward →
//! state matches at every intermediate step. Also exercises the
//! "load fixture as initial position" pattern (endgame puzzle mode).

use chess_core::moves::Move;
use chess_core::replay::{Replay, ReplayMeta};
use chess_core::rules::{HouseRules, RuleSet};
use chess_core::state::GameState;

fn play_n_moves(state: &mut GameState, n: usize) {
    for _ in 0..n {
        let moves = state.legal_moves();
        let Some(m) = moves.into_iter().next() else { break };
        state.make_move(&m).expect("legal move applies");
    }
}

#[test]
fn xiangqi_full_replay_round_trip() {
    let mut original = GameState::new(RuleSet::xiangqi());
    let mut step_states = vec![original.clone()];
    play_n_moves(&mut original, 8);
    // Capture the state at every step (we played 1 at a time but the loop
    // above doesn't snapshot — re-run with explicit snapshots):
    let mut original = GameState::new(RuleSet::xiangqi());
    for _ in 0..8 {
        let m = original.legal_moves().into_iter().next().unwrap();
        original.make_move(&m).unwrap();
        step_states.push(original.clone());
    }

    // Build replay from the played-out state.
    let replay =
        Replay::from_game(&original, ReplayMeta { red: Some("test".into()), ..Default::default() })
            .expect("replay capture");

    // JSON round-trip.
    let json = replay.to_json().expect("serialize");
    let decoded = Replay::from_json(&json).expect("deserialize");
    assert_eq!(replay, decoded, "JSON round-trip preserves replay");

    // Every intermediate state matches the live-played snapshot.
    for (k, expected) in step_states.iter().enumerate() {
        let got = decoded.play_to(k).expect("play_to in range");
        assert_eq!(got, *expected, "step {k} replayed state mismatch");
    }
}

#[test]
fn banqi_replay_with_seed_round_trip() {
    let mut original = GameState::new(RuleSet::banqi_with_seed(HouseRules::CHAIN_CAPTURE, 11));
    play_n_moves(&mut original, 5);
    let replay = Replay::from_game(&original, ReplayMeta::empty()).unwrap();
    let json = replay.to_json().unwrap();
    let decoded = Replay::from_json(&json).unwrap();
    assert_eq!(decoded.final_state().unwrap(), original);
}

#[test]
fn fixture_loaded_as_replay_initial() {
    // Endgame puzzle mode: load a hand-crafted position, treat it as the
    // initial state of a fresh replay, hand the player a GameState to work
    // from.
    let text = std::fs::read_to_string("tests/fixtures/xiangqi/three-chariot-mate.pos")
        .expect("fixture exists");
    let initial = GameState::from_pos_text(&text).expect("parse fixture");
    let replay = Replay::new(initial.clone(), ReplayMeta::empty());
    let starting = replay.play_to(0).expect("play_to(0) returns initial clone");
    assert_eq!(starting, initial);
    assert_eq!(replay.len(), 0);
}

#[test]
fn fork_from_replay_midpoint_can_play_forward() {
    // Play 6 moves, fork at step 3, make a different move on the fork,
    // confirm the fork is a fully-functional GameState that can keep going.
    let mut original = GameState::new(RuleSet::xiangqi());
    play_n_moves(&mut original, 6);
    let replay = Replay::from_game(&original, ReplayMeta::empty()).unwrap();

    let mut fork = replay.play_to(3).expect("fork at step 3");
    let recorded_step3_move = &replay.moves[3];

    let alt = fork
        .legal_moves()
        .into_iter()
        .find(|m| !same_move(m, recorded_step3_move))
        .expect("must be a different legal move available");
    fork.make_move(&alt).unwrap();

    // The fork is now diverged from the original replay.
    let diverged_at_step3 = replay.play_to(4).unwrap();
    assert_ne!(fork.board, diverged_at_step3.board, "fork should diverge");

    // And the fork can keep playing.
    play_n_moves(&mut fork, 3);
    assert!(fork.history.len() >= 4, "fork should accumulate its own history");
}

fn same_move(a: &Move, b: &Move) -> bool {
    a == b
}
