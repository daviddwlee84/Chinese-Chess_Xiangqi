use leptos::*;
use leptos_router::{Route, Router, Routes};

use crate::components::pwa::{OfflineIndicator, PwaState, PwaUpdateToast};
use crate::pages::{lobby::LobbyPage, local::LocalPage, picker::Picker, play::PlayPage};
use crate::prefs::Prefs;
use crate::routes::base_path;

#[component]
pub fn App() -> impl IntoView {
    // Hydrate FX prefs once at app boot and share via context. Pages
    // and components grab the signals with `expect_context::<Prefs>()`.
    let prefs = Prefs::hydrate();
    provide_context(prefs);

    // PWA bridge — install / update / online state, sourced from the
    // `window.__pwa` shim in `public/pwa.js`. Hydrated once at boot.
    let pwa = PwaState::hydrate();
    provide_context(pwa);

    view! {
        <Router base=base_path()>
            <AppShell/>
        </Router>
    }
}

#[component]
fn AppShell() -> impl IntoView {
    view! {
        <main class="app-shell">
            <Routes base=base_path().to_string()>
                <Route path="/" view=Picker/>
                <Route path="/local/:variant" view=LocalPage/>
                <Route path="/lobby" view=LobbyPage/>
                <Route path="/play/:room" view=PlayPage/>
                <Route path="/*any" view=Picker/>
            </Routes>
            <PwaUpdateToast/>
            <OfflineIndicator/>
        </main>
    }
}
