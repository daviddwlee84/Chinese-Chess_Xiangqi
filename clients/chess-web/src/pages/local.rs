use leptos::*;
use leptos_router::use_params_map;

#[component]
pub fn LocalPage() -> impl IntoView {
    let params = use_params_map();
    let variant = move || params.with(|p| p.get("variant").cloned().unwrap_or_default());
    view! {
        <section class="game-page">
            <div>
                <a href="/" class="back-link">"← Back to picker"</a>
                <h2>{move || format!("Local play: {}", variant())}</h2>
                <p class="subtitle">"Board renderer ships in the next commit."</p>
            </div>
        </section>
    }
}
