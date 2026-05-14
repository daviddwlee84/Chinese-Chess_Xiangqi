//! Variant-slug parsing for `/local/:variant` plus URL-query rule encoding.
//!
//! Pure logic — native-testable. The picker page builds a query string from
//! its form state via [`build_local_query`]; the local page parses it back
//! into a [`RuleSet`] via [`parse_local_rules`] + [`build_rule_set`].

use chess_ai::{Difficulty, Randomness, Strategy};
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
    /// `VsAi` only — randomness preset. `None` = use the difficulty
    /// default (`Difficulty::default_randomness`); `Some(_)` = explicit
    /// override (typically a [`Randomness`] preset like
    /// [`Randomness::STRICT`]). URL token: `&variation=strict|subtle|varied|chaotic`.
    pub ai_variation: Option<Randomness>,
    /// `VsAi` only — search depth override. `None` = use
    /// `Difficulty::default_depth` (Easy=1, Normal=3, Hard=4); `Some(N)`
    /// = explicit. Capped at `MAX_AI_DEPTH` to keep the worst-case
    /// browser response under ~10 s. URL token: `&depth=N`.
    pub ai_depth: Option<u8>,
    /// `VsAi` only — per-search node-count cap override. `None` = use
    /// the engine's default (v5 scales with depth via
    /// `chess_ai::search::node_budget_for_depth`; v1-v4 use the flat
    /// `NODE_BUDGET = 250_000`). `Some(N)` forces the cap regardless
    /// of strategy or depth. Capped at [`MAX_AI_NODE_BUDGET`] so a
    /// typo'd 9-figure value can't lock the browser tab. URL token:
    /// `&budget=N`.
    pub ai_node_budget: Option<u32>,
    /// `VsAi` only — opens the AI debug panel showing the engine's full
    /// scored root-move list, hover-to-highlight on the board, and
    /// search metadata (depth, nodes, strategy, randomness). URL token:
    /// `&debug=1`. Default off (consumes ~no extra search cost — the
    /// scored list is computed regardless; `&debug=1` just exposes it).
    pub ai_debug: bool,
    /// User-facing alias for [`Self::ai_debug`]: opens the same AI
    /// hint / debug panel via the friendlier URL token `&hints=1`.
    /// Local mode treats `ai_debug || ai_hints` as the trigger to
    /// mount the panel — both flags are functionally equivalent
    /// offline (e.g. on the GitHub Pages build). In net mode (`/play/`)
    /// neither flag is enough on its own — the server's
    /// `hints_allowed` (set at room creation by the first joiner's
    /// `?hints=1`) gates the panel for fairness; see `pages/play.rs`.
    pub ai_hints: bool,
    /// Enables the live AI win-rate display: vertical eval bar attached
    /// to the right edge of the board, sidebar `紅 % • 黑 %` badge,
    /// per-ply samples cached for the post-game trend chart. Works in
    /// both vs-AI and pass-and-play (PvP).
    ///
    /// Reuses the `chess_ai::analyze` calls the AI move pump and hint
    /// pump already make — when this flag is on, the hint pump runs
    /// every turn (even without `?hints=1`) so each ply gets sampled.
    /// Costs ~100-300 ms WASM per turn at default Hard depth, on top
    /// of any existing AI/hint search work.
    ///
    /// URL token: `&evalbar=1`. Default off.
    /// Net mode (`/play/`) currently ignores this flag — see TODO.md
    /// "chess-net protocol v6" for the spectator-side broadcast follow-up.
    pub ai_evalbar: bool,
    /// Xiangqi pass-and-play only. When `true`, Black's piece glyphs are
    /// rendered rotated 180° so a player sitting on the opposite side of
    /// the device reads their pieces upright. Coordinate system is
    /// unchanged. Ignored when `mode == VsAi`, banqi, or three-kingdom.
    /// URL token: `&mirror=1`.
    pub mirror: bool,
}

/// Hard cap on user-supplied depth. Picked so that browser WASM v3 Hard
/// at depth 8 (~5 s) is the worst tolerable response time. Higher than
/// 8 essentially demands iterative deepening + time budget — that's the
/// v5 roadmap entry, not a knob to twist.
pub const MAX_AI_DEPTH: u8 = 10;

/// Hard cap on user-supplied node budget. Picked so the worst-case v5
/// search at MAX_AI_DEPTH stays under ~30 s on a typical 2024 laptop
/// running WASM (the engine's own auto-scaled cap is 16M, so 64M
/// gives the user 4× headroom for "go really deep on a big machine"
/// scenarios but still bounds the worst case). The picker's number
/// input clamps to this value at parse time.
pub const MAX_AI_NODE_BUDGET: u32 = 64_000_000;

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
            ai_variation: None,
            ai_depth: None,
            ai_node_budget: None,
            ai_debug: false,
            ai_hints: false,
            ai_evalbar: false,
            mirror: false,
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
    let ai_variation = get("variation").as_deref().and_then(Randomness::parse);
    let ai_depth = get("depth")
        .and_then(|s| s.parse::<u32>().ok())
        .map(|d| d.clamp(1, MAX_AI_DEPTH as u32) as u8);
    let ai_node_budget = get("budget").and_then(|s| s.parse::<u64>().ok()).map(|b| {
        // Parse as u64 so a typo'd 11-digit value clamps to the cap
        // rather than failing to parse and silently falling back to
        // None (`u32::parse` overflows above ~4.29B).
        b.clamp(1, MAX_AI_NODE_BUDGET as u64) as u32
    });
    let ai_debug = matches!(get("debug").as_deref(), Some("1") | Some("true") | Some("on"));
    let ai_hints = matches!(get("hints").as_deref(), Some("1") | Some("true") | Some("on"));
    let ai_evalbar = matches!(get("evalbar").as_deref(), Some("1") | Some("true") | Some("on"));
    let mirror = matches!(get("mirror").as_deref(), Some("1") | Some("true") | Some("on"));
    LocalRulesParams {
        strict,
        house: house::normalize(house),
        seed,
        mode,
        ai_side,
        ai_difficulty,
        ai_strategy,
        ai_variation,
        ai_depth,
        ai_node_budget,
        ai_debug,
        ai_hints,
        ai_evalbar,
        mirror,
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
                // Only emit variation= when the user explicitly overrode
                // the difficulty default (and the override is one of the
                // known presets — custom Randomness values can't be
                // round-tripped through the URL).
                if let Some(name) = params.ai_variation.and_then(|r| r.preset_name()) {
                    parts.push(format!("variation={}", name));
                }
                // Only emit depth= when the user overrode the difficulty's
                // default. Allows the canonical short URL for the
                // recommended setup to stay short.
                if let Some(d) = params.ai_depth {
                    parts.push(format!("depth={}", d));
                }
                // Only emit budget= when the user explicitly overrode
                // the engine's default node-count cap. Keeps the
                // canonical "I'm using a preset difficulty" URL short.
                if let Some(b) = params.ai_node_budget {
                    parts.push(format!("budget={}", b));
                }
                // Debug panel toggle (vs-AI only — debug shows the AI's
                // own POV cached after each AI move; meaningless in PvP
                // where there's no AI move pump to fill the cache).
                if params.ai_debug {
                    parts.push("debug=1".to_string());
                }
            }
            // Hint mode toggle is emitted REGARDLESS of mode — hints
            // work in both vs-AI (analysis from human's POV when it's
            // their turn) AND pass-and-play (analysis from current
            // side-to-move's POV — both humans can ask the bot for
            // advice on every move). If we only emitted hints=1 inside
            // the VsAi block above, the picker's PvP-mode + checked
            // hints box would silently drop the flag and the player
            // would see no panel after clicking Start.
            if params.ai_hints {
                parts.push("hints=1".to_string());
            }
            // Win-rate display flag — same reason as hints=1 above:
            // works in both vs-AI and PvP, so emitted unconditionally
            // (when set). The local mode pages then thread it through
            // the same hint pump that powers `?hints=1`.
            if params.ai_evalbar {
                parts.push("evalbar=1".to_string());
            }
            // Mirror is pass-and-play only — vs-AI has no opponent on
            // the far side of the device, so it would just confuse the
            // single human. We still allow `?mirror=1` to round-trip
            // when set even with mode=ai (so URL hand-edits don't get
            // silently dropped on parse), but the picker only offers
            // the checkbox when mode == Pvp.
            if params.mirror && params.mode == PlayMode::Pvp {
                parts.push("mirror=1".to_string());
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

/// Canonical comma-separated form of a `HouseRules` set, e.g. `"chain,rush"`.
/// Used by `build_local_query` for URL encoding AND by `state::describe_rules`
/// for the per-game rules summary line — single source of truth so both
/// surfaces agree on flag names + ordering.
pub(crate) fn house_csv(rules: HouseRules) -> String {
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
    (HouseRules::PREASSIGN_COLORS, "preassign"),
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
    fn xiangqi_variation_round_trips() {
        for (token, expected) in &[
            ("strict", Randomness::STRICT),
            ("subtle", Randomness::SUBTLE),
            ("varied", Randomness::VARIED),
            ("chaotic", Randomness::CHAOTIC),
        ] {
            let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("variation", token)]));
            assert_eq!(p.ai_variation, Some(*expected));
            let q = build_local_query(Variant::Xiangqi, &p);
            assert!(
                q.contains(&format!("variation={}", token)),
                "round-trip failed for {}: {}",
                token,
                q
            );
        }
    }

    #[test]
    fn xiangqi_variation_omitted_when_default() {
        let p = LocalRulesParams { mode: PlayMode::VsAi, ..Default::default() };
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(
            !q.contains("variation="),
            "default (None) variation should not be emitted; got {}",
            q
        );
    }

    #[test]
    fn xiangqi_variation_unknown_token_falls_back_to_none() {
        let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("variation", "garbage")]));
        assert_eq!(p.ai_variation, None);
    }

    #[test]
    fn xiangqi_depth_round_trips() {
        let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("depth", "6")]));
        assert_eq!(p.ai_depth, Some(6));
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(q.contains("depth=6"), "depth round-trip: {}", q);
    }

    #[test]
    fn xiangqi_depth_clamped_to_max() {
        let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("depth", "999")]));
        assert_eq!(p.ai_depth, Some(MAX_AI_DEPTH));
    }

    #[test]
    fn xiangqi_depth_zero_clamped_to_one() {
        let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("depth", "0")]));
        assert_eq!(p.ai_depth, Some(1));
    }

    #[test]
    fn xiangqi_depth_omitted_when_default() {
        let p = LocalRulesParams { mode: PlayMode::VsAi, ..Default::default() };
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(!q.contains("depth="), "default (None) depth should not be emitted; got {}", q);
    }

    #[test]
    fn xiangqi_depth_garbage_falls_back_to_none() {
        let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("depth", "abc")]));
        assert_eq!(p.ai_depth, None);
    }

    #[test]
    fn xiangqi_node_budget_round_trips() {
        let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("budget", "2000000")]));
        assert_eq!(p.ai_node_budget, Some(2_000_000));
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(q.contains("budget=2000000"), "budget round-trip: {}", q);
    }

    #[test]
    fn xiangqi_node_budget_clamped_to_max() {
        let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("budget", "9999999999")]));
        assert_eq!(p.ai_node_budget, Some(MAX_AI_NODE_BUDGET));
    }

    #[test]
    fn xiangqi_node_budget_zero_clamped_to_one() {
        let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("budget", "0")]));
        assert_eq!(p.ai_node_budget, Some(1));
    }

    #[test]
    fn xiangqi_node_budget_omitted_when_default() {
        let p = LocalRulesParams { mode: PlayMode::VsAi, ..Default::default() };
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(!q.contains("budget="), "default (None) budget should not be emitted; got {}", q);
    }

    #[test]
    fn xiangqi_node_budget_garbage_falls_back_to_none() {
        let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("budget", "lots")]));
        assert_eq!(p.ai_node_budget, None);
    }

    /// Depth + budget together is the picker's "Custom" combo: both
    /// must round-trip independently.
    #[test]
    fn xiangqi_depth_and_node_budget_round_trip_together() {
        let p =
            parse_local_rules(from_pairs(&[("mode", "ai"), ("depth", "8"), ("budget", "4000000")]));
        assert_eq!(p.ai_depth, Some(8));
        assert_eq!(p.ai_node_budget, Some(4_000_000));
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(q.contains("depth=8"));
        assert!(q.contains("budget=4000000"));
    }

    #[test]
    fn xiangqi_debug_round_trips() {
        let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("debug", "1")]));
        assert!(p.ai_debug);
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(q.contains("debug=1"), "debug= round-trip: {}", q);
    }

    #[test]
    fn xiangqi_debug_default_off_omitted_from_query() {
        let p = LocalRulesParams { mode: PlayMode::VsAi, ..Default::default() };
        assert!(!p.ai_debug);
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(!q.contains("debug="), "off should not be emitted; got {}", q);
    }

    #[test]
    fn xiangqi_debug_accepts_truthy_aliases() {
        for token in ["1", "true", "on"] {
            let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("debug", token)]));
            assert!(p.ai_debug, "token {:?} should enable debug", token);
        }
        for token in ["0", "false", "off", "garbage"] {
            let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("debug", token)]));
            assert!(!p.ai_debug, "token {:?} should NOT enable debug", token);
        }
    }

    #[test]
    fn xiangqi_hints_round_trips() {
        let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("hints", "1")]));
        assert!(p.ai_hints);
        // ai_debug stays independent (hint is its own URL flag).
        assert!(!p.ai_debug);
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(q.contains("hints=1"), "hints= round-trip: {}", q);
    }

    #[test]
    fn xiangqi_hints_and_debug_can_coexist() {
        // Both URL tokens together → both flags set; canonical
        // emission preserves both. In local.rs the panel mounts when
        // either is true.
        let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("debug", "1"), ("hints", "1")]));
        assert!(p.ai_debug && p.ai_hints);
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(q.contains("debug=1") && q.contains("hints=1"), "both kept: {}", q);
    }

    #[test]
    fn xiangqi_hints_default_off_omitted_from_query() {
        let p = LocalRulesParams { mode: PlayMode::VsAi, ..Default::default() };
        assert!(!p.ai_hints);
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(!q.contains("hints="), "off should not be emitted; got {}", q);
    }

    #[test]
    fn xiangqi_pvp_hints_emitted_even_without_ai_mode() {
        // Regression: previously `hints=1` lived inside the
        // `if mode == VsAi` block in build_local_query, so a user
        // checking the picker's "🧠 Allow AI hints" box in PvP mode
        // would get a `/local/xiangqi` URL with no `hints=1`, and the
        // game page would show no panel. Hints work in PvP (analysis
        // for current side-to-move) so the flag must round-trip.
        let p = LocalRulesParams { mode: PlayMode::Pvp, ai_hints: true, ..Default::default() };
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(q.contains("hints=1"), "PvP+hints must round-trip; got {:?}", q);
        // mode=ai must NOT appear (we're in Pvp).
        assert!(!q.contains("mode=ai"), "PvP must not emit mode=ai; got {:?}", q);
    }

    #[test]
    fn xiangqi_pvp_hints_round_trips() {
        // Full encode → decode round-trip for PvP+hints (the no-AI
        // pass-and-play path that was previously broken).
        let p = LocalRulesParams { mode: PlayMode::Pvp, ai_hints: true, ..Default::default() };
        let q = build_local_query(Variant::Xiangqi, &p);
        let pairs: Vec<(String, String)> = q
            .split('&')
            .filter_map(|kv| kv.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
            .collect();
        let p2 =
            parse_local_rules(|k| pairs.iter().find(|(pk, _)| pk == k).map(|(_, v)| v.clone()));
        assert!(p2.ai_hints);
        assert_eq!(p2.mode, PlayMode::Pvp);
    }

    #[test]
    fn xiangqi_evalbar_round_trips_in_vs_ai() {
        let p = parse_local_rules(from_pairs(&[("mode", "ai"), ("evalbar", "1")]));
        assert!(p.ai_evalbar);
        // Independent of hints / debug — three flags, no aliasing.
        assert!(!p.ai_hints);
        assert!(!p.ai_debug);
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(q.contains("evalbar=1"), "evalbar= round-trip: {}", q);
    }

    #[test]
    fn xiangqi_pvp_evalbar_emitted_even_without_ai_mode() {
        // Same shape as the PvP+hints regression — evalbar works in
        // PvP (samples come from the hint pump that runs every turn
        // when evalbar is on), so the flag must round-trip.
        let p = LocalRulesParams { mode: PlayMode::Pvp, ai_evalbar: true, ..Default::default() };
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(q.contains("evalbar=1"), "PvP+evalbar must round-trip; got {:?}", q);
        assert!(!q.contains("mode=ai"));
    }

    #[test]
    fn xiangqi_evalbar_default_off_omitted() {
        let p = LocalRulesParams { mode: PlayMode::VsAi, ..Default::default() };
        assert!(!p.ai_evalbar);
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(!q.contains("evalbar="), "off should not be emitted; got {}", q);
    }

    #[test]
    fn xiangqi_evalbar_truthy_aliases_parse() {
        for token in ["1", "true", "on"] {
            let p = parse_local_rules(from_pairs(&[("evalbar", token)]));
            assert!(p.ai_evalbar, "token {:?} should enable evalbar", token);
        }
        for token in ["0", "false", "off", "garbage"] {
            let p = parse_local_rules(from_pairs(&[("evalbar", token)]));
            assert!(!p.ai_evalbar, "token {:?} should NOT enable evalbar", token);
        }
    }

    #[test]
    fn xiangqi_three_insight_flags_can_coexist() {
        // hints + debug + evalbar all set → all three preserved.
        let p = parse_local_rules(from_pairs(&[
            ("mode", "ai"),
            ("debug", "1"),
            ("hints", "1"),
            ("evalbar", "1"),
        ]));
        assert!(p.ai_debug && p.ai_hints && p.ai_evalbar);
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(
            q.contains("debug=1") && q.contains("hints=1") && q.contains("evalbar=1"),
            "all three preserved: {}",
            q
        );
    }

    #[test]
    fn xiangqi_default_mode_omits_ai_query() {
        let p = LocalRulesParams::default();
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(q.is_empty(), "default mode should not emit query, got {:?}", q);
    }

    #[test]
    fn xiangqi_mirror_round_trips_in_pvp() {
        let p = parse_local_rules(from_pairs(&[("mirror", "1")]));
        assert!(p.mirror);
        assert_eq!(p.mode, PlayMode::Pvp);
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(q.contains("mirror=1"), "PvP+mirror must round-trip; got {:?}", q);
    }

    #[test]
    fn xiangqi_mirror_default_off_omitted() {
        let p = LocalRulesParams::default();
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(!q.contains("mirror="), "default off must not emit; got {:?}", q);
    }

    #[test]
    fn xiangqi_mirror_dropped_in_vs_ai_emission() {
        let p = LocalRulesParams { mode: PlayMode::VsAi, mirror: true, ..Default::default() };
        let q = build_local_query(Variant::Xiangqi, &p);
        assert!(!q.contains("mirror="), "mirror must not be emitted in vs-AI; got {:?}", q);
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
