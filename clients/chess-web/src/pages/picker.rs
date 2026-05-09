use chess_ai::{Difficulty, Randomness, Strategy};
use chess_core::piece::Side;
use chess_core::rules::{HouseRules, Variant, PRESET_AGGRESSIVE, PRESET_PURIST, PRESET_TAIWAN};
use leptos::*;

use crate::routes::{app_href, build_local_href, LocalRulesParams, PlayMode, MAX_AI_DEPTH};

#[component]
pub fn Picker() -> impl IntoView {
    view! {
        <section class="picker">
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
        </section>
    }
}

#[component]
fn XiangqiCard() -> impl IntoView {
    let strict = create_rw_signal(false);
    let mode = create_rw_signal(PlayMode::Pvp);
    let player_red = create_rw_signal(true);
    let difficulty = create_rw_signal(Difficulty::Normal);
    let ai_strategy = create_rw_signal(Strategy::default());
    // None = "use difficulty default"; Some(_) = explicit override.
    // Encoded into the URL via `&variation=`.
    let ai_variation = create_rw_signal::<Option<Randomness>>(None);
    // Empty = use difficulty default depth; numeric = override (clamped
    // 1..=MAX_AI_DEPTH on parse).
    let ai_depth_text = create_rw_signal(String::new());
    let href = move || {
        let ai_side = if player_red.get() { Side::BLACK } else { Side::RED };
        let ai_depth =
            ai_depth_text.with(|s| s.trim().parse::<u8>().ok().map(|d| d.clamp(1, MAX_AI_DEPTH)));
        let params = LocalRulesParams {
            strict: strict.get(),
            mode: mode.get(),
            ai_side,
            ai_difficulty: difficulty.get(),
            ai_strategy: ai_strategy.get(),
            ai_variation: ai_variation.get(),
            ai_depth,
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
                            prop:checked=move || difficulty.get() == Difficulty::Easy
                            on:change=move |_| difficulty.set(Difficulty::Easy)
                        />
                        <span>"Easy — depth 1, picks at random from the top three replies."</span>
                    </label>
                    <label class="radio-row">
                        <input
                            type="radio"
                            name="xiangqi-difficulty"
                            prop:checked=move || difficulty.get() == Difficulty::Normal
                            on:change=move |_| difficulty.set(Difficulty::Normal)
                        />
                        <span>"Normal — depth 3, mostly plays the best line."</span>
                    </label>
                    <label class="radio-row">
                        <input
                            type="radio"
                            name="xiangqi-difficulty"
                            prop:checked=move || difficulty.get() == Difficulty::Hard
                            on:change=move |_| difficulty.set(Difficulty::Hard)
                        />
                        <span>"Hard — depth 4, may take a couple of seconds per move."</span>
                    </label>
                </fieldset>
                <fieldset class="card-fieldset">
                    <legend>"Search depth (advanced)"</legend>
                    <p class="hint">
                        "Override the difficulty's default search depth (Easy=1, Normal=3, Hard=4). Higher = stronger but slower. Cap: "
                        {MAX_AI_DEPTH.to_string()}". Leave blank to use the difficulty default."
                    </p>
                    <input
                        type="number"
                        inputmode="numeric"
                        min="1"
                        max=MAX_AI_DEPTH.to_string()
                        placeholder="(default)"
                        class="text-input"
                        prop:value=move || ai_depth_text.get()
                        on:input=move |ev| ai_depth_text.set(event_target_value(&ev))
                    />
                </fieldset>
                <fieldset class="card-fieldset">
                    <legend>"Engine"</legend>
                    <label class="radio-row">
                        <input
                            type="radio"
                            name="xiangqi-engine"
                            prop:checked=move || ai_strategy.get() == Strategy::QuiescenceMvvLvaV4
                            on:change=move |_| ai_strategy.set(Strategy::QuiescenceMvvLvaV4)
                        />
                        <span>"v4 — quiescence + MVV-LVA (recommended). Avoids horizon-effect blunders on captures."</span>
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
