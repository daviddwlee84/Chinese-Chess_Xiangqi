use chess_core::board::BoardShape;
use chess_core::coord::Square;
use chess_core::piece::Side;
use chess_core::rules::RuleSet;
use chess_core::state::GameStatus;
use chess_core::view::{PlayerView, VisibleCell};
use chess_net::protocol::{ChatLine, ClientMsg, ServerMsg};
use leptos::*;
use leptos_router::{use_params_map, use_query_map};

use crate::components::board::Board;
use crate::components::chat_panel::ChatPanel;
use crate::components::end_overlay::EndOverlay;
use crate::config::room_url;
use crate::prefs::Prefs;
use crate::routes::variant_slug;
use crate::state::{end_chain_move, find_move, truncate_front, ClientRole};
use crate::ws::{connect, ConnState, WsHandle};

const CHAT_HISTORY_CAP: usize = 50;

#[component]
pub fn PlayPage() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let room = move || params.with(|p| p.get("room").cloned().unwrap_or_default());
    let password = move || query.with(|q| q.get("password").cloned().filter(|s| !s.is_empty()));
    let watch_only = move || {
        query.with(|q| q.get("role").map(|r| r.eq_ignore_ascii_case("spectator")).unwrap_or(false))
    };

    // Open the WS once on mount. Spectator opt-in propagates into the URL.
    let url = room_url(&room(), password().as_deref(), watch_only());
    let (handle, incoming, conn) = connect(url);

    let (rules, set_rules) = create_signal::<Option<RuleSet>>(None);
    let (role, set_role) = create_signal::<Option<ClientRole>>(None);
    let (view_signal, set_view) = create_signal::<Option<PlayerView>>(None);
    let (chat, set_chat) = create_signal::<Vec<ChatLine>>(Vec::new());
    let (toast, set_toast) = create_signal::<Option<String>>(None);

    create_effect(move |_| match incoming.get() {
        Some(ServerMsg::Hello { observer, rules: r, view: v, .. }) => {
            set_role.set(Some(ClientRole::Player(observer)));
            set_rules.set(Some(r));
            set_view.set(Some(v));
            set_toast.set(None);
        }
        Some(ServerMsg::Spectating { rules: r, view: v, .. }) => {
            set_role.set(Some(ClientRole::Spectator));
            set_rules.set(Some(r));
            set_view.set(Some(v));
            set_toast.set(None);
        }
        Some(ServerMsg::Update { view: v }) => {
            set_view.set(Some(v));
        }
        Some(ServerMsg::ChatHistory { lines }) => {
            set_chat.set(lines);
        }
        Some(ServerMsg::Chat { line }) => {
            set_chat.update(|buf| {
                buf.push(line);
                truncate_front(buf, CHAT_HISTORY_CAP);
            });
        }
        Some(ServerMsg::Error { message }) => {
            set_toast.set(Some(message));
        }
        Some(ServerMsg::Rooms { .. }) | None => {}
    });

    let chat_handle = handle.clone();
    let on_chat_send: Callback<String> = Callback::new(move |text: String| {
        chat_handle.send(ClientMsg::Chat { text });
    });

    let handle_for_board = handle.clone();
    let handle_for_sidebar = handle;
    let room_label = room();

    let prefs = expect_context::<Prefs>();
    let fx_confetti: Signal<bool> = prefs.fx_confetti.into();
    // The overlay needs a non-`Option` view signal once the welcome lands.
    // Wrap in a `Show` so we don't mount it pre-welcome (and so the watcher
    // inside doesn't see a fake "transition" on first connect).
    let view_for_overlay = Signal::derive(move || view_signal.get());
    let role_for_overlay: Signal<Option<ClientRole>> = role.into();

    view! {
        <section class="game-page">
            <div class="board-pane">
                <Show
                    when=move || view_signal.get().is_some() && role.get().is_some()
                    fallback=move || view! { <ConnPlaceholder room=room() conn=conn/> }
                >
                    <BoardWrapper
                        view_signal=view_signal
                        role=role
                        handle=handle_for_board.clone()
                    />
                    <EndOverlay
                        view=Signal::derive(move || view_for_overlay.get().expect("guarded by Show"))
                        role=role_for_overlay
                        enabled=fx_confetti
                    />
                </Show>
                <Show when=move || toast.get().is_some()>
                    <div class="toast" on:click=move |_| set_toast.set(None)>
                        {move || toast.get().unwrap_or_default()}
                    </div>
                </Show>
            </div>
            <div class="right-column">
                <OnlineSidebar
                    room=room_label
                    role=role
                    view_signal=view_signal
                    rules=rules
                    conn=conn
                    handle=handle_for_sidebar
                />
                <ChatPanel
                    role=role.into()
                    log=chat.into()
                    on_send=on_chat_send
                />
            </div>
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
    role: ReadSignal<Option<ClientRole>>,
    handle: WsHandle,
) -> impl IntoView {
    let view = Signal::derive(move || view_signal.get().expect("guarded by Show"));
    let resolved_role = role.get_untracked().expect("guarded by Show");
    let obs = resolved_role.observer();
    let shape = view.with_untracked(|v| v.shape);

    let selected = create_rw_signal::<Option<Square>>(None);
    // Spectators see-only — no click handlers on the board for them.
    let is_spectator = resolved_role.is_spectator();

    // Surface engine chain_lock as the highlighted "selected" piece, so
    // legal-target dots render around it during chain mode.
    let effective_selected: Signal<Option<Square>> = Signal::derive(move || {
        let v = view.get();
        v.chain_lock.or_else(|| selected.get())
    });

    let click_handle = handle.clone();
    let on_click: Callback<Square> = Callback::new(move |sq: Square| {
        if is_spectator {
            return;
        }
        let v = view.get();
        if v.observer != v.side_to_move {
            return;
        }

        // Chain mode: only the locked piece may move (captures only).
        // Clicking the locked piece itself releases the chain.
        if let Some(locked) = v.chain_lock {
            if sq == locked {
                if let Some(mv) = end_chain_move(&v) {
                    click_handle.send(ClientMsg::Move { mv });
                }
                selected.set(None);
                return;
            }
            if let Some(mv) = find_move(&v, locked, sq) {
                click_handle.send(ClientMsg::Move { mv });
                selected.set(None);
                return;
            }
            return;
        }

        let cur = selected.get();
        if cur == Some(sq) {
            selected.set(None);
            return;
        }
        if let Some(from) = cur {
            if let Some(mv) = find_move(&v, from, sq) {
                click_handle.send(ClientMsg::Move { mv });
                selected.set(None);
                return;
            }
        }
        let cell = v.cells[sq.0 as usize];
        let is_selectable_revealed = matches!(cell, VisibleCell::Revealed(_))
            && v.legal_moves.iter().any(|m| m.origin_square() == sq);
        if is_selectable_revealed {
            selected.set(Some(sq));
        } else if matches!(cell, VisibleCell::Hidden) {
            if let Some(mv) = find_move(&v, sq, sq) {
                click_handle.send(ClientMsg::Move { mv });
                selected.set(None);
            }
        } else {
            selected.set(None);
        }
    });

    let chain_active = Signal::derive(move || view.with(|v| v.chain_lock.is_some()));
    let end_handle = handle.clone();
    let on_end_chain = move |_| {
        let v = view.get();
        if let Some(mv) = end_chain_move(&v) {
            end_handle.send(ClientMsg::Move { mv });
        }
    };

    view! {
        <Board
            shape=shape
            observer=obs
            view=view
            selected=effective_selected
            on_click=on_click
        />
        <Show when=move || chain_active.get() && !is_spectator>
            <div class="chain-banner">
                <span>"連吃 — 繼續吃 or "</span>
                <button class="btn btn-ghost btn-sm" on:click=on_end_chain.clone()>
                    "End chain"
                </button>
            </div>
        </Show>
    }
}

#[component]
fn OnlineSidebar(
    room: String,
    role: ReadSignal<Option<ClientRole>>,
    view_signal: ReadSignal<Option<PlayerView>>,
    rules: ReadSignal<Option<RuleSet>>,
    conn: ReadSignal<ConnState>,
    handle: WsHandle,
) -> impl IntoView {
    let role_label = move || match role.get() {
        Some(ClientRole::Player(Side::RED)) => "You play Red 紅".to_string(),
        Some(ClientRole::Player(Side::BLACK)) => "You play Black 黑".to_string(),
        Some(ClientRole::Player(_)) => "You play Green 綠".to_string(),
        Some(ClientRole::Spectator) => "Spectator — read-only".to_string(),
        None => "Awaiting seat assignment".to_string(),
    };
    let role_class = move || match role.get() {
        Some(ClientRole::Spectator) => "spectator-badge",
        _ => "",
    };

    let variant_label = move || match rules.get() {
        Some(r) => variant_slug(r.variant).to_string(),
        None => "—".to_string(),
    };

    let turn_label = move || match view_signal.get() {
        Some(v) => match v.current_color {
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
    let is_player = move || role.get().map(|r| r.is_player()).unwrap_or(false);
    let resign_disabled = move || !is_player() || game_over();
    let rematch_disabled = move || !is_player() || !game_over();

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

    let prefs = expect_context::<Prefs>();
    let fx_confetti = prefs.fx_confetti;
    let fx_check_banner = prefs.fx_check_banner;
    let show_check = move || {
        let Some(v) = view_signal.get() else { return false };
        fx_check_banner.get()
            && matches!(v.shape, BoardShape::Xiangqi9x10)
            && v.in_check
            && matches!(v.status, GameStatus::Ongoing)
    };

    view! {
        <aside class="sidebar">
            <h3 class="variant-label">{format!("Online — room {}", room)}</h3>
            <p class="muted">{variant_label}</p>
            <p class=role_class>{role_label}</p>
            <Show when=show_check>
                <div class="check-badge" role="status">"⚠ 將軍 / CHECK"</div>
            </Show>
            <p>{turn_label}</p>
            <p class="muted">{conn_label}</p>
            <div class="sidebar-actions">
                <button class="btn" on:click=resign disabled=resign_disabled>"Resign"</button>
                <button class="btn" on:click=rematch disabled=rematch_disabled>"Rematch"</button>
            </div>
            <div class="fx-toggles">
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || fx_confetti.get()
                        on:change=move |ev| fx_confetti.set(event_target_checked(&ev))
                    />
                    <span>"Victory effects (confetti + banner)"</span>
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || fx_check_banner.get()
                        on:change=move |ev| fx_check_banner.set(event_target_checked(&ev))
                    />
                    <span>"將軍 / CHECK warning"</span>
                </label>
            </div>
            <a class="back-link" href="/lobby">"← Back to lobby"</a>
        </aside>
    }
}
