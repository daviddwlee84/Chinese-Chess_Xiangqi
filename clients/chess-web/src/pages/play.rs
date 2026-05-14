use chess_ai::{AiAnalysis, AiOptions, Difficulty, Strategy};
use chess_core::board::BoardShape;
use chess_core::coord::Square;
use chess_core::moves::Move;
use chess_core::piece::Side;
use chess_core::rules::RuleSet;
use chess_core::state::GameStatus;
use chess_core::view::{PlayerView, VisibleCell};
use chess_net::protocol::{ChatLine, ClientMsg, ServerMsg};
use leptos::*;
use leptos_router::{use_params_map, use_query_map};
use std::rc::Rc;

use crate::components::board::{Board, ThreatOverlay};
use crate::components::captured::CapturedStrip;
use crate::components::chat_panel::ChatPanel;
use crate::components::debug_panel::DebugPanel;
use crate::components::end_overlay::EndOverlay;
use crate::components::ws_setup::WsSetup;
use crate::config::{
    append_query_pairs, resolve_ws_base, room_url, ws_query_pair, WsBase, WsBaseChoiceError,
    WS_QUERY_KEY,
};
use crate::prefs::{Prefs, ThreatMode};
use crate::routes::{app_href, WsBaseError};
use crate::state::{
    describe_rules, end_chain_move, find_move, hover_threat_squares,
    reconstruct_xiangqi_state_for_analysis, truncate_front, ClientRole,
};
use crate::transport::{self, ConnState, Session, Transport};

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
    let debug_enabled = query.with(|q| {
        matches!(q.get("debug").map(|s| s.as_str()), Some("1") | Some("true") | Some("on"))
    });
    let hints_requested = query.with(|q| {
        matches!(q.get("hints").map(|s| s.as_str()), Some("1") | Some("true") | Some("on"))
    });
    let raw_ws = query.with(|q| q.get(WS_QUERY_KEY).cloned());

    let room_now = room();
    let next_path = {
        let mut pairs = Vec::new();
        if let Some(pw) = password() {
            pairs.push(format!("password={}", urlencode(&pw)));
        }
        if watch_only() {
            pairs.push("role=spectator".to_string());
        }
        if hints_requested {
            // Forward hints flag through the WS-base setup wall so the
            // first-joiner request reaches the server even if we have
            // to detour through the WsSetup screen first.
            pairs.push("hints=1".to_string());
        }
        append_query_pairs(&format!("/play/{room_now}"), pairs)
    };
    let ws_base = match resolve_ws_base(raw_ws) {
        Ok(base) => base,
        Err(err) => {
            return view! {
                <WsSetup
                    title="Online room"
                    next_path=next_path
                    initial_error=ws_error_label(err).unwrap_or("")
                />
            }
            .into_view();
        }
    };

    view! {
        <PlayConnected
            ws_base=ws_base
            room=room_now
            password=password()
            watch_only=watch_only()
            debug_enabled=debug_enabled
            hints_requested=hints_requested
        />
    }
    .into_view()
}

#[component]
pub fn PlayConnected(
    ws_base: WsBase,
    room: String,
    password: Option<String>,
    watch_only: bool,
    debug_enabled: bool,
    /// `?hints=1` was set on the URL. The server may or may not honor
    /// it: if this client is the first joiner the room becomes
    /// `hints_allowed=true`; otherwise the existing room's flag wins
    /// and we may end up with `hints_allowed=false` — in which case
    /// the panel stays hidden + a toast warns the user.
    hints_requested: bool,
    /// Pre-constructed `Session`. Provided by `pages::lan` (Phase 5)
    /// so the host's in-process room or the joiner's WebRTC peer
    /// reuses the same play UI as chess-net mode. `None` (chess-net
    /// path) means we open a WebSocket via `transport::ws::connect`
    /// from the args above.
    #[prop(optional)]
    injected_session: Option<Session>,
    /// Override for the "← Back to lobby" link in the sidebar.
    /// `None` (chess-net path) defaults to `/lobby?ws=...`. LAN
    /// pages set this to `/lan/host` so the user lands back at the
    /// pairing entry point.
    #[prop(optional)]
    back_link_override: Option<String>,
) -> impl IntoView {
    // Open the WS once on mount UNLESS the caller injected a Session
    // (LAN host / joiner path). Spectator opt-in propagates into the
    // URL; hints_requested → `?hints=1` so the server can sanction.
    let session = injected_session.unwrap_or_else(|| {
        let url = room_url(&ws_base, &room, password.as_deref(), watch_only, hints_requested);
        transport::ws::connect(url)
    });
    let handle = session.handle.clone();
    let incoming = session.incoming.clone();
    let conn = session.state;

    let (rules, set_rules) = create_signal::<Option<RuleSet>>(None);
    let (role, set_role) = create_signal::<Option<ClientRole>>(None);
    let (view_signal, set_view) = create_signal::<Option<PlayerView>>(None);
    let (chat, set_chat) = create_signal::<Vec<ChatLine>>(Vec::new());
    let (toast, set_toast) = create_signal::<Option<String>>(None);
    // `Some(true|false)` once Hello/Spectating arrives; `None` while
    // waiting for the welcome.
    let (hints_allowed, set_hints_allowed) = create_signal::<Option<bool>>(None);

    // Phase 6: surface a peer-disconnect toast when the underlying
    // transport closes mid-game. For chess-net mode the existing
    // "Disconnected — refresh" board placeholder already covers it;
    // for LAN mode the host's session.state stays Open (the host's
    // local sink doesn't drop), so this watcher mainly catches the
    // joiner side when the host's DC closes.
    //
    // The host-side equivalent is in `host_room.rs::drop_peer`,
    // which pushes a synthetic `ServerMsg::Error` onto the host's
    // local sink — the existing match-arm above handles that path.
    {
        let prev_conn: std::rc::Rc<std::cell::Cell<ConnState>> =
            std::rc::Rc::new(std::cell::Cell::new(ConnState::Connecting));
        create_effect(move |_| {
            let now = conn.get();
            let was = prev_conn.replace(now);
            if matches!(was, ConnState::Open) && matches!(now, ConnState::Closed | ConnState::Error)
            {
                // Only surface as a toast — the existing fallback
                // board placeholder will still show "Disconnected"
                // for the chess-net mode wall.
                set_toast.set(Some("Peer disconnected — LAN game ended.".into()));
            }
        });
    }

    create_effect(move |_| {
        // Drain the queue: read all currently-pending ServerMsgs in
        // arrival order via `Incoming::drain`. The drain reads ALL
        // pending messages each tick — critical for the LAN host
        // path, where `HostRoom::new` synchronously pushes both
        // Hello + ChatHistory back-to-back, AND for any post-mount
        // push (e.g. host's own move's `Update` echo through the
        // local sink). The Incoming type guarantees no message gets
        // dropped (no `set(Vec::new())` race; pure VecDeque +
        // monotonic tick signal). See `transport::Incoming` doc.
        incoming.drain(|msg| match msg {
            ServerMsg::Hello { observer, rules: r, view: v, hints_allowed: h, .. } => {
                set_role.set(Some(ClientRole::Player(observer)));
                set_rules.set(Some(r));
                set_view.set(Some(v));
                set_hints_allowed.set(Some(h));
                set_toast.set(None);
                // If the user asked for hints and the room said no,
                // warn visibly. Don't block the game — they can
                // still play.
                if (debug_enabled || hints_requested) && !h {
                    set_toast.set(Some(
                        "Hints not enabled in this room — the panel is hidden for fairness.".into(),
                    ));
                }
            }
            ServerMsg::Spectating { rules: r, view: v, hints_allowed: h, .. } => {
                set_role.set(Some(ClientRole::Spectator));
                set_rules.set(Some(r));
                set_view.set(Some(v));
                set_hints_allowed.set(Some(h));
                set_toast.set(None);
                if (debug_enabled || hints_requested) && !h {
                    set_toast.set(Some(
                        "Hints not enabled in this room — the panel is hidden for fairness.".into(),
                    ));
                }
            }
            ServerMsg::Update { view: v } => {
                set_view.set(Some(v));
            }
            ServerMsg::ChatHistory { lines } => {
                set_chat.set(lines);
            }
            ServerMsg::Chat { line } => {
                set_chat.update(|buf| {
                    buf.push(line);
                    truncate_front(buf, CHAT_HISTORY_CAP);
                });
            }
            ServerMsg::Error { message } => {
                set_toast.set(Some(message));
            }
            ServerMsg::Rooms { .. } => {}
        });
    });

    let chat_handle = handle.clone();
    let on_chat_send: Callback<String> = Callback::new(move |text: String| {
        chat_handle.send(ClientMsg::Chat { text });
    });

    let handle_for_board = handle.clone();
    let handle_for_sidebar = handle;
    let room_label = room.clone();

    let prefs = expect_context::<Prefs>();
    let fx_confetti: Signal<bool> = prefs.fx_confetti.into();
    // The overlay needs a non-`Option` view signal once the welcome lands.
    // Wrap in a `Show` so we don't mount it pre-welcome (and so the watcher
    // inside doesn't see a fake "transition" on first connect).
    let view_for_overlay = Signal::derive(move || view_signal.get());
    let role_for_overlay: Signal<Option<ClientRole>> = role.into();

    // Net debug / hint overlay. Net mode adds a server-permission gate
    // on top of the URL flag: the room creator must have set
    // `?hints=1` so the server's `hints_allowed` is true. This closes
    // the previous client-only `?debug=1` cheat hole — the panel only
    // mounts when the room sanctioned it. Local mode (offline /
    // GitHub Pages standalone) skips this gate entirely; see
    // `pages/local.rs`.
    //
    // Two distinct UX modes share the same `<DebugPanel>` mount:
    //
    // - `?debug=1` (sticky / power-user): panel always visible once
    //   `hints_allowed` arrives. Shows the AI's analysis of the side-
    //   to-move's options after every PlayerView update.
    //
    // - `?hints=1` (on-demand / player-friendly): a "🧠 Show AI hint"
    //   toggle appears in the sidebar. Default hidden so the player
    //   actively asks for help. When opened, the same analysis fires.
    //
    // Both still require server permission (`hints_allowed = true`).
    let panel_requested = debug_enabled || hints_requested;
    let ai_analysis = create_rw_signal::<Option<AiAnalysis>>(None);
    let highlighted_pv = create_rw_signal::<Vec<Move>>(Vec::new());
    let hints_open = create_rw_signal::<bool>(false);
    let panel_visible = Signal::derive(move || {
        if !hints_allowed.get().unwrap_or(false) {
            return false;
        }
        // ?debug=1 → sticky; ?hints=1 → user-controlled toggle.
        debug_enabled || (hints_requested && hints_open.get())
    });
    // Sidebar toggle button visibility: only when hints (not debug) is
    // the controlling flag AND the server sanctioned it. Debug-mode
    // users get a sticky always-on panel instead — no extra button.
    let show_hint_button = Signal::derive(move || {
        hints_requested && !debug_enabled && hints_allowed.get().unwrap_or(false)
    });
    // Dynamic panel header — match local mode's labeling so users
    // recognize Hint vs Debug at a glance.
    //
    // Net mode has only ONE analysis pump (always for the side-to-move),
    // unlike local vs-AI which has separate AI-POV-cache + hint pumps.
    // So the "🔍 + 🧠" combined label from the previous attempt was
    // misleading — both flags surface the same data here. Pick the
    // label by user intent (hints = player asking for help, debug =
    // developer / power-user).
    let panel_title = Signal::derive(move || {
        if hints_requested {
            "🧠 AI Hint".to_string()
        } else {
            "🔍 AI Debug".to_string()
        }
    });
    let panel_subtitle = Signal::derive(move || {
        let stm = view_signal.with(|v| v.as_ref().map(|view| view.side_to_move));
        match stm {
            Some(Side::RED) => "Red 紅 to move".to_string(),
            Some(Side::BLACK) => "Black 黑 to move".to_string(),
            Some(_) => "Green 綠 to move".to_string(),
            None => String::new(),
        }
    });
    if panel_requested {
        create_effect(move |_| {
            // Only run analysis when the panel is going to be shown —
            // server-denied requests AND closed-toggle requests skip
            // the search work entirely.
            if !panel_visible.get() {
                return;
            }
            let Some(v) = view_signal.get() else {
                ai_analysis.set(None);
                return;
            };
            // Reconstruct best-effort GameState; bail (None) for non-
            // xiangqi or terminated games.
            let Some(state) = reconstruct_xiangqi_state_for_analysis(&v) else {
                ai_analysis.set(None);
                return;
            };
            // Analyze with the side-to-move's POV. Use a stable seed
            // tied to the position (no UI-driven RNG) so subsequent
            // hovers don't redo work — analyze() is deterministic
            // given (state, opts).
            let opts = AiOptions {
                difficulty: Difficulty::Hard,
                max_depth: None,
                seed: Some(0xDEBA9_u64),
                strategy: Strategy::default(),
                randomness: Some(chess_ai::Randomness::STRICT),
                node_budget: None,
            };
            // Yield a frame so the board re-paints first; keeps the UI
            // snappy even if v5 Hard takes ~300 ms in WASM.
            wasm_bindgen_futures::spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(40).await;
                // Time the search so the debug panel can show the
                // wall-clock cost — chess-ai itself can't measure on
                // wasm (Instant::now panics) so we patch the result
                // in here. See `crate::time::perf_now_ms` doc.
                let start = crate::time::perf_now_ms();
                let mut analysis = chess_ai::analyze(&state, &opts);
                let elapsed = crate::time::perf_now_ms().saturating_sub(start);
                if let Some(ref mut a) = analysis {
                    a.elapsed_ms = Some(elapsed);
                }
                ai_analysis.set(analysis);
            });
        });
    }

    let highlighted_pv_signal: Signal<Vec<Move>> = highlighted_pv.into();
    let analysis_signal: Signal<Option<AiAnalysis>> = ai_analysis.into();
    let on_debug_hover: Callback<Vec<Move>> =
        Callback::new(move |pv: Vec<Move>| highlighted_pv.set(pv));

    view! {
        <section class="game-page">
            <div class="board-pane">
                <Show when=move || hints_allowed.get().unwrap_or(false)>
                    <div class="hints-banner">
                        "🧠 AI hints enabled in this room — visible to both players + spectators."
                    </div>
                </Show>
                <Show
                    when=move || view_signal.get().is_some() && role.get().is_some()
                    fallback=move || view! { <ConnPlaceholder room=room.clone() conn=conn/> }
                >
                    <BoardWrapper
                        view_signal=view_signal
                        role=role
                        handle=handle_for_board.clone()
                        highlighted_pv=highlighted_pv_signal
                    />
                    <EndOverlay
                        view=Signal::derive(move || view_for_overlay.get().expect("guarded by Show"))
                        role=role_for_overlay
                        enabled=fx_confetti
                    />
                    <CapturedStrip
                        view=Signal::derive(move || view_for_overlay.get().expect("guarded by Show"))
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
                    ws_base=ws_base.clone()
                    show_hint_button=show_hint_button
                    hint_open=hints_open
                    back_link_override=back_link_override
                />
                <ChatPanel
                    role=role.into()
                    log=chat.into()
                    on_send=on_chat_send
                />
            </div>
            <Show when=move || panel_visible.get()>
                {move || {
                    // Pre-build a fresh xiangqi board for ICCS encoding.
                    // Net mode doesn't have a fixed Board reference; the
                    // debug panel only uses it for `iccs::encode_move`
                    // which only needs board dimensions, not positions.
                    let board = chess_core::board::Board::new(BoardShape::Xiangqi9x10);
                    view! {
                        <DebugPanel
                            analysis=analysis_signal
                            board=board
                            on_hover=on_debug_hover
                            title=panel_title
                            subtitle=panel_subtitle
                        />
                    }
                }}
            </Show>
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
    handle: Rc<dyn Transport>,
    /// Optional debug PV chain to highlight on the board (no-op in
    /// non-debug mode). Always-empty signal in normal play.
    #[prop(into)]
    highlighted_pv: Signal<Vec<Move>>,
) -> impl IntoView {
    let view = Signal::derive(move || view_signal.get().expect("guarded by Show"));
    let resolved_role = role.get_untracked().expect("guarded by Show");
    let obs = resolved_role.observer();
    let shape = view.with_untracked(|v| v.shape);

    let selected = create_rw_signal::<Option<Square>>(None);
    // Spectators see-only — no click handlers on the board for them.
    let is_spectator = resolved_role.is_spectator();

    // Clear stale selection when the seat-to-move flips (mirrors the
    // local page) — covers the chain-mode tail and any other path that
    // forgets to reset `selected` on its own.
    let side_signal = Signal::derive(move || view.get().side_to_move);
    create_effect(move |prev: Option<Side>| {
        let cur = side_signal.get();
        if prev.is_some_and(|p| p != cur) {
            selected.set(None);
        }
        cur
    });

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
        // Banqi pre-first-flip: either seat may flip. The server is
        // still authoritative — relaxing the optimistic UI block lets
        // the click reach `ClientMsg::Move`, where the server's
        // relaxed `process_move` guard attributes the flip to us.
        if v.observer != v.side_to_move && !v.banqi_awaiting_first_flip {
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

    // Threat-highlight overlay (Display setting). Mirrors local.rs's
    // derivation — see that file for the rationale and the hover-vs-mode
    // semantics. We compute even for spectators (they may want to see
    // who's hung what); they just can't click anything.
    let prefs_threat = expect_context::<Prefs>();
    let fx_threat_mode = prefs_threat.fx_threat_mode;
    let fx_threat_hover = prefs_threat.fx_threat_hover;
    let fx_last_move = prefs_threat.fx_last_move;
    let threat_overlay: Signal<ThreatOverlay> = Signal::derive(move || {
        let v = view.get();
        let mode = fx_threat_mode.get();
        let (static_squares, mate_squares): (Vec<Square>, Vec<Square>) = match mode {
            ThreatMode::Off => (Vec::new(), Vec::new()),
            ThreatMode::Attacked => (v.threats.attacked.clone(), Vec::new()),
            ThreatMode::NetLoss => (v.threats.net_loss.clone(), Vec::new()),
            ThreatMode::MateThreat => (Vec::new(), v.threats.mate_threats.clone()),
        };
        let hover_squares = if fx_threat_hover.get() && mode != ThreatMode::Off {
            if let Some(sq) = effective_selected.get() {
                hover_threat_squares(&v, obs, sq)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        ThreatOverlay { static_squares, mate_squares, hover_squares }
    });

    // "Highlight latest move" — see local.rs for the same derivation.
    // Spectators benefit equally from this: they often arrive
    // mid-game and need to see what was just played to catch up.
    let last_move_signal: Signal<Option<chess_core::moves::Move>> = Signal::derive(move || {
        if !fx_last_move.get() {
            return None;
        }
        view.with(|v| v.last_move.clone())
    });

    view! {
        <Board
            shape=shape
            observer=obs
            view=view
            selected=effective_selected
            on_click=on_click
            highlighted_pv=highlighted_pv
            threats=threat_overlay
            last_move=last_move_signal
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
    handle: Rc<dyn Transport>,
    ws_base: WsBase,
    /// Reactive predicate: when true, mounts the "🧠 Show / Hide AI
    /// hint" toggle button. Net-mode-specific because the gating depends
    /// on the server's `hints_allowed`, which arrives async via Hello.
    /// Pass `Signal::derive(|| false)` to disable entirely.
    #[prop(into)]
    show_hint_button: Signal<bool>,
    /// Toggle state for the hint button. Owner is the page so it can
    /// gate the `<DebugPanel>` mount on the same value. Always
    /// allocated; only consumed when `show_hint_button.get() == true`.
    hint_open: RwSignal<bool>,
    /// Override for the "← Back" link. `None` defaults to back-to-lobby
    /// (chess-net mode). LAN host/joiner pages set this to `/lan/host`
    /// so the link makes sense in the LAN context.
    back_link_override: Option<String>,
) -> impl IntoView {
    // Pre-first-flip banqi: neither seat has a piece-colour yet, so
    // "You play Red 紅" would be misleading. Substitute the awaiting
    // message until the engine locks the assignment.
    let awaiting_first_flip =
        move || view_signal.get().map(|v| v.banqi_awaiting_first_flip).unwrap_or(false);
    let role_label = move || {
        if awaiting_first_flip() {
            return "未翻牌 — 任一方皆可先翻".to_string();
        }
        match role.get() {
            Some(ClientRole::Player(Side::RED)) => "You play Red 紅".to_string(),
            Some(ClientRole::Player(Side::BLACK)) => "You play Black 黑".to_string(),
            Some(ClientRole::Player(_)) => "You play Green 綠".to_string(),
            Some(ClientRole::Spectator) => "Spectator — read-only".to_string(),
            None => "Awaiting seat assignment".to_string(),
        }
    };
    let role_class = move || match role.get() {
        Some(ClientRole::Spectator) => "spectator-badge",
        _ => "",
    };

    // Full one-line rules summary — variant + xiangqi strict/casual or
    // banqi house flags + seed. Shows up under the room title so the
    // joiner / spectator can see what the host configured (LAN) or what
    // the server was launched with (chess-net) before the first move.
    let variant_label = move || match rules.get() {
        Some(r) => describe_rules(&r),
        None => "—".to_string(),
    };

    let turn_label = move || match view_signal.get() {
        Some(v) if v.banqi_awaiting_first_flip => "Awaiting first flip".to_string(),
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
            <Show when=move || show_hint_button.get()>
                {
                    let label = move || if hint_open.get() {
                        "🧠 Hide AI hint"
                    } else {
                        "🧠 Show AI hint"
                    };
                    let cls = move || if hint_open.get() {
                        "btn btn-hint btn-hint--on"
                    } else {
                        "btn btn-hint"
                    };
                    view! {
                        <button
                            class=cls
                            title="Toggle the AI hint panel — runs the bot from the side-to-move's perspective. Visible to the room (server-sanctioned)."
                            on:click=move |_| hint_open.update(|b| *b = !*b)
                        >
                            {label}
                        </button>
                    }
                }
            </Show>
            {
                let (href, label) = match back_link_override {
                    Some(h) => (h, "← Back"),
                    None => (back_to_lobby_href(&ws_base), "← Back to lobby"),
                };
                view! {
                    <a class="back-link" href=href rel="external">{label}</a>
                }
            }
        </aside>
    }
}

fn back_to_lobby_href(ws_base: &WsBase) -> String {
    app_href(&append_query_pairs("/lobby", ws_query_pair(ws_base)))
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

fn urlencode(s: &str) -> String {
    js_sys::encode_uri_component(s).as_string().unwrap_or_default()
}
