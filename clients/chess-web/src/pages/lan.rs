//! `/lan/host` and `/lan/join` routes — LAN multiplayer over WebRTC.
//!
//! Phase 5 of `backlog/webrtc-lan-pairing.md`. Pairs two browsers
//! on the same WiFi via the Phase 3 `transport::webrtc` plumbing
//! and the Phase 4 `host_room::HostRoom` authority, then drops the
//! resulting `Session` into the existing `pages/play::PlayConnected`
//! so the actual game UI is unchanged from chess-net mode.
//!
//! No QR / camera scanning yet — Phase 5 v1 uses textareas + the
//! browser clipboard for the offer/answer exchange. QR is
//! deferred (Phase 5.5 if requested).

// === module skeleton — section bodies filled in via edits below ===

// SECTION: imports
use std::cell::RefCell;
use std::rc::Rc;

use chess_core::rules::{
    HouseRules, RuleSet, Variant, PRESET_AGGRESSIVE, PRESET_PURIST, PRESET_TAIWAN,
};
use leptos::*;
use leptos_router::use_query_map;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlTextAreaElement;

use crate::beforeunload::use_beforeunload_guard;
use crate::camera::has_camera;
use crate::components::pwa::{LanIndicator, PwaState};
use crate::components::qr::QrCodeView;
use crate::components::qr_scanner::QrScanner;
use crate::config::WsBase;
use crate::host_room::HostRoom;
use crate::net_diag::{classify, parse_candidate_addrs, NetDiag};
use crate::pages::play::PlayConnected;
use crate::routes::{
    app_href, build_rule_set, parse_local_rules, parse_variant_slug, LocalRulesParams,
};
use crate::state::describe_rules;
use crate::transport::webrtc::{
    connect_as_host, connect_as_joiner, wait_for_dc_open, AnswerBlob, HostHandshake, IceDiag,
    IceMode, JoinerHandshake, OfferBlob, WebRtcConfig,
};
use crate::transport::{ConnState, Session};
use web_sys::{RtcIceConnectionState, RtcIceGatheringState, RtcPeerConnectionState};

// SECTION: ICE diag rendering helpers

fn ice_label(s: RtcIceConnectionState) -> &'static str {
    match s {
        RtcIceConnectionState::New => "new",
        RtcIceConnectionState::Checking => "checking",
        RtcIceConnectionState::Connected => "connected",
        RtcIceConnectionState::Completed => "completed",
        RtcIceConnectionState::Failed => "failed",
        RtcIceConnectionState::Disconnected => "disconnected",
        RtcIceConnectionState::Closed => "closed",
        _ => "?",
    }
}

fn conn_label(s: RtcPeerConnectionState) -> &'static str {
    match s {
        RtcPeerConnectionState::New => "new",
        RtcPeerConnectionState::Connecting => "connecting",
        RtcPeerConnectionState::Connected => "connected",
        RtcPeerConnectionState::Disconnected => "disconnected",
        RtcPeerConnectionState::Failed => "failed",
        RtcPeerConnectionState::Closed => "closed",
        _ => "?",
    }
}

fn gather_label(s: RtcIceGatheringState) -> &'static str {
    match s {
        RtcIceGatheringState::New => "new",
        RtcIceGatheringState::Gathering => "gathering",
        RtcIceGatheringState::Complete => "complete",
        _ => "?",
    }
}

/// Format an [`IceDiag`] snapshot as a single line for the inline badge.
fn diag_line(d: &IceDiag) -> String {
    format!(
        "ICE: {} · connection: {} · gathering: {} · candidates: {}",
        ice_label(d.ice),
        conn_label(d.conn),
        gather_label(d.gather),
        d.candidates,
    )
}

/// Hint string appended to the 10-s DC-open timeout error, varied by
/// what we can infer from the host's own SDP candidates. Surfacing
/// "your VPN is hijacking the LAN" instead of the generic mDNS hint
/// when the host candidate IP betrays a VPN tunnel saves the user
/// from blaming the wrong layer.
fn timeout_hint_for(sdp: &str) -> &'static str {
    match classify(&parse_candidate_addrs(sdp)) {
        NetDiag::VpnTunnel => {
            "VPN tunnel detected (host candidate is in 198.18.0.0/15). \
             DISABLE the VPN on both devices and retry — VPNs route LAN \
             traffic through the tunnel and replace your real LAN IP \
             with a fake address. If the VPN must stay on, configure \
             split-tunnel to bypass 192.168.0.0/16, 10.0.0.0/8, \
             172.16.0.0/12, 172.20.10.0/28 (iOS hotspot), and \
             224.0.0.0/4 (multicast / mDNS)."
        }
        NetDiag::Cgnat => {
            "CGNAT detected (host candidate is in 100.64.0.0/10). \
             Your ISP / carrier is using Carrier-Grade NAT, which often \
             blocks direct P2P even with STUN. Try an iPhone/Android \
             personal hotspot instead."
        }
        NetDiag::Plain => {
            "Common fixes: switch both devices to an iPhone/Android \
             personal hotspot (this network's WebRTC mDNS resolution \
             may be failing), or enable \"Use STUN\"."
        }
    }
}
// SECTION: LanHostPage component

#[component]
pub fn LanHostPage() -> impl IntoView {
    // ── Page state ────────────────────────────────────────────
    // We progress through:
    //   Idle → Generating → AwaitingAnswer → AcceptingAnswer →
    //   WaitingForChannel → Playing
    // Each transition mutates the corresponding signals; the view
    // renders accordingly.
    let (status, set_status) = create_signal::<HostStatus>(HostStatus::Idle);
    let (use_stun, set_use_stun) = create_signal::<bool>(false);
    let (offer_blob, set_offer_blob) = create_signal::<String>(String::new());
    let (answer_input, set_answer_input) = create_signal::<String>(String::new());
    let (error_msg, set_error_msg) = create_signal::<Option<String>>(None);

    // Phase 6: tab-close protection while a game is in progress.
    // Closing the host tab kills the LAN room with no recovery, so
    // ask for confirmation. Modern browsers ignore the message text
    // and show a generic prompt — we still pass one for older
    // browser compat.
    use_beforeunload_guard(
        Signal::derive(move || matches!(status.get(), HostStatus::Playing)),
        "Closing this tab will end the LAN room. Continue?",
    );

    // Phase 6: drive the corner OfflineIndicator's LAN state.
    // Effect derives `LanIndicator` from `status` so every status
    // transition automatically updates the indicator without
    // sprinkling updates at each `set_status.set(...)` call site.
    // On unmount, reset to Idle so navigating to a non-LAN page
    // shows the plain online/offline badge again.
    if let Some(pwa) = use_context::<PwaState>() {
        create_effect(move |_| {
            pwa.lan.set(match status.get() {
                HostStatus::Idle
                | HostStatus::Generating
                | HostStatus::AwaitingAnswer
                | HostStatus::AcceptingAnswer => LanIndicator::HostWaiting,
                HostStatus::Playing => LanIndicator::Connected,
            });
        });
        on_cleanup(move || pwa.lan.set(LanIndicator::Idle));
    }

    // ── Variant + rules form state ─────────────────────────────
    // Mirrors picker.rs::BanqiCard: one bool signal per HouseRules
    // bitflag (rather than one signal holding the whole bitset) so
    // each checkbox can bind directly to `prop:checked` without a
    // closure dance. `apply_preset` flips all five at once.
    //
    // Initial values come from the URL query string (`?variant=banqi
    // &house=chain,rush&seed=42`) via the same `parse_local_rules`
    // parser the picker uses. Lets a future picker "Host on LAN"
    // button deep-link a fully-configured /lan/host without a fresh
    // parser; today, hand-typed URLs are the only consumer.
    let query = use_query_map();
    let initial_variant = query.with_untracked(|q| {
        q.get("variant").and_then(|s| parse_variant_slug(s)).unwrap_or(Variant::Xiangqi)
    });
    // Three-kingdom is 3-player; not a valid LAN choice — clamp to xiangqi.
    let initial_variant = match initial_variant {
        Variant::ThreeKingdomBanqi => Variant::Xiangqi,
        v => v,
    };
    let initial_params: LocalRulesParams =
        query.with_untracked(|q| parse_local_rules(|k| q.get(k).cloned()));

    let variant = create_rw_signal(initial_variant);
    let strict = create_rw_signal(initial_params.strict);
    let chain = create_rw_signal(initial_params.house.contains(HouseRules::CHAIN_CAPTURE));
    let dark = create_rw_signal(initial_params.house.contains(HouseRules::DARK_CAPTURE));
    let dark_trade =
        create_rw_signal(initial_params.house.contains(HouseRules::DARK_CAPTURE_TRADE));
    let rush = create_rw_signal(initial_params.house.contains(HouseRules::CHARIOT_RUSH));
    let horse = create_rw_signal(initial_params.house.contains(HouseRules::HORSE_DIAGONAL));
    let seed_text =
        create_rw_signal(initial_params.seed.map(|s| s.to_string()).unwrap_or_default());

    // Snapshot of the RuleSet that was passed to HostRoom::new — set
    // once on successful `on_accept`. Drives the post-Idle "Playing:
    // …" status line that replaces the form once pairing starts.
    let (chosen_rules, set_chosen_rules) = create_signal::<Option<RuleSet>>(None);

    let apply_preset = move |preset: HouseRules| {
        chain.set(preset.contains(HouseRules::CHAIN_CAPTURE));
        dark.set(preset.contains(HouseRules::DARK_CAPTURE));
        dark_trade.set(preset.contains(HouseRules::DARK_CAPTURE_TRADE));
        rush.set(preset.contains(HouseRules::CHARIOT_RUSH));
        horse.set(preset.contains(HouseRules::HORSE_DIAGONAL));
    };

    let build_params = move || -> LocalRulesParams {
        let mut house = HouseRules::empty();
        if chain.get_untracked() {
            house.insert(HouseRules::CHAIN_CAPTURE);
        }
        if dark.get_untracked() {
            house.insert(HouseRules::DARK_CAPTURE);
        }
        if dark_trade.get_untracked() {
            house.insert(HouseRules::DARK_CAPTURE_TRADE);
        }
        if rush.get_untracked() {
            house.insert(HouseRules::CHARIOT_RUSH);
        }
        if horse.get_untracked() {
            house.insert(HouseRules::HORSE_DIAGONAL);
        }
        let seed = seed_text.with_untracked(|s| s.trim().parse::<u64>().ok());
        LocalRulesParams { strict: strict.get_untracked(), house, seed, ..Default::default() }
    };

    // QR scanner modal state. `cam_available` is set asynchronously
    // on mount; default false to keep the Scan-camera button hidden
    // until detection completes (no flash-of-button-then-disappear
    // for cameraless devices).
    let (scanner_open, set_scanner_open) = create_signal::<bool>(false);
    let (cam_available, set_cam_available) = create_signal::<bool>(false);
    spawn_local(async move {
        if has_camera().await {
            set_cam_available.set(true);
        }
    });

    // Keep the in-flight HostHandshake alive in the page; closures
    // and accept_answer need a stable reference.
    let handshake: Rc<RefCell<Option<HostHandshake>>> = Rc::new(RefCell::new(None));
    // The HostRoom + its Session aren't constructed until the
    // DataChannel actually opens; on `state == Open` we instantiate
    // both and flip status to `Playing`.
    let host_room: Rc<RefCell<Option<Rc<HostRoom>>>> = Rc::new(RefCell::new(None));
    let (play_session, set_play_session) = create_signal::<Option<Session>>(None);
    // Holder for the handshake's live ICE diag signal so the inline
    // status badge can render it reactively. None before Open room
    // succeeds; Some thereafter.
    let (ice_diag_holder, set_ice_diag_holder) = create_signal::<Option<ReadSignal<IceDiag>>>(None);

    // Re-snapshot the offer SDP whenever the diag signal fires (every
    // `oniceicecandidate` / connection / gathering state change). Catches
    // late-arriving srflx candidates that wouldn't be in the half-
    // gathered SDP captured at connect_as_host return time. See
    // HostHandshake::current_offer for the rationale.
    {
        let handshake = handshake.clone();
        create_effect(move |_| {
            if let Some(sig) = ice_diag_holder.get() {
                let _ = sig.get(); // subscribe to inner changes
                if let Some(hh) = handshake.borrow().as_ref() {
                    set_offer_blob.set(hh.current_offer().0);
                }
            }
        });
    }

    // ── Open room (generate offer) ────────────────────────────
    let handshake_for_open = handshake.clone();
    let on_open: Callback<()> = Callback::new(move |_: ()| {
        if !matches!(status.get_untracked(), HostStatus::Idle) {
            return;
        }
        // Commit the form selection to `chosen_rules` BEFORE status
        // leaves Idle (which hides the form). Without this, the form
        // disappears immediately but the "Playing: …" summary line
        // doesn't appear until on_accept fires — leaving a window
        // with no visible record of the chosen rules. The actual
        // `HostRoom::new(…)` call still happens inside `on_accept`
        // and reads `chosen_rules.get_untracked()` to avoid drift.
        let rules = build_rule_set(variant.get_untracked(), &build_params());
        set_chosen_rules.set(Some(rules));
        set_error_msg.set(None);
        set_status.set(HostStatus::Generating);
        let cfg = WebRtcConfig {
            ice_mode: if use_stun.get_untracked() { IceMode::WithStun } else { IceMode::LanOnly },
        };
        let handshake_slot = handshake_for_open.clone();
        spawn_local(async move {
            match connect_as_host(cfg).await {
                Ok(hh) => {
                    set_offer_blob.set(hh.offer.0.clone());
                    set_ice_diag_holder.set(Some(hh.ice_diag));
                    *handshake_slot.borrow_mut() = Some(hh);
                    set_status.set(HostStatus::AwaitingAnswer);
                }
                Err(e) => {
                    set_error_msg.set(Some(format!("connect_as_host failed: {e:?}")));
                    set_status.set(HostStatus::Idle);
                }
            }
        });
    });

    // ── Accept answer ──────────────────────────────────────────
    let handshake_for_accept = handshake.clone();
    let host_room_slot = host_room.clone();
    let on_accept: Callback<()> = Callback::new(move |_: ()| {
        if !matches!(status.get_untracked(), HostStatus::AwaitingAnswer) {
            return;
        }
        let blob = answer_input.get_untracked();
        if blob.trim().is_empty() {
            set_error_msg.set(Some("paste the joiner's answer SDP first".into()));
            return;
        }
        // Rules were committed in `on_open` (before the form hid).
        // Recover them here for the actual room construction; this
        // can't be None unless the open flow short-circuited, in
        // which case we're not in AwaitingAnswer either.
        let rules = match chosen_rules.get_untracked() {
            Some(r) => r,
            None => {
                set_error_msg.set(Some("rules not configured — reopen the page".into()));
                set_status.set(HostStatus::Idle);
                return;
            }
        };
        set_error_msg.set(None);
        set_status.set(HostStatus::AcceptingAnswer);
        let handshake_slot = handshake_for_accept.clone();
        let host_room_slot = host_room_slot.clone();
        spawn_local(async move {
            let hh = match handshake_slot.borrow_mut().take() {
                Some(hh) => hh,
                None => {
                    set_error_msg.set(Some("no handshake in flight".into()));
                    set_status.set(HostStatus::Idle);
                    return;
                }
            };
            if let Err(e) = hh.accept_answer(AnswerBlob(blob)).await {
                set_error_msg.set(Some(format!("accept_answer failed: {e:?}")));
                set_status.set(HostStatus::Idle);
                return;
            }
            // CRITICAL: `accept_answer` resolves as soon as
            // `setRemoteDescription` returns — BEFORE the SCTP
            // handshake completes. If we call `attach_remote_player_dc`
            // immediately, the `dc.send_with_str(...)` for Hello +
            // ChatHistory silently fails (DC ready_state is still
            // "connecting"). Wait for actual DC open first.
            let dc = match hh.dc.borrow().clone() {
                Some(d) => d,
                None => {
                    set_error_msg.set(Some("DataChannel slot is empty".into()));
                    set_status.set(HostStatus::Idle);
                    return;
                }
            };
            let ice_diag = hh.ice_diag;
            if !wait_for_dc_open(&dc, 10_000).await {
                let d = ice_diag.get_untracked();
                let sdp = hh.pc.local_description().map(|desc| desc.sdp()).unwrap_or_default();
                set_error_msg.set(Some(format!(
                    "DataChannel did not open within 10 s — {}. {}",
                    diag_line(&d),
                    timeout_hint_for(&sdp),
                )));
                set_status.set(HostStatus::Idle);
                return;
            }
            let (room, session) = HostRoom::new(rules, None, /* hints */ false);
            if let Err(e) = room.attach_remote_player_dc(dc) {
                set_error_msg.set(Some(format!("attach joiner failed: {e:?}")));
                set_status.set(HostStatus::Idle);
                return;
            }
            *host_room_slot.borrow_mut() = Some(room);
            set_play_session.set(Some(session));
            set_status.set(HostStatus::Playing);
        });
    });

    // ── Reset (full page reload) ──────────────────────────────
    let on_reset: Callback<()> = Callback::new(move |_: ()| {
        if let Some(win) = web_sys::window() {
            let _ = win.location().reload();
        }
    });

    view! {
        <div class="lan-page" style="max-width:720px;margin:1.5rem auto;padding:1rem">
            <Show
                when=move || matches!(status.get(), HostStatus::Playing)
                fallback=move || view! {
                    <a href=app_href("/") rel="external" class="back-link">"← Back to picker"</a>
                    <h1>"LAN host (WebRTC)"</h1>
                    <Show when=move || matches!(status.get(), HostStatus::Idle)>
                        <fieldset class="card-fieldset" style="margin-bottom:0.5rem">
                            <legend>"Game"</legend>
                            <label class="radio-row">
                                <span style="margin-right:0.5rem;min-width:5rem">"Variant"</span>
                                <select
                                    style="flex:1"
                                    on:change=move |ev| {
                                        let s = event_target_value(&ev);
                                        if let Some(v) = parse_variant_slug(&s) {
                                            variant.set(v);
                                        }
                                    }
                                >
                                    <option value="xiangqi"
                                        prop:selected=move || variant.get() == Variant::Xiangqi>
                                        "Xiangqi 象棋"
                                    </option>
                                    <option value="banqi"
                                        prop:selected=move || variant.get() == Variant::Banqi>
                                        "Banqi 暗棋"
                                    </option>
                                </select>
                            </label>
                            <p class="hint">
                                "Three-Kingdom 三國暗棋 is 3-player and unsupported on the \
                                 2-peer LAN channel — pick another variant, or use online lobby."
                            </p>
                        </fieldset>

                        <Show when=move || variant.get() == Variant::Xiangqi>
                            <fieldset class="card-fieldset" style="margin-bottom:0.5rem">
                                <legend>"Xiangqi rules"</legend>
                                <label class="check-row">
                                    <input type="checkbox"
                                        prop:checked=move || strict.get()
                                        on:change=move |ev| strict.set(event_target_checked(&ev))/>
                                    <span>
                                        "Strict — leaving your general capturable is illegal \
                                         (standard tournament rule). OFF = casual; the game \
                                         only ends when the general is actually captured."
                                    </span>
                                </label>
                            </fieldset>
                        </Show>

                        <Show when=move || variant.get() == Variant::Banqi>
                            <fieldset class="card-fieldset" style="margin-bottom:0.5rem">
                                <legend>"Banqi preset"</legend>
                                <div class="preset-row">
                                    <button class="btn btn-ghost" type="button"
                                        on:click=move |_| apply_preset(PRESET_PURIST)>"Purist"</button>
                                    <button class="btn btn-ghost" type="button"
                                        on:click=move |_| apply_preset(PRESET_TAIWAN)>"Taiwan"</button>
                                    <button class="btn btn-ghost" type="button"
                                        on:click=move |_| apply_preset(PRESET_AGGRESSIVE)>"Aggressive"</button>
                                </div>
                            </fieldset>
                            <fieldset class="card-fieldset" style="margin-bottom:0.5rem">
                                <legend>"Banqi house rules"</legend>
                                <label class="check-row">
                                    <input type="checkbox"
                                        prop:checked=move || chain.get()
                                        on:change=move |ev| chain.set(event_target_checked(&ev))/>
                                    <span>"連吃 — chain captures along a line"</span>
                                </label>
                                <label class="check-row">
                                    <input type="checkbox"
                                        prop:checked=move || dark.get()
                                        on:change=move |ev| dark.set(event_target_checked(&ev))/>
                                    <span>"暗吃 — atomic reveal+capture; on rank-fail your piece stays put (probe)"</span>
                                </label>
                                <label class="check-row">
                                    <input type="checkbox"
                                        prop:checked=move || dark_trade.get()
                                        on:change=move |ev| dark_trade.set(event_target_checked(&ev))/>
                                    <span>"暗吃·搏命 — on rank-fail your attacker dies (implies 暗吃)"</span>
                                </label>
                                <label class="check-row">
                                    <input type="checkbox"
                                        prop:checked=move || rush.get()
                                        on:change=move |ev| rush.set(event_target_checked(&ev))/>
                                    <span>"車衝 — chariot rays the full board; with a gap, captures any piece"</span>
                                </label>
                                <label class="check-row">
                                    <input type="checkbox"
                                        prop:checked=move || horse.get()
                                        on:change=move |ev| horse.set(event_target_checked(&ev))/>
                                    <span>"馬斜 — horse adds diagonal one-step moves; diagonal captures any piece"</span>
                                </label>
                                <p class="hint">
                                    "炮快移 is accepted by the engine but not yet wired into move-gen "
                                    "(see "<code>"TODO.md"</code>")."
                                </p>
                            </fieldset>
                            <fieldset class="card-fieldset" style="margin-bottom:0.5rem">
                                <legend>"Seed (optional)"</legend>
                                <input
                                    type="text"
                                    inputmode="numeric"
                                    placeholder="leave blank for random"
                                    class="text-input"
                                    prop:value=move || seed_text.get()
                                    on:input=move |ev| seed_text.set(event_target_value(&ev))
                                />
                                <p class="hint">
                                    "Same seed = same shuffle on both devices. Use it for puzzle \
                                     replays or to make the layout reproducible."
                                </p>
                            </fieldset>
                        </Show>
                    </Show>

                    <Show when=move || chosen_rules.with(|r| r.is_some())>
                        <p style="margin:0.5rem 0;font-size:14px">
                            "Playing: "
                            <b>{move || chosen_rules.get()
                                .as_ref()
                                .map(describe_rules)
                                .unwrap_or_default()}</b>
                        </p>
                    </Show>
                    <p class="muted">
                        "iOS hint: do not switch apps after tapping Open room. iOS Safari pauses \
                         WebRTC when the page is backgrounded. If you must AirDrop the offer, \
                         keep this Safari tab in the foreground (split-view works)."
                    </p>
                    <p class="muted">
                        "VPN hint: disable any VPN (Cloudflare WARP, NordVPN, etc.) on BOTH \
                         devices before pairing. VPNs replace the real LAN IP with a tunnel \
                         address (often 198.18.x.x) and route LAN traffic through the tunnel, \
                         which breaks WebRTC's direct device discovery. If the VPN must stay on, \
                         configure split-tunnel to bypass: 192.168.0.0/16 + 10.0.0.0/8 + \
                         172.16.0.0/12 (private LANs), 172.20.10.0/28 (iOS hotspot), and \
                         224.0.0.0/4 (multicast / mDNS)."
                    </p>
                    <p class="muted">
                        "STUN hint: enabling \"Use STUN\" asks public servers (Miwifi / Tencent / \
                         Cloudflare / Google) to tell the browser its own public IP and adds it \
                         as an extra \"srflx\" candidate. Turn it ON when: (a) same LAN but \
                         mDNS resolution is broken (some routers), or (b) the two devices are on \
                         different networks. Leave it OFF on a healthy same-LAN setup — mDNS \
                         works directly and STUN just adds up to 5 s of gather delay. STUN can't \
                         rescue symmetric NAT / CGNAT (mobile carriers, hotel WiFi); those would \
                         need a TURN relay (not shipped — fall back to running chess-net and \
                         using /lobby instead)."
                    </p>
                    <p>
                        <button
                            on:click=move |_| on_open.call(())
                            disabled=move || !matches!(status.get(), HostStatus::Idle)
                        >
                            "1. Open room"
                        </button>
                        <button on:click=move |_| on_reset.call(()) style="margin-left:0.5rem">
                            "Reset"
                        </button>
                        <label style="margin-left:1rem;font-size:13px">
                            <input
                                type="checkbox"
                                prop:checked=move || use_stun.get()
                                disabled=move || !matches!(status.get(), HostStatus::Idle)
                                on:change=move |ev| {
                                    let v = ev.target()
                                        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                        .map(|el| el.checked())
                                        .unwrap_or(false);
                                    set_use_stun.set(v);
                                }
                            />
                            " Use STUN (workaround for hostile networks)"
                        </label>
                    </p>
                    <p>"Status: " {move || format!("{:?}", status.get())}</p>
                    <Show when=move || ice_diag_holder.with(|h| h.is_some())>
                        <p style="font-size:13px;color:#888;margin-top:-0.5rem">
                            {move || ice_diag_holder.get()
                                .map(|sig| diag_line(&sig.get()))
                                .unwrap_or_default()}
                        </p>
                    </Show>
                    <Show when=move || !offer_blob.with(|s| s.is_empty())>
                        <p>"Have the joiner scan this QR — or copy the text and AirDrop / Messages it:"</p>
                        <QrCodeView
                            payload=Signal::derive(move || offer_blob.get())
                            label="LAN host offer SDP"
                        />
                        <p style="margin-top:0.75rem">"Or copy as text:"</p>
                        <textarea
                            rows="6"
                            readonly=true
                            style="width:100%;font-family:monospace;font-size:12px"
                            prop:value=move || offer_blob.get()
                        />
                        <p>
                            <button on:click=move |_| {
                                let blob = offer_blob.get();
                                if let Some(win) = web_sys::window() {
                                    let _ = win.navigator().clipboard().write_text(&blob);
                                }
                            }>
                                "Copy offer"
                            </button>
                            <span style="margin-left:0.5rem;font-size:12px;color:#666">
                                "Bytes: " {move || offer_blob.with(|s| s.len())}
                            </span>
                        </p>
                        <p>"2. Get the joiner's answer back. Either scan their QR with the camera, or paste their text below:"</p>
                        <Show when=move || cam_available.get()>
                            <p style="margin:0.25rem 0 0.5rem 0">
                                <button on:click=move |_| set_scanner_open.set(true)>
                                    "📷 Scan answer QR"
                                </button>
                            </p>
                        </Show>
                        <textarea
                            rows="6"
                            style="width:100%;font-family:monospace;font-size:12px"
                            on:input=move |ev| {
                                let v = ev.target()
                                    .and_then(|t| t.dyn_into::<HtmlTextAreaElement>().ok())
                                    .map(|el| el.value())
                                    .unwrap_or_default();
                                set_answer_input.set(v);
                            }
                            prop:value=move || answer_input.get()
                        />
                        <button
                            on:click=move |_| on_accept.call(())
                            disabled=move || !matches!(status.get(), HostStatus::AwaitingAnswer)
                        >
                            "3. Accept answer"
                        </button>
                    </Show>
                    <QrScanner
                        open=scanner_open
                        on_decode=Callback::new(move |text: String| {
                            set_answer_input.set(text);
                            set_scanner_open.set(false);
                            // Auto-fire Accept (only if we're in the right state).
                            if matches!(status.get_untracked(), HostStatus::AwaitingAnswer) {
                                on_accept.call(());
                            }
                        })
                        on_cancel=Callback::new(move |_| set_scanner_open.set(false))
                    />
                    {move || error_msg.get().map(|e| view! {
                        <p style="color:#c33;margin-top:1rem">"ERROR: " {e}</p>
                    })}
                }
            >
                {move || play_session.get().map(|s| view! {
                    <PlayConnected
                        ws_base=lan_dummy_ws_base()
                        room="lan-host".to_string()
                        password=None
                        watch_only=false
                        debug_enabled=false
                        hints_requested=false
                        injected_session=s
                        back_link_override=app_href("/lan/host")
                    />
                })}
            </Show>
        </div>
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum HostStatus {
    Idle,
    Generating,
    AwaitingAnswer,
    AcceptingAnswer,
    Playing,
}
// SECTION: LanJoinPage component

#[component]
pub fn LanJoinPage() -> impl IntoView {
    let (status, set_status) = create_signal::<JoinStatus>(JoinStatus::Idle);
    let (use_stun, set_use_stun) = create_signal::<bool>(false);
    let (offer_input, set_offer_input) = create_signal::<String>(String::new());
    let (answer_blob, set_answer_blob) = create_signal::<String>(String::new());
    let (error_msg, set_error_msg) = create_signal::<Option<String>>(None);

    // Phase 6: tab-close protection while a game is in progress.
    // Joiner closing the tab leaves the host with a half-game and
    // no recovery (peer disconnect ends the room).
    use_beforeunload_guard(
        Signal::derive(move || matches!(status.get(), JoinStatus::Playing)),
        "Closing this tab will end the LAN game. Continue?",
    );

    // Phase 6: drive the corner OfflineIndicator's LAN state.
    // See `LanHostPage` for the rationale.
    if let Some(pwa) = use_context::<PwaState>() {
        create_effect(move |_| {
            pwa.lan.set(match status.get() {
                JoinStatus::Idle | JoinStatus::Generating | JoinStatus::WaitingForOpen => {
                    LanIndicator::JoinerWaiting
                }
                JoinStatus::Playing => LanIndicator::Connected,
            });
        });
        on_cleanup(move || pwa.lan.set(LanIndicator::Idle));
    }

    // QR scanner modal state. See LanHostPage for the same pattern.
    let (scanner_open, set_scanner_open) = create_signal::<bool>(false);
    let (cam_available, set_cam_available) = create_signal::<bool>(false);
    spawn_local(async move {
        if has_camera().await {
            set_cam_available.set(true);
        }
    });

    // Joiner side: store the JoinerHandshake so we can pull its
    // session out once the DC opens.
    let handshake: Rc<RefCell<Option<JoinerHandshake>>> = Rc::new(RefCell::new(None));
    let (play_session, set_play_session) = create_signal::<Option<Session>>(None);
    // Diag-signal holder mirroring the host page. Populated once
    // `connect_as_joiner` resolves; drives the inline ICE-state badge
    // until the joiner's `handshake` is dropped (at which point the
    // PC's event handlers stop firing).
    let (ice_diag_holder, set_ice_diag_holder) = create_signal::<Option<ReadSignal<IceDiag>>>(None);

    // Re-snapshot the answer SDP on every diag fire (candidates,
    // connection state, gathering). Mirrors the host page; see
    // JoinerHandshake::current_answer.
    {
        let handshake = handshake.clone();
        create_effect(move |_| {
            if let Some(sig) = ice_diag_holder.get() {
                let _ = sig.get();
                if let Some(jh) = handshake.borrow().as_ref() {
                    set_answer_blob.set(jh.current_answer().0);
                }
            }
        });
    }

    // Holder pattern: spawn_local runs without a Leptos owner context,
    // so a `create_effect` inside it would be GC'd immediately (no
    // subscriptions retained — `state.set(Open)` would fire but no
    // effect would react). Solution: define the holder signals and
    // the effect at component scope (properly owned), and have
    // spawn_local just `set` the holders. The component-scope effect
    // re-runs when the holders change AND when the inner state
    // signal changes.
    let (joiner_state_holder, set_joiner_state_holder) =
        create_signal::<Option<ReadSignal<ConnState>>>(None);
    let session_holder: Rc<RefCell<Option<Session>>> = Rc::new(RefCell::new(None));

    {
        let session_holder = session_holder.clone();
        create_effect(move |_| {
            // Read both signals to register subscriptions:
            //   * joiner_state_holder fires once when handshake completes.
            //   * the inner state signal fires when DC opens.
            if let Some(state_sig) = joiner_state_holder.get() {
                if state_sig.get() == ConnState::Open {
                    if let Some(session) = session_holder.borrow().clone() {
                        set_play_session.set(Some(session));
                        set_status.set(JoinStatus::Playing);
                    }
                }
            }
        });
    }

    // Phase 6: surface peer-disconnect on the corner LAN indicator.
    // Watches the joiner's inner state signal for an Open → Closed
    // transition (host's DC closed mid-game). Independent of the
    // toast in `pages/play.rs::PlayConnected` which surfaces the
    // user-facing message.
    if let Some(pwa) = use_context::<PwaState>() {
        let prev: Rc<std::cell::Cell<ConnState>> =
            Rc::new(std::cell::Cell::new(ConnState::Connecting));
        create_effect(move |_| {
            if let Some(state_sig) = joiner_state_holder.get() {
                let now = state_sig.get();
                let was = prev.replace(now);
                if matches!(was, ConnState::Open)
                    && matches!(now, ConnState::Closed | ConnState::Error)
                {
                    pwa.lan.set(LanIndicator::Disconnected);
                }
            }
        });
    }

    let handshake_for_gen = handshake.clone();
    let session_holder_for_gen = session_holder.clone();
    let on_generate: Callback<()> = Callback::new(move |_: ()| {
        if !matches!(status.get_untracked(), JoinStatus::Idle) {
            return;
        }
        let blob = offer_input.get_untracked();
        if blob.trim().is_empty() {
            set_error_msg.set(Some("paste the host's offer SDP first".into()));
            return;
        }
        set_error_msg.set(None);
        set_status.set(JoinStatus::Generating);
        let cfg = WebRtcConfig {
            ice_mode: if use_stun.get_untracked() { IceMode::WithStun } else { IceMode::LanOnly },
        };
        let handshake_slot = handshake_for_gen.clone();
        let session_holder = session_holder_for_gen.clone();
        spawn_local(async move {
            match connect_as_joiner(cfg, OfferBlob(blob)).await {
                Ok(jh) => {
                    set_answer_blob.set(jh.answer.0.clone());
                    let state = jh.session.state;
                    set_ice_diag_holder.set(Some(jh.ice_diag));
                    *session_holder.borrow_mut() = Some(jh.session.clone());
                    *handshake_slot.borrow_mut() = Some(jh);
                    set_status.set(JoinStatus::WaitingForOpen);
                    // Triggers the component-scope effect to subscribe
                    // to the state signal. Once DC opens, that effect
                    // flips status → Playing.
                    set_joiner_state_holder.set(Some(state));
                }
                Err(e) => {
                    set_error_msg.set(Some(format!("connect_as_joiner failed: {e:?}")));
                    set_status.set(JoinStatus::Idle);
                }
            }
        });
    });

    let on_reset: Callback<()> = Callback::new(move |_: ()| {
        if let Some(win) = web_sys::window() {
            let _ = win.location().reload();
        }
    });

    view! {
        <div class="lan-page" style="max-width:720px;margin:1.5rem auto;padding:1rem">
            <Show
                when=move || matches!(status.get(), JoinStatus::Playing)
                fallback=move || view! {
                    <a href=app_href("/") rel="external" class="back-link">"← Back to picker"</a>
                    <h1>"LAN join (WebRTC)"</h1>
                    <p class="muted">
                        "Paste the host's offer SDP, generate an answer, send it back to the host."
                    </p>
                    <p class="muted">
                        "VPN hint: disable any VPN (Cloudflare WARP, NordVPN, etc.) on BOTH \
                         devices before pairing. VPNs replace the real LAN IP with a tunnel \
                         address (often 198.18.x.x) and route LAN traffic through the tunnel, \
                         which breaks WebRTC's direct device discovery. If the VPN must stay on, \
                         configure split-tunnel to bypass: 192.168.0.0/16 + 10.0.0.0/8 + \
                         172.16.0.0/12 (private LANs), 172.20.10.0/28 (iOS hotspot), and \
                         224.0.0.0/4 (multicast / mDNS)."
                    </p>
                    <p class="muted">
                        "STUN hint: \"Use STUN\" adds public-IP \"srflx\" candidates so peers on \
                         different networks (or behind a router that breaks LAN mDNS) can still \
                         find each other. Skip it on a healthy same-LAN setup. Must match the \
                         host's setting. Doesn't help with symmetric NAT / CGNAT."
                    </p>
                    <p>
                        <button on:click=move |_| on_reset.call(())>"Reset"</button>
                        <label style="margin-left:1rem;font-size:13px">
                            <input
                                type="checkbox"
                                prop:checked=move || use_stun.get()
                                disabled=move || !matches!(status.get(), JoinStatus::Idle)
                                on:change=move |ev| {
                                    let v = ev.target()
                                        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                        .map(|el| el.checked())
                                        .unwrap_or(false);
                                    set_use_stun.set(v);
                                }
                            />
                            " Use STUN (must match host's setting)"
                        </label>
                    </p>
                    <p>"Status: " {move || format!("{:?}", status.get())}</p>
                    <Show when=move || ice_diag_holder.with(|h| h.is_some())>
                        <p style="font-size:13px;color:#888;margin-top:-0.5rem">
                            {move || ice_diag_holder.get()
                                .map(|sig| diag_line(&sig.get()))
                                .unwrap_or_default()}
                        </p>
                    </Show>
                    <p>"1. Get the host's offer. Either scan their QR with the camera, or paste their text below:"</p>
                    <Show when=move || cam_available.get()>
                        <p style="margin:0.25rem 0 0.5rem 0">
                            <button
                                on:click=move |_| set_scanner_open.set(true)
                                disabled=move || !matches!(status.get(), JoinStatus::Idle)
                            >
                                "📷 Scan offer QR"
                            </button>
                        </p>
                    </Show>
                    <textarea
                        rows="6"
                        style="width:100%;font-family:monospace;font-size:12px"
                        on:input=move |ev| {
                            let v = ev.target()
                                .and_then(|t| t.dyn_into::<HtmlTextAreaElement>().ok())
                                .map(|el| el.value())
                                .unwrap_or_default();
                            set_offer_input.set(v);
                        }
                        prop:value=move || offer_input.get()
                    />
                    <button
                        on:click=move |_| on_generate.call(())
                        disabled=move || !matches!(status.get(), JoinStatus::Idle)
                    >
                        "2. Generate answer"
                    </button>
                    <QrScanner
                        open=scanner_open
                        on_decode=Callback::new(move |text: String| {
                            set_offer_input.set(text);
                            set_scanner_open.set(false);
                            // Auto-fire Generate answer if still idle.
                            if matches!(status.get_untracked(), JoinStatus::Idle) {
                                on_generate.call(());
                            }
                        })
                        on_cancel=Callback::new(move |_| set_scanner_open.set(false))
                    />
                    <Show when=move || !answer_blob.with(|s| s.is_empty())>
                        <p>"Show this QR back to the host — or copy + AirDrop the text:"</p>
                        <QrCodeView
                            payload=Signal::derive(move || answer_blob.get())
                            label="LAN joiner answer SDP"
                        />
                        <p style="margin-top:0.75rem">"Or copy as text:"</p>
                        <textarea
                            rows="6"
                            readonly=true
                            style="width:100%;font-family:monospace;font-size:12px"
                            prop:value=move || answer_blob.get()
                        />
                        <p>
                            <button on:click=move |_| {
                                let blob = answer_blob.get();
                                if let Some(win) = web_sys::window() {
                                    let _ = win.navigator().clipboard().write_text(&blob);
                                }
                            }>
                                "Copy answer"
                            </button>
                            <span style="margin-left:0.5rem;font-size:12px;color:#666">
                                "Bytes: " {move || answer_blob.with(|s| s.len())}
                            </span>
                        </p>
                        <p class="muted" style="font-size:13px">
                            "Waiting for host to accept the answer + the DataChannel to open. \
                             Once host taps 'Accept answer' on their side, this page should \
                             flip to the game view."
                        </p>
                    </Show>
                    {move || error_msg.get().map(|e| view! {
                        <p style="color:#c33;margin-top:1rem">"ERROR: " {e}</p>
                    })}
                }
            >
                {move || play_session.get().map(|s| view! {
                    <PlayConnected
                        ws_base=lan_dummy_ws_base()
                        room="lan-join".to_string()
                        password=None
                        watch_only=false
                        debug_enabled=false
                        hints_requested=false
                        injected_session=s
                        back_link_override=app_href("/lan/join")
                    />
                })}
            </Show>
        </div>
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum JoinStatus {
    Idle,
    Generating,
    WaitingForOpen,
    Playing,
}
// SECTION: shared UI helpers (textarea + Copy button + link back)

/// LAN play has no chess-net `WsBase` — but `PlayConnected` still
/// requires one for the OnlineSidebar's "back to lobby" path
/// (which we override anyway via `back_link_override`). Construct
/// a stub `WsBase` whose `base` is empty + `persist_in_urls = false`
/// so any code path that DID happen to use it for URL building
/// produces something obviously LAN-shaped.
fn lan_dummy_ws_base() -> WsBase {
    WsBase { base: String::new(), persist_in_urls: false }
}
