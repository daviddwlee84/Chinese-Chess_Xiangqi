//! `chess-cli` — REPL test harness for chess-core.
//!
//! Commands:
//!   xiangqi                                start standard xiangqi
//!   banqi [--house chain,rush,...] [--seed N] [--preset taiwan|aggressive|purist]
//!   moves                                  list legal moves (ICCS)
//!   play <move>                            apply a move (e.g. h2e2 or 'flip a0')
//!   view [<side>]                          render the board (default: side-to-move)
//!   undo                                   undo last move
//!   status                                 print game status
//!   help                                   show this help
//!   quit / exit                            exit

use std::io::{self, BufRead, Write};

use chess_core::board::BoardShape;
use chess_core::notation::iccs;
use chess_core::piece::{PieceKind, Side};
use chess_core::rules::{HouseRules, RuleSet};
use chess_core::state::GameState;
use chess_core::view::{PlayerView, VisibleCell};

fn main() {
    println!("chess-cli ready. Type 'help' for commands.");
    let stdin = io::stdin();
    let mut state: Option<GameState> = None;
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let cmd = line.trim();
        if cmd.is_empty() {
            print_prompt();
            continue;
        }
        match dispatch(cmd, &mut state) {
            Ok(Action::Continue) => {}
            Ok(Action::Quit) => break,
            Err(e) => println!("error: {e}"),
        }
        print_prompt();
    }
}

fn print_prompt() {
    print!("> ");
    let _ = io::stdout().flush();
}

enum Action {
    Continue,
    Quit,
}

fn dispatch(line: &str, state: &mut Option<GameState>) -> Result<Action, String> {
    let mut parts = line.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();
    match cmd {
        "help" | "?" => {
            print_help();
            Ok(Action::Continue)
        }
        "quit" | "exit" => Ok(Action::Quit),
        "xiangqi" => {
            *state = Some(GameState::new(RuleSet::xiangqi()));
            println!("Started standard xiangqi. Red to move.");
            Ok(Action::Continue)
        }
        "banqi" => {
            let rules = parse_banqi_args(rest)?;
            *state = Some(GameState::new(rules));
            println!("Started banqi. Players have 32 face-down pieces; first flip locks colors.");
            Ok(Action::Continue)
        }
        "moves" => {
            let s = state.as_ref().ok_or("start a game first (xiangqi / banqi)")?;
            let moves = s.legal_moves();
            println!("{} legal moves:", moves.len());
            for (i, m) in moves.iter().enumerate() {
                println!("  [{i:>3}] {}", iccs::encode_move(&s.board, m));
            }
            Ok(Action::Continue)
        }
        "play" => {
            let s = state.as_mut().ok_or("start a game first")?;
            let m = iccs::decode_move(s, rest).map_err(|e| e.to_string())?;
            s.make_move(&m).map_err(|e| e.to_string())?;
            s.refresh_status();
            println!("Played: {}", iccs::encode_move(&s.board, &m));
            print_status(s);
            Ok(Action::Continue)
        }
        "undo" => {
            let s = state.as_mut().ok_or("start a game first")?;
            s.unmake_move().map_err(|e| e.to_string())?;
            println!("Undone. Now {:?} to move.", s.side_to_move);
            Ok(Action::Continue)
        }
        "view" => {
            let s = state.as_ref().ok_or("start a game first")?;
            let observer = if rest.is_empty() {
                s.side_to_move
            } else {
                Side(rest.parse::<u8>().map_err(|_| "view <side> expects 0,1,2")?)
            };
            let view = PlayerView::project(s, observer);
            print_view(&view);
            Ok(Action::Continue)
        }
        "status" => {
            let s = state.as_ref().ok_or("start a game first")?;
            print_status(s);
            Ok(Action::Continue)
        }
        other => Err(format!("unknown command: {other}")),
    }
}

fn parse_banqi_args(rest: &str) -> Result<RuleSet, String> {
    let mut house = HouseRules::empty();
    let mut seed: Option<u64> = None;
    let mut tokens = rest.split_whitespace().peekable();
    while let Some(tok) = tokens.next() {
        match tok {
            "--house" => {
                let val = tokens.next().ok_or("--house needs a value")?;
                for flag in val.split(',') {
                    house |= match flag {
                        "chain" => HouseRules::CHAIN_CAPTURE,
                        "dark-chain" | "dark" | "dark-capture" => HouseRules::DARK_CAPTURE,
                        "dark-trade" | "trade" => HouseRules::DARK_CAPTURE_TRADE,
                        "rush" | "chariot-rush" => HouseRules::CHARIOT_RUSH,
                        "horse" | "horse-diagonal" | "diag" => HouseRules::HORSE_DIAGONAL,
                        "cannon-fast" | "fast-cannon" => HouseRules::CANNON_FAST_MOVE,
                        other => return Err(format!("unknown house flag: {other}")),
                    };
                }
            }
            "--preset" => {
                let val = tokens.next().ok_or("--preset needs a value")?;
                house |= match val {
                    "purist" => chess_core::rules::PRESET_PURIST,
                    "taiwan" => chess_core::rules::PRESET_TAIWAN,
                    "aggressive" => chess_core::rules::PRESET_AGGRESSIVE,
                    other => return Err(format!("unknown preset: {other}")),
                };
            }
            "--seed" => {
                let val = tokens.next().ok_or("--seed needs a value")?;
                seed = Some(val.parse().map_err(|_| "seed must be u64")?);
            }
            other => return Err(format!("unknown banqi arg: {other}")),
        }
    }
    let house = chess_core::rules::house::normalize(house);
    Ok(match seed {
        Some(s) => RuleSet::banqi_with_seed(house, s),
        None => RuleSet::banqi(house),
    })
}

fn print_status(s: &GameState) {
    println!(
        "  side_to_move={:?}  no_progress_plies={}  status={:?}",
        s.side_to_move, s.no_progress_plies, s.status
    );
}

fn print_help() {
    println!(
        "Commands:
  xiangqi
  banqi [--preset purist|taiwan|aggressive] [--house chain,dark-chain,rush,horse-diagonal,cannon-fast] [--seed N]
  moves
  play <iccs>             e.g. h2e2 or 'flip a0' or 'a3xb3xc3' for chains
  undo
  view [side]             0 = RED, 1 = BLACK, 2 = third (3-kingdoms)
  status
  help
  quit"
    );
}

fn print_view(view: &PlayerView) {
    let (w, h) = (view.width as usize, view.height as usize);
    println!("Observer: {:?}  Side to move: {:?}", view.observer, view.side_to_move);
    // Print top-down (highest rank first).
    for r in (0..h).rev() {
        print!(" {r:>2} |");
        for f in 0..w {
            let idx = r * w + f;
            let glyph = match &view.cells[idx] {
                VisibleCell::Empty => '.',
                VisibleCell::Hidden => '?',
                VisibleCell::Revealed(pos) => {
                    let ch = piece_char(pos.piece.kind);
                    if pos.piece.side == Side::RED {
                        ch.to_ascii_uppercase()
                    } else {
                        ch
                    }
                }
            };
            print!(" {glyph}");
            if matches!(view.shape, BoardShape::Xiangqi9x10)
                && (f == 8 || (h == 10 && r == 5 && f + 1 == w))
            {
                // No-op marker; the river break is the gap between ranks 4 and 5,
                // rendered below by an interstitial line.
            }
        }
        println!();
        // Xiangqi river between ranks 4 and 5
        if matches!(view.shape, BoardShape::Xiangqi9x10) && r == 5 {
            println!("    | -- 楚河 漢界 --");
        }
    }
    print!("    +");
    for _ in 0..w {
        print!("--");
    }
    println!();
    print!("      ");
    for f in 0..w {
        print!(" {}", (b'a' + f as u8) as char);
    }
    println!();
    println!("Status: {:?}  legal moves available: {}", view.status, view.legal_moves.len());
}

fn piece_char(kind: PieceKind) -> char {
    match kind {
        PieceKind::General => 'g',
        PieceKind::Advisor => 'a',
        PieceKind::Elephant => 'b', // 'b' for bishop-equivalent
        PieceKind::Chariot => 'r',  // rook-equivalent
        PieceKind::Horse => 'h',
        PieceKind::Cannon => 'c',
        PieceKind::Soldier => 'p', // pawn-equivalent
    }
}
