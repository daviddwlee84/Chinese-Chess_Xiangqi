use chess_core::rules::{HouseRules, Variant, PRESET_AGGRESSIVE, PRESET_PURIST, PRESET_TAIWAN};
use leptos::*;
use leptos_router::A;

use crate::routes::{build_local_href, LocalRulesParams};

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
        build_local_href(Variant::Xiangqi, &params)
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
            <A href=href class="btn btn-primary card-cta">"Start Xiangqi →"</A>
        </div>
    }
}

#[component]
fn BanqiCard() -> impl IntoView {
    let chain = create_rw_signal(false);
    let dark = create_rw_signal(false);
    let rush = create_rw_signal(false);
    let seed_text = create_rw_signal(String::new());

    let house = move || {
        let mut h = HouseRules::empty();
        if chain.get() {
            h.insert(HouseRules::CHAIN_CAPTURE);
        }
        if dark.get() {
            h.insert(HouseRules::DARK_CHAIN);
        }
        if rush.get() {
            h.insert(HouseRules::CHARIOT_RUSH);
        }
        h
    };
    let apply_preset = move |preset: HouseRules| {
        chain.set(preset.contains(HouseRules::CHAIN_CAPTURE));
        dark.set(preset.contains(HouseRules::DARK_CHAIN));
        rush.set(preset.contains(HouseRules::CHARIOT_RUSH));
    };
    let href = move || {
        let seed = seed_text.with(|s| s.trim().parse::<u64>().ok());
        let params = LocalRulesParams { strict: false, house: house(), seed };
        build_local_href(Variant::Banqi, &params)
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
                    <span>"暗連 — chain through face-down squares (implies 連吃)"</span>
                </label>
                <label class="check-row">
                    <input type="checkbox" prop:checked=move || rush.get()
                        on:change=move |ev| rush.set(event_target_checked(&ev))/>
                    <span>"車衝 — chariot rays the full board on a capture"</span>
                </label>
                <p class="hint">
                    "馬斜 / 炮快移 are accepted by the engine but not yet wired into move-gen "
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
            <A href=href class="btn btn-primary card-cta">"Start Banqi →"</A>
        </div>
    }
}

#[component]
fn ThreeKingdomCard() -> impl IntoView {
    view! {
        <A href="/local/three-kingdom" class="variant-card">
            <h2>"Three-Kingdom 三國暗棋"</h2>
            <p>"3-player banqi. Engine still WIP — board renders but moves are gated."</p>
        </A>
    }
}

#[component]
fn OnlineCard() -> impl IntoView {
    view! {
        <A href="/lobby" class="variant-card variant-card--online">
            <h2>"Online"</h2>
            <p>"Browse rooms or create your own."</p>
        </A>
    }
}
