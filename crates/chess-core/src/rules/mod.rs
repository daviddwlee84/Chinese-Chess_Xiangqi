//! Rules.
//!
//! `RuleSet` is plain data; move generation is free functions dispatching
//! on `Variant` and consulting `HouseRules` bitflags. See ADR-0003.

pub mod banqi;
pub mod house;
pub mod three_kingdom;
pub mod xiangqi;

pub use house::{HouseRules, PRESET_AGGRESSIVE, PRESET_PURIST, PRESET_TAIWAN};

use serde::{Deserialize, Serialize};

use crate::moves::MoveList;
use crate::state::GameState;

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum Variant {
    Xiangqi,
    Banqi,
    /// 三國暗棋. Move-gen lands in PR 2; type exists from PR 1.
    ThreeKingdomBanqi,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct DrawPolicy {
    /// Plies without progress (capture/reveal) before a draw is forced.
    pub no_progress_plies: u16,
    /// How many position repetitions trigger a draw.
    pub repetition_threshold: u8,
}

impl DrawPolicy {
    pub const XIANGQI: DrawPolicy = DrawPolicy { no_progress_plies: 60, repetition_threshold: 3 };
    pub const BANQI: DrawPolicy = DrawPolicy { no_progress_plies: 40, repetition_threshold: 3 };
}

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct RuleSet {
    pub variant: Variant,
    pub house: HouseRules,
    pub draw_policy: DrawPolicy,
    /// Seed for banqi shuffling. `None` = nondeterministic.
    pub banqi_seed: Option<u64>,
    /// Casual / training mode for xiangqi: when true, the legality filter
    /// no longer rejects moves that leave the mover's own general in check.
    /// The game ends only when a general is actually captured
    /// (`WinReason::GeneralCaptured`). Default `false` (standard rules).
    /// `#[serde(default)]` keeps older snapshots loadable.
    #[serde(default)]
    pub xiangqi_allow_self_check: bool,
}

impl RuleSet {
    pub fn xiangqi() -> Self {
        Self {
            variant: Variant::Xiangqi,
            house: HouseRules::empty(),
            draw_policy: DrawPolicy::XIANGQI,
            banqi_seed: None,
            xiangqi_allow_self_check: false,
        }
    }

    /// Xiangqi without the self-check legality filter — moves that expose
    /// your own general are permitted; you lose when the general is actually
    /// captured. Useful for casual / "let me lose if I want" play.
    pub fn xiangqi_casual() -> Self {
        Self { xiangqi_allow_self_check: true, ..Self::xiangqi() }
    }

    pub fn banqi(house: HouseRules) -> Self {
        Self {
            variant: Variant::Banqi,
            house,
            draw_policy: DrawPolicy::BANQI,
            banqi_seed: None,
            xiangqi_allow_self_check: false,
        }
    }

    pub fn banqi_with_seed(house: HouseRules, seed: u64) -> Self {
        Self {
            variant: Variant::Banqi,
            house,
            draw_policy: DrawPolicy::BANQI,
            banqi_seed: Some(seed),
            xiangqi_allow_self_check: false,
        }
    }

    pub fn three_kingdom() -> Self {
        Self {
            variant: Variant::ThreeKingdomBanqi,
            house: HouseRules::empty(),
            draw_policy: DrawPolicy::BANQI,
            banqi_seed: None,
            xiangqi_allow_self_check: false,
        }
    }
}

/// Single dispatch point for move generation. Each variant's `generate`
/// fills `out` with all legal moves for the side to move.
pub fn generate_moves(state: &GameState, out: &mut MoveList) {
    match state.rules.variant {
        Variant::Xiangqi => xiangqi::generate(state, out),
        Variant::Banqi => banqi::generate(state, out),
        Variant::ThreeKingdomBanqi => three_kingdom::generate(state, out),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xiangqi_ruleset_defaults() {
        let r = RuleSet::xiangqi();
        assert_eq!(r.variant, Variant::Xiangqi);
        assert!(r.house.is_empty());
        assert_eq!(r.draw_policy.no_progress_plies, 60);
    }

    #[test]
    fn banqi_ruleset_carries_house_rules() {
        let r = RuleSet::banqi(HouseRules::CHAIN_CAPTURE | HouseRules::CHARIOT_RUSH);
        assert!(r.house.contains(HouseRules::CHAIN_CAPTURE));
        assert!(r.house.contains(HouseRules::CHARIOT_RUSH));
        assert!(!r.house.contains(HouseRules::DARK_CHAIN));
    }

    #[test]
    fn banqi_ruleset_with_seed() {
        let r = RuleSet::banqi_with_seed(HouseRules::empty(), 42);
        assert_eq!(r.banqi_seed, Some(42));
    }
}
