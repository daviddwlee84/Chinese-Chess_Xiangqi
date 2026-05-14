//! Banqi house rules.
//!
//! Five independent toggles + three named presets. See `docs/rules/banqi-house.md`.

use bitflags::bitflags;

bitflags! {
    /// Toggleable banqi house rules. Combine with `|`.
    #[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
    #[derive(serde::Serialize, serde::Deserialize)]
    pub struct HouseRules: u32 {
        /// 連吃 — chain captures along the same line.
        const CHAIN_CAPTURE       = 1 << 0;
        /// 暗吃 — atomic reveal+capture of a face-down piece (probe variant:
        /// rank-fail leaves both pieces in place with target now revealed).
        const DARK_CAPTURE        = 1 << 1;
        /// 車衝 — chariot moves multi-square in a line.
        const CHARIOT_RUSH        = 1 << 2;
        /// 馬斜 — horse adds 4 diagonal one-step moves; diagonal captures
        /// ignore rank (any piece).
        const HORSE_DIAGONAL      = 1 << 3;
        /// 炮快移 — cannon non-capturing move slides any distance.
        const CANNON_FAST_MOVE    = 1 << 4;
        /// 暗吃 trade variant — on rank-fail the small attacker dies (is
        /// removed), target stays revealed. Implies DARK_CAPTURE.
        const DARK_CAPTURE_TRADE  = 1 << 5;
        /// 預先指定顏色 — banqi only. When set, seat ↔ piece-colour is
        /// fixed at room creation (host = the configured colour, which
        /// defaults to Red and therefore moves first). When unset
        /// (default), banqi defers colour assignment until the first
        /// reveal: either seat may flip the first hidden tile and the
        /// revealed colour locks the seat→colour mapping per the
        /// existing Taiwan rule (flipper plays the colour they reveal).
        /// See `GameState::banqi_awaiting_first_flip` and ADR/banqi docs.
        const PREASSIGN_COLORS    = 1 << 6;
    }
}

/// Classic banqi, no house rules.
pub const PRESET_PURIST: HouseRules = HouseRules::empty();

/// Common Taiwanese house ruleset: chain captures + chariot rush.
pub const PRESET_TAIWAN: HouseRules = HouseRules::CHAIN_CAPTURE.union(HouseRules::CHARIOT_RUSH);

/// Aggressive ruleset enabling chain, dark-capture, chariot rush, horse
/// diagonal.
pub const PRESET_AGGRESSIVE: HouseRules = HouseRules::CHAIN_CAPTURE
    .union(HouseRules::DARK_CAPTURE)
    .union(HouseRules::CHARIOT_RUSH)
    .union(HouseRules::HORSE_DIAGONAL);

/// `DARK_CAPTURE_TRADE` implies `DARK_CAPTURE`. Returns the canonical form.
pub fn normalize(mut rules: HouseRules) -> HouseRules {
    if rules.contains(HouseRules::DARK_CAPTURE_TRADE) {
        rules.insert(HouseRules::DARK_CAPTURE);
    }
    rules
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_distinct() {
        assert_ne!(PRESET_PURIST, PRESET_TAIWAN);
        assert_ne!(PRESET_TAIWAN, PRESET_AGGRESSIVE);
        assert_ne!(PRESET_PURIST, PRESET_AGGRESSIVE);
    }

    #[test]
    fn taiwan_preset_has_chain_and_rush() {
        assert!(PRESET_TAIWAN.contains(HouseRules::CHAIN_CAPTURE));
        assert!(PRESET_TAIWAN.contains(HouseRules::CHARIOT_RUSH));
        assert!(!PRESET_TAIWAN.contains(HouseRules::DARK_CAPTURE));
    }

    #[test]
    fn dark_capture_trade_normalizes_to_include_dark_capture() {
        let r = normalize(HouseRules::DARK_CAPTURE_TRADE);
        assert!(r.contains(HouseRules::DARK_CAPTURE));
        assert!(r.contains(HouseRules::DARK_CAPTURE_TRADE));
    }
}
