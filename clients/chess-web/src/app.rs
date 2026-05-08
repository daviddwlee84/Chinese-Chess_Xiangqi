use leptos::*;
use leptos_router::{Route, Router, Routes};

use crate::pages::{lobby::LobbyPage, local::LocalPage, picker::Picker, play::PlayPage};
use crate::prefs::Prefs;
use crate::routes::router_base;

#[component]
pub fn App() -> impl IntoView {
    // Hydrate FX prefs once at app boot and share via context. Pages
    // and components grab the signals with `expect_context::<Prefs>()`.
    let prefs = Prefs::hydrate();
    provide_context(prefs);

    if let Some(base) = router_base() {
        view! {
            <Router base=base>
                <AppShell/>
            </Router>
        }
        .into_view()
    } else {
        view! {
            <Router>
                <AppShell/>
            </Router>
        }
        .into_view()
    }
}

#[component]
fn AppShell() -> impl IntoView {
    view! {
            <main class="app-shell">
                <Routes>
                    <Route path="/" view=Picker/>
                    <Route path="/local/:variant" view=LocalPage/>
                    <Route path="/lobby" view=LobbyPage/>
                    <Route path="/play/:room" view=PlayPage/>
                </Routes>
            </main>
    }
}
