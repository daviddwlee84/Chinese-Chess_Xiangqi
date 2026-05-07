use leptos::*;
use leptos_router::{Route, Router, Routes};

use crate::pages::{lobby::LobbyPage, local::LocalPage, picker::Picker, play::PlayPage};

#[component]
pub fn App() -> impl IntoView {
    view! {
        <Router>
            <main class="app-shell">
                <Routes>
                    <Route path="/" view=Picker/>
                    <Route path="/local/:variant" view=LocalPage/>
                    <Route path="/lobby" view=LobbyPage/>
                    <Route path="/play/:room" view=PlayPage/>
                </Routes>
            </main>
        </Router>
    }
}
