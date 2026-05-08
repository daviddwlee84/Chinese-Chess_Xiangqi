use chess_core::rules::{HouseRules, Variant, PRESET_AGGRESSIVE, PRESET_PURIST, PRESET_TAIWAN};
use leptos::*;

use crate::routes::{app_href, build_local_href, LocalRulesParams};

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
    let href = move || {
        let params = LocalRulesParams { strict: strict.get(), ..Default::default() };
        app_href(&build_local_href(Variant::Xiangqi, &params))
    };
    view! {
        <div class="variant-card variant-card--form">
            <h2>"Xiangqi 象棋"</h2>
            <p>"Standard 9×10 Chinese chess."</p>
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
        let params = LocalRulesParams { strict: false, house: house(), seed };
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
