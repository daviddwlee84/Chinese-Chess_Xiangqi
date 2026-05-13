use chess_net::protocol::{ClientMsg, RoomStatus, RoomSummary, ServerMsg};
use leptos::*;
use leptos_router::use_query_map;

use crate::components::ws_setup::WsSetup;
use crate::config::{
    append_query_pairs, lobby_url, resolve_ws_base, ws_query_pair, WsBase, WsBaseChoiceError,
    WS_QUERY_KEY,
};
use crate::routes::{app_href, WsBaseError};
use crate::transport::{self, ConnState};

#[component]
pub fn LobbyPage() -> impl IntoView {
    let query = use_query_map();
    let raw_ws = query.with(|q| q.get(WS_QUERY_KEY).cloned());
    match resolve_ws_base(raw_ws) {
        Ok(ws_base) => view! { <LobbyConnected ws_base=ws_base/> }.into_view(),
        Err(err) => view! {
            <WsSetup
                title="Online lobby"
                next_path="/lobby".to_string()
                initial_error=ws_error_label(err).unwrap_or("")
            />
        }
        .into_view(),
    }
}

#[component]
fn LobbyConnected(ws_base: WsBase) -> impl IntoView {
    let (rooms, set_rooms) = create_signal::<Vec<RoomSummary>>(vec![]);
    let (room_input, set_room_input) = create_signal::<String>(String::new());
    let (password_input, set_password_input) = create_signal::<String>(String::new());
    // Opt-in checkbox: when checked, "Create / Join" appends `?hints=1`
    // so this client becomes the room's first joiner with hints
    // sanctioned. If joining an existing room, the existing room's
    // setting wins (server-side first-write-wins, like password).
    let (allow_hints, set_allow_hints) = create_signal::<bool>(false);

    let session = transport::ws::connect(lobby_url(&ws_base));
    let incoming = session.incoming;
    let conn = session.state;

    // Fan ServerMsg::Rooms into the rooms signal. Drains the
    // incoming queue (see `transport::Session` doc for why
    // incoming is a Vec, not a latched Option).
    create_effect(move |_| {
        let msgs = incoming.get();
        if msgs.is_empty() {
            return;
        }
        for msg in msgs {
            if let ServerMsg::Rooms { rooms: list } = msg {
                set_rooms.set(list);
            }
        }
        incoming.set(Vec::new());
    });

    let refresh_handle = session.handle.clone();
    let refresh = move |_| {
        refresh_handle.send(ClientMsg::ListRooms);
    };

    let ws_for_join = ws_base.clone();
    let go_to_room: Callback<String> = Callback::new(move |id: String| {
        let pw = password_input.get_untracked();
        let want_hints = allow_hints.get_untracked();
        let mut pairs = Vec::new();
        if let Some(pair) = ws_query_pair(&ws_for_join) {
            pairs.push(pair);
        }
        if !pw.is_empty() {
            pairs.push(format!("password={}", urlencode(&pw)));
        }
        if want_hints {
            pairs.push("hints=1".to_string());
        }
        navigate_external(&append_query_pairs(&format!("/play/{id}"), pairs));
    });

    let ws_for_watch = ws_base.clone();
    let watch_room: Callback<String> = Callback::new(move |id: String| {
        let pw = password_input.get_untracked();
        let want_hints = allow_hints.get_untracked();
        let mut pairs = Vec::new();
        if let Some(pair) = ws_query_pair(&ws_for_watch) {
            pairs.push(pair);
        }
        pairs.push("role=spectator".to_string());
        if !pw.is_empty() {
            pairs.push(format!("password={}", urlencode(&pw)));
        }
        if want_hints {
            // Spectators forward hints=1 too — useful for the "watch
            // your friend's game with AI commentary" flow when the
            // creator forgot to enable it. Server still gates on
            // first-joiner; spectators arrive after seats fill so this
            // mostly applies when watching an empty / new room.
            pairs.push("hints=1".to_string());
        }
        navigate_external(&append_query_pairs(&format!("/play/{id}"), pairs));
    });

    let create_or_join = move |_| {
        let id = room_input.get_untracked().trim().to_string();
        if id.is_empty() {
            return;
        }
        go_to_room.call(id);
    };

    view! {
        <section class="lobby">
            <a href=app_href("/") rel="external" class="back-link">"← Back to picker"</a>
            <h2>"Online lobby"</h2>
            <p class="subtitle">"Pick an existing room or create a new one. The server's variant + house rules apply to every room it hosts."</p>

            <ConnBanner conn=conn/>

            <div class="lobby-form">
                <input
                    class="text-input"
                    type="text"
                    placeholder="room name (e.g. 'a-friendly-game')"
                    prop:value=room_input
                    on:input=move |ev| set_room_input.set(event_target_value(&ev))
                />
                <input
                    class="text-input"
                    type="password"
                    placeholder="password (optional)"
                    prop:value=password_input
                    on:input=move |ev| set_password_input.set(event_target_value(&ev))
                />
                <label class="check-row" title="When you create a new room with this checked, both players + spectators get the AI hint panel. Frozen at creation; existing rooms keep their setting.">
                    <input
                        type="checkbox"
                        prop:checked=move || allow_hints.get()
                        on:change=move |ev| set_allow_hints.set(event_target_checked(&ev))
                    />
                    <span>"🧠 Allow AI hints in this room (anti-cheat: must be set BEFORE the game starts)"</span>
                </label>
                <button class="btn btn-primary" on:click=create_or_join>
                    "Create / Join"
                </button>
                <button class="btn" on:click=refresh>"Refresh"</button>
            </div>

            <h3>"Active rooms"</h3>
            <Show
                when=move || !rooms.get().is_empty()
                fallback=|| view! { <p class="muted">"No rooms yet. Create one above."</p> }
            >
                <ul class="rooms-list">
                    <For
                        each=move || rooms.get()
                        key=|r| format!(
                            "{}|{}|{}|{:?}|{}",
                            r.id,
                            r.seats,
                            r.spectators,
                            r.status,
                            r.hints_allowed,
                        )
                        children=move |room| {
                            let id_join = room.id.clone();
                            let id_watch = room.id.clone();
                            let seats_label = if room.spectators > 0 {
                                format!("{}/2 seats · {} 👁", room.seats, room.spectators)
                            } else {
                                format!("{}/2 seats", room.seats)
                            };
                            let join_disabled = room.seats >= 2;
                            view! {
                                <li class="room-row">
                                    <span class="room-id">{room.id.clone()}</span>
                                    <span class="room-variant">{room.variant.clone()}</span>
                                    <span class="room-seats">{seats_label}</span>
                                    {if room.has_password {
                                        view! { <span class="room-lock" title="password protected">"🔒"</span> }.into_view()
                                    } else { ().into_view() }}
                                    {if room.hints_allowed {
                                        view! { <span class="room-hints" title="AI hints enabled — both players + spectators see the panel">"🧠"</span> }.into_view()
                                    } else { ().into_view() }}
                                    <span class=room_status_class(room.status)>{room_status_label(room.status)}</span>
                                    <span class="room-actions">
                                        <button
                                            class="btn"
                                            on:click=move |ev| { ev.stop_propagation(); go_to_room.call(id_join.clone()); }
                                            disabled=join_disabled
                                        >
                                            "Join"
                                        </button>
                                        <button
                                            class="btn btn-ghost"
                                            on:click=move |ev| { ev.stop_propagation(); watch_room.call(id_watch.clone()); }
                                        >
                                            "Watch"
                                        </button>
                                    </span>
                                </li>
                            }
                        }
                    />
                </ul>
            </Show>
        </section>
    }
}

fn navigate_external(path: &str) {
    if let Some(win) = web_sys::window() {
        let _ = win.location().set_href(&app_href(path));
    }
}

fn ws_error_label(err: WsBaseChoiceError) -> Option<&'static str> {
    match err {
        WsBaseChoiceError::Missing => None,
        WsBaseChoiceError::Invalid(WsBaseError::Empty) => Some("Enter a websocket server URL."),
        WsBaseChoiceError::Invalid(WsBaseError::BadScheme) => {
            Some("Use a full ws:// or wss:// server URL.")
        }
    }
}

#[component]
fn ConnBanner(conn: ReadSignal<ConnState>) -> impl IntoView {
    let label = move || match conn.get() {
        ConnState::Connecting => "Connecting…",
        ConnState::Open => "Connected",
        ConnState::Closed => "Disconnected — refresh to reconnect.",
        ConnState::Error => "Connection error — is the server running?",
    };
    let cls = move || match conn.get() {
        ConnState::Open => "conn-banner ok",
        ConnState::Connecting => "conn-banner pending",
        _ => "conn-banner error",
    };
    view! { <p class=cls>{label}</p> }
}

fn room_status_label(s: RoomStatus) -> &'static str {
    match s {
        RoomStatus::Lobby => "waiting",
        RoomStatus::Playing => "playing",
        RoomStatus::Finished => "finished",
    }
}

fn room_status_class(s: RoomStatus) -> &'static str {
    match s {
        RoomStatus::Lobby => "room-status lobby",
        RoomStatus::Playing => "room-status playing",
        RoomStatus::Finished => "room-status finished",
    }
}

fn urlencode(s: &str) -> String {
    js_sys::encode_uri_component(s).as_string().unwrap_or_default()
}
