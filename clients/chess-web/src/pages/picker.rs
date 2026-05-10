use chess_ai::{Difficulty, Randomness, Strategy};
use chess_core::piece::Side;
use chess_core::rules::{HouseRules, Variant, PRESET_AGGRESSIVE, PRESET_PURIST, PRESET_TAIWAN};
use leptos::*;

use crate::components::pwa::PwaInstallBanner;
use crate::prefs::{Prefs, ThreatMode};
use crate::routes::{
    app_href, build_local_href, LocalRulesParams, PlayMode, MAX_AI_DEPTH, MAX_AI_NODE_BUDGET,
};

#[component]
pub fn Picker() -> impl IntoView {
    view! {
        <section class="picker">
            <PwaInstallBanner/>
            <header class="picker-hero">
                <div class="picker-hero__glyph" aria-hidden="true">"帥"</div>
                <div class="picker-hero__copy">
                    <h1>"Chinese Chess 中國象棋"</h1>
                    <p>
                        "Pick a variant for local pass-and-play, or join the online lobby. "
                        "Rule choices encode into the URL — bookmark to share a configuration."
                    </p>
                </div>
                <a
                    class="picker-hero__source"
                    href="https://github.com/daviddwlee84/Chinese-Chess_Xiangqi"
                    target="_blank"
                    rel="noopener noreferrer"
                    aria-label="View source on GitHub"
                    title="View source on GitHub"
                >
                    <svg viewBox="0 0 16 16" width="22" height="22" aria-hidden="true">
                        <path fill="currentColor" fill-rule="evenodd" d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/>
                    </svg>
                </a>
            </header>
            <div class="picker-grid">
                <XiangqiCard/>
                <BanqiCard/>
                <ThreeKingdomCard/>
                <OnlineCard/>
            </div>
            <DisplaySettings/>
        </section>
    }
}

#[component]
fn DisplaySettings() -> impl IntoView {
    let prefs = expect_context::<Prefs>();
    let fx_confetti = prefs.fx_confetti;
    let fx_check_banner = prefs.fx_check_banner;
    let fx_threat_mode = prefs.fx_threat_mode;
    let fx_threat_hover = prefs.fx_threat_hover;
    // The `<select>` reports value-strings; we map back to ThreatMode
    // via `ThreatMode::from_str` (unknown → recommended NetLoss
    // default — keeps the UI graceful if a future option is added).
    let on_mode_change = move |ev: leptos::ev::Event| {
        let s = event_target_value(&ev);
        fx_threat_mode.set(ThreatMode::from_str(&s));
    };
    view! {
        <details class="picker-settings">
            <summary>"⚙ Display settings"</summary>
            <div class="fx-toggles">
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || fx_confetti.get()
                        on:change=move |ev| fx_confetti.set(event_target_checked(&ev))
                    />
                    <span>"Victory effects (confetti + banner)"</span>
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || fx_check_banner.get()
                        on:change=move |ev| fx_check_banner.set(event_target_checked(&ev))
                    />
                    <span>"將軍 / CHECK warning"</span>
                </label>
                // Threat highlight mode selector. Default `NetLoss`
                // ('被捉') — visually clean enough to leave on. The
                // user-visible labels are bilingual so first-time
                // viewers can guess the semantic without diving into
                // docs; the option values are the lowercase camelCase
                // that `ThreatMode::as_str` writes to localStorage.
                <label class="threat-mode-row">
                    <span>"威脅提示 / Threat highlight:"</span>
                    <select
                        class="threat-mode-select"
                        on:change=on_mode_change
                        prop:value=move || fx_threat_mode.get().as_str()
                    >
                        <option value="off">"關閉 / Off"</option>
                        <option value="attacked">"被攻擊 / Attacked (busy)"</option>
                        <option value="netLoss">"被捉 / Net loss (recommended)"</option>
                        <option value="mateThreat">"叫殺 / Mate threat"</option>
                    </select>
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || fx_threat_hover.get()
                        on:change=move |ev| fx_threat_hover.set(event_target_checked(&ev))
                    />
                    <span>"Hover preview — show what an opponent could capture if my hovered/selected piece doesn't move"</span>
                </label>
            </div>
        </details>
    }
}

#[component]
fn XiangqiCard() -> impl IntoView {
    let strict = create_rw_signal(false);
    let mode = create_rw_signal(PlayMode::Pvp);
    let player_red = create_rw_signal(true);
    // Difficulty signal stores the selected preset (Easy/Normal/Hard).
    // When `is_custom` is true, this signal is ignored for depth
    // resolution but still feeds the randomness fallback (since
    // `Difficulty::default_randomness()` provides per-level defaults
    // and there's no neutral "custom" randomness option). We pin the
    // underlying value to `Hard` while in Custom mode so the fallback
    // is `Randomness::SUBTLE` — power-user appropriate. Users wanting
    // a different mix override via the Variation fieldset.
    let difficulty = create_rw_signal(Difficulty::Normal);
    // Custom is rendered as a 4th radio in the Difficulty fieldset
    // — click reveals the Search-depth + Node-budget inputs below
    // (they're conditionally mounted via <Show when=is_custom>).
    let is_custom = create_rw_signal(false);
    let ai_strategy = create_rw_signal(Strategy::default());
    // None = "use difficulty default"; Some(_) = explicit override.
    // Encoded into the URL via `&variation=`.
    let ai_variation = create_rw_signal::<Option<Randomness>>(None);
    // Empty = use difficulty default depth; numeric = override (clamped
    // 1..=MAX_AI_DEPTH on parse). ONLY emitted to the URL when
    // is_custom is true — preset difficulties always defer to
    // `Difficulty::default_depth()`.
    let ai_depth_text = create_rw_signal(String::new());
    // Empty = use the engine's auto-scaled budget (v5:
    // node_budget_for_depth(target); v1-v4: flat NODE_BUDGET).
    // Numeric = explicit override clamped to MAX_AI_NODE_BUDGET. Only
    // emitted when is_custom is true.
    let ai_budget_text = create_rw_signal(String::new());
    // Two independent advanced toggles — see the corresponding fields
    // on `LocalRulesParams` for the semantic split:
    //   - ai_show_hints → URL `hints=1` → in-game 🧠 toggle button,
    //     default hidden, runs analyze from the human's POV when
    //     opened (player-friendly).
    //   - ai_show_debug → URL `debug=1` → sticky always-on panel
    //     showing the AI's POV after each AI move (power-user).
    // Both can be enabled together; `pages/local.rs` mounts a single
    // `<DebugPanel>` and lets either source feed it.
    let ai_show_hints = create_rw_signal(false);
    let ai_show_debug = create_rw_signal(false);
    // Win-rate display: separate URL flag (`?evalbar=1`). Independent
    // of hints/debug — works in both vs-Computer and Pass-and-play
    // (samples come from the same hint pump that powers `?hints=1`,
    // which is unconditionally enabled when either flag is set).
    let ai_show_evalbar = create_rw_signal(false);
    // Pass-and-play only: rotate Black piece glyphs 180° so the player
    // sitting on the opposite side of the device reads their own pieces
    // upright. Coordinate system unchanged.
    let mirror_black = create_rw_signal(false);
    let href = move || {
        let ai_side = if player_red.get() { Side::BLACK } else { Side::RED };
        // Custom mode: parse depth + budget; preset modes: leave both None
        // so the engine defers to Difficulty::default_depth() and the
        // engine-internal node-budget policy.
        let (ai_depth, ai_node_budget, effective_difficulty) = if is_custom.get() {
            let depth = ai_depth_text
                .with(|s| s.trim().parse::<u8>().ok().map(|d| d.clamp(1, MAX_AI_DEPTH)));
            let budget = ai_budget_text
                .with(|s| s.trim().parse::<u32>().ok().map(|b| b.clamp(1, MAX_AI_NODE_BUDGET)));
            // Custom rides on top of `Difficulty::Hard` so the
            // randomness fallback (when Variation = Default) lands on
            // `Randomness::SUBTLE` — the power-user-appropriate
            // default. See the comment on `difficulty` above.
            (depth, budget, Difficulty::Hard)
        } else {
            (None, None, difficulty.get())
        };
        let params = LocalRulesParams {
            strict: strict.get(),
            mode: mode.get(),
            ai_side,
            ai_difficulty: effective_difficulty,
            ai_strategy: ai_strategy.get(),
            ai_variation: ai_variation.get(),
            ai_depth,
            ai_node_budget,
            ai_debug: ai_show_debug.get(),
            ai_hints: ai_show_hints.get(),
            ai_evalbar: ai_show_evalbar.get(),
            mirror: mirror_black.get(),
            ..Default::default()
        };
        app_href(&build_local_href(Variant::Xiangqi, &params))
    };
    view! {
        <div class="variant-card variant-card--form">
            <h2>"Xiangqi 象棋"</h2>
            <p>"Standard 9×10 Chinese chess."</p>
            <fieldset class="card-fieldset">
                <legend>"Mode"</legend>
                <label class="radio-row">
                    <input
                        type="radio"
                        name="xiangqi-mode"
                        prop:checked=move || mode.get() == PlayMode::Pvp
                        on:change=move |_| mode.set(PlayMode::Pvp)
                    />
                    <span>"Pass-and-play — two humans share this device."</span>
                </label>
                <label class="radio-row">
                    <input
                        type="radio"
                        name="xiangqi-mode"
                        prop:checked=move || mode.get() == PlayMode::VsAi
                        on:change=move |_| mode.set(PlayMode::VsAi)
                    />
                    <span>"vs Computer — alpha-beta engine, runs entirely in your browser."</span>
                </label>
            </fieldset>
            <Show when=move || mode.get() == PlayMode::Pvp>
                <fieldset class="card-fieldset">
                    <legend>"Seat layout"</legend>
                    <label class="check-row">
                        <input
                            type="checkbox"
                            prop:checked=move || mirror_black.get()
                            on:change=move |ev| mirror_black.set(event_target_checked(&ev))
                        />
                        <span>"鏡像黑方 — Mirror Black's pieces 180° for a player sitting opposite (phone flat on a table). Captured pieces split to each side's edge."</span>
                    </label>
                </fieldset>
            </Show>
            <Show when=move || mode.get() == PlayMode::VsAi>
                <fieldset class="card-fieldset">
                    <legend>"You play"</legend>
                    <label class="radio-row">
                        <input
                            type="radio"
                            name="xiangqi-player-side"
                            prop:checked=move || player_red.get()
                            on:change=move |_| player_red.set(true)
                        />
                        <span>"紅 Red — moves first."</span>
                    </label>
                    <label class="radio-row">
                        <input
                            type="radio"
                            name="xiangqi-player-side"
                            prop:checked=move || !player_red.get()
                            on:change=move |_| player_red.set(false)
                        />
                        <span>"黑 Black — AI moves first."</span>
                    </label>
                </fieldset>
                <fieldset class="card-fieldset">
                    <legend>"Difficulty"</legend>
                    <label class="radio-row">
                        <input
                            type="radio"
                            name="xiangqi-difficulty"
                            prop:checked=move || !is_custom.get() && difficulty.get() == Difficulty::Easy
                            on:change=move |_| { is_custom.set(false); difficulty.set(Difficulty::Easy); }
                        />
                        <span>"Easy — depth 1, picks at random from the top three replies."</span>
                    </label>
                    <label class="radio-row">
                        <input
                            type="radio"
                            name="xiangqi-difficulty"
                            prop:checked=move || !is_custom.get() && difficulty.get() == Difficulty::Normal
                            on:change=move |_| { is_custom.set(false); difficulty.set(Difficulty::Normal); }
                        />
                        <span>"Normal — depth 3, mostly plays the best line."</span>
                    </label>
                    <label class="radio-row">
                        <input
                            type="radio"
                            name="xiangqi-difficulty"
                            prop:checked=move || !is_custom.get() && difficulty.get() == Difficulty::Hard
                            on:change=move |_| { is_custom.set(false); difficulty.set(Difficulty::Hard); }
                        />
                        <span>"Hard — depth 4, may take a couple of seconds per move."</span>
                    </label>
                    // Custom is the 4th radio in the same Difficulty
                    // group — selecting it reveals depth + node-budget
                    // inputs below. Internally this rides on top of
                    // Hard so the Variation fieldset's "Default" still
                    // gets a sensible randomness fallback (SUBTLE).
                    <label class="radio-row">
                        <input
                            type="radio"
                            name="xiangqi-difficulty"
                            prop:checked=move || is_custom.get()
                            on:change=move |_| is_custom.set(true)
                        />
                        <span>"Custom (advanced) — set search depth and node budget directly."</span>
                    </label>
                    <Show when=move || is_custom.get()>
                        <div class="custom-difficulty-inputs">
                            <p class="hint">
                                "Search depth = how many plies the engine looks ahead. "
                                "Node budget = the search bails when this many positions have been visited (lower = faster but shallower realized depth). "
                                "Leave both blank to use the engine's depth-scaled defaults; the AI Debug panel reports both reached and target depth."
                            </p>
                            <label class="custom-input-row">
                                <span class="custom-input-label">
                                    "Search depth (max " {MAX_AI_DEPTH.to_string()} ")"
                                </span>
                                <input
                                    type="number"
                                    inputmode="numeric"
                                    min="1"
                                    max=MAX_AI_DEPTH.to_string()
                                    placeholder="auto (Hard = 4)"
                                    class="text-input depth-input"
                                    prop:value=move || ai_depth_text.get()
                                    on:input=move |ev| ai_depth_text.set(event_target_value(&ev))
                                />
                            </label>
                            <label class="custom-input-row">
                                <span class="custom-input-label">
                                    "Node budget (max " {MAX_AI_NODE_BUDGET.to_string()} ")"
                                </span>
                                <input
                                    type="number"
                                    inputmode="numeric"
                                    min="1"
                                    max=MAX_AI_NODE_BUDGET.to_string()
                                    placeholder="auto (depth-scaled)"
                                    class="text-input depth-input"
                                    prop:value=move || ai_budget_text.get()
                                    on:input=move |ev| ai_budget_text.set(event_target_value(&ev))
                                />
                            </label>
                            // Variation lives inline here as well, so
                            // Custom users get all three preset-equivalent
                            // knobs in one place. The standalone
                            // "Variation" fieldset below hides while
                            // Custom is active (the same `ai_variation`
                            // signal feeds both — choices persist when
                            // toggling between Custom and a preset).
                            <fieldset class="custom-variation">
                                <legend>"Variation"</legend>
                                <label class="radio-row">
                                    <input
                                        type="radio"
                                        name="xiangqi-variation-custom"
                                        prop:checked=move || ai_variation.get().is_none()
                                        on:change=move |_| ai_variation.set(None)
                                    />
                                    <span>"Default — Hard's preset (Subtle, top-3 within ±20 cp)."</span>
                                </label>
                                <label class="radio-row">
                                    <input
                                        type="radio"
                                        name="xiangqi-variation-custom"
                                        prop:checked=move || ai_variation.get() == Some(Randomness::STRICT)
                                        on:change=move |_| ai_variation.set(Some(Randomness::STRICT))
                                    />
                                    <span>"Strict — always the best move (deterministic)."</span>
                                </label>
                                <label class="radio-row">
                                    <input
                                        type="radio"
                                        name="xiangqi-variation-custom"
                                        prop:checked=move || ai_variation.get() == Some(Randomness::SUBTLE)
                                        on:change=move |_| ai_variation.set(Some(Randomness::SUBTLE))
                                    />
                                    <span>"Subtle — top-3 within ±20 cp."</span>
                                </label>
                                <label class="radio-row">
                                    <input
                                        type="radio"
                                        name="xiangqi-variation-custom"
                                        prop:checked=move || ai_variation.get() == Some(Randomness::VARIED)
                                        on:change=move |_| ai_variation.set(Some(Randomness::VARIED))
                                    />
                                    <span>"Varied — top-5 within ±60 cp."</span>
                                </label>
                                <label class="radio-row">
                                    <input
                                        type="radio"
                                        name="xiangqi-variation-custom"
                                        prop:checked=move || ai_variation.get() == Some(Randomness::CHAOTIC)
                                        on:change=move |_| ai_variation.set(Some(Randomness::CHAOTIC))
                                    />
                                    <span>"Chaotic — top-10 within ±150 cp."</span>
                                </label>
                            </fieldset>
                        </div>
                    </Show>
                </fieldset>
                <fieldset class="card-fieldset">
                    <legend>"Engine"</legend>
                    <label class="radio-row">
                        <input
                            type="radio"
                            name="xiangqi-engine"
                            prop:checked=move || ai_strategy.get() == Strategy::IterativeDeepeningTtV5
                            on:change=move |_| ai_strategy.set(Strategy::IterativeDeepeningTtV5)
                        />
                        <span>"v5 — iterative deepening + transposition table (recommended). v4's evaluator with TT-amortized search; ~50% fewer nodes in endgames."</span>
                    </label>
                    <label class="radio-row">
                        <input
                            type="radio"
                            name="xiangqi-engine"
                            prop:checked=move || ai_strategy.get() == Strategy::QuiescenceMvvLvaV4
                            on:change=move |_| ai_strategy.set(Strategy::QuiescenceMvvLvaV4)
                        />
                        <span>"v4 — quiescence + MVV-LVA. Avoids horizon-effect blunders on captures."</span>
                    </label>
                    <label class="radio-row">
                        <input
                            type="radio"
                            name="xiangqi-engine"
                            prop:checked=move || ai_strategy.get() == Strategy::MaterialKingSafetyPstV3
                            on:change=move |_| ai_strategy.set(Strategy::MaterialKingSafetyPstV3)
                        />
                        <span>"v3 — material + PSTs + king safety. Defends against 1-ply mates but has horizon-effect blunders on captures."</span>
                    </label>
                    <label class="radio-row">
                        <input
                            type="radio"
                            name="xiangqi-engine"
                            prop:checked=move || ai_strategy.get() == Strategy::MaterialPstV2
                            on:change=move |_| ai_strategy.set(Strategy::MaterialPstV2)
                        />
                        <span>"v2 — material + piece-square tables. Plays principled openings but can lose its general in casual mode."</span>
                    </label>
                    <label class="radio-row">
                        <input
                            type="radio"
                            name="xiangqi-engine"
                            prop:checked=move || ai_strategy.get() == Strategy::MaterialV1
                            on:change=move |_| ai_strategy.set(Strategy::MaterialV1)
                        />
                        <span>"v1 — material only (original MVP). Picks at random in the opening."</span>
                    </label>
                </fieldset>
                // Standalone Variation fieldset is the override knob
                // for preset Easy/Normal/Hard. When the user is in
                // Custom mode, Variation lives inline inside the
                // Custom block (see above) so all three preset-
                // equivalent overrides (depth, budget, variation) sit
                // in one place. Same `ai_variation` signal feeds both
                // — flipping between Custom and a preset preserves the
                // user's variation choice.
                <Show when=move || !is_custom.get()>
                    <fieldset class="card-fieldset">
                        <legend>"Variation"</legend>
                        <label class="radio-row">
                            <input
                                type="radio"
                                name="xiangqi-variation"
                                prop:checked=move || ai_variation.get().is_none()
                                on:change=move |_| ai_variation.set(None)
                            />
                            <span>"Default — chosen by difficulty (Easy=Chaotic, Normal=Varied, Hard=Subtle)."</span>
                        </label>
                        <label class="radio-row">
                            <input
                                type="radio"
                                name="xiangqi-variation"
                                prop:checked=move || ai_variation.get() == Some(Randomness::STRICT)
                                on:change=move |_| ai_variation.set(Some(Randomness::STRICT))
                            />
                            <span>"Strict — always the best move (deterministic, no variation)."</span>
                        </label>
                        <label class="radio-row">
                            <input
                                type="radio"
                                name="xiangqi-variation"
                                prop:checked=move || ai_variation.get() == Some(Randomness::SUBTLE)
                                on:change=move |_| ai_variation.set(Some(Randomness::SUBTLE))
                            />
                            <span>"Subtle — top-3 within ±20 cp."</span>
                        </label>
                        <label class="radio-row">
                            <input
                                type="radio"
                                name="xiangqi-variation"
                                prop:checked=move || ai_variation.get() == Some(Randomness::VARIED)
                                on:change=move |_| ai_variation.set(Some(Randomness::VARIED))
                            />
                            <span>"Varied — top-5 within ±60 cp."</span>
                        </label>
                        <label class="radio-row">
                            <input
                                type="radio"
                                name="xiangqi-variation"
                                prop:checked=move || ai_variation.get() == Some(Randomness::CHAOTIC)
                                on:change=move |_| ai_variation.set(Some(Randomness::CHAOTIC))
                            />
                            <span>"Chaotic — top-10 within ±150 cp (weak Hard, fun for learners)."</span>
                        </label>
                    </fieldset>
                </Show>
            </Show>
            <fieldset class="card-fieldset">
                <legend>"AI insight panels (advanced)"</legend>
                <label class="check-row">
                    <input
                        type="checkbox"
                        prop:checked=move || ai_show_hints.get()
                        on:change=move |ev| ai_show_hints.set(event_target_checked(&ev))
                    />
                    <span>"🧠 Allow AI hints — adds a toggle button in the sidebar during play. Click it on your turn to see what the bot would play in your shoes (analysis from the side-to-move's POV). Hidden by default; you stay in control. Works in both vs-Computer and Pass-and-play."</span>
                </label>
                <label class="check-row">
                    <input
                        type="checkbox"
                        prop:checked=move || ai_show_evalbar.get()
                        on:change=move |ev| ai_show_evalbar.set(event_target_checked(&ev))
                    />
                    <span>"📊 Win-rate display — adds a vertical eval bar to the right of the board, a 紅 % • 黑 % badge in the sidebar, and shows a trend chart of the whole game when it ends. Works in both vs-Computer and Pass-and-play. Costs ~100-300 ms per turn (the bot quietly analyzes the position behind the scenes)."</span>
                </label>
                // Debug checkbox is vs-Computer ONLY: it shows the AI's
                // own POV cached after each AI move. In Pass-and-play
                // there's no AI move pump to fill that cache so the
                // panel would be permanently empty — confusing UX.
                // Hint mode covers PvP analysis needs.
                <Show when=move || mode.get() == PlayMode::VsAi>
                    <label class="check-row">
                        <input
                            type="checkbox"
                            prop:checked=move || ai_show_debug.get()
                            on:change=move |ev| ai_show_debug.set(event_target_checked(&ev))
                        />
                        <span>"🔍 AI debug panel — sticky always-on panel showing why the bot picked its move (analysis cached from the bot's POV after each AI turn). For development / curiosity. When combined with hints, the sidebar button toggles the panel between debug content (default) and live hint content."</span>
                    </label>
                </Show>
            </fieldset>
            <fieldset class="card-fieldset">
                <legend>"Rules"</legend>
                <label class="radio-row">
                    <input
                        type="radio"
                        name="xiangqi-strict"
                        prop:checked=move || !strict.get()
                        on:change=move |_| strict.set(false)
                    />
                    <span>"Casual — leaving your general in check is allowed; you lose if it's actually captured."</span>
                </label>
                <label class="radio-row">
                    <input
                        type="radio"
                        name="xiangqi-strict"
                        prop:checked=move || strict.get()
                        on:change=move |_| strict.set(true)
                    />
                    <span>"Strict — standard rules; self-check moves are illegal."</span>
                </label>
            </fieldset>
            <a href=href rel="external" class="btn btn-primary card-cta">"Start Xiangqi →"</a>
        </div>
    }
}

#[component]
fn BanqiCard() -> impl IntoView {
    let chain = create_rw_signal(false);
    let dark = create_rw_signal(false);
    let dark_trade = create_rw_signal(false);
    let rush = create_rw_signal(false);
    let horse = create_rw_signal(false);
    let seed_text = create_rw_signal(String::new());

    let house = move || {
        let mut h = HouseRules::empty();
        if chain.get() {
            h.insert(HouseRules::CHAIN_CAPTURE);
        }
        if dark.get() {
            h.insert(HouseRules::DARK_CAPTURE);
        }
        if dark_trade.get() {
            h.insert(HouseRules::DARK_CAPTURE_TRADE);
        }
        if rush.get() {
            h.insert(HouseRules::CHARIOT_RUSH);
        }
        if horse.get() {
            h.insert(HouseRules::HORSE_DIAGONAL);
        }
        h
    };
    let apply_preset = move |preset: HouseRules| {
        chain.set(preset.contains(HouseRules::CHAIN_CAPTURE));
        dark.set(preset.contains(HouseRules::DARK_CAPTURE));
        dark_trade.set(preset.contains(HouseRules::DARK_CAPTURE_TRADE));
        rush.set(preset.contains(HouseRules::CHARIOT_RUSH));
        horse.set(preset.contains(HouseRules::HORSE_DIAGONAL));
    };
    let href = move || {
        let seed = seed_text.with(|s| s.trim().parse::<u64>().ok());
        let params = LocalRulesParams { house: house(), seed, ..Default::default() };
        app_href(&build_local_href(Variant::Banqi, &params))
    };

    view! {
        <div class="variant-card variant-card--form">
            <h2>"Banqi 暗棋"</h2>
            <p>"Hidden-piece variant on a 4×8 board. Whoever flips first plays that color."</p>
            <fieldset class="card-fieldset">
                <legend>"Preset"</legend>
                <div class="preset-row">
                    <button class="btn btn-ghost" type="button"
                        on:click=move |_| apply_preset(PRESET_PURIST)>"Purist"</button>
                    <button class="btn btn-ghost" type="button"
                        on:click=move |_| apply_preset(PRESET_TAIWAN)>"Taiwan"</button>
                    <button class="btn btn-ghost" type="button"
                        on:click=move |_| apply_preset(PRESET_AGGRESSIVE)>"Aggressive"</button>
                </div>
            </fieldset>
            <fieldset class="card-fieldset">
                <legend>"House rules"</legend>
                <label class="check-row">
                    <input type="checkbox" prop:checked=move || chain.get()
                        on:change=move |ev| chain.set(event_target_checked(&ev))/>
                    <span>"連吃 — chain captures along a line"</span>
                </label>
                <label class="check-row">
                    <input type="checkbox" prop:checked=move || dark.get()
                        on:change=move |ev| dark.set(event_target_checked(&ev))/>
                    <span>"暗吃 — atomic reveal+capture; on rank-fail your piece stays put (probe)"</span>
                </label>
                <label class="check-row">
                    <input type="checkbox" prop:checked=move || dark_trade.get()
                        on:change=move |ev| dark_trade.set(event_target_checked(&ev))/>
                    <span>"暗吃·搏命 — on rank-fail your attacker dies (implies 暗吃)"</span>
                </label>
                <label class="check-row">
                    <input type="checkbox" prop:checked=move || rush.get()
                        on:change=move |ev| rush.set(event_target_checked(&ev))/>
                    <span>"車衝 — chariot rays the full board; with a gap, captures any piece"</span>
                </label>
                <label class="check-row">
                    <input type="checkbox" prop:checked=move || horse.get()
                        on:change=move |ev| horse.set(event_target_checked(&ev))/>
                    <span>"馬斜 — horse adds diagonal one-step moves; diagonal captures any piece"</span>
                </label>
                <p class="hint">
                    "炮快移 is accepted by the engine but not yet wired into move-gen "
                    "(see "<code>"TODO.md"</code>")."
                </p>
            </fieldset>
            <fieldset class="card-fieldset">
                <legend>"Seed (optional)"</legend>
                <input
                    type="text"
                    inputmode="numeric"
                    placeholder="leave blank for random"
                    class="text-input"
                    prop:value=move || seed_text.get()
                    on:input=move |ev| seed_text.set(event_target_value(&ev))
                />
                <p class="hint">"Same seed = same shuffle, useful for puzzles or rematches."</p>
            </fieldset>
            <a href=href rel="external" class="btn btn-primary card-cta">"Start Banqi →"</a>
        </div>
    }
}

#[component]
fn ThreeKingdomCard() -> impl IntoView {
    view! {
        <a href=app_href("/local/three-kingdom") rel="external" class="variant-card">
            <h2>"Three-Kingdom 三國暗棋"</h2>
            <p>"3-player banqi. Engine still WIP — board renders but moves are gated."</p>
        </a>
    }
}

#[component]
fn OnlineCard() -> impl IntoView {
    view! {
        <a href=app_href("/lobby") rel="external" class="variant-card variant-card--online">
            <h2>"Online"</h2>
            <p>"Browse rooms or create your own."</p>
        </a>
    }
}
