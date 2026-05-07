//! JSON round-trip for the wire enums. Catches stray `#[serde(rename)]`,
//! tag-style drift, or accidental Reveal-canonicalization on the wire.

use chess_core::coord::Square;
use chess_core::moves::Move;
use chess_core::piece::Side;
use chess_core::rules::RuleSet;
use chess_core::state::GameState;
use chess_core::view::PlayerView;
use chess_net::protocol::{
    variant_label, ChatLine, ClientMsg, RoomStatus, RoomSummary, ServerMsg, PROTOCOL_VERSION,
};

fn roundtrip_server(msg: &ServerMsg) -> ServerMsg {
    let json = serde_json::to_string(msg).expect("encode");
    serde_json::from_str(&json).expect("decode")
}

fn roundtrip_client(msg: &ClientMsg) -> ClientMsg {
    let json = serde_json::to_string(msg).expect("encode");
    serde_json::from_str(&json).expect("decode")
}

#[test]
fn server_hello_roundtrips() {
    let state = GameState::new(RuleSet::xiangqi_casual());
    let view = PlayerView::project(&state, Side::RED);
    let msg = ServerMsg::Hello {
        protocol: PROTOCOL_VERSION,
        observer: Side::RED,
        rules: state.rules.clone(),
        view,
    };
    let back = roundtrip_server(&msg);
    match back {
        ServerMsg::Hello { protocol, observer, .. } => {
            assert_eq!(protocol, PROTOCOL_VERSION);
            assert_eq!(observer, Side::RED);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn server_update_and_error_roundtrip() {
    let state = GameState::new(RuleSet::xiangqi_casual());
    let view = PlayerView::project(&state, Side::BLACK);
    let upd = ServerMsg::Update { view };
    assert!(matches!(roundtrip_server(&upd), ServerMsg::Update { .. }));

    let err = ServerMsg::Error { message: "illegal move: foo".into() };
    match roundtrip_server(&err) {
        ServerMsg::Error { message } => assert_eq!(message, "illegal move: foo"),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn client_move_step_roundtrips() {
    let mv = Move::Step { from: Square(7), to: Square(16) };
    let msg = ClientMsg::Move { mv: mv.clone() };
    match roundtrip_client(&msg) {
        ClientMsg::Move { mv: back } => assert_eq!(mv, back),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn client_reveal_none_stays_none_on_wire() {
    // The wire ABI: clients send `revealed: None`; the server fills it locally.
    // If anyone ever serializes the engine-canonicalized form by accident,
    // this test breaks before it ships.
    let mv = Move::Reveal { at: Square(5), revealed: None };
    let json = serde_json::to_string(&ClientMsg::Move { mv: mv.clone() }).unwrap();
    assert!(json.contains("\"revealed\":null"), "got: {json}");

    match roundtrip_client(&ClientMsg::Move { mv: mv.clone() }) {
        ClientMsg::Move { mv: back } => assert_eq!(back, mv),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn client_resign_roundtrips() {
    let json = serde_json::to_string(&ClientMsg::Resign).unwrap();
    assert!(json.contains("Resign"), "got: {json}");
    assert!(matches!(roundtrip_client(&ClientMsg::Resign), ClientMsg::Resign));
}

#[test]
fn client_rematch_roundtrips() {
    let json = serde_json::to_string(&ClientMsg::Rematch).unwrap();
    assert!(json.contains("Rematch"), "got: {json}");
    assert!(matches!(roundtrip_client(&ClientMsg::Rematch), ClientMsg::Rematch));
}

#[test]
fn protocol_uses_type_tag() {
    // Sanity check: the schema uses `"type"` as the tag, matching our docs
    // (so `wscat` users can switch on it directly).
    let json = serde_json::to_string(&ClientMsg::Move {
        mv: Move::Step { from: Square(0), to: Square(1) },
    })
    .unwrap();
    assert!(json.starts_with("{\"type\":\"Move\""), "got: {json}");
}

#[test]
fn server_rooms_roundtrips() {
    let msg = ServerMsg::Rooms {
        rooms: vec![
            RoomSummary {
                id: "main".into(),
                variant: "xiangqi".into(),
                seats: 0,
                spectators: 0,
                has_password: false,
                status: RoomStatus::Lobby,
            },
            RoomSummary {
                id: "locked".into(),
                variant: "banqi".into(),
                seats: 2,
                spectators: 3,
                has_password: true,
                status: RoomStatus::Playing,
            },
        ],
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"Rooms\""), "got: {json}");
    assert!(json.contains("\"status\":\"playing\""), "lowercase rename: {json}");
    match roundtrip_server(&msg) {
        ServerMsg::Rooms { rooms } => {
            assert_eq!(rooms.len(), 2);
            assert_eq!(rooms[0].id, "main");
            assert!(!rooms[0].has_password);
            assert_eq!(rooms[1].status, RoomStatus::Playing);
            assert_eq!(rooms[1].spectators, 3);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn v2_room_summary_decodes_into_v3_with_zero_spectators() {
    // A v2 server emitted RoomSummary without the `spectators` field.
    // v3 clients deserializing that JSON should land on `spectators: 0`
    // without a serde error — that's what `#[serde(default)]` buys us.
    let v2_json = r#"{
        "id": "main",
        "variant": "xiangqi",
        "seats": 1,
        "has_password": false,
        "status": "lobby"
    }"#;
    let parsed: RoomSummary = serde_json::from_str(v2_json).expect("decode v2 RoomSummary");
    assert_eq!(parsed.spectators, 0);
    assert_eq!(parsed.id, "main");
    assert_eq!(parsed.seats, 1);
}

#[test]
fn server_spectating_roundtrips() {
    let state = GameState::new(RuleSet::xiangqi_casual());
    let view = PlayerView::project(&state, Side::RED);
    let msg =
        ServerMsg::Spectating { protocol: PROTOCOL_VERSION, rules: state.rules.clone(), view };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"Spectating\""), "got: {json}");
    match roundtrip_server(&msg) {
        ServerMsg::Spectating { protocol, .. } => assert_eq!(protocol, PROTOCOL_VERSION),
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn server_chat_history_roundtrips() {
    let lines = vec![
        ChatLine { from: Side::RED, text: "hi".into(), ts_ms: 1_700_000_000_000 },
        ChatLine { from: Side::BLACK, text: "hello".into(), ts_ms: 1_700_000_001_000 },
    ];
    let msg = ServerMsg::ChatHistory { lines };
    match roundtrip_server(&msg) {
        ServerMsg::ChatHistory { lines } => {
            assert_eq!(lines.len(), 2);
            assert_eq!(lines[0].text, "hi");
            assert_eq!(lines[1].from, Side::BLACK);
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn server_chat_roundtrips() {
    let msg = ServerMsg::Chat {
        line: ChatLine { from: Side::RED, text: "good game".into(), ts_ms: 12345 },
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"Chat\""), "got: {json}");
    match roundtrip_server(&msg) {
        ServerMsg::Chat { line } => {
            assert_eq!(line.text, "good game");
            assert_eq!(line.from, Side::RED);
            assert_eq!(line.ts_ms, 12345);
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn client_chat_roundtrips() {
    let msg = ClientMsg::Chat { text: "well played".into() };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"Chat\""), "got: {json}");
    assert!(json.contains("\"text\":\"well played\""), "got: {json}");
    match roundtrip_client(&msg) {
        ClientMsg::Chat { text } => assert_eq!(text, "well played"),
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn client_list_rooms_roundtrips() {
    let json = serde_json::to_string(&ClientMsg::ListRooms).unwrap();
    assert!(json.contains("ListRooms"), "got: {json}");
    assert!(matches!(roundtrip_client(&ClientMsg::ListRooms), ClientMsg::ListRooms));
}

#[test]
fn variant_label_covers_all_variants() {
    assert_eq!(variant_label(&RuleSet::xiangqi()), "xiangqi-strict");
    assert_eq!(variant_label(&RuleSet::xiangqi_casual()), "xiangqi");
    assert_eq!(variant_label(&RuleSet::banqi(Default::default())), "banqi");
    assert_eq!(variant_label(&RuleSet::three_kingdom()), "three-kingdom");
}
