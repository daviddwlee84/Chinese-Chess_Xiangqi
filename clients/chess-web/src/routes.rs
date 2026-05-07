//! Variant-slug parsing for `/local/:variant`. Pure logic — native-testable.

use chess_core::rules::Variant;

pub fn parse_variant_slug(slug: &str) -> Option<Variant> {
    match slug {
        "xiangqi" => Some(Variant::Xiangqi),
        "banqi" => Some(Variant::Banqi),
        "three-kingdom" => Some(Variant::ThreeKingdomBanqi),
        _ => None,
    }
}

pub fn variant_slug(variant: Variant) -> &'static str {
    match variant {
        Variant::Xiangqi => "xiangqi",
        Variant::Banqi => "banqi",
        Variant::ThreeKingdomBanqi => "three-kingdom",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_slugs_parse() {
        assert!(matches!(parse_variant_slug("xiangqi"), Some(Variant::Xiangqi)));
        assert!(matches!(parse_variant_slug("banqi"), Some(Variant::Banqi)));
        assert!(matches!(parse_variant_slug("three-kingdom"), Some(Variant::ThreeKingdomBanqi)));
    }

    #[test]
    fn unknown_slug_returns_none() {
        assert!(parse_variant_slug("chess").is_none());
        assert!(parse_variant_slug("").is_none());
        assert!(parse_variant_slug("XIANGQI").is_none());
    }

    #[test]
    fn slug_round_trip() {
        for v in [Variant::Xiangqi, Variant::Banqi, Variant::ThreeKingdomBanqi] {
            assert_eq!(parse_variant_slug(variant_slug(v)), Some(v));
        }
    }
}
