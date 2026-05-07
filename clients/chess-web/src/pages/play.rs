use chess_core::coord::Square;
use chess_core::piece::Side;
use chess_core::rules::RuleSet;
use chess_core::state::GameStatus;
use chess_core::view::{PlayerView, VisibleCell};
use chess_net::protocol::{ClientMsg, ServerMsg};
use leptos::*;
use leptos_router::{use_params_map, use_query_map};

use crate::components::board::Board;
use crate::config::room_url;
use crate::routes::variant_slug;
use crate::state::find_move;
use crate::ws::{connect, ConnState, WsHandle};

#[component]
pub fn PlayPage() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let room = move || params.with(|p| p.get("room").cloned().unwrap_or_default());
    let password = move || query.with(|q| q.get("password").cloned().filter(|s| !s.is_empty()));

    // Open the WS once on mount.
    let url = room_url(&room(), password().as_deref());
    let (handle, incoming, conn) = connect(url);

    let (rules, set_rules) = create_signal::<Option<RuleSet>>(None);
    let (observer, set_observer) = create_signal::<Option<Side>>(None);
    let (view_signal, set_view) = create_signal::<Option<PlayerView>>(None);
    let (toast, set_toast) = create_signal::<Option<String>>(None);

    create_effect(move |_| match incoming.get() {
        Some(ServerMsg::Hello { observer: obs, rules: r, view: v, .. }) => {
            set_observer.set(Some(obs));
            set_rules.set(Some(r));
            set_view.set(Some(v));
            set_toast.set(None);
        }
        Some(ServerMsg::Update { view: v }) => {
            set_view.set(Some(v));
        }
        Some(ServerMsg::Error { message }) => {
            set_toast.set(Some(message));
        }
        Some(ServerMsg::Rooms { .. }) | None => {}
    });

    let handle_for_board = handle.clone();
    let handle_for_sidebar = handle;
    let room_label = room();

    view! {
        <section class="game-page">
            <div class="board-pane">
                <Show
                    when=move || view_signal.get().is_some() && observer.get().is_some()
                    fallback=move || view! { <ConnPlaceholder room=room() conn=conn/> }
                >
                    <BoardWrapper
                        view_signal=view_signal
                        observer=observer
                        handle=handle_for_board.clone()
                    />
                </Show>
                <Show when=move || toast.get().is_some()>
                    <div class="toast" on:click=move |_| set_toast.set(None)>
                        {move || toast.get().unwrap_or_default()}
                    </div>
                </Show>
            </div>
            <OnlineSidebar
                room=room_label
                observer=observer
                view_signal=view_signal
                rules=rules
                conn=conn
                handle=handle_for_sidebar
            />
        </section>
    }
}

#[component]
fn ConnPlaceholder(room: String, conn: ReadSignal<ConnState>) -> impl IntoView {
    let label = move || match conn.get() {
        ConnState::Connecting => "Connecting…".to_string(),
        ConnState::Open => "Waiting for server greeting…".to_string(),
        ConnState::Closed => "Disconnected — refresh to reconnect.".to_string(),
        ConnState::Error => "Connection error — is the server running?".to_string(),
    };
    view! {
        <div class="conn-placeholder">
            <p class="muted">{format!("Room: {}", room)}</p>
            <p>{label}</p>
        </div>
    }
}

#[component]
fn BoardWrapper(
    view_signal: ReadSignal<Option<PlayerView>>,
    observer: ReadSignal<Option<Side>>,
    handle: WsHandle,
) -> impl IntoView {
    let view = Signal::derive(move || view_signal.get().expect("guarded by Show"));
    let obs = observer.get_untracked().expect("guarded by Show");
    let shape = view.with_untracked(|v| v.shape);

    let selected = create_rw_signal::<Option<Square>>(None);

    let on_click: Callback<Square> = Callback::new(move |sq: Square| {
        let v = view.get();
        // Only the side-to-move can move (server rejects otherwise; the local
        // legal-move list is empty for the opponent so this is a hard gate).
        if v.observer != v.side_to_move {
            return;
        }
        let cur = selected.get();
        if cur == Some(sq) {
            selected.set(None);
            return;
        }
        if let Some(from) = cur {
            if let Some(mv) = find_move(&v, from, sq) {
                handle.send(ClientMsg::Move { mv });
                selected.set(None);
                return;
            }
        }
        let cell = v.cells[sq.0 as usize];
        match cell {
            VisibleCell::Revealed(p) if p.piece.side == v.side_to_move => {
                selected.set(Some(sq));
            }
            VisibleCell::Hidden => {
                if let Some(mv) = find_move(&v, sq, sq) {
                    handle.send(ClientMsg::Move { mv });
                    selected.set(None);
                }
            }
            _ => selected.set(None),
        }
    });

    view! {
        <Board
            shape=shape
            observer=obs
            view=view
            selected=selected
            on_click=on_click
        />
    }
}

#[component]
fn OnlineSidebar(
    room: String,
    observer: ReadSignal<Option<Side>>,
    view_signal: ReadSignal<Option<PlayerView>>,
    rules: ReadSignal<Option<RuleSet>>,
    conn: ReadSignal<ConnState>,
    handle: WsHandle,
) -> impl IntoView {
    let observer_label = move || match observer.get() {
        Some(Side::RED) => "You play Red 紅",
        Some(Side::BLACK) => "You play Black 黑",
        Some(_) => "You play Green 綠",
        None => "Awaiting seat assignment",
    };

    let variant_label = move || match rules.get() {
        Some(r) => variant_slug(r.variant).to_string(),
        None => "—".to_string(),
    };

    let turn_label = move || match view_signal.get() {
        Some(v) => match v.side_to_move {
            Side::RED => "Red 紅 to move",
            Side::BLACK => "Black 黑 to move",
            _ => "Green 綠 to move",
        }
        .to_string(),
        None => "—".to_string(),
    };

    let game_over = move || {
        matches!(
            view_signal.get().map(|v| v.status),
            Some(GameStatus::Won { .. } | GameStatus::Drawn { .. })
        )
    };

    let conn_label = move || match conn.get() {
        ConnState::Connecting => "Connecting…",
        ConnState::Open => "Connected",
        ConnState::Closed => "Disconnected",
        ConnState::Error => "Error",
    };

    let resign = {
        let handle = handle.clone();
        move |_| {
            handle.send(ClientMsg::Resign);
        }
    };
    let rematch = {
        let handle = handle.clone();
        move |_| {
            handle.send(ClientMsg::Rematch);
        }
    };

    view! {
        <aside class="sidebar">
            <h3 class="variant-label">{format!("Online — room {}", room)}</h3>
            <p class="muted">{variant_label}</p>
            <p>{observer_label}</p>
            <p>{turn_label}</p>
            <p class="muted">{conn_label}</p>
            <div class="sidebar-actions">
                <button class="btn" on:click=resign disabled=game_over>"Resign"</button>
                <button class="btn" on:click=rematch disabled=move || !game_over()>"Rematch"</button>
            </div>
            <a class="back-link" href="/lobby">"← Back to lobby"</a>
        </aside>
    }
}
