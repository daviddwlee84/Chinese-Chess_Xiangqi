//! Replay format: initial state + move sequence + metadata.
//!
//! Solves three problems with one primitive:
//! - **Save/load games** — `to_json` / `from_json`.
//! - **Animation playback** — `play_to(step)` returns the position at each
//!   intermediate step; UIs scrub through them on a timer.
//! - **Fork from any state** — `play_to(k)` returns a fresh `GameState` you
//!   can keep playing on; the fork has its own independent history.
//!
//! Endgame puzzle mode uses this too: load a `Replay` whose `initial` is the
//! puzzle position with `moves: []`, hand the player a `GameState` from
//! `play_to(0)`, and let them try to find the win.

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::moves::Move;
use crate::state::GameState;

/// Optional metadata attached to a replay. All fields free-form.
#[derive(Clone, Default, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct ReplayMeta {
    pub date: Option<String>,
    pub red: Option<String>,
    pub black: Option<String>,
    /// PGN-style result: `"1-0"`, `"0-1"`, `"1/2-1/2"`, `"*"`. Free-form.
    pub result: Option<String>,
    pub comment: Option<String>,
}

impl ReplayMeta {
    pub fn empty() -> Self {
        Self::default()
    }
}

/// A complete replay: the initial position + every move played from it.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct Replay {
    pub version: u32,
    pub metadata: ReplayMeta,
    pub initial: GameState,
    pub moves: Vec<Move>,
}

const REPLAY_VERSION: u32 = 1;

impl Replay {
    /// Build a fresh replay from a starting position. `moves` starts empty.
    pub fn new(initial: GameState, metadata: ReplayMeta) -> Self {
        Self { version: REPLAY_VERSION, metadata, initial, moves: Vec::new() }
    }

    /// Capture an in-progress game's history into a Replay. The given
    /// `state.history` is replayed back to the starting position via
    /// `unmake_move`, then those moves are recorded in order.
    pub fn from_game(state: &GameState, metadata: ReplayMeta) -> Result<Self, CoreError> {
        let moves: Vec<Move> = state.history.iter().map(|r| r.the_move.clone()).collect();
        let mut initial = state.clone();
        for _ in 0..state.history.len() {
            initial.unmake_move()?;
        }
        Ok(Self { version: REPLAY_VERSION, metadata, initial, moves })
    }

    /// Number of moves recorded.
    #[inline]
    pub fn len(&self) -> usize {
        self.moves.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.moves.is_empty()
    }

    /// Append a move to the replay. Caller is responsible for legality
    /// (typically by calling `state.make_move(&m)` and then mirroring it
    /// here).
    pub fn push(&mut self, m: Move) {
        self.moves.push(m);
    }

    /// Replay forward `step` moves from `initial`, returning the resulting
    /// `GameState`. `step == 0` returns a clone of `initial`. The returned
    /// state has `history.len() == step` and is fully usable — call
    /// `make_move` on it to fork.
    pub fn play_to(&self, step: usize) -> Result<GameState, CoreError> {
        if step > self.moves.len() {
            return Err(CoreError::Illegal("step exceeds replay length"));
        }
        let mut state = self.initial.clone();
        for m in &self.moves[..step] {
            state.make_move(m)?;
        }
        Ok(state)
    }

    /// Final state (alias for `play_to(self.len())`).
    pub fn final_state(&self) -> Result<GameState, CoreError> {
        self.play_to(self.len())
    }

    /// Iterate intermediate states: yields `(step, state_before_move)` for
    /// each step in the replay, ending with the final state at
    /// `step == self.len()`.
    pub fn iter_states(&self) -> impl Iterator<Item = Result<GameState, CoreError>> + '_ {
        (0..=self.moves.len()).map(move |i| self.play_to(i))
    }

    pub fn to_json(&self) -> Result<String, CoreError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| CoreError::BadNotation(format!("replay json serialize: {e}")))
    }

    pub fn from_json(s: &str) -> Result<Self, CoreError> {
        serde_json::from_str(s)
            .map_err(|e| CoreError::BadNotation(format!("replay json parse: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::RuleSet;

    fn play_a_few(state: &mut GameState, n: usize) {
        for _ in 0..n {
            let moves = state.legal_moves();
            if let Some(m) = moves.into_iter().next() {
                state.make_move(&m).unwrap();
            }
        }
    }

    #[test]
    fn from_game_captures_history_and_initial() {
        let mut state = GameState::new(RuleSet::xiangqi());
        let initial_clone = state.clone();
        play_a_few(&mut state, 4);
        assert_eq!(state.history.len(), 4);

        let replay = Replay::from_game(&state, ReplayMeta::empty()).unwrap();
        assert_eq!(replay.len(), 4);
        assert_eq!(replay.initial.board, initial_clone.board);
        assert_eq!(replay.initial.history.len(), 0);
    }

    #[test]
    fn play_to_round_trip() {
        let mut state = GameState::new(RuleSet::xiangqi());
        play_a_few(&mut state, 3);

        let replay = Replay::from_game(&state, ReplayMeta::empty()).unwrap();
        let final_state = replay.final_state().unwrap();
        assert_eq!(final_state, state);
    }

    #[test]
    fn play_to_intermediate_steps_match_history() {
        // Build a played-out state, snapshot the state at each step,
        // then verify replay.play_to(k) matches the snapshot.
        let mut state = GameState::new(RuleSet::xiangqi());
        let mut snapshots = vec![state.clone()];
        for _ in 0..5 {
            let m = state.legal_moves().into_iter().next().unwrap();
            state.make_move(&m).unwrap();
            snapshots.push(state.clone());
        }
        let replay = Replay::from_game(&state, ReplayMeta::empty()).unwrap();
        for (k, expected) in snapshots.iter().enumerate() {
            let got = replay.play_to(k).unwrap();
            assert_eq!(got, *expected, "step {k} state mismatch");
        }
    }

    #[test]
    fn fork_from_midgame_diverges() {
        let mut state = GameState::new(RuleSet::xiangqi());
        play_a_few(&mut state, 4);
        let replay = Replay::from_game(&state, ReplayMeta::empty()).unwrap();

        let mut fork_a = replay.play_to(2).unwrap();
        let mut fork_b = replay.play_to(2).unwrap();
        assert_eq!(fork_a, fork_b, "two forks at the same step start identical");

        // Make different moves on each fork.
        let moves_a = fork_a.legal_moves();
        let moves_b = fork_b.legal_moves();
        let m_a = moves_a.iter().next().unwrap();
        let m_b = moves_b.iter().find(|m| *m != m_a).expect("at least two legal moves at step 2");
        fork_a.make_move(m_a).unwrap();
        fork_b.make_move(m_b).unwrap();
        assert_ne!(fork_a.board, fork_b.board, "different moves should diverge");
    }

    #[test]
    fn json_round_trip() {
        let mut state = GameState::new(RuleSet::xiangqi());
        play_a_few(&mut state, 3);
        let replay = Replay::from_game(
            &state,
            ReplayMeta {
                red: Some("Alice".into()),
                black: Some("Bob".into()),
                result: Some("*".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let json = replay.to_json().unwrap();
        let decoded = Replay::from_json(&json).unwrap();
        assert_eq!(replay, decoded);
    }

    #[test]
    fn play_to_out_of_range_errors() {
        let state = GameState::new(RuleSet::xiangqi());
        let replay = Replay::from_game(&state, ReplayMeta::empty()).unwrap();
        assert!(replay.play_to(0).is_ok());
        assert!(replay.play_to(1).is_err()); // empty replay
    }

    #[test]
    fn iter_states_yields_n_plus_one() {
        let mut state = GameState::new(RuleSet::xiangqi());
        play_a_few(&mut state, 3);
        let replay = Replay::from_game(&state, ReplayMeta::empty()).unwrap();
        let count = replay.iter_states().filter_map(Result::ok).count();
        assert_eq!(count, 4); // before move 0, after move 0, 1, 2
    }
}
