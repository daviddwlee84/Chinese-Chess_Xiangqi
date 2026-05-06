//! JSON round-trip for the wire enums. Catches stray `#[serde(rename)]`,
//! tag-style drift, or accidental Reveal-canonicalization on the wire.

use chess_core::coord::Square;
use chess_core::moves::Move;
use chess_core::piece::Side;
use chess_core::rules::RuleSet;
use chess_core::state::GameState;
use chess_core::view::PlayerView;
use chess_net::protocol::{ClientMsg, ServerMsg, PROTOCOL_VERSION};

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
fn protocol_uses_type_tag() {
    // Sanity check: the schema uses `"type"` as the tag, matching our docs
    // (so `wscat` users can switch on it directly).
    let json = serde_json::to_string(&ClientMsg::Move {
        mv: Move::Step { from: Square(0), to: Square(1) },
    })
    .unwrap();
    assert!(json.starts_with("{\"type\":\"Move\""), "got: {json}");
}
