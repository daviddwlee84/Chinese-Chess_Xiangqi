use leptos::*;
use leptos_router::{Route, Router, Routes};

use crate::pages::{lobby::LobbyPage, local::LocalPage, picker::Picker, play::PlayPage};
use crate::prefs::Prefs;

#[component]
pub fn App() -> impl IntoView {
    // Hydrate FX prefs once at app boot and share via context. Pages
    // and components grab the signals with `expect_context::<Prefs>()`.
    let prefs = Prefs::hydrate();
    provide_context(prefs);

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
