//! App state + dispatch. The AppState holds a `GameState`, UI cursor, and a
//! short-lived flash message; actions from `input.rs` mutate it.

use std::collections::VecDeque;

use chess_ai::{AiAnalysis, AiOptions, Difficulty, Randomness, Strategy};
use chess_core::board::BoardShape;
use chess_core::coord::Square;
use chess_core::moves::Move;
use chess_core::piece::{Piece, PieceKind, Side};
use chess_core::rules::{HouseRules, RuleSet};
use chess_core::state::{GameState, GameStatus, WinReason};
use chess_core::view::{PlayerView, VisibleCell};
use chess_net::{ChatLine, ClientMsg, RoomSummary, ServerMsg};

const CHAT_HISTORY_CAP: usize = 50;
const CHAT_INPUT_MAX: usize = 256;
const COORD_INPUT_MAX: usize = 16;

use crate::confetti::ConfettiAnim;
use crate::glyph::Style;
use crate::input::{Action, CoordKind, InputMode};
use crate::net::{NetClient, NetEvent};
use crate::orient;
use crate::text_input;
use crate::url::{normalize_host_url, urlencode, valid_room_id};

/// Variant + preset choices in the picker, in display order.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum PickerEntry {
    Xiangqi,
    XiangqiStrict,
    /// Local vs computer (xiangqi). Uses defaults: player Red, AI Black,
    /// Normal difficulty, v2 (material+PST) engine. CLI flags
    /// `--ai-side / --ai-difficulty / --ai-engine` override these. The
    /// picker entry intentionally has no sub-screen — the CLI is the
    /// power-user surface; the picker is the "I just want to play" one.
    XiangqiVsAi,
    BanqiPurist,
    BanqiTaiwan,
    BanqiAggressive,
    /// Open the tree-style custom-rules sub-screen (xiangqi).
    CustomXiangqi,
    /// Open the tree-style custom-rules sub-screen (banqi).
    CustomBanqi,
    /// Open the host prompt → lobby browser → online play flow.
    ConnectToServer,
    Quit,
}

impl PickerEntry {
    pub const ALL: [PickerEntry; 10] = [
        PickerEntry::Xiangqi,
        PickerEntry::XiangqiStrict,
        PickerEntry::XiangqiVsAi,
        PickerEntry::BanqiPurist,
        PickerEntry::BanqiTaiwan,
        PickerEntry::BanqiAggressive,
        PickerEntry::CustomXiangqi,
        PickerEntry::CustomBanqi,
        PickerEntry::ConnectToServer,
        PickerEntry::Quit,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PickerEntry::Xiangqi => "Xiangqi (象棋)",
            PickerEntry::XiangqiStrict => "Xiangqi (象棋, strict — must defend check)",
            PickerEntry::XiangqiVsAi => "Xiangqi (象棋) vs Computer (alpha-beta, in-process)",
            PickerEntry::BanqiPurist => "Banqi (暗棋) — purist",
            PickerEntry::BanqiTaiwan => "Banqi (暗棋) — Taiwan house rules",
            PickerEntry::BanqiAggressive => "Banqi (暗棋) — aggressive house rules",
            PickerEntry::CustomXiangqi => "Xiangqi (象棋) — custom rules…",
            PickerEntry::CustomBanqi => "Banqi (暗棋) — custom rules…",
            PickerEntry::ConnectToServer => "Connect to server… (online)",
            PickerEntry::Quit => "Quit",
        }
    }

    pub fn rules(self) -> Option<RuleSet> {
        match self {
            // Default xiangqi is casual: more permissive, you lose by general
            // capture. Strict (standard rules) is one row down.
            PickerEntry::Xiangqi => Some(RuleSet::xiangqi_casual()),
            PickerEntry::XiangqiStrict => Some(RuleSet::xiangqi()),
            PickerEntry::XiangqiVsAi => Some(RuleSet::xiangqi_casual()),
            PickerEntry::BanqiPurist => Some(RuleSet::banqi(HouseRules::empty())),
            PickerEntry::BanqiTaiwan => Some(RuleSet::banqi(chess_core::rules::PRESET_TAIWAN)),
            PickerEntry::BanqiAggressive => {
                Some(RuleSet::banqi(chess_core::rules::PRESET_AGGRESSIVE))
            }
            PickerEntry::CustomXiangqi
            | PickerEntry::CustomBanqi
            | PickerEntry::ConnectToServer
            | PickerEntry::Quit => None,
        }
    }
}

/// Local vs-AI configuration. Mirrors `clients/chess-web/src/pages/local.rs::VsAiConfig`
/// — same field semantics so future shared-helpers can lift this up.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct VsAiConfig {
    /// Side the AI plays. Player gets the opposite.
    pub ai_side: Side,
    pub difficulty: Difficulty,
    pub strategy: Strategy,
    /// `None` = use difficulty default; `Some(_)` = explicit override.
    pub randomness: Option<Randomness>,
    /// `None` = use difficulty default depth; `Some(N)` = explicit override.
    pub depth: Option<u8>,
    /// `None` = use the engine's auto-scaled budget (v5: depth-scaled,
    /// v1-v4: flat NODE_BUDGET); `Some(N)` = explicit override. Set
    /// via the `--ai-budget N` CLI flag.
    pub node_budget: Option<u32>,
}

impl Default for VsAiConfig {
    fn default() -> Self {
        Self {
            ai_side: Side::BLACK,
            difficulty: Difficulty::Normal,
            strategy: Strategy::default(),
            randomness: None,
            depth: None,
            node_budget: None,
        }
    }
}

/// Variant chooser for the custom-rules sub-screen. We don't reuse
/// `chess_core::rules::Variant` because three-kingdom isn't shipped yet
/// and the picker only offers xiangqi + banqi here.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum CustomVariant {
    Xiangqi,
    Banqi,
}

/// Banqi preset selector for the custom-rules sub-screen. `Custom` is the
/// "I edited the flags individually" sentinel so the radio doesn't lie
/// about which preset is active.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum BanqiPreset {
    Purist,
    Taiwan,
    Aggressive,
    Custom,
}

impl BanqiPreset {
    pub fn flags(self) -> Option<HouseRules> {
        match self {
            BanqiPreset::Purist => Some(chess_core::rules::PRESET_PURIST),
            BanqiPreset::Taiwan => Some(chess_core::rules::PRESET_TAIWAN),
            BanqiPreset::Aggressive => Some(chess_core::rules::PRESET_AGGRESSIVE),
            BanqiPreset::Custom => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BanqiPreset::Purist => "Purist (no house rules)",
            BanqiPreset::Taiwan => "Taiwan (chain + chariot rush)",
            BanqiPreset::Aggressive => "Aggressive (chain + dark + rush + horse-diag)",
            BanqiPreset::Custom => "Custom (manual flags below)",
        }
    }

    pub fn from_flags(flags: HouseRules) -> Self {
        if flags == chess_core::rules::PRESET_PURIST {
            BanqiPreset::Purist
        } else if flags == chess_core::rules::PRESET_TAIWAN {
            BanqiPreset::Taiwan
        } else if flags == chess_core::rules::PRESET_AGGRESSIVE {
            BanqiPreset::Aggressive
        } else {
            BanqiPreset::Custom
        }
    }
}

/// One selectable row in the custom-rules sub-screen. Cursor index walks
/// the `Vec<CustomRulesItem>` returned by `CustomRulesView::items`; render
/// uses the same list to know what to draw + which prefix to show.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum CustomRulesItem {
    /// Xiangqi radio button. `bool` = strict (true) or casual (false).
    XiangqiPreset(bool),
    BanqiPresetItem(BanqiPreset),
    BanqiFlagItem(HouseRules),
    BanqiSeed,
    Start,
}

/// State for the tree-style "Custom rules…" sub-screen. Reachable from the
/// picker via `PickerEntry::CustomXiangqi` / `CustomBanqi`. Maintains a
/// cursor over `items()` plus per-variant rule state. `Start` builds a
/// `RuleSet` and transitions to `Screen::Game`; `Esc` returns to the
/// picker with the original cursor restored.
pub struct CustomRulesView {
    pub variant: CustomVariant,
    pub cursor: usize,
    /// Xiangqi: true = standard self-check, false = casual (default).
    pub xiangqi_strict: bool,
    pub banqi_preset: BanqiPreset,
    pub banqi_flags: HouseRules,
    /// User-typed seed buffer. Empty = nondeterministic shuffle.
    pub banqi_seed: String,
    pub last_msg: Option<String>,
    /// Picker cursor index when this screen was opened — used to restore
    /// the picker selection on Esc / Back.
    pub picker_cursor: usize,
}

/// Six house-rule flag rows shown in the custom-rules screen, paired with
/// their CLI tokens / Chinese subtitles for parity with the web client's
/// `parse_house_csv`.
pub const CUSTOM_BANQI_FLAGS: [(HouseRules, &str, &str); 6] = [
    (HouseRules::CHAIN_CAPTURE, "chain", "連吃"),
    (HouseRules::DARK_CAPTURE, "dark", "暗吃"),
    (HouseRules::CHARIOT_RUSH, "rush", "車衝"),
    (HouseRules::HORSE_DIAGONAL, "horse", "馬斜"),
    (HouseRules::CANNON_FAST_MOVE, "cannon", "砲快"),
    (HouseRules::DARK_CAPTURE_TRADE, "dark-trade", "暗吃換子"),
];

impl CustomRulesView {
    pub fn new_xiangqi(picker_cursor: usize) -> Self {
        Self {
            variant: CustomVariant::Xiangqi,
            cursor: 0,
            // Match the existing picker default: Xiangqi entry is casual.
            xiangqi_strict: false,
            banqi_preset: BanqiPreset::Purist,
            banqi_flags: HouseRules::empty(),
            banqi_seed: String::new(),
            last_msg: None,
            picker_cursor,
        }
    }

    pub fn new_banqi(picker_cursor: usize) -> Self {
        Self {
            variant: CustomVariant::Banqi,
            cursor: 0,
            xiangqi_strict: false,
            banqi_preset: BanqiPreset::Purist,
            banqi_flags: chess_core::rules::PRESET_PURIST,
            banqi_seed: String::new(),
            last_msg: None,
            picker_cursor,
        }
    }

    /// All selectable rows, in order. Cursor index walks this list; the
    /// renderer interleaves headers between the section boundaries.
    pub fn items(&self) -> Vec<CustomRulesItem> {
        match self.variant {
            CustomVariant::Xiangqi => vec![
                CustomRulesItem::XiangqiPreset(false),
                CustomRulesItem::XiangqiPreset(true),
                CustomRulesItem::Start,
            ],
            CustomVariant::Banqi => {
                let mut items = vec![
                    CustomRulesItem::BanqiPresetItem(BanqiPreset::Purist),
                    CustomRulesItem::BanqiPresetItem(BanqiPreset::Taiwan),
                    CustomRulesItem::BanqiPresetItem(BanqiPreset::Aggressive),
                    CustomRulesItem::BanqiPresetItem(BanqiPreset::Custom),
                ];
                for (flag, _, _) in CUSTOM_BANQI_FLAGS {
                    items.push(CustomRulesItem::BanqiFlagItem(flag));
                }
                items.push(CustomRulesItem::BanqiSeed);
                items.push(CustomRulesItem::Start);
                items
            }
        }
    }

    /// Build the chosen `RuleSet`. `Err` only when the seed buffer is
    /// non-empty but not a valid `u64` — surfaces as `last_msg` and keeps
    /// the screen open so the user can fix it.
    pub fn to_rule_set(&self) -> Result<RuleSet, String> {
        match self.variant {
            CustomVariant::Xiangqi => {
                Ok(if self.xiangqi_strict { RuleSet::xiangqi() } else { RuleSet::xiangqi_casual() })
            }
            CustomVariant::Banqi => {
                let flags = chess_core::rules::house::normalize(self.banqi_flags);
                let seed_trim = self.banqi_seed.trim();
                if seed_trim.is_empty() {
                    Ok(RuleSet::banqi(flags))
                } else {
                    let seed: u64 = seed_trim
                        .parse()
                        .map_err(|_| format!("Seed must be a number (got `{seed_trim}`)."))?;
                    Ok(RuleSet::banqi_with_seed(flags, seed))
                }
            }
        }
    }
}

pub struct PickerView {
    pub cursor: usize,
}

pub struct GameView {
    pub state: GameState,
    pub cursor: (u8, u8),
    pub selected: Option<Square>,
    pub last_msg: Option<String>,
    /// `Some(state)` while the user is typing a coordinate move (`:` or `m`).
    /// Mutually exclusive with `chat_input` in Net mode.
    pub coord_input: Option<CoordInputState>,
    /// `Some(_)` for vs-AI games; `None` for plain pass-and-play.
    /// Drives the AI move pump in `apply_move` — after each *human* move,
    /// if it's now the AI's turn we synchronously call `chess_ai::choose_move`
    /// and apply the result before returning to the event loop.
    pub ai: Option<VsAiConfig>,
    /// Most recent AI analysis (full scored root-move list + PVs).
    /// Populated by `ai_reply` after every AI move regardless of the
    /// `debug_open` state — ratatui re-renders cheaply, so we eagerly
    /// produce the data and only render the panel when toggled.
    /// Cleared on undo / new game.
    pub last_analysis: Option<AiAnalysis>,
    /// Per-ply win-rate samples, populated when the user enabled
    /// `--evalbar` at startup. Empty otherwise. The sidebar's eval
    /// headline + ASCII chart read from this. Sample at index `i`
    /// represents the position **after** the `(i+1)`-th half-move
    /// — `samples[0]` corresponds to history.len() == 1, etc.
    /// Cleared on undo / new game alongside `last_analysis`.
    pub eval_samples: Vec<crate::eval::EvalSample>,
}

/// Per-prompt state for coord-input mode (`:` instant or `m` live preview).
/// Lives on both `GameView` and `NetView`; mirrored shape so the dispatchers
/// can share helpers.
pub struct CoordInputState {
    pub kind: CoordKind,
    pub buf: String,
    /// Live mode only: `(cursor, selected)` snapshot taken on entry, restored
    /// on Esc. Always `None` for `Instant` (instant mode never touches them).
    pub snapshot: Option<((u8, u8), Option<Square>)>,
}

/// Role assigned by the server: a seated player (with a side) or a
/// read-only spectator. `None` on `NetView` until the welcome arrives.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum NetRole {
    Player(Side),
    Spectator,
}

impl NetRole {
    pub fn is_player(self) -> bool {
        matches!(self, NetRole::Player(_))
    }

    pub fn is_spectator(self) -> bool {
        matches!(self, NetRole::Spectator)
    }

    /// Side from whose POV the board is rendered. Spectators view from
    /// Red's perspective (matches what chess-net's broadcast_update
    /// projects for spectator updates).
    pub fn observer(self) -> Side {
        match self {
            NetRole::Player(s) => s,
            NetRole::Spectator => Side::RED,
        }
    }
}

pub struct NetView {
    pub client: NetClient,
    pub url: String,
    pub last_view: Option<PlayerView>,
    pub rules: Option<RuleSet>,
    /// Server-assigned role. `None` until `Hello` / `Spectating` arrives.
    pub role: Option<NetRole>,
    pub cursor: (u8, u8),
    pub selected: Option<Square>,
    pub last_msg: Option<String>,
    /// True between Connected and Disconnected events. Used by the sidebar.
    pub connected: bool,
    /// In-room chat ring buffer (cap [`CHAT_HISTORY_CAP`]). Mirrors the
    /// server's per-room buffer so the sidebar can replay history.
    pub chat: VecDeque<ChatLine>,
    /// `Some(buf)` while the user is typing a chat line (entered via 't').
    /// Players only — the chat-start action is a no-op for spectators.
    pub chat_input: Option<String>,
    /// `Some(state)` while the user is typing a coord move (`:` or `m`).
    /// Mutually exclusive with `chat_input`.
    pub coord_input: Option<CoordInputState>,
    /// Server's `hints_allowed` flag for this room (v5+ protocol). `None`
    /// while waiting for the welcome; `Some(b)` once Hello/Spectating
    /// arrives. The TUI debug panel (`D` key) only shows analysis in
    /// net mode when this is `Some(true)`.
    pub hints_allowed: Option<bool>,
    /// Cached `chess_ai::AiAnalysis` for the current `last_view`. Lazily
    /// computed when the user opens the debug panel and `hints_allowed`
    /// is true. Reset to `None` on every `Update`.
    pub last_analysis: Option<chess_ai::AiAnalysis>,
}

/// Free-text "ws://host:port" prompt entered before the lobby.
pub struct HostPromptView {
    pub buf: String,
    pub error: Option<String>,
}

/// Live room browser. Reads from a separate `NetClient` connected to the
/// server's `/lobby` endpoint; joining a room spawns a fresh `NetClient` to
/// `/ws/<id>?password=…` and transitions to `Screen::Net`.
pub struct LobbyView {
    pub client: NetClient,
    /// Original `ws://host:port` (no path). The lobby ws is `host/lobby`;
    /// joining builds `host/ws/<id>` from the same prefix.
    pub host: String,
    pub rooms: Vec<RoomSummary>,
    pub cursor: usize,
    pub last_msg: Option<String>,
    pub connected: bool,
    /// When `Some`, the user picked a password-locked room and we're
    /// reading the password into this buffer before issuing the join.
    pub pending_join: Option<PendingJoin>,
    /// CLI `--hints` flag — appended to every join URL emitted from
    /// this lobby. The server ignores it for existing rooms (first-
    /// joiner wins); it's only effective when this user creates a
    /// fresh room.
    pub want_hints: bool,
}

pub struct PendingJoin {
    pub room_id: String,
    pub password_buf: String,
    /// `true` when the user is joining as a spectator (watch flow); the
    /// resulting URL appends `?role=spectator` in addition to the password.
    pub as_spectator: bool,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum CreateRoomField {
    Id,
    Password,
    Submit,
}

/// Form for creating a new room (server auto-creates on first join).
pub struct CreateRoomView {
    pub host: String,
    pub id_buf: String,
    pub password_buf: String,
    pub focus: CreateRoomField,
    pub error: Option<String>,
}

pub enum Screen {
    Picker(PickerView),
    Game(Box<GameView>),
    Net(Box<NetView>),
    HostPrompt(HostPromptView),
    Lobby(Box<LobbyView>),
    CreateRoom(CreateRoomView),
    /// Tree-style custom-rules sub-screen, reached from the picker.
    CustomRules(Box<CustomRulesView>),
}

pub struct AppState {
    pub screen: Screen,
    pub style: Style,
    pub use_color: bool,
    pub observer: Side,
    pub help_open: bool,
    pub rules_open: bool,
    /// User toggle: show the move-history panel in the sidebar.
    /// Default false (saves screen real estate); toggled by `H`.
    pub history_open: bool,
    /// User toggle: show the AI debug overlay in the sidebar (vs-AI
    /// only). Highlighted PV is overlaid on the board. Default false;
    /// toggled by `D`.
    pub debug_open: bool,
    /// AI debug panel cursor — index into `GameView::last_analysis.scored`.
    /// `,`/`.` move it (clamped). Determines which row's PV is rendered
    /// on the board. Reset to 0 (the chosen move) on every new analysis
    /// so the user starts on the AI's pick.
    pub debug_cursor: usize,
    /// Set by main.rs from the `--evalbar` CLI flag. When `true`, the
    /// app pushes a fresh win-rate sample after every move (running an
    /// extra `chess_ai::analyze` in PvP mode where the AI pump
    /// wouldn't otherwise fire), and the sidebar shows a live
    /// `紅 % • 黑 %` headline. Press `E` to toggle the in-sidebar
    /// ASCII trend chart on/off (`evalbar_open`). Off by default.
    pub evalbar_enabled: bool,
    /// User toggle: show the in-sidebar ASCII trend chart (Game mode,
    /// requires `evalbar_enabled`). Default false; toggled by `E`.
    /// The headline is shown regardless of this; only the chart is
    /// gated.
    pub evalbar_open: bool,
    /// True while the y/N quit-confirm dialog is shown. Set when the user
    /// presses 'q' / Ctrl-C during an in-progress game (status `Ongoing` and
    /// at least one move played). Picker / game-over `q` skip the prompt.
    pub quit_confirm_open: bool,
    /// True while the y/N resign-confirm dialog is shown.
    pub resign_confirm_open: bool,
    /// Rect of the board widget last drawn (terminal coords). Used for
    /// mouse-click hit-testing. ui.rs writes this each frame.
    pub board_rect: Option<RectPx>,
    pub should_quit: bool,
    /// User pref: render confetti + big banner on game end. Default true,
    /// disable via `--no-confetti`. Preserved across screen transitions.
    pub show_confetti: bool,
    /// User pref: render the "將軍 / CHECK" banner when the side-to-move's
    /// general is under attack. Default true, disable via `--no-check-banner`.
    /// Xiangqi-only — banqi never sets `in_check`.
    pub show_check_banner: bool,
    /// Active confetti burst. Spawned by `ui.rs` once the board area is
    /// known (so we can place particles in the board sub-rect); cleared
    /// when the burst expires.
    pub confetti_anim: Option<ConfettiAnim>,
    /// Set by `note_status_transition` when the game just ended and a
    /// confetti burst should fire on the next draw. `ui.rs` consumes the
    /// flag once the board rect is known.
    pub confetti_pending: bool,
    /// Last observed status of the local game (`Game` screen) — used to
    /// detect Ongoing → ended transitions for the confetti trigger.
    pub prev_status_local: Option<GameStatus>,
    /// Last observed status of the net game (`Net` screen) — same as
    /// `prev_status_local` but tracked separately because a Net rematch
    /// resets the status without dropping the screen.
    pub prev_status_net: Option<GameStatus>,
    /// User pref: sort order for the sidebar captured-pieces panel.
    /// Default `Time`, toggle with `g`. Preserved across screens.
    pub captured_sort: CapturedSort,
    /// User pref: which threat-highlight mode to render on the board.
    /// Default `NetLoss` ('被捉'); set via `--threat-mode`. Preserved
    /// across screen transitions like the FX prefs above. Same
    /// semantic as the web client's `Prefs::fx_threat_mode`.
    pub threat_mode: ThreatMode,
    /// User pref: when ON, treat 'piece selected' as a hover signal
    /// for the 'what-if' threat preview — ring any of the player's
    /// other pieces that would become newly vulnerable if the
    /// selected piece moves away. OFF by default; toggle via the
    /// `--threat-on-select` CLI flag (no in-game keybind yet — file
    /// in `TODO.md` if it gets requested). Mirrors the web's
    /// `Prefs::fx_threat_hover` semantically.
    pub threat_on_select: bool,
    /// User pref: highlight the from/to squares of the most recent
    /// move so you can see what was just played at a glance. ON by
    /// default; disable with `--no-last-move`. Mirrors the web's
    /// `Prefs::fx_last_move`. Reads `view.last_move` (engine-projected;
    /// `EndChain` is filtered out by the projection).
    pub show_last_move: bool,
}

/// Sort order for the sidebar "captured pieces" panel. `Time` keeps
/// the chronological order returned by `GameState::captured_pieces()`;
/// `Rank` re-sorts each side's row by piece value (largest first).
/// User toggles via `g` in `Game` mode and `--captured-sort` on the
/// CLI; preserved across screen transitions like the FX prefs.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub enum CapturedSort {
    #[default]
    Time,
    Rank,
}

impl CapturedSort {
    pub fn toggled(self) -> Self {
        match self {
            CapturedSort::Time => CapturedSort::Rank,
            CapturedSort::Rank => CapturedSort::Time,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CapturedSort::Time => "time",
            CapturedSort::Rank => "rank",
        }
    }
}

/// Threat-highlight mode for the board renderer. TUI mirror of the
/// web client's `crate::prefs::ThreatMode`; both share the same four
/// semantics. The TUI doesn't persist this across runs (no
/// localStorage equivalent without bringing in a config file) — set
/// it per-invocation with `--threat-mode`.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub enum ThreatMode {
    /// Off — no highlight rendered.
    Off,
    /// Mode A — every revealed piece an opponent could capture in
    /// one ply (the literal '被攻擊'). Visually busy mid-game.
    Attacked,
    /// Mode B — pieces whose Static Exchange Evaluation predicts
    /// material loss ('被捉'). The recommended default.
    #[default]
    NetLoss,
    /// Mode C — opponent piece-squares constituting a checkmate-in-1
    /// threat ('叫殺'). Xiangqi only.
    MateThreat,
}

/// Rank-ordering for `CapturedSort::Rank`. Larger value = stronger piece
/// (General > Advisor > Elephant > Chariot > Horse > Cannon > Soldier).
/// Used by both the TUI panel and a future shared client crate.
pub fn piece_rank_value(kind: PieceKind) -> u8 {
    match kind {
        PieceKind::General => 6,
        PieceKind::Advisor => 5,
        PieceKind::Elephant => 4,
        PieceKind::Chariot => 3,
        PieceKind::Horse => 2,
        PieceKind::Cannon => 1,
        PieceKind::Soldier => 0,
    }
}

/// Sort `pieces` (chronological order from the engine) into a per-side
/// view appropriate for the sidebar panel. Returns `(red, black)`.
pub fn split_and_sort_captured(pieces: &[Piece], sort: CapturedSort) -> (Vec<Piece>, Vec<Piece>) {
    let mut red: Vec<Piece> = pieces.iter().filter(|p| p.side == Side::RED).copied().collect();
    let mut black: Vec<Piece> = pieces.iter().filter(|p| p.side == Side::BLACK).copied().collect();
    if sort == CapturedSort::Rank {
        red.sort_by_key(|p| std::cmp::Reverse(piece_rank_value(p.kind)));
        black.sort_by_key(|p| std::cmp::Reverse(piece_rank_value(p.kind)));
    }
    (red, black)
}

/// Minimal Rect copy so app.rs doesn't depend on ratatui types directly.
#[derive(Copy, Clone, Debug)]
pub struct RectPx {
    pub x: u16,
    pub y: u16,
    /// Width in terminal cols of one cell (glyph + padding).
    pub cell_cols: u16,
    /// Height in terminal rows of one cell (rank row + between row).
    pub cell_rows: u16,
    /// Offset (cols) of the first cell's start from rect.x.
    pub left_pad: u16,
    /// Offset (rows) of the first row from rect.y.
    pub top_pad: u16,
}

impl AppState {
    pub fn new_picker(style: Style, use_color: bool, observer: Side) -> Self {
        Self {
            screen: Screen::Picker(PickerView { cursor: 0 }),
            style,
            use_color,
            observer,
            help_open: false,
            rules_open: false,
            quit_confirm_open: false,
            resign_confirm_open: false,
            board_rect: None,
            should_quit: false,
            ..Self::fx_defaults()
        }
    }

    /// Default values for the FX-related fields, factored out so each
    /// constructor can `..Self::fx_defaults()` instead of repeating five
    /// lines of init. Both `show_*` flags default to true; `main.rs`
    /// flips them back off when the user passes `--no-*`.
    fn fx_defaults() -> Self {
        // Construct a placeholder; only the FX fields are read via the
        // `..` syntax. The other fields are immediately overridden by the
        // caller, so picking arbitrary defaults here is fine.
        Self {
            screen: Screen::Picker(PickerView { cursor: 0 }),
            style: Style::Cjk,
            use_color: true,
            observer: Side::RED,
            help_open: false,
            rules_open: false,
            history_open: false,
            debug_open: false,
            debug_cursor: 0,
            evalbar_enabled: false,
            evalbar_open: false,
            quit_confirm_open: false,
            resign_confirm_open: false,
            board_rect: None,
            should_quit: false,
            show_confetti: true,
            show_check_banner: true,
            confetti_anim: None,
            confetti_pending: false,
            prev_status_local: None,
            prev_status_net: None,
            captured_sort: CapturedSort::Time,
            threat_mode: ThreatMode::default(),
            threat_on_select: false,
            show_last_move: true,
        }
    }

    pub fn new_game(rules: RuleSet, style: Style, use_color: bool, observer: Side) -> Self {
        Self::new_game_inner(rules, style, use_color, observer, None)
    }

    /// Local game vs the in-process [`chess_ai`] engine. Player gets the
    /// side opposite `cfg.ai_side`; board is rendered from the player's
    /// perspective (overrides the picker `--as` flag).
    ///
    /// If the AI plays Red it will move first — `apply_move` doesn't fire
    /// because no human move has happened yet, so we trigger one initial
    /// AI move synchronously here.
    pub fn new_game_vs_ai(rules: RuleSet, style: Style, use_color: bool, cfg: VsAiConfig) -> Self {
        // Render from the player's perspective regardless of the global
        // --as flag, so the player's pieces always sit on the bottom.
        let player_side = cfg.ai_side.opposite();
        let mut s = Self::new_game_inner(rules, style, use_color, player_side, Some(cfg));
        let variation_label = match cfg.randomness.and_then(|r| r.preset_name()) {
            Some(name) => format!(", variation={}", name),
            None => String::new(),
        };
        let depth_label = match cfg.depth {
            Some(d) => format!(", depth={}", d),
            None => String::new(),
        };
        let budget_label = match cfg.node_budget {
            Some(b) => format!(", budget={}", b),
            None => String::new(),
        };
        let label = format!(
            "vs AI ({}, engine={}{}{}{}). You play {}. Press ?/help for keys.",
            cfg.difficulty.as_str(),
            cfg.strategy.as_str(),
            variation_label,
            depth_label,
            budget_label,
            if player_side == Side::RED { "RED" } else { "BLACK" },
        );
        if let Screen::Game(g) = &mut s.screen {
            g.last_msg = Some(label);
            // If the AI plays Red, it moves first.
            if g.state.side_to_move == cfg.ai_side {
                Self::ai_reply(g, cfg);
            }
        }
        s
    }

    fn new_game_inner(
        rules: RuleSet,
        style: Style,
        use_color: bool,
        observer: Side,
        ai: Option<VsAiConfig>,
    ) -> Self {
        let state = GameState::new(rules);
        let shape = state.board.shape();
        let (rows, cols) = orient::display_dims(shape);
        let cursor = (rows / 2, cols / 2);
        Self {
            screen: Screen::Game(Box::new(GameView {
                state,
                cursor,
                selected: None,
                last_msg: Some(
                    "Welcome. Arrows/hjkl move cursor. Enter selects. r=rules, ?=help, n=new, q=quit."
                        .into(),
                ),
                coord_input: None,
                ai,
                last_analysis: None,
                eval_samples: Vec::new(),
            })),
            style,
            use_color,
            observer,
            help_open: false,
            rules_open: false,
            quit_confirm_open: false,
            resign_confirm_open: false,
            board_rect: None,
            should_quit: false,
            ..Self::fx_defaults()
        }
    }

    pub fn new_net(url: String, style: Style, use_color: bool) -> Self {
        let client = NetClient::spawn(url.clone());
        Self {
            screen: Screen::Net(Box::new(NetView {
                client,
                url,
                last_view: None,
                rules: None,
                role: None,
                cursor: (0, 0),
                selected: None,
                last_msg: Some("Connecting…".into()),
                connected: false,
                chat: VecDeque::with_capacity(CHAT_HISTORY_CAP),
                chat_input: None,
                coord_input: None,
                hints_allowed: None,
                last_analysis: None,
            })),
            style,
            use_color,
            // Pre-welcome, we render as Red until the server tells us our role.
            observer: Side::RED,
            help_open: false,
            rules_open: false,
            quit_confirm_open: false,
            resign_confirm_open: false,
            board_rect: None,
            should_quit: false,
            ..Self::fx_defaults()
        }
    }

    /// Skip the picker and land on the host-prompt screen so the user can
    /// type a server URL. Used by the `--lobby` flag with no host argument
    /// — currently main always passes a URL, so this is the safety net for
    /// future entrypoints (e.g. picker → "Connect to server…").
    pub fn new_host_prompt(style: Style, use_color: bool, observer: Side) -> Self {
        Self {
            screen: Screen::HostPrompt(HostPromptView { buf: "ws://".into(), error: None }),
            style,
            use_color,
            observer,
            help_open: false,
            rules_open: false,
            quit_confirm_open: false,
            resign_confirm_open: false,
            board_rect: None,
            should_quit: false,
            ..Self::fx_defaults()
        }
    }

    /// Open the lobby browser against `host` (e.g. `"ws://127.0.0.1:7878"`).
    /// `want_hints = true` makes every room-join URL emitted from this
    /// lobby append `?hints=1` (only effective when this user creates
    /// a fresh room — server first-write-wins).
    pub fn new_lobby(
        host: String,
        style: Style,
        use_color: bool,
        observer: Side,
        want_hints: bool,
    ) -> Self {
        let client = NetClient::spawn(format!("{host}/lobby"));
        Self {
            screen: Screen::Lobby(Box::new(LobbyView {
                client,
                host,
                rooms: Vec::new(),
                cursor: 0,
                last_msg: Some("Connecting to lobby…".into()),
                connected: false,
                pending_join: None,
                want_hints,
            })),
            style,
            use_color,
            observer,
            help_open: false,
            rules_open: false,
            quit_confirm_open: false,
            resign_confirm_open: false,
            board_rect: None,
            should_quit: false,
            ..Self::fx_defaults()
        }
    }

    /// Detect Ongoing → ended transitions and arm the confetti burst for the
    /// next draw. Called automatically at the end of `dispatch` (Local) and
    /// `tick_net` (Net); rematches that move from Won/Drawn → Ongoing are
    /// silent (we update the remembered status without firing).
    fn note_status_transition(&mut self, cur: Option<GameStatus>, is_net: bool) {
        let prev = if is_net { self.prev_status_net } else { self.prev_status_local };
        let was_ongoing = matches!(prev, Some(GameStatus::Ongoing));
        let now_ended =
            matches!(cur, Some(GameStatus::Won { .. }) | Some(GameStatus::Drawn { .. }));
        if was_ongoing && now_ended && self.show_confetti {
            self.confetti_pending = true;
        }
        if is_net {
            self.prev_status_net = cur;
        } else {
            self.prev_status_local = cur;
        }
    }

    fn current_local_status(&self) -> Option<GameStatus> {
        match &self.screen {
            Screen::Game(g) => Some(g.state.status),
            _ => None,
        }
    }

    fn current_net_status(&self) -> Option<GameStatus> {
        match &self.screen {
            Screen::Net(n) => n.last_view.as_ref().map(|v| v.status),
            _ => None,
        }
    }

    /// Replace `self` with `fresh` while preserving the user's FX prefs
    /// (`show_confetti`, `show_check_banner`). Constructors always default
    /// FX to ON; this helper carries the user's actual choices across screen
    /// transitions like `NewGame` → picker, lobby → game, etc. Tracking
    /// state (`prev_status_*`, active `confetti_anim`) is reset because the
    /// new screen is starting fresh.
    fn replace_preserving_prefs(&mut self, fresh: AppState) {
        let confetti = self.show_confetti;
        let check = self.show_check_banner;
        let captured_sort = self.captured_sort;
        let history_open = self.history_open;
        let debug_open = self.debug_open;
        let evalbar_enabled = self.evalbar_enabled;
        let evalbar_open = self.evalbar_open;
        let threat_mode = self.threat_mode;
        let threat_on_select = self.threat_on_select;
        let show_last_move = self.show_last_move;
        *self = fresh;
        self.show_confetti = confetti;
        self.show_check_banner = check;
        self.captured_sort = captured_sort;
        self.history_open = history_open;
        self.debug_open = debug_open;
        self.evalbar_enabled = evalbar_enabled;
        self.evalbar_open = evalbar_open;
        self.threat_mode = threat_mode;
        self.threat_on_select = threat_on_select;
        self.show_last_move = show_last_move;
        // debug_cursor is NOT preserved — fresh game = different scored
        // moves, so the cursor must reset.
    }

    /// Drain ws events from the worker thread(s) and apply them to the
    /// active `NetView` / `LobbyView`. Called once per main-loop tick
    /// (no-op outside Net / Lobby modes).
    pub fn tick_net(&mut self) {
        match &mut self.screen {
            Screen::Net(n) => {
                while let Ok(evt) = n.client.evt_rx.try_recv() {
                    apply_net_event(n, evt);
                }
            }
            Screen::Lobby(l) => {
                while let Ok(evt) = l.client.evt_rx.try_recv() {
                    apply_lobby_event(l, evt);
                }
            }
            _ => {}
        }
        // Server pushes can change the game outcome at any time; check after
        // every drained batch so the next draw sees the confetti flag.
        let cur = self.current_net_status();
        self.note_status_transition(cur, true);
    }

    /// Compute the input mode for the current screen so main.rs can drive
    /// `from_key` without poking at private state.
    pub fn input_mode(&self) -> InputMode {
        match &self.screen {
            Screen::Picker(_) => InputMode::Picker,
            Screen::Lobby(l) => {
                if l.pending_join.is_some() {
                    InputMode::Text
                } else {
                    InputMode::Lobby
                }
            }
            Screen::HostPrompt(_) | Screen::CreateRoom(_) => InputMode::Text,
            // While typing a chat line or a coord move, hijack the keymap so
            // printable chars append to the buffer instead of moving the cursor.
            Screen::Net(n) if n.chat_input.is_some() || n.coord_input.is_some() => InputMode::Text,
            Screen::Game(g) if g.coord_input.is_some() => InputMode::Text,
            Screen::Game(_) | Screen::Net(_) => InputMode::Game,
            Screen::CustomRules(_) => InputMode::CustomRules,
        }
    }

    pub fn dispatch(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::ConfirmYes => {
                if self.resign_confirm_open {
                    self.resign_confirm_open = false;
                    self.execute_resign();
                } else {
                    self.quit_confirm_open = false;
                    self.should_quit = true;
                }
            }
            Action::ConfirmNo => {
                self.quit_confirm_open = false;
                self.resign_confirm_open = false;
            }
            Action::Quit => {
                if self.is_game_in_progress() {
                    self.quit_confirm_open = true;
                } else {
                    self.should_quit = true;
                }
            }
            Action::HelpToggle => self.help_open = !self.help_open,
            Action::RulesToggle => self.rules_open = !self.rules_open,
            Action::HistoryToggle => {
                self.history_open = !self.history_open;
                let msg = format!(
                    "Move history panel: {}",
                    if self.history_open { "shown" } else { "hidden" }
                );
                match &mut self.screen {
                    Screen::Game(g) => g.last_msg = Some(msg),
                    Screen::Net(n) => n.last_msg = Some(msg),
                    _ => {}
                }
            }
            Action::DebugToggle => {
                self.debug_open = !self.debug_open;
                let has_analysis =
                    matches!(&self.screen, Screen::Game(g) if g.last_analysis.is_some());
                // Net-mode message accounts for hints permission and the
                // (current) lack of in-TUI analysis. Web client has the
                // full panel — nudge users there until TUI catches up.
                let net_hints =
                    if let Screen::Net(n) = &self.screen { n.hints_allowed } else { None };
                let msg = if !self.debug_open {
                    "AI debug panel: hidden".to_string()
                } else if matches!(&self.screen, Screen::Net(_)) {
                    match net_hints {
                        None => "AI debug panel: waiting for room welcome".into(),
                        Some(false) => {
                            "AI debug panel: this room has hints disabled (creator must set ?hints=1)"
                                .into()
                        }
                        Some(true) => {
                            "AI debug panel: hints sanctioned for this room — use the web client for the full panel"
                                .into()
                        }
                    }
                } else if has_analysis {
                    "AI debug panel: shown — ',' / '.' to navigate scored moves; PV overlays board"
                        .to_string()
                } else {
                    "AI debug panel: shown — waiting for AI's first move".to_string()
                };
                match &mut self.screen {
                    Screen::Game(g) => g.last_msg = Some(msg),
                    Screen::Net(n) => n.last_msg = Some(msg),
                    _ => {}
                }
            }
            Action::DebugCursorUp => {
                if self.debug_open {
                    self.debug_cursor = self.debug_cursor.saturating_sub(1);
                }
            }
            Action::EvalbarToggle => {
                if !self.evalbar_enabled {
                    let msg = "Win-rate panel: not enabled (relaunch with --evalbar)".to_string();
                    match &mut self.screen {
                        Screen::Game(g) => g.last_msg = Some(msg),
                        Screen::Net(n) => n.last_msg = Some(msg),
                        _ => {}
                    }
                } else {
                    self.evalbar_open = !self.evalbar_open;
                    let has_samples =
                        matches!(&self.screen, Screen::Game(g) if !g.eval_samples.is_empty());
                    let msg = match (self.evalbar_open, has_samples) {
                        (false, _) => "Win-rate panel: chart hidden (headline still shown)".into(),
                        (true, true) => {
                            "Win-rate panel: chart shown — every recorded ply plotted".into()
                        }
                        (true, false) => {
                            "Win-rate panel: chart shown — waiting for first sample".into()
                        }
                    };
                    match &mut self.screen {
                        Screen::Game(g) => g.last_msg = Some(msg),
                        Screen::Net(n) => n.last_msg = Some(msg),
                        _ => {}
                    }
                }
            }
            Action::DebugCursorDown => {
                if self.debug_open {
                    let max = match &self.screen {
                        Screen::Game(g) => {
                            g.last_analysis.as_ref().map(|a| a.scored.len()).unwrap_or(0)
                        }
                        _ => 0,
                    };
                    if max > 0 {
                        self.debug_cursor = (self.debug_cursor + 1).min(max - 1);
                    }
                }
            }
            Action::CapturedSortToggle => {
                self.captured_sort = self.captured_sort.toggled();
                let msg = format!("Captured sort: {}", self.captured_sort.label());
                match &mut self.screen {
                    Screen::Game(g) => g.last_msg = Some(msg),
                    Screen::Net(n) => n.last_msg = Some(msg),
                    _ => {}
                }
            }
            Action::Resign => self.dispatch_resign(),
            Action::NewGame => {
                if matches!(self.screen, Screen::Net(_)) {
                    // In Net mode, 'n' requests a rematch via the server
                    // instead of dropping the connection. Game must be over.
                    if let Screen::Net(n) = &mut self.screen {
                        if !n.role.map(|r| r.is_player()).unwrap_or(false) {
                            n.last_msg = Some("Spectators cannot request a rematch.".into());
                            return;
                        }
                        let status = n.last_view.as_ref().map(|v| v.status);
                        match status {
                            Some(GameStatus::Won { .. }) | Some(GameStatus::Drawn { .. }) => {
                                let _ = n.client.cmd_tx.send(ClientMsg::Rematch);
                                n.last_msg =
                                    Some("Rematch requested. Waiting for opponent…".into());
                            }
                            Some(GameStatus::Ongoing) => {
                                n.last_msg = Some(
                                    "'n' requests a rematch only after the game is over.".into(),
                                );
                            }
                            None => {
                                n.last_msg = Some("Not connected yet.".into());
                            }
                        }
                    }
                } else {
                    let style = self.style;
                    let use_color = self.use_color;
                    let observer = self.observer;
                    self.replace_preserving_prefs(AppState::new_picker(style, use_color, observer));
                }
            }
            Action::Back => self.dispatch_back(),
            Action::PickerUp | Action::PickerDown | Action::PickerSelect => match &self.screen {
                Screen::Picker(_) => self.dispatch_picker(action),
                Screen::Lobby(_) => self.dispatch_lobby(action),
                Screen::CustomRules(_) => self.dispatch_custom_rules(action),
                _ => {}
            },
            Action::LobbyCreate | Action::LobbyRefresh => {
                if matches!(self.screen, Screen::Lobby(_)) {
                    self.dispatch_lobby(action);
                }
            }
            Action::TextInput(_)
            | Action::TextBackspace
            | Action::FocusNext
            | Action::FocusPrev
            | Action::Submit => self.dispatch_text(action),
            Action::ChatStart => self.dispatch_chat_start(),
            Action::CoordStart(kind) => self.dispatch_coord_start(kind),
            Action::LobbyWatch => {
                if matches!(self.screen, Screen::Lobby(_)) {
                    self.dispatch_lobby_watch();
                }
            }
            _ => match &self.screen {
                Screen::Net(_) => self.dispatch_net(action),
                Screen::Game(_) => self.dispatch_game(action),
                Screen::Picker(_)
                | Screen::HostPrompt(_)
                | Screen::Lobby(_)
                | Screen::CreateRoom(_)
                | Screen::CustomRules(_) => {}
            },
        }
        // Local moves go through dispatch_game / dispatch_coord_*, both of
        // which can land us on a terminal status. Re-check after every
        // dispatch so the confetti trigger fires reliably regardless of the
        // entry path. Net status is checked separately in `tick_net`.
        let cur = self.current_local_status();
        self.note_status_transition(cur, false);
    }

    fn dispatch_back(&mut self) {
        let style = self.style;
        let use_color = self.use_color;
        let observer = self.observer;
        match &mut self.screen {
            Screen::HostPrompt(_) => {
                self.replace_preserving_prefs(AppState::new_picker(style, use_color, observer));
            }
            Screen::CustomRules(c) => {
                let picker_cursor = c.picker_cursor;
                let mut fresh = AppState::new_picker(style, use_color, observer);
                if let Screen::Picker(p) = &mut fresh.screen {
                    p.cursor = picker_cursor.min(PickerEntry::ALL.len() - 1);
                }
                self.replace_preserving_prefs(fresh);
            }
            Screen::Lobby(l) => {
                if l.pending_join.is_some() {
                    l.pending_join = None;
                    l.last_msg = None;
                    return;
                }
                self.replace_preserving_prefs(AppState::new_picker(style, use_color, observer));
            }
            Screen::CreateRoom(c) => {
                let host = c.host.clone();
                self.replace_preserving_prefs(AppState::new_lobby(
                    host, style, use_color, observer, /*want_hints=*/ false,
                ));
            }
            // Esc inside Game / Net while a coord-input prompt is open: close
            // the prompt and (Live mode only) restore the snapshotted cursor +
            // selected highlight that existed before the prompt was opened.
            Screen::Game(g) if g.coord_input.is_some() => {
                let ci = g.coord_input.take().expect("guarded by is_some");
                if let Some((cur, sel)) = ci.snapshot {
                    g.cursor = cur;
                    g.selected = sel;
                }
                g.last_msg = None;
            }
            Screen::Net(n) if n.coord_input.is_some() => {
                let ci = n.coord_input.take().expect("guarded by is_some");
                if let Some((cur, sel)) = ci.snapshot {
                    n.cursor = cur;
                    n.selected = sel;
                }
                n.last_msg = None;
            }
            // Esc inside Net mode — if we're typing a chat line, cancel the
            // input rather than navigating away.
            Screen::Net(n) if n.chat_input.is_some() => {
                n.chat_input = None;
                n.last_msg = None;
            }
            _ => {}
        }
    }

    fn dispatch_chat_start(&mut self) {
        let Screen::Net(n) = &mut self.screen else {
            return;
        };
        if n.coord_input.is_some() {
            n.last_msg = Some("Finish or cancel coord-input (Esc) before chat.".into());
            return;
        }
        match n.role {
            Some(NetRole::Player(_)) => {
                n.chat_input = Some(String::new());
                n.last_msg = Some("Chat: type a line, Enter sends, Esc cancels.".into());
            }
            Some(NetRole::Spectator) => {
                n.last_msg = Some("Spectators can read but not chat.".into());
            }
            None => {
                n.last_msg = Some("Not connected yet.".into());
            }
        }
    }

    fn dispatch_coord_start(&mut self, kind: CoordKind) {
        let msg = coord_help_msg(kind);
        match &mut self.screen {
            Screen::Game(g) => {
                let snapshot = matches!(kind, CoordKind::Live).then_some((g.cursor, g.selected));
                if matches!(kind, CoordKind::Live) {
                    g.selected = None;
                }
                g.coord_input = Some(CoordInputState { kind, buf: String::new(), snapshot });
                g.last_msg = Some(msg);
            }
            Screen::Net(n) => {
                if n.chat_input.is_some() {
                    n.last_msg = Some("Finish or cancel chat (Esc) before move-input.".into());
                    return;
                }
                match n.role {
                    Some(NetRole::Player(_)) => {}
                    Some(NetRole::Spectator) => {
                        n.last_msg = Some("Spectators cannot move.".into());
                        return;
                    }
                    None => {
                        n.last_msg = Some("Not connected yet.".into());
                        return;
                    }
                }
                let snapshot = matches!(kind, CoordKind::Live).then_some((n.cursor, n.selected));
                if matches!(kind, CoordKind::Live) {
                    n.selected = None;
                }
                n.coord_input = Some(CoordInputState { kind, buf: String::new(), snapshot });
                n.last_msg = Some(msg);
            }
            _ => {}
        }
    }

    fn dispatch_lobby_watch(&mut self) {
        let Screen::Lobby(l) = &mut self.screen else {
            return;
        };
        if l.pending_join.is_some() || l.rooms.is_empty() {
            return;
        }
        let cursor = l.cursor.min(l.rooms.len() - 1);
        let room = l.rooms[cursor].clone();
        if room.has_password {
            // Spectator joins to locked rooms still need the password.
            l.pending_join = Some(PendingJoin {
                room_id: room.id,
                password_buf: String::new(),
                as_spectator: true,
            });
            l.last_msg = Some("Type password to spectate, Enter to join, Esc to cancel.".into());
            return;
        }
        let host = l.host.clone();
        let want_hints = l.want_hints;
        let style = self.style;
        let use_color = self.use_color;
        let mut url = format!("{host}/ws/{}?role=spectator", room.id);
        if want_hints {
            url.push_str("&hints=1");
        }
        self.replace_preserving_prefs(AppState::new_net(url, style, use_color));
    }

    fn is_game_in_progress(&self) -> bool {
        match &self.screen {
            Screen::Game(g) => {
                matches!(g.state.status, GameStatus::Ongoing) && !g.state.history.is_empty()
            }
            Screen::Net(n) => match &n.last_view {
                Some(view) => matches!(view.status, GameStatus::Ongoing) && n.connected,
                None => false,
            },
            Screen::Picker(_)
            | Screen::HostPrompt(_)
            | Screen::Lobby(_)
            | Screen::CreateRoom(_)
            | Screen::CustomRules(_) => false,
        }
    }

    fn dispatch_resign(&mut self) {
        match &self.screen {
            Screen::Game(g) => {
                if !matches!(g.state.status, GameStatus::Ongoing) {
                    return;
                }
                self.resign_confirm_open = true;
            }
            Screen::Net(n) => {
                let is_player = n.role.map(|r| r.is_player()).unwrap_or(false);
                let is_ongoing =
                    n.last_view.as_ref().is_some_and(|v| matches!(v.status, GameStatus::Ongoing));
                if !is_player || !is_ongoing {
                    if let Screen::Net(n) = &mut self.screen {
                        n.last_msg = if !is_player {
                            Some("Spectators cannot resign.".into())
                        } else {
                            Some("No game in progress.".into())
                        };
                    }
                    return;
                }
                self.resign_confirm_open = true;
            }
            _ => {}
        }
    }

    fn execute_resign(&mut self) {
        let evalbar_enabled = self.evalbar_enabled;
        match &mut self.screen {
            Screen::Game(g) => {
                let winner = g.state.side_to_move.opposite();
                g.state.status = GameStatus::Won { winner, reason: WinReason::Resignation };
                // Final win-rate sample so the headline / chart jump
                // to the actual outcome (mirrors the apply_move
                // post-move hook). Without this the panel would stay
                // frozen at whatever the pre-resign analysis showed —
                // often misleading since players resign worse-than-
                // they-look positions all the time.
                if evalbar_enabled {
                    Self::record_eval_final(g);
                }
            }
            Screen::Net(n) => {
                let _ = n.client.cmd_tx.send(ClientMsg::Resign);
            }
            _ => {}
        }
    }

    fn dispatch_picker(&mut self, action: Action) {
        let Screen::Picker(p) = &mut self.screen else {
            return;
        };
        let n = PickerEntry::ALL.len();
        match action {
            Action::PickerUp => p.cursor = (p.cursor + n - 1) % n,
            Action::PickerDown => p.cursor = (p.cursor + 1) % n,
            Action::PickerSelect => {
                let entry = PickerEntry::ALL[p.cursor];
                let picker_cursor = p.cursor;
                let observer = self.observer;
                let style = self.style;
                let use_color = self.use_color;
                match entry {
                    PickerEntry::ConnectToServer => {
                        self.replace_preserving_prefs(AppState::new_host_prompt(
                            style, use_color, observer,
                        ));
                    }
                    PickerEntry::CustomXiangqi => {
                        self.screen = Screen::CustomRules(Box::new(CustomRulesView::new_xiangqi(
                            picker_cursor,
                        )));
                    }
                    PickerEntry::CustomBanqi => {
                        self.screen = Screen::CustomRules(Box::new(CustomRulesView::new_banqi(
                            picker_cursor,
                        )));
                    }
                    PickerEntry::Quit => self.should_quit = true,
                    PickerEntry::XiangqiVsAi => {
                        self.replace_preserving_prefs(AppState::new_game_vs_ai(
                            RuleSet::xiangqi_casual(),
                            style,
                            use_color,
                            VsAiConfig::default(),
                        ));
                    }
                    other => {
                        if let Some(rules) = other.rules() {
                            self.replace_preserving_prefs(AppState::new_game(
                                rules, style, use_color, observer,
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn dispatch_custom_rules(&mut self, action: Action) {
        // Up / Down navigate the cursor; Activate (Enter / Space) acts on the
        // current row. Seed editing arrives via `dispatch_text` because the
        // input mode routes printable digits there.
        let Screen::CustomRules(c) = &mut self.screen else {
            return;
        };
        let items = c.items();
        if items.is_empty() {
            return;
        }
        match action {
            Action::PickerUp => {
                c.cursor = (c.cursor + items.len() - 1) % items.len();
                c.last_msg = None;
            }
            Action::PickerDown => {
                c.cursor = (c.cursor + 1) % items.len();
                c.last_msg = None;
            }
            Action::PickerSelect => {
                let cur = c.cursor.min(items.len() - 1);
                let item = items[cur];
                match item {
                    CustomRulesItem::XiangqiPreset(strict) => {
                        c.xiangqi_strict = strict;
                        c.last_msg = None;
                    }
                    CustomRulesItem::BanqiPresetItem(p) => {
                        c.banqi_preset = p;
                        if let Some(flags) = p.flags() {
                            c.banqi_flags = flags;
                        }
                        c.last_msg = None;
                    }
                    CustomRulesItem::BanqiFlagItem(flag) => {
                        c.banqi_flags.toggle(flag);
                        c.banqi_flags = chess_core::rules::house::normalize(c.banqi_flags);
                        c.banqi_preset = BanqiPreset::from_flags(c.banqi_flags);
                        c.last_msg = None;
                    }
                    CustomRulesItem::BanqiSeed => {
                        // No-op: seed edits arrive via TextInput. Ignore Enter/Space
                        // so the user doesn't accidentally jump screens.
                    }
                    CustomRulesItem::Start => match c.to_rule_set() {
                        Ok(rules) => {
                            let style = self.style;
                            let use_color = self.use_color;
                            let observer = self.observer;
                            self.replace_preserving_prefs(AppState::new_game(
                                rules, style, use_color, observer,
                            ));
                        }
                        Err(msg) => {
                            c.last_msg = Some(msg);
                        }
                    },
                }
            }
            _ => {}
        }
    }

    fn dispatch_lobby(&mut self, action: Action) {
        let Screen::Lobby(l) = &mut self.screen else {
            return;
        };
        // Pending password prompt eats list-cursor inputs.
        if l.pending_join.is_some() {
            return;
        }
        match action {
            Action::PickerUp if !l.rooms.is_empty() => {
                l.cursor = (l.cursor + l.rooms.len() - 1) % l.rooms.len();
            }
            Action::PickerDown if !l.rooms.is_empty() => {
                l.cursor = (l.cursor + 1) % l.rooms.len();
            }
            Action::PickerSelect => {
                if l.rooms.is_empty() {
                    l.last_msg =
                        Some("No rooms yet. Press 'c' to create one, or 'r' to refresh.".into());
                    return;
                }
                let cursor = l.cursor.min(l.rooms.len() - 1);
                let room = l.rooms[cursor].clone();
                if room.seats >= 2 {
                    l.last_msg = Some(format!("Room '{}' is full (2/2).", room.id));
                    return;
                }
                if room.has_password {
                    l.pending_join = Some(PendingJoin {
                        room_id: room.id,
                        password_buf: String::new(),
                        as_spectator: false,
                    });
                    l.last_msg = Some("Type password, Enter to join, Esc to cancel.".into());
                    return;
                }
                let host = l.host.clone();
                let want_hints = l.want_hints;
                let style = self.style;
                let use_color = self.use_color;
                let url = if want_hints {
                    format!("{host}/ws/{}?hints=1", room.id)
                } else {
                    format!("{host}/ws/{}", room.id)
                };
                self.replace_preserving_prefs(AppState::new_net(url, style, use_color));
            }
            Action::LobbyCreate => {
                let host = l.host.clone();
                self.screen = Screen::CreateRoom(CreateRoomView {
                    host,
                    id_buf: String::new(),
                    password_buf: String::new(),
                    focus: CreateRoomField::Id,
                    error: None,
                });
            }
            Action::LobbyRefresh => {
                let _ = l.client.cmd_tx.send(ClientMsg::ListRooms);
                l.last_msg = Some("Refresh requested.".into());
            }
            _ => {}
        }
    }

    fn dispatch_text(&mut self, action: Action) {
        match &mut self.screen {
            Screen::HostPrompt(h) => match action {
                Action::TextInput(c) => text_input::push_char(&mut h.buf, c, 128),
                Action::TextBackspace => text_input::backspace(&mut h.buf),
                Action::Submit => {
                    let raw = h.buf.trim().to_string();
                    let host = match normalize_host_url(&raw) {
                        Ok(u) => u,
                        Err(e) => {
                            h.error = Some(e);
                            return;
                        }
                    };
                    let style = self.style;
                    let use_color = self.use_color;
                    let observer = self.observer;
                    self.replace_preserving_prefs(AppState::new_lobby(
                        host, style, use_color, observer, /*want_hints=*/ false,
                    ));
                }
                _ => {}
            },
            Screen::CreateRoom(c) => match action {
                Action::TextInput(ch) => match c.focus {
                    CreateRoomField::Id => text_input::push_char(&mut c.id_buf, ch, 32),
                    CreateRoomField::Password => text_input::push_char(&mut c.password_buf, ch, 64),
                    CreateRoomField::Submit => {
                        if matches!(ch, ' ' | '\n') {
                            self.dispatch_text(Action::Submit);
                        }
                    }
                },
                Action::TextBackspace => match c.focus {
                    CreateRoomField::Id => text_input::backspace(&mut c.id_buf),
                    CreateRoomField::Password => text_input::backspace(&mut c.password_buf),
                    _ => {}
                },
                Action::FocusNext => {
                    c.focus = match c.focus {
                        CreateRoomField::Id => CreateRoomField::Password,
                        CreateRoomField::Password => CreateRoomField::Submit,
                        CreateRoomField::Submit => CreateRoomField::Id,
                    };
                }
                Action::FocusPrev => {
                    c.focus = match c.focus {
                        CreateRoomField::Id => CreateRoomField::Submit,
                        CreateRoomField::Password => CreateRoomField::Id,
                        CreateRoomField::Submit => CreateRoomField::Password,
                    };
                }
                Action::Submit => {
                    let id = c.id_buf.trim().to_string();
                    if !valid_room_id(&id) {
                        c.error = Some("Room id must be 1–32 chars of [a-zA-Z0-9_-].".into());
                        return;
                    }
                    let host = c.host.clone();
                    let password =
                        if c.password_buf.is_empty() { None } else { Some(c.password_buf.clone()) };
                    let url = match password {
                        Some(pw) => format!("{host}/ws/{id}?password={}", urlencode(&pw)),
                        None => format!("{host}/ws/{id}"),
                    };
                    // CreateRoom is reached from the lobby — no straight-
                    // forward way to read `LobbyView.want_hints` here
                    // (we've already transitioned to `CreateRoom`). For
                    // now CreateRoom doesn't carry the flag; users who
                    // want a hint-sanctioned room should join via the
                    // lobby's `j` (which respects `--hints`) or pass
                    // `--connect ws://.../ws/<id>?hints=1` directly.
                    let style = self.style;
                    let use_color = self.use_color;
                    self.replace_preserving_prefs(AppState::new_net(url, style, use_color));
                }
                _ => {}
            },
            Screen::Lobby(l) => {
                let Some(pj) = l.pending_join.as_mut() else {
                    return;
                };
                match action {
                    Action::TextInput(c) => text_input::push_char(&mut pj.password_buf, c, 64),
                    Action::TextBackspace => text_input::backspace(&mut pj.password_buf),
                    Action::Submit => {
                        let host = l.host.clone();
                        let want_hints = l.want_hints;
                        let pj_owned = l.pending_join.take().unwrap();
                        let mut url = format!(
                            "{host}/ws/{}?password={}",
                            pj_owned.room_id,
                            urlencode(&pj_owned.password_buf)
                        );
                        if pj_owned.as_spectator {
                            url.push_str("&role=spectator");
                        }
                        if want_hints {
                            url.push_str("&hints=1");
                        }
                        let style = self.style;
                        let use_color = self.use_color;
                        self.replace_preserving_prefs(AppState::new_net(url, style, use_color));
                    }
                    _ => {}
                }
            }
            // Custom-rules screen: digits / Backspace edit the seed buffer
            // when the cursor is on the seed row. Other text input is a
            // no-op so spurious keys don't bleed into the seed.
            Screen::CustomRules(c) => {
                let items = c.items();
                let cur = c.cursor.min(items.len().saturating_sub(1));
                if matches!(items.get(cur), Some(CustomRulesItem::BanqiSeed)) {
                    match action {
                        Action::TextInput(ch) if ch.is_ascii_digit() => {
                            text_input::push_char(&mut c.banqi_seed, ch, 20);
                            c.last_msg = None;
                        }
                        Action::TextBackspace => {
                            text_input::backspace(&mut c.banqi_seed);
                            c.last_msg = None;
                        }
                        _ => {}
                    }
                }
            }
            // Game mode hijacks Text input while the user is typing a coord
            // move (`:` instant or `m` live preview). Submit applies the move;
            // Esc / Back closes (and Live mode restores the snapshot).
            Screen::Game(g) if g.coord_input.is_some() => {
                let observer = self.observer;
                let evalbar_enabled = self.evalbar_enabled;
                Self::dispatch_coord_text_game(g, action, observer, evalbar_enabled);
            }
            // Net mode: coord-input takes priority over chat-input (mutual
            // exclusion is enforced at start time), so check coord first.
            Screen::Net(n) => {
                if n.coord_input.is_some() {
                    dispatch_coord_text_net(n, action);
                } else if n.chat_input.is_some() {
                    let buf = n.chat_input.as_mut().expect("guarded by is_some");
                    match action {
                        Action::TextInput(c) => text_input::push_char(buf, c, CHAT_INPUT_MAX),
                        Action::TextBackspace => text_input::backspace(buf),
                        Action::Submit => {
                            let buf = n.chat_input.take().unwrap_or_default();
                            let trimmed = buf.trim();
                            if trimmed.is_empty() {
                                n.last_msg = Some("Empty chat — nothing sent.".into());
                                return;
                            }
                            let _ =
                                n.client.cmd_tx.send(ClientMsg::Chat { text: trimmed.to_string() });
                            n.last_msg = Some("Chat sent.".into());
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn dispatch_coord_text_game(
        g: &mut GameView,
        action: Action,
        observer: Side,
        evalbar_enabled: bool,
    ) {
        // Pull `kind` and the buf-edit decisions out before we re-borrow `g`
        // for the live-preview / submit path.
        let Some(ci) = g.coord_input.as_mut() else {
            return;
        };
        let kind = ci.kind;
        match action {
            Action::TextInput(c) => {
                text_input::push_char(&mut ci.buf, c, COORD_INPUT_MAX);
                if matches!(kind, CoordKind::Live) {
                    live_preview_game(g, observer);
                }
            }
            Action::TextBackspace => {
                text_input::backspace(&mut ci.buf);
                if matches!(kind, CoordKind::Live) {
                    live_preview_game(g, observer);
                }
            }
            Action::Submit => {
                let buf = std::mem::take(&mut ci.buf);
                match chess_core::notation::iccs::decode_move(&g.state, buf.trim()) {
                    Ok(mv) => {
                        g.coord_input = None;
                        Self::apply_move(g, mv, evalbar_enabled);
                    }
                    Err(e) => {
                        if let Some(ci) = g.coord_input.as_mut() {
                            ci.buf = buf;
                        }
                        g.last_msg = Some(format!("Bad move: {e}"));
                    }
                }
            }
            _ => {}
        }
    }

    fn dispatch_game(&mut self, action: Action) {
        // Snapshot AppState fields before borrowing self.screen so the
        // helper calls below (apply_move, etc.) can dispatch on them
        // without re-borrowing self.
        let evalbar_enabled = self.evalbar_enabled;
        let Screen::Game(g) = &mut self.screen else {
            return;
        };
        let shape = g.state.board.shape();
        let (rows, cols) = orient::display_dims(shape);
        match action {
            Action::CursorUp if g.cursor.0 > 0 => {
                g.cursor.0 -= 1;
            }
            Action::CursorDown if g.cursor.0 + 1 < rows => {
                g.cursor.0 += 1;
            }
            Action::CursorLeft if g.cursor.1 > 0 => {
                g.cursor.1 -= 1;
            }
            Action::CursorRight if g.cursor.1 + 1 < cols => {
                g.cursor.1 += 1;
            }
            Action::Cancel => {
                if let Some(at) = g.state.chain_lock {
                    // Engine is in 連吃 mode — Esc ends the chain.
                    Self::apply_move(g, Move::EndChain { at }, evalbar_enabled);
                } else {
                    g.selected = None;
                    g.last_msg = None;
                }
            }
            Action::SelectOrCommit => {
                let observer = self.observer;
                Self::handle_select_or_commit(g, observer, evalbar_enabled);
            }
            Action::Undo => {
                // vs-AI: undo two plies so the human is back on move.
                // (Undoing only one ply would put the AI on the move and
                // the apply_move pump would immediately re-play.) Mirrors
                // the web `on_undo` rule. Falls back to a single undo
                // when only one ply is on the stack — the typical
                // "AI moved first, player hasn't moved yet" case.
                let plies = if g.ai.is_some() { 2 } else { 1 };
                let mut undone = 0;
                let mut last_err: Option<String> = None;
                for _ in 0..plies {
                    match g.state.unmake_move() {
                        Ok(()) => undone += 1,
                        Err(e) => {
                            last_err = Some(format!("{e}"));
                            break;
                        }
                    }
                }
                if undone > 0 {
                    g.state.refresh_status();
                    g.selected = None;
                    // Undoing the AI's move invalidates the analysis it
                    // produced (the position changed). Clear so the
                    // debug panel doesn't show stale data.
                    g.last_analysis = None;
                    // Same reasoning for the win-rate samples — drop
                    // any sample whose ply > the new history length.
                    let new_len = g.state.history.len();
                    g.eval_samples.retain(|s| s.ply <= new_len);
                    g.last_msg =
                        Some(format!("Undone {} ply. {:?} to move.", undone, g.state.side_to_move));
                } else if let Some(e) = last_err {
                    g.last_msg = Some(format!("Cannot undo: {e}"));
                }
            }
            Action::Flip => {
                if !matches!(g.state.status, chess_core::state::GameStatus::Ongoing) {
                    g.last_msg = Some(
                        "Game over. Press 'n' for new game, 'u' to take back, 'q' to quit.".into(),
                    );
                    return;
                }
                let observer = self.observer;
                let Some(sq) = orient::square_at_display(g.cursor.0, g.cursor.1, observer, shape)
                else {
                    g.last_msg = Some("Cursor not on a playable square.".into());
                    return;
                };
                let m = Move::Reveal { at: sq, revealed: None };
                Self::apply_move(g, m, evalbar_enabled);
            }
            Action::Click { term_col, term_row } => {
                if let Some(rect) = self.board_rect {
                    let observer = self.observer;
                    if let Some((row, col)) = hit_test(rect, term_col, term_row, rows, cols) {
                        g.cursor = (row, col);
                        Self::handle_select_or_commit(g, observer, evalbar_enabled);
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_select_or_commit(g: &mut GameView, observer: Side, evalbar_enabled: bool) {
        if !matches!(g.state.status, chess_core::state::GameStatus::Ongoing) {
            g.last_msg =
                Some("Game over. Press 'n' for new game, 'u' to take back, 'q' to quit.".into());
            return;
        }
        let shape = g.state.board.shape();
        let Some(sq) = orient::square_at_display(g.cursor.0, g.cursor.1, observer, shape) else {
            g.last_msg = Some("Cursor not on a playable square.".into());
            return;
        };

        let view = PlayerView::project(&g.state, g.state.side_to_move);

        // Engine chain mode: only the locked piece may move (captures
        // only). Press Enter on the locked piece to end the chain.
        if let Some(locked) = view.chain_lock {
            if sq == locked {
                Self::apply_move(g, Move::EndChain { at: locked }, evalbar_enabled);
                return;
            }
            let candidate = view
                .legal_moves
                .iter()
                .find(|m| m.origin_square() == locked && m.to_square() == Some(sq))
                .cloned();
            match candidate {
                Some(m) => Self::apply_move(g, m, evalbar_enabled),
                None => {
                    g.last_msg = Some(
                        "連吃 active — capture from the locked piece, or press Enter on it to end."
                            .into(),
                    )
                }
            }
            return;
        }

        match g.selected {
            None => {
                // Try to select a piece. Allowed if there's a legal move from this square.
                let any = view.legal_moves.iter().any(|m| m.origin_square() == sq);
                if any {
                    g.selected = Some(sq);
                    g.last_msg = None;
                } else {
                    // Maybe it's a banqi hidden cell — suggest 'f' to flip.
                    match g.state.board.get(sq) {
                        Some(p) if !p.revealed => {
                            g.last_msg = Some("Hidden piece. Press 'f' to flip.".into());
                        }
                        _ => g.last_msg = Some("No legal move from that square.".into()),
                    }
                }
            }
            Some(from) if from == sq => {
                g.selected = None;
                g.last_msg = None;
            }
            Some(from) => {
                let candidate = view
                    .legal_moves
                    .iter()
                    .find(|m| m.origin_square() == from && m.to_square() == Some(sq))
                    .cloned();
                match candidate {
                    Some(m) => Self::apply_move(g, m, evalbar_enabled),
                    None => {
                        g.last_msg = Some("Illegal move.".into());
                        g.selected = None;
                    }
                }
            }
        }
    }

    /// Apply a human move and (in vs-AI mode) immediately reply with
    /// the AI's move. When `evalbar_enabled` AND there's no AI in this
    /// game (PvP), runs an extra `chess_ai::analyze` after the human's
    /// move so the win-rate panel has fresh data — see
    /// [`apply_move_with_pvp_eval`][Self::apply_move_with_pvp_eval] for
    /// the cost discussion.
    fn apply_move(g: &mut GameView, m: Move, evalbar_enabled: bool) {
        match g.state.make_move(&m) {
            Ok(()) => {
                g.state.refresh_status();
                g.selected = None;
                g.last_msg = Some(format!(
                    "Played: {}",
                    chess_core::notation::iccs::encode_move(&g.state.board, &m)
                ));
            }
            Err(e) => {
                g.last_msg = Some(format!("Engine rejected move: {e}"));
                g.selected = None;
                return;
            }
        }
        // vs-AI mode: if it's now the AI's turn, reply synchronously
        // before returning to the event loop. Native search at depth ≤ 4
        // typically completes in <100 ms; the user-perceptible pause is
        // acceptable. Web-Worker / async pump is a v5 follow-up.
        if let Some(cfg) = g.ai {
            if matches!(g.state.status, GameStatus::Ongoing) && g.state.side_to_move == cfg.ai_side
            {
                Self::ai_reply(g, cfg);
            }
        } else if evalbar_enabled && matches!(g.state.status, GameStatus::Ongoing) {
            // PvP mode + evalbar on: run a side-effect-only analyze so
            // the win-rate panel keeps growing every ply. vs-AI mode
            // (`g.ai = Some`) gets samples for free from `ai_reply`'s
            // own analysis output, so we skip the extra search there.
            Self::run_pvp_eval(g);
        }
        // Game-end definitive sample. Mirrors chess-web's
        // `pages/local.rs` Ongoing→ended effect: the per-move sample
        // writers above bail when the game ends, so the last recorded
        // sample is from the *previous* position. Push a 100/0 (or
        // 50/50 for draws) sample so the headline / chart jump to the
        // real outcome instead of staying frozen at the AI's pre-loss
        // optimism. Idempotent via `record_eval_final`'s ply-dedup.
        if evalbar_enabled && !matches!(g.state.status, GameStatus::Ongoing) {
            Self::record_eval_final(g);
        }
    }

    /// Side-effect-only analyze used by [`apply_move`] in PvP mode
    /// when `--evalbar` is on. ~10–300 ms native at default Hard depth.
    fn run_pvp_eval(g: &mut GameView) {
        let opts = AiOptions {
            difficulty: Difficulty::Hard,
            max_depth: None,
            seed: Some(g.state.position_hash ^ 0xEBA1_BAA1_u64),
            strategy: Strategy::default(),
            randomness: Some(chess_ai::Randomness::STRICT),
            node_budget: None,
        };
        if let Some(analysis) = chess_ai::analyze(&g.state, &opts) {
            Self::record_eval_sample(g, analysis.chosen.score);
        }
    }

    /// Record a win-rate sample for the position currently in `g.state`
    /// (i.e. AFTER the most recent move). Idempotent — checks if a
    /// sample for this ply count already exists and skips if so.
    fn record_eval_sample(g: &mut GameView, cp_stm_pov: i32) {
        let ply = g.state.history.len();
        if g.eval_samples.iter().any(|s| s.ply == ply) {
            return;
        }
        let sample = crate::eval::EvalSample::new(ply, g.state.side_to_move, cp_stm_pov);
        g.eval_samples.push(sample);
    }

    /// Record (or replace) the definitive end-of-game sample. Unlike
    /// [`record_eval_sample`][Self::record_eval_sample] this *replaces*
    /// any existing sample at the same ply — the per-move writer may
    /// have just landed a pre-final-move sample with the same ply
    /// count (no-op for AI-played terminal positions, since the AI
    /// pump bails when status flips, but PvP mode's `run_pvp_eval`
    /// only bails on the same status check from the caller). Net
    /// effect: the trend chart's last point and the headline % both
    /// jump to the actual outcome the moment the banner appears.
    fn record_eval_final(g: &mut GameView) {
        let ply = g.state.history.len();
        let stm = g.state.side_to_move;
        let Some(sample) = crate::eval::EvalSample::final_outcome(ply, stm, &g.state.status) else {
            return;
        };
        if let Some(idx) = g.eval_samples.iter().position(|s| s.ply == ply) {
            g.eval_samples[idx] = sample;
        } else {
            g.eval_samples.push(sample);
        }
    }

    /// Run the configured engine once and apply its move. Surfaces the
    /// played move in `last_msg` so the user can see what the AI did.
    /// Falls back gracefully when the engine has no move (game over /
    /// no legal moves) — the next status refresh will pick that up.
    fn ai_reply(g: &mut GameView, cfg: VsAiConfig) {
        // Seed: hash of (history length, side) so identical positions
        // reached via different paths still get fresh randomness on
        // Easy/Normal but Hard remains deterministic.
        let seed = (g.state.history.len() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (g.state.side_to_move.raw() as u64);
        let opts = AiOptions {
            difficulty: cfg.difficulty,
            max_depth: cfg.depth,
            seed: Some(seed),
            strategy: cfg.strategy,
            randomness: cfg.randomness,
            node_budget: cfg.node_budget,
        };
        // Always call analyze (same cost as choose_move under the hood
        // — choose_move just discards the scored list). The TUI debug
        // panel reads `last_analysis` when toggled. No-debug TUIs pay
        // the small Vec<ScoredMove> alloc cost for nothing, but it's
        // negligible vs the ms-scale search itself.
        //
        // Time the search so the debug header can show the wall-clock
        // cost (matches the chess-web debug panel's "Time" row). chess-
        // ai itself doesn't measure because `Instant::now()` panics on
        // wasm32; native callers like this one wrap it.
        let start = std::time::Instant::now();
        let Some(mut analysis) = chess_ai::analyze(&g.state, &opts) else {
            return;
        };
        let elapsed_ms = u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX);
        analysis.elapsed_ms = Some(elapsed_ms);
        let mv = analysis.chosen.mv.clone();
        let nodes = analysis.chosen.nodes;
        let score = analysis.chosen.score;
        g.last_analysis = Some(analysis);
        match g.state.make_move(&mv) {
            Ok(()) => {
                g.state.refresh_status();
                g.last_msg = Some(format!(
                    "AI played: {} ({} nodes, score {})",
                    chess_core::notation::iccs::encode_move(&g.state.board, &mv),
                    nodes,
                    score,
                ));
                // Record a win-rate sample. negamax's `score` is from
                // the AI's POV after the move it just played; the new
                // `side_to_move` is the opponent, so to express the
                // sample in stm-relative cp we negate.
                Self::record_eval_sample(g, -score);
            }
            Err(e) => {
                g.last_msg = Some(format!("AI engine bug — rejected its own move: {e}"));
            }
        }
    }

    fn dispatch_net(&mut self, action: Action) {
        let Screen::Net(n) = &mut self.screen else {
            return;
        };
        // Pre-welcome: no view yet, nothing to dispatch on.
        let Some(view) = n.last_view.as_ref() else {
            return;
        };
        let shape = view.shape;
        let (rows, cols) = orient::display_dims(shape);
        let role = n.role.unwrap_or(NetRole::Player(Side::RED));
        let observer = role.observer();
        match action {
            Action::CursorUp if n.cursor.0 > 0 => {
                n.cursor.0 -= 1;
            }
            Action::CursorDown if n.cursor.0 + 1 < rows => {
                n.cursor.0 += 1;
            }
            Action::CursorLeft if n.cursor.1 > 0 => {
                n.cursor.1 -= 1;
            }
            Action::CursorRight if n.cursor.1 + 1 < cols => {
                n.cursor.1 += 1;
            }
            Action::Cancel => {
                if let Some(at) = view.chain_lock {
                    // Engine 連吃 mode: Esc → Move::EndChain.
                    if !role.is_spectator() {
                        let _ = n.client.cmd_tx.send(ClientMsg::Move { mv: Move::EndChain { at } });
                    }
                } else {
                    n.selected = None;
                    n.last_msg = None;
                }
            }
            Action::SelectOrCommit => {
                if role.is_spectator() {
                    n.last_msg = Some("Spectators cannot move.".into());
                    return;
                }
                let outcome = compute_select_outcome(n, observer);
                apply_select_outcome(n, outcome);
            }
            Action::Flip => {
                if role.is_spectator() {
                    n.last_msg = Some("Spectators cannot move.".into());
                    return;
                }
                if !matches!(view.status, GameStatus::Ongoing) {
                    n.last_msg = Some("Game over.".into());
                    return;
                }
                if view.side_to_move != observer {
                    n.last_msg = Some("Not your turn.".into());
                    return;
                }
                let Some(sq) = orient::square_at_display(n.cursor.0, n.cursor.1, observer, shape)
                else {
                    n.last_msg = Some("Cursor not on a playable square.".into());
                    return;
                };
                let _ = n
                    .client
                    .cmd_tx
                    .send(ClientMsg::Move { mv: Move::Reveal { at: sq, revealed: None } });
                n.last_msg = Some("Reveal sent.".into());
            }
            Action::Undo => {
                n.last_msg = Some("Undo not supported in online mode yet.".into());
            }
            Action::Click { term_col, term_row } => {
                if role.is_spectator() {
                    return;
                }
                if let Some(rect) = self.board_rect {
                    if let Some((row, col)) = hit_test(rect, term_col, term_row, rows, cols) {
                        let n = match &mut self.screen {
                            Screen::Net(b) => b,
                            _ => return,
                        };
                        n.cursor = (row, col);
                        let outcome = compute_select_outcome(n, observer);
                        apply_select_outcome(n, outcome);
                    }
                }
            }
            _ => {}
        }
    }
}

enum SelectOutcome {
    Ignore,
    Msg(String),
    ClearAndMsg(String),
    Select(Square),
    Deselect,
    Commit(Move),
}

fn compute_select_outcome(n: &NetView, observer: Side) -> SelectOutcome {
    let Some(view) = n.last_view.as_ref() else {
        return SelectOutcome::Ignore;
    };
    if !matches!(view.status, GameStatus::Ongoing) {
        return SelectOutcome::Msg("Game over.".into());
    }
    let shape = view.shape;
    let Some(sq) = orient::square_at_display(n.cursor.0, n.cursor.1, observer, shape) else {
        return SelectOutcome::Msg("Cursor not on a playable square.".into());
    };
    if view.side_to_move != observer {
        return SelectOutcome::Msg("Not your turn.".into());
    }

    // Engine 連吃 chain mode: only the locked piece may move (captures
    // only). Clicking the locked piece itself ends the chain.
    if let Some(locked) = view.chain_lock {
        if sq == locked {
            return SelectOutcome::Commit(Move::EndChain { at: locked });
        }
        let candidate = view
            .legal_moves
            .iter()
            .find(|m| m.origin_square() == locked && m.to_square() == Some(sq))
            .cloned();
        return match candidate {
            Some(mv) => SelectOutcome::Commit(mv),
            None => SelectOutcome::Msg(
                "連吃 active — capture from the locked piece, or Esc / Enter on it to end.".into(),
            ),
        };
    }

    match n.selected {
        None => {
            if view.legal_moves.iter().any(|m| m.origin_square() == sq) {
                SelectOutcome::Select(sq)
            } else if matches!(view.cells[sq.0 as usize], VisibleCell::Hidden) {
                SelectOutcome::Msg("Hidden piece. Press 'f' to flip.".into())
            } else {
                SelectOutcome::Msg("No legal move from that square.".into())
            }
        }
        Some(from) if from == sq => SelectOutcome::Deselect,
        Some(from) => {
            let candidate = view
                .legal_moves
                .iter()
                .find(|m| m.origin_square() == from && m.to_square() == Some(sq))
                .cloned();
            match candidate {
                Some(mv) => SelectOutcome::Commit(mv),
                None => SelectOutcome::ClearAndMsg("Illegal move.".into()),
            }
        }
    }
}

fn coord_help_msg(kind: CoordKind) -> String {
    match kind {
        CoordKind::Instant => {
            "Coord (instant): type ICCS (e.g. h2e2 / flip a0), Enter commits, Esc cancels.".into()
        }
        CoordKind::Live => {
            "Coord (live): type ICCS — selected/cursor preview as you go, Esc restores.".into()
        }
    }
}

/// Net-side coord-input dispatcher. Mirrors `dispatch_coord_text_game` but
/// gates on connection / role / turn / game-status before sending the move.
fn dispatch_coord_text_net(n: &mut NetView, action: Action) {
    let Some(ci) = n.coord_input.as_mut() else {
        return;
    };
    let kind = ci.kind;
    match action {
        Action::TextInput(c) => {
            text_input::push_char(&mut ci.buf, c, COORD_INPUT_MAX);
            if matches!(kind, CoordKind::Live) {
                live_preview_net(n);
            }
        }
        Action::TextBackspace => {
            text_input::backspace(&mut ci.buf);
            if matches!(kind, CoordKind::Live) {
                live_preview_net(n);
            }
        }
        Action::Submit => {
            let Some(view) = n.last_view.as_ref() else {
                n.last_msg = Some("Not connected yet.".into());
                return;
            };
            let role = n.role.unwrap_or(NetRole::Player(Side::RED));
            if role.is_spectator() {
                n.last_msg = Some("Spectators cannot move.".into());
                n.coord_input = None;
                return;
            }
            if !matches!(view.status, GameStatus::Ongoing) {
                n.last_msg = Some("Game over.".into());
                n.coord_input = None;
                return;
            }
            if view.side_to_move != role.observer() {
                n.last_msg = Some("Not your turn.".into());
                return; // keep buffer so the user can resubmit when their turn comes
            }
            let buf = match n.coord_input.as_mut() {
                Some(ci) => std::mem::take(&mut ci.buf),
                None => return,
            };
            match chess_core::notation::iccs::decode_move_from_view(view, buf.trim()) {
                Ok(mv) => {
                    n.coord_input = None;
                    n.selected = None;
                    let _ = n.client.cmd_tx.send(ClientMsg::Move { mv });
                    n.last_msg = Some("Sent.".into());
                }
                Err(e) => {
                    if let Some(ci) = n.coord_input.as_mut() {
                        ci.buf = buf;
                    }
                    n.last_msg = Some(format!("Bad move: {e}"));
                }
            }
        }
        _ => {}
    }
}

/// Result of one live-preview re-parse: the selected-square highlight to
/// show, and an optional cursor jump (only set when a destination square is
/// fully typed). Used by both Game and Net live-preview hooks.
struct LivePreview {
    selected: Option<Square>,
    cursor_jump: Option<(u8, u8)>,
}

fn live_preview_game(g: &mut GameView, observer: Side) {
    let buf = match g.coord_input.as_ref() {
        Some(ci) => ci.buf.clone(),
        None => return,
    };
    let shape = g.state.board.shape();
    let lp = apply_live_preview(
        &buf,
        |s| chess_core::notation::iccs::parse_square_str(&g.state.board, s),
        shape,
        observer,
    );
    g.selected = lp.selected;
    if let Some(c) = lp.cursor_jump {
        g.cursor = c;
    }
}

fn live_preview_net(n: &mut NetView) {
    let buf = match n.coord_input.as_ref() {
        Some(ci) => ci.buf.clone(),
        None => return,
    };
    let Some(view) = n.last_view.as_ref() else {
        return;
    };
    let shape = view.shape;
    let observer = n.role.map(|r| r.observer()).unwrap_or(Side::RED);
    let board = chess_core::board::Board::new(shape);
    let lp = apply_live_preview(
        &buf,
        |s| chess_core::notation::iccs::parse_square_str(&board, s),
        shape,
        observer,
    );
    n.selected = lp.selected;
    if let Some(c) = lp.cursor_jump {
        n.cursor = c;
    }
}

/// Re-parses `buf` and computes the highlight + cursor-jump for the
/// live-preview coord prompt. Intentionally tolerant: parse failures simply
/// don't advance the highlight, so the user can backspace and retry without
/// an error popup.
///
/// - `""` or 1-char prefix → `selected = None`, no jump.
/// - 2 chars valid square → `selected = Some(origin)`, no jump.
/// - 4 chars valid `from+to` (no separator) → also `cursor_jump = display(to)`.
/// - `<from>x<to>` (single hop, e.g. `h2xh9`) → same destination jump.
/// - `flip <sq>` form → highlight the named square as `selected`.
fn apply_live_preview<F>(
    buf: &str,
    mut parse_sq: F,
    shape: BoardShape,
    observer: Side,
) -> LivePreview
where
    F: FnMut(&str) -> Option<Square>,
{
    let none = LivePreview { selected: None, cursor_jump: None };
    if buf.is_empty() {
        return none;
    }
    if let Some(rest) = buf.strip_prefix("flip ") {
        if rest.len() >= 2 {
            if let Some(sq) = parse_sq(&rest[..2]) {
                return LivePreview { selected: Some(sq), cursor_jump: None };
            }
        }
        return none;
    }
    if buf.len() < 2 {
        return none;
    }
    let Some(origin) = parse_sq(&buf[..2]) else {
        return none;
    };
    let dest_str: Option<&str> = if buf.len() == 4 && !buf.contains('x') {
        Some(&buf[2..4])
    } else if buf.contains('x') {
        let parts: Vec<&str> = buf.split('x').collect();
        parts.iter().rev().find_map(|p| if p.len() == 2 { Some(*p) } else { None })
    } else {
        None
    };
    let cursor_jump =
        dest_str.and_then(parse_sq).map(|dest| orient::project_cell(dest, observer, shape));
    LivePreview { selected: Some(origin), cursor_jump }
}

fn apply_select_outcome(n: &mut NetView, outcome: SelectOutcome) {
    match outcome {
        SelectOutcome::Ignore => {}
        SelectOutcome::Msg(m) => n.last_msg = Some(m),
        SelectOutcome::ClearAndMsg(m) => {
            n.selected = None;
            n.last_msg = Some(m);
        }
        SelectOutcome::Select(sq) => {
            n.selected = Some(sq);
            n.last_msg = None;
        }
        SelectOutcome::Deselect => {
            n.selected = None;
            n.last_msg = None;
        }
        SelectOutcome::Commit(mv) => {
            let _ = n.client.cmd_tx.send(ClientMsg::Move { mv });
            n.selected = None;
            n.last_msg = Some("Sent.".into());
        }
    }
}

fn apply_net_event(n: &mut NetView, evt: NetEvent) {
    match evt {
        NetEvent::Connected => {
            n.connected = true;
            n.last_msg = Some("Connected. Waiting for welcome…".into());
        }
        NetEvent::Server(boxed) => match *boxed {
            ServerMsg::Hello { observer, rules, view, hints_allowed, .. } => {
                let was_seated = n.role.is_some();
                n.role = Some(NetRole::Player(observer));
                n.rules = Some(rules);
                let shape = view.shape;
                let (rows, cols) = orient::display_dims(shape);
                n.cursor = (rows / 2, cols / 2);
                n.selected = None;
                n.last_view = Some(view);
                n.connected = true;
                n.hints_allowed = Some(hints_allowed);
                n.last_analysis = None;
                // First Hello = "Joined as X". Subsequent Hello = rematch reset.
                n.last_msg = Some(if was_seated {
                    "Rematch — new game.".into()
                } else {
                    let hints_note = if hints_allowed { " (🧠 hints allowed)" } else { "" };
                    format!("Joined as {}{}.", side_label(observer), hints_note)
                });
            }
            ServerMsg::Spectating { rules, view, hints_allowed, .. } => {
                let was_welcomed = n.role.is_some();
                n.role = Some(NetRole::Spectator);
                n.rules = Some(rules);
                let shape = view.shape;
                let (rows, cols) = orient::display_dims(shape);
                n.cursor = (rows / 2, cols / 2);
                n.selected = None;
                n.last_view = Some(view);
                n.connected = true;
                n.hints_allowed = Some(hints_allowed);
                n.last_analysis = None;
                n.last_msg = Some(if was_welcomed {
                    "Rematch — new game (spectating).".into()
                } else {
                    let hints_note = if hints_allowed { " (🧠 hints allowed)" } else { "" };
                    format!("Joined as spectator (read-only){}.", hints_note)
                });
            }
            ServerMsg::Update { view } => {
                n.last_view = Some(view);
                if n.last_msg.as_deref() == Some("Sent.") {
                    n.last_msg = None;
                }
            }
            ServerMsg::ChatHistory { lines } => {
                n.chat.clear();
                for line in lines.into_iter().take(CHAT_HISTORY_CAP) {
                    n.chat.push_back(line);
                }
            }
            ServerMsg::Chat { line } => {
                n.chat.push_back(line);
                while n.chat.len() > CHAT_HISTORY_CAP {
                    n.chat.pop_front();
                }
            }
            ServerMsg::Error { message } => {
                n.last_msg = Some(message);
            }
            ServerMsg::Rooms { .. } => {
                n.last_msg = Some("(unexpected lobby payload on game socket)".into());
            }
        },
        NetEvent::Disconnected(reason) => {
            n.connected = false;
            n.last_msg = Some(format!("Disconnected: {reason}"));
        }
    }
}

fn side_label(side: Side) -> &'static str {
    if side == Side::RED {
        "Red 紅"
    } else if side == Side::BLACK {
        "Black 黑"
    } else {
        "Green 綠"
    }
}

fn apply_lobby_event(l: &mut LobbyView, evt: NetEvent) {
    match evt {
        NetEvent::Connected => {
            l.connected = true;
            l.last_msg = Some("Lobby connected.".into());
        }
        NetEvent::Server(boxed) => match *boxed {
            ServerMsg::Rooms { rooms } => {
                let prev_id = l.rooms.get(l.cursor).map(|r| r.id.clone());
                l.rooms = rooms;
                l.rooms.sort_by(|a, b| a.id.cmp(&b.id));
                // Try to keep the cursor on the same room id; otherwise clamp.
                if let Some(id) = prev_id {
                    if let Some(idx) = l.rooms.iter().position(|r| r.id == id) {
                        l.cursor = idx;
                    } else if l.cursor >= l.rooms.len() && !l.rooms.is_empty() {
                        l.cursor = l.rooms.len() - 1;
                    } else if l.rooms.is_empty() {
                        l.cursor = 0;
                    }
                } else if l.cursor >= l.rooms.len() && !l.rooms.is_empty() {
                    l.cursor = l.rooms.len() - 1;
                }
            }
            ServerMsg::Error { message } => {
                l.last_msg = Some(message);
            }
            ServerMsg::Hello { .. }
            | ServerMsg::Update { .. }
            | ServerMsg::Spectating { .. }
            | ServerMsg::ChatHistory { .. }
            | ServerMsg::Chat { .. } => {
                // Game-socket payloads should never arrive on a lobby ws.
                // If they do (server bug), surface for debugging.
                l.last_msg = Some("(unexpected game payload on lobby socket)".into());
            }
        },
        NetEvent::Disconnected(reason) => {
            l.connected = false;
            l.last_msg = Some(format!("Lobby disconnected: {reason}"));
        }
    }
}

/// Convert terminal click coords to (display_row, display_col) within board.
///
/// Cells are `cell_rows × cell_cols` terminal cells. With the intersection
/// layout, `cell_rows = 2` (rank row + between row) so clicks on either of
/// those rows resolve to the same display cell — including the river row,
/// which simply replaces the between-row at index 4 without changing the
/// row layout.
fn hit_test(rect: RectPx, term_col: u16, term_row: u16, rows: u8, cols: u8) -> Option<(u8, u8)> {
    if term_col < rect.x + rect.left_pad || term_row < rect.y + rect.top_pad {
        return None;
    }
    let col_off = term_col - rect.x - rect.left_pad;
    let row_off = term_row - rect.y - rect.top_pad;
    if rect.cell_cols == 0 || rect.cell_rows == 0 {
        return None;
    }
    let cell_col = col_off / rect.cell_cols;
    let cell_row = row_off / rect.cell_rows;
    if cell_row >= rows as u16 || cell_col >= cols as u16 {
        return None;
    }
    Some((cell_row as u8, cell_col as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_xiangqi() -> AppState {
        AppState::new_game(RuleSet::xiangqi(), Style::Ascii, false, Side::RED)
    }

    fn type_str(app: &mut AppState, s: &str) {
        for c in s.chars() {
            app.dispatch(Action::TextInput(c));
        }
    }

    fn game_view(app: &AppState) -> &GameView {
        match &app.screen {
            Screen::Game(g) => g,
            _ => panic!("expected Screen::Game"),
        }
    }

    #[test]
    fn coord_instant_commits_on_enter() {
        let mut app = fresh_xiangqi();
        app.dispatch(Action::CoordStart(CoordKind::Instant));
        type_str(&mut app, "h2e2");
        app.dispatch(Action::Submit);
        let g = game_view(&app);
        assert_eq!(g.state.history.len(), 1);
        assert!(g.coord_input.is_none());
    }

    #[test]
    fn coord_live_sets_selected_at_two_chars() {
        let mut app = fresh_xiangqi();
        app.dispatch(Action::CoordStart(CoordKind::Live));
        type_str(&mut app, "h2");
        let g = game_view(&app);
        // h2 = file 7, rank 2 in 9-wide → square index 2*9 + 7 = 25.
        assert_eq!(g.selected.map(|sq| sq.0), Some(25));
    }

    #[test]
    fn coord_live_jumps_cursor_at_four_chars() {
        let mut app = fresh_xiangqi();
        let prev_cursor = game_view(&app).cursor;
        app.dispatch(Action::CoordStart(CoordKind::Live));
        type_str(&mut app, "h2e2");
        let g = game_view(&app);
        // e2 projected for Red observer: (10 - 1 - 2, 4) = (7, 4).
        assert_eq!(g.cursor, (7, 4));
        assert_ne!(g.cursor, prev_cursor);
        assert_eq!(g.selected.map(|sq| sq.0), Some(25)); // origin still h2
    }

    #[test]
    fn coord_live_esc_restores_snapshot() {
        let mut app = fresh_xiangqi();
        // Mutate before opening the prompt — these are what should be restored.
        if let Screen::Game(g) = &mut app.screen {
            g.cursor = (5, 3);
        }
        app.dispatch(Action::CoordStart(CoordKind::Live));
        type_str(&mut app, "h2e2");
        // Pre-Esc: cursor jumped, selected set.
        assert_eq!(game_view(&app).cursor, (7, 4));
        assert!(game_view(&app).selected.is_some());
        app.dispatch(Action::Back);
        let g = game_view(&app);
        assert_eq!(g.cursor, (5, 3));
        assert!(g.selected.is_none());
        assert!(g.coord_input.is_none());
    }

    #[test]
    fn coord_bad_notation_keeps_prompt_open() {
        let mut app = fresh_xiangqi();
        app.dispatch(Action::CoordStart(CoordKind::Instant));
        type_str(&mut app, "z9z9");
        app.dispatch(Action::Submit);
        let g = game_view(&app);
        assert_eq!(g.state.history.len(), 0);
        assert!(g.coord_input.is_some());
        assert!(g.last_msg.as_ref().map(|m| m.starts_with("Bad move:")).unwrap_or(false));
    }

    #[test]
    fn coord_instant_does_not_snapshot() {
        // Instant mode never touches cursor / selected, so snapshot is None
        // and Esc is a plain close.
        let mut app = fresh_xiangqi();
        if let Screen::Game(g) = &mut app.screen {
            g.cursor = (3, 3);
            g.selected = None;
        }
        app.dispatch(Action::CoordStart(CoordKind::Instant));
        type_str(&mut app, "h2");
        // Instant mode: typing should NOT update selected.
        assert!(game_view(&app).selected.is_none());
        assert_eq!(game_view(&app).cursor, (3, 3));
        app.dispatch(Action::Back);
        let g = game_view(&app);
        assert!(g.coord_input.is_none());
        assert_eq!(g.cursor, (3, 3));
    }

    // --- CapturedSort + split_and_sort_captured -----------------------
    //
    // The TUI carries its own copy of these helpers (the chess-web
    // crate has the canonical version under `crate::state`). Tests
    // here lock in the contract so the two copies don't drift —
    // promoting them into a shared crate is tracked in the backlog.

    #[test]
    fn captured_sort_toggle_round_trips() {
        assert_eq!(CapturedSort::Time.toggled(), CapturedSort::Rank);
        assert_eq!(CapturedSort::Rank.toggled(), CapturedSort::Time);
    }

    #[test]
    fn split_and_sort_captured_groups_by_side_and_keeps_chronology_under_time() {
        let chronological = vec![
            Piece::new(Side::BLACK, PieceKind::Soldier),
            Piece::new(Side::RED, PieceKind::Cannon),
            Piece::new(Side::BLACK, PieceKind::Horse),
            Piece::new(Side::RED, PieceKind::Chariot),
        ];
        let (red, black) = split_and_sort_captured(&chronological, CapturedSort::Time);
        assert_eq!(
            red.iter().map(|p| p.kind).collect::<Vec<_>>(),
            vec![PieceKind::Cannon, PieceKind::Chariot]
        );
        assert_eq!(
            black.iter().map(|p| p.kind).collect::<Vec<_>>(),
            vec![PieceKind::Soldier, PieceKind::Horse]
        );
    }

    #[test]
    fn split_and_sort_captured_orders_largest_first_under_rank() {
        let chronological = vec![
            Piece::new(Side::RED, PieceKind::Soldier),
            Piece::new(Side::RED, PieceKind::General),
            Piece::new(Side::RED, PieceKind::Horse),
            Piece::new(Side::RED, PieceKind::Chariot),
        ];
        let (red, _black) = split_and_sort_captured(&chronological, CapturedSort::Rank);
        assert_eq!(
            red.iter().map(|p| p.kind).collect::<Vec<_>>(),
            vec![PieceKind::General, PieceKind::Chariot, PieceKind::Horse, PieceKind::Soldier]
        );
    }

    #[test]
    fn captured_sort_toggle_action_flips_app_state() {
        let mut app = fresh_xiangqi();
        assert_eq!(app.captured_sort, CapturedSort::Time, "default is Time");
        app.dispatch(Action::CapturedSortToggle);
        assert_eq!(app.captured_sort, CapturedSort::Rank);
        app.dispatch(Action::CapturedSortToggle);
        assert_eq!(app.captured_sort, CapturedSort::Time);
    }

    #[test]
    fn captured_sort_preserved_across_screen_replace() {
        let mut app = fresh_xiangqi();
        app.captured_sort = CapturedSort::Rank;
        // Going back to the picker (NewGame Action) goes through
        // `replace_preserving_prefs`. The sort must survive.
        app.dispatch(Action::NewGame);
        assert_eq!(
            app.captured_sort,
            CapturedSort::Rank,
            "captured_sort must survive screen transitions"
        );
    }
}
