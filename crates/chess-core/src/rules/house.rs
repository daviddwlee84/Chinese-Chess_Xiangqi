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
        const CHAIN_CAPTURE     = 1 << 0;
        /// 暗連 — chain through face-down squares (implies CHAIN_CAPTURE).
        const DARK_CHAIN        = 1 << 1;
        /// 車衝 — chariot moves multi-square in a line.
        const CHARIOT_RUSH      = 1 << 2;
        /// 馬斜 — horse moves like xiangqi (L with leg).
        const HORSE_DIAGONAL    = 1 << 3;
        /// 炮快移 — cannon non-capturing move slides any distance.
        const CANNON_FAST_MOVE  = 1 << 4;
    }
}

/// Classic banqi, no house rules.
pub const PRESET_PURIST: HouseRules = HouseRules::empty();

/// Common Taiwanese house ruleset: chain captures + chariot rush.
pub const PRESET_TAIWAN: HouseRules = HouseRules::CHAIN_CAPTURE.union(HouseRules::CHARIOT_RUSH);

/// Aggressive ruleset enabling chain (with dark), chariot rush, horse diagonal.
pub const PRESET_AGGRESSIVE: HouseRules = HouseRules::CHAIN_CAPTURE
    .union(HouseRules::DARK_CHAIN)
    .union(HouseRules::CHARIOT_RUSH)
    .union(HouseRules::HORSE_DIAGONAL);

/// `DARK_CHAIN` implies `CHAIN_CAPTURE`. Returns the canonical form.
pub fn normalize(mut rules: HouseRules) -> HouseRules {
    if rules.contains(HouseRules::DARK_CHAIN) {
        rules.insert(HouseRules::CHAIN_CAPTURE);
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
        assert!(!PRESET_TAIWAN.contains(HouseRules::DARK_CHAIN));
    }

    #[test]
    fn dark_chain_normalizes_to_include_chain() {
        let r = normalize(HouseRules::DARK_CHAIN);
        assert!(r.contains(HouseRules::CHAIN_CAPTURE));
        assert!(r.contains(HouseRules::DARK_CHAIN));
    }
}
