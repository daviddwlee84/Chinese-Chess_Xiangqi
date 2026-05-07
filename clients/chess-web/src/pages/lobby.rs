use chess_net::protocol::{ClientMsg, RoomStatus, RoomSummary, ServerMsg};
use leptos::*;
use leptos_router::use_navigate;

use crate::config::lobby_url;
use crate::ws::{connect, ConnState};

#[component]
pub fn LobbyPage() -> impl IntoView {
    let (rooms, set_rooms) = create_signal::<Vec<RoomSummary>>(vec![]);
    let (room_input, set_room_input) = create_signal::<String>(String::new());
    let (password_input, set_password_input) = create_signal::<String>(String::new());

    let (handle, incoming, conn) = connect(lobby_url());

    // Fan ServerMsg::Rooms into the rooms signal.
    create_effect(move |_| {
        if let Some(ServerMsg::Rooms { rooms: list }) = incoming.get() {
            set_rooms.set(list);
        }
    });

    let refresh_handle = handle.clone();
    let refresh = move |_| {
        refresh_handle.send(ClientMsg::ListRooms);
    };

    let navigate = use_navigate();
    let nav_for_join = navigate.clone();
    let go_to_room: Callback<String> = Callback::new(move |id: String| {
        let pw = password_input.get_untracked();
        let target = if pw.is_empty() {
            format!("/play/{}", id)
        } else {
            format!("/play/{}?password={}", id, urlencode(&pw))
        };
        nav_for_join(&target, Default::default());
    });

    let nav_for_watch = navigate;
    let watch_room: Callback<String> = Callback::new(move |id: String| {
        let pw = password_input.get_untracked();
        let qs = if pw.is_empty() {
            "role=spectator".to_string()
        } else {
            format!("role=spectator&password={}", urlencode(&pw))
        };
        nav_for_watch(&format!("/play/{}?{}", id, qs), Default::default());
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
            <a href="/" class="back-link">"← Back to picker"</a>
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
                            "{}|{}|{}|{:?}",
                            r.id,
                            r.seats,
                            r.spectators,
                            r.status
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
