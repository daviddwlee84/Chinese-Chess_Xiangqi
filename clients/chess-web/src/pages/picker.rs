use leptos::*;
use leptos_router::A;

#[component]
pub fn Picker() -> impl IntoView {
    view! {
        <section class="picker">
            <h1>"Chinese Chess"</h1>
            <p class="subtitle">
                "Pick a variant for local pass-and-play, or join the online lobby."
            </p>
            <div class="picker-grid">
                <A href="/local/xiangqi" class="variant-card">
                    <h2>"Xiangqi 象棋"</h2>
                    <p>"Standard 9×10 Chinese chess."</p>
                </A>
                <A href="/local/banqi" class="variant-card">
                    <h2>"Banqi 暗棋"</h2>
                    <p>"Hidden-piece variant on a 4×8 board."</p>
                </A>
                <A href="/local/three-kingdom" class="variant-card">
                    <h2>"Three-Kingdom 三國暗棋"</h2>
                    <p>"3-player banqi (engine still WIP)."</p>
                </A>
                <A href="/lobby" class="variant-card variant-card--online">
                    <h2>"Online"</h2>
                    <p>"Browse rooms or create your own."</p>
                </A>
            </div>
        </section>
    }
}
