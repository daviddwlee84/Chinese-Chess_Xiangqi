use leptos::*;

#[component]
pub fn LobbyPage() -> impl IntoView {
    view! {
        <section class="game-page">
            <div>
                <a href="/" class="back-link">"← Back to picker"</a>
                <h2>"Online lobby"</h2>
                <p class="subtitle">"Wires up to chess-net /lobby in a later commit."</p>
            </div>
        </section>
    }
}
