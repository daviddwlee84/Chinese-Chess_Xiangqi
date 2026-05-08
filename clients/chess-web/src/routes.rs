//! Variant-slug parsing for `/local/:variant` plus URL-query rule encoding.
//!
//! Pure logic — native-testable. The picker page builds a query string from
//! its form state via [`build_local_query`]; the local page parses it back
//! into a [`RuleSet`] via [`parse_local_rules`] + [`build_rule_set`].

use chess_ai::{Difficulty, Strategy};
use chess_core::piece::Side;
use chess_core::rules::{
    house, HouseRules, RuleSet, Variant, PRESET_AGGRESSIVE, PRESET_PURIST, PRESET_TAIWAN,
};

/// Local-page play mode. `Pvp` is the default pass-and-play; `VsAi`
/// drives the in-process alpha-beta engine (xiangqi only).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub enum PlayMode {
    #[default]
    Pvp,
    VsAi,
}

pub fn hosting_mode() -> &'static str {
    env!("CHESS_WEB_HOSTING")
}

pub fn is_static_hosting() -> bool {
    hosting_mode() == "static"
}

pub fn base_path() -> &'static str {
    env!("CHESS_WEB_BASE_PATH")
}

pub fn router_base() -> Option<&'static str> {
    let base = base_path();
    if base.is_empty() {
        None
    } else {
        Some(base)
    }
}

pub fn app_href(path: &str) -> String {
    app_href_with_base(base_path(), path)
}

pub fn app_href_with_base(base: &str, path: &str) -> String {
    let path = if path.is_empty() { "/" } else { path };
    let path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
    let base = normalize_base_path(base);
    if base.is_empty() {
        path
    } else if path == "/" {
        format!("{base}/")
    } else {
        format!("{base}{path}")
    }
}

fn normalize_base_path(base: &str) -> String {
    let trimmed = base.trim().trim_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("/{trimmed}")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WsBaseError {
    Empty,
    BadScheme,
}

pub fn normalize_ws_base(raw: &str) -> Result<String, WsBaseError> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(WsBaseError::Empty);
    }
    if !(trimmed.starts_with("ws://") || trimmed.starts_with("wss://")) {
        return Err(WsBaseError::BadScheme);
    }
    Ok(trimmed.to_string())
}

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

/// URL-derived rule choices for `/local/:variant`. Used as an intermediate
/// between the picker form and the engine `RuleSet`.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct LocalRulesParams {
    /// Xiangqi only. `false` (default) = casual / no self-check filter;
    /// `true` = standard rules where leaving your general in check is illegal.
    pub strict: bool,
    /// Banqi only. Bitflag set; `DARK_CAPTURE_TRADE` auto-implies
    /// `DARK_CAPTURE` after [`house::normalize`] in the parser.
    pub house: HouseRules,
    /// Banqi only. `None` = engine picks (non-deterministic on native;
    /// browser uses `getrandom` JS feature).
    pub seed: Option<u64>,
    /// Xiangqi only (banqi/three-kingdom ignore). Default `Pvp`.
    pub mode: PlayMode,
    /// `VsAi` only — which side the AI plays. Default Black (player is Red,
    /// the traditional first-mover). Ignored when `mode == Pvp`.
    pub ai_side: Side,
    /// `VsAi` only — search difficulty. Ignored when `mode == Pvp`.
    pub ai_difficulty: Difficulty,
    /// `VsAi` only — engine version. Defaults to [`Strategy::default`]
    /// (v2 since 2026-05-08). Versioned, switchable, non-overwriting:
    /// older strategies stay reachable via `?engine=v1`.
    pub ai_strategy: Strategy,
}

impl Default for LocalRulesParams {
    fn default() -> Self {
        Self {
            strict: false,
            house: HouseRules::empty(),
            seed: None,
            mode: PlayMode::Pvp,
            ai_side: Side::BLACK,
            ai_difficulty: Difficulty::Normal,
            ai_strategy: Strategy::default(),
        }
    }
}

/// Parse `?strict=1&house=chain,rush&preset=taiwan&seed=42` into a
/// [`LocalRulesParams`]. Unknown tokens are silently dropped — this is a
/// best-effort decoder for shareable URLs.
///
/// `house=` overrides `preset=` when both are present; the picker only
/// emits `house=` so this just covers manually-typed URLs.
pub fn parse_local_rules(get: impl Fn(&str) -> Option<String>) -> LocalRulesParams {
    let strict = matches!(get("strict").as_deref(), Some("1") | Some("true"));
    let preset = get("preset").as_deref().and_then(parse_preset_token);
    let house_csv = get("house").as_deref().map(parse_house_csv);
    let house = house_csv.or(preset).unwrap_or_else(HouseRules::empty);
    let seed = get("seed").and_then(|s| s.parse::<u64>().ok());
    let mode = match get("mode").as_deref() {
        Some("ai") | Some("vsai") => PlayMode::VsAi,
        _ => PlayMode::Pvp,
    };
    let ai_side = match get("ai").as_deref() {
        Some("red") => Side::RED,
        _ => Side::BLACK,
    };
    let ai_difficulty =
        get("diff").as_deref().and_then(Difficulty::parse).unwrap_or(Difficulty::Normal);
    let ai_strategy = get("engine").as_deref().and_then(Strategy::parse).unwrap_or_default();
    LocalRulesParams {
        strict,
        house: house::normalize(house),
        seed,
        mode,
        ai_side,
        ai_difficulty,
        ai_strategy,
    }
}

/// Inverse of [`parse_local_rules`] — emits a stable canonical query string
/// (no leading `?`). Empty result means "use defaults" — the picker drops the
/// `?` entirely so `/local/xiangqi` with no query is the default-casual URL.
pub fn build_local_query(variant: Variant, params: &LocalRulesParams) -> String {
    let mut parts: Vec<String> = Vec::new();
    match variant {
        Variant::Xiangqi => {
            if params.strict {
                parts.push("strict=1".to_string());
            }
            if params.mode == PlayMode::VsAi {
                parts.push("mode=ai".to_string());
                parts.push(format!(
                    "ai={}",
                    if params.ai_side == Side::RED { "red" } else { "black" }
                ));
                parts.push(format!("diff={}", params.ai_difficulty.as_str()));
                // Only emit engine= when the user picked a non-default
                // strategy. Keeps the canonical short URL unchanged for
                // the recommended setup.
                if params.ai_strategy != Strategy::default() {
                    parts.push(format!("engine={}", params.ai_strategy.as_str()));
                }
            }
        }
        Variant::Banqi => {
            let csv = house_csv(params.house);
            if !csv.is_empty() {
                parts.push(format!("house={}", csv));
            }
            if let Some(seed) = params.seed {
                parts.push(format!("seed={}", seed));
            }
        }
        Variant::ThreeKingdomBanqi => {}
    }
    parts.join("&")
}

/// Build the full path for the picker's "Start" link given the current form.
/// Always returns `"/local/<slug>"` with the query string appended only when
/// non-empty.
pub fn build_local_href(variant: Variant, params: &LocalRulesParams) -> String {
    let q = build_local_query(variant, params);
    if q.is_empty() {
        format!("/local/{}", variant_slug(variant))
    } else {
        format!("/local/{}?{}", variant_slug(variant), q)
    }
}

/// Convert parsed [`LocalRulesParams`] into the engine `RuleSet` for the
/// chosen variant. The picker → URL → page flow is the only call site.
pub fn build_rule_set(variant: Variant, params: &LocalRulesParams) -> RuleSet {
    match variant {
        Variant::Xiangqi => {
            if params.strict {
                RuleSet::xiangqi()
            } else {
                RuleSet::xiangqi_casual()
            }
        }
        Variant::Banqi => match params.seed {
            Some(seed) => RuleSet::banqi_with_seed(params.house, seed),
            None => RuleSet::banqi(params.house),
        },
        Variant::ThreeKingdomBanqi => RuleSet::three_kingdom(),
    }
}

fn parse_house_csv(s: &str) -> HouseRules {
    let mut out = HouseRules::empty();
    for tok in s.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        if let Some(flag) = parse_house_token(tok) {
            out.insert(flag);
        }
    }
    out
}

fn house_csv(rules: HouseRules) -> String {
    let mut parts: Vec<&'static str> = Vec::new();
    for (flag, tok) in HOUSE_TOKENS {
        if rules.contains(*flag) {
            parts.push(tok);
        }
    }
    parts.join(",")
}

fn parse_house_token(tok: &str) -> Option<HouseRules> {
    HOUSE_TOKENS
        .iter()
        .chain(HOUSE_TOKEN_ALIASES.iter())
        .find(|(_, t)| t.eq_ignore_ascii_case(tok))
        .map(|(f, _)| *f)
}

fn parse_preset_token(tok: &str) -> Option<HouseRules> {
    match tok.to_ascii_lowercase().as_str() {
        "purist" => Some(PRESET_PURIST),
        "taiwan" => Some(PRESET_TAIWAN),
        "aggressive" => Some(PRESET_AGGRESSIVE),
        _ => None,
    }
}

const HOUSE_TOKENS: &[(HouseRules, &str)] = &[
    (HouseRules::CHAIN_CAPTURE, "chain"),
    (HouseRules::DARK_CAPTURE, "dark"),
    (HouseRules::CHARIOT_RUSH, "rush"),
    (HouseRules::HORSE_DIAGONAL, "horse"),
    (HouseRules::CANNON_FAST_MOVE, "cannon"),
    (HouseRules::DARK_CAPTURE_TRADE, "dark-trade"),
];

/// Aliases recognised on parse only (older URLs / pre-rename snapshots).
/// Encoding uses the canonical `HOUSE_TOKENS` only.
const HOUSE_TOKEN_ALIASES: &[(HouseRules, &str)] = &[(HouseRules::DARK_CAPTURE, "dark-chain")];

#[cfg(test)]
mod tests {
    use super::*;

    fn from_pairs<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k: &str| pairs.iter().find(|(pk, _)| *pk == k).map(|(_, v)| v.to_string())
    }

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

    #[test]
    fn xiangqi_default_is_casual() {
        let p = parse_local_rules(from_pairs(&[]));
        assert!(!p.strict);
        let r = build_rule_set(Variant::Xiangqi, &p);
        assert!(r.xiangqi_allow_self_check);
    }

    #[test]
    fn xiangqi_strict_query_yields_strict_ruleset() {
        let p = parse_local_rules(from_pairs(&[("strict", "1")]));
        assert!(p.strict);
        let r = build_rule_set(Variant::Xiangqi, &p);
        assert!(!r.xiangqi_allow_self_check);
    }

    #[test]
    fn banqi_house_csv_round_trips() {
        let p = parse_local_rules(from_pairs(&[("house", "chain,rush"), ("seed", "42")]));
        assert!(p.house.contains(HouseRules::CHAIN_CAPTURE));
        assert!(p.house.contains(HouseRules::CHARIOT_RUSH));
        assert_eq!(p.seed, Some(42));
        let q = build_local_query(Variant::Banqi, &p);
        assert!(q.contains("house=chain,rush"));
        assert!(q.contains("seed=42"));
    }

    #[test]
    fn banqi_dark_capture_token_parses() {
        let p = parse_local_rules(from_pairs(&[("house", "dark")]));
        assert!(p.house.contains(HouseRules::DARK_CAPTURE));
    }

    #[test]
    fn banqi_dark_chain_alias_still_parses_to_dark_capture() {
        let p = parse_local_rules(from_pairs(&[("house", "dark-chain")]));
        assert!(p.house.contains(HouseRules::DARK_CAPTURE));
    }

    #[test]
    fn banqi_dark_trade_implies_dark_capture() {
        let p = parse_local_rules(from_pairs(&[("house", "dark-trade")]));
        assert!(p.house.contains(HouseRules::DARK_CAPTURE_TRADE));
        assert!(p.house.contains(HouseRules::DARK_CAPTURE));
    }

    #[test]
    fn banqi_preset_taiwan_picks_chain_and_rush() {
        let p = parse_local_rules(from_pairs(&[("preset", "taiwan")]));
        assert!(p.house.contains(HouseRules::CHAIN_CAPTURE));
        assert!(p.house.contains(HouseRules::CHARIOT_RUSH));
    }

    #[test]
    fn banqi_house_overrides_preset_when_both_set() {
        let p = parse_local_rules(from_pairs(&[("preset", "taiwan"), ("house", "")]));
        // Empty house= still wins → no flags.
        assert!(p.house.is_empty());
    }

    #[test]
    fn unknown_house_token_is_dropped_silently() {
        let p = parse_local_rules(from_pairs(&[("house", "chain,bogus,rush")]));
        assert!(p.house.contains(HouseRules::CHAIN_CAPTURE));
        assert!(p.house.contains(HouseRules::CHARIOT_RUSH));
    }

    #[test]
    fn build_local_href_skips_query_when_defaults() {
        let p = LocalRulesParams::default();
        assert_eq!(build_local_href(Variant::Xiangqi, &p), "/local/xiangqi");
        assert_eq!(build_local_href(Variant::Banqi, &p), "/local/banqi");
    }

    #[test]
    fn app_href_adds_base_path() {
        assert_eq!(app_href_with_base("", "/local/xiangqi"), "/local/xiangqi");
        assert_eq!(app_href_with_base("/", "/local/xiangqi"), "/local/xiangqi");
        assert_eq!(
            app_href_with_base("/chinese-chess", "/local/xiangqi"),
            "/chinese-chess/local/xiangqi"
        );
        assert_eq!(app_href_with_base("chinese-chess", "lobby"), "/chinese-chess/lobby");
        assert_eq!(app_href_with_base("/chinese-chess/", "/"), "/chinese-chess/");
        assert_eq!(
            app_href_with_base("/chinese-chess", "/local/banqi?seed=7"),
            "/chinese-chess/local/banqi?seed=7"
        );
    }

    #[test]
    fn ws_base_normalizes_websocket_urls_only() {
        assert_eq!(
            normalize_ws_base(" wss://example.com/ws-root/ "),
            Ok("wss://example.com/ws-root".to_string())
        );
        assert_eq!(normalize_ws_base("ws://127.0.0.1:7878"), Ok("ws://127.0.0.1:7878".to_string()));
        assert_eq!(normalize_ws_base(""), Err(WsBaseError::Empty));
        assert_eq!(normalize_ws_base("https://example.com"), Err(WsBaseError::BadScheme));
        assert_eq!(normalize_ws_base("/ws"), Err(WsBaseError::BadScheme));
    }

    #[test]
    fn build_local_href_includes_query_when_non_default() {
        let p = LocalRulesParams { strict: true, ..Default::default() };
        assert_eq!(build_local_href(Variant::Xiangqi, &p), "/local/xiangqi?strict=1");
        let p = LocalRulesParams {
            house: HouseRules::CHAIN_CAPTURE | HouseRules::CHARIOT_RUSH,
            seed: Some(7),
            ..Default::default()
        };
        assert_eq!(build_local_href(Variant::Banqi, &p), "/local/banqi?house=chain,rush&seed=7");
    }

    #[test]
    fn xiangqi_vs_ai_query_round_trips() {
        let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("ai", "red"), ("diff", "hard")]));
        assert_eq!(p.mode, PlayMode::VsAi);
        assert_eq!(p.ai_side, Side::RED);
        assert_eq!(p.ai_difficulty, Difficulty::Hard);
        assert_eq!(p.ai_strategy, Strategy::default(), "missing engine= → default");
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(q.contains("mode=ai"));
        assert!(q.contains("ai=red"));
        assert!(q.contains("diff=hard"));
        assert!(!q.contains("engine="), "default strategy should be omitted from query");
    }

    #[test]
    fn xiangqi_engine_v1_round_trips() {
        let p = parse_local_rules(from_pairs(&[
            ("mode", "ai"),
            ("ai", "black"),
            ("diff", "normal"),
            ("engine", "v1"),
        ]));
        assert_eq!(p.ai_strategy, Strategy::MaterialV1);
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(q.contains("engine=v1"), "non-default strategy must round-trip; got {:?}", q);
    }

    #[test]
    fn xiangqi_engine_v2_alias_parses() {
        let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("engine", "material-pst")]));
        assert_eq!(p.ai_strategy, Strategy::MaterialPstV2);
    }

    #[test]
    fn xiangqi_engine_v3_alias_parses() {
        let p = parse_local_rules(from_pairs(&[
            ("mode", "ai"),
            ("engine", "material-king-safety-pst"),
        ]));
        assert_eq!(p.ai_strategy, Strategy::MaterialKingSafetyPstV3);
        let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("engine", "king-safety")]));
        assert_eq!(p.ai_strategy, Strategy::MaterialKingSafetyPstV3);
    }

    #[test]
    fn xiangqi_engine_unknown_token_falls_back_to_default() {
        let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("engine", "v999")]));
        assert_eq!(p.ai_strategy, Strategy::default());
    }

    #[test]
    fn xiangqi_default_mode_omits_ai_query() {
        let p = LocalRulesParams::default();
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(q.is_empty(), "default mode should not emit query, got {:?}", q);
    }

    #[test]
    fn three_kingdom_ignores_query() {
        let p =
            parse_local_rules(from_pairs(&[("strict", "1"), ("house", "chain"), ("seed", "5")]));
        let r = build_rule_set(Variant::ThreeKingdomBanqi, &p);
        assert_eq!(r.variant, Variant::ThreeKingdomBanqi);
        assert!(r.house.is_empty());
        assert!(build_local_query(Variant::ThreeKingdomBanqi, &p).is_empty());
    }
}
