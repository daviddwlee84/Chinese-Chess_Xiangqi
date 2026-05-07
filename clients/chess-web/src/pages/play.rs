use leptos::*;
use leptos_router::use_params_map;

#[component]
pub fn PlayPage() -> impl IntoView {
    let params = use_params_map();
    let room = move || params.with(|p| p.get("room").cloned().unwrap_or_default());
    view! {
        <section class="game-page">
            <div>
                <a href="/lobby" class="back-link">"← Back to lobby"</a>
                <h2>{move || format!("Online play: room {}", room())}</h2>
                <p class="subtitle">"Wires up to chess-net /ws/<room> in a later commit."</p>
            </div>
        </section>
    }
}
