//! `chess-tui` — ratatui frontend for chess-core.
//!
//! Run modes:
//!   chess-tui                                       # variant picker
//!   chess-tui xiangqi
//!   chess-tui banqi --preset taiwan --seed 42
//!   chess-tui --style ascii xiangqi
//!   chess-tui --no-color xiangqi
//!   chess-tui --as black xiangqi                    # debug: render as Black

mod app;
mod banner;
mod confetti;
mod glyph;
mod input;
mod net;
mod orient;
mod text_input;
mod ui;
mod url;

use std::io::{self, Stdout};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use chess_core::piece::Side;
use chess_core::rules::{HouseRules, RuleSet};
use clap::{Parser, Subcommand, ValueEnum};
use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::app::{AppState, CapturedSort};
use crate::glyph::Style;
use crate::input::{from_key, from_mouse, Action};
use crate::url::{normalize_connect_url, normalize_lobby_url};

#[derive(Parser, Debug)]
#[command(name = "chess-tui", about = "ratatui frontend for chess-core")]
struct Cli {
    /// Render style.
    #[arg(long, value_enum, default_value_t = StyleArg::Cjk)]
    style: StyleArg,

    /// Disable ANSI color (keeps the chosen glyph set).
    #[arg(long)]
    no_color: bool,

    /// Disable end-of-game confetti + big VICTORY/DEFEAT/DRAW overlay.
    /// Sidebar status text is unaffected.
    #[arg(long)]
    no_confetti: bool,

    /// Disable the "將軍 / CHECK" sidebar warning. Xiangqi-only — banqi
    /// has no general so the banner never fires there anyway.
    #[arg(long)]
    no_check_banner: bool,

    /// Initial sort order for the sidebar captured-pieces panel
    /// ("graveyard"). Toggle in-game with `g`. Default `time`
    /// (chronological — useful for following 暗連 chains live).
    #[arg(long, value_enum, default_value_t = CapturedSortArg::Time)]
    captured_sort: CapturedSortArg,

    /// Render from this side's perspective (debug; default RED). Ignored
    /// when `--connect` is set — the server assigns a side on join.
    #[arg(long = "as", value_enum, default_value_t = SideArg::Red)]
    observer: SideArg,

    /// Connect to a chess-net-server instead of starting a local game.
    /// Example: --connect ws://127.0.0.1:7878 (the trailing /ws is added
    /// automatically if missing). Use ws://host/ws/<room-id> to target a
    /// specific room on a multi-room server; pair with --password if the
    /// room is locked.
    #[arg(long)]
    connect: Option<String>,

    /// Open the lobby browser for the given chess-net server. Example:
    /// --lobby ws://127.0.0.1:7878 — picks a room from the live list (or
    /// creates one) instead of joining a fixed URL. Mutually exclusive
    /// with --connect; if both are provided, --lobby wins.
    #[arg(long)]
    lobby: Option<String>,

    /// Password for password-locked rooms when paired with --connect. The
    /// in-TUI lobby browser prompts for the password directly, so this
    /// flag is mostly for scripted reconnects.
    #[arg(long)]
    password: Option<String>,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Xiangqi (9×10). Casual rules by default — moves that leave your own
    /// general capturable are allowed; the game ends when the general is
    /// actually captured. Pass `--strict` for the standard self-check filter.
    Xiangqi {
        /// Strict mode: standard rules. Moves that would leave your own
        /// general in check are rejected.
        #[arg(long)]
        strict: bool,
    },
    /// Banqi (4×8 face-down).
    Banqi {
        /// Preset bundle of house rules.
        #[arg(long, value_enum)]
        preset: Option<PresetArg>,
        /// Comma-separated house rules: chain,dark,rush,horse-diagonal,cannon-fast.
        #[arg(long)]
        house: Option<String>,
        /// Deterministic shuffle seed.
        #[arg(long)]
        seed: Option<u64>,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum StyleArg {
    Cjk,
    Ascii,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum SideArg {
    Red,
    Black,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum CapturedSortArg {
    Time,
    Rank,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum PresetArg {
    Purist,
    Taiwan,
    Aggressive,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let style = match cli.style {
        StyleArg::Cjk => Style::Cjk,
        StyleArg::Ascii => Style::Ascii,
    };
    let observer = match cli.observer {
        SideArg::Red => Side::RED,
        SideArg::Black => Side::BLACK,
    };
    let use_color = !cli.no_color;

    let show_confetti = !cli.no_confetti;
    let show_check_banner = !cli.no_check_banner;
    let captured_sort = match cli.captured_sort {
        CapturedSortArg::Time => CapturedSort::Time,
        CapturedSortArg::Rank => CapturedSort::Rank,
    };

    let mut app = if let Some(host) = cli.lobby.as_deref() {
        let url = normalize_lobby_url(host).map_err(|e| anyhow!(e))?;
        // Strip the /lobby suffix before storing as `host` so the lobby
        // can re-derive `/ws/<id>` for joins.
        let host = url.trim_end_matches("/lobby").to_string();
        AppState::new_lobby(host, style, use_color, observer)
    } else if let Some(url) = cli.connect.as_deref() {
        let mut url = normalize_connect_url(url).map_err(|e| anyhow!(e))?;
        if let Some(pw) = cli.password.as_deref() {
            let sep = if url.contains('?') { '&' } else { '?' };
            url = format!("{url}{sep}password={}", crate::url::urlencode(pw));
        }
        AppState::new_net(url, style, use_color)
    } else {
        match &cli.cmd {
            None => AppState::new_picker(style, use_color, observer),
            Some(Cmd::Xiangqi { strict }) => {
                let rules = if *strict { RuleSet::xiangqi() } else { RuleSet::xiangqi_casual() };
                AppState::new_game(rules, style, use_color, observer)
            }
            Some(Cmd::Banqi { preset, house, seed }) => {
                let rules = build_banqi_rules(preset.as_ref(), house.as_deref(), *seed)?;
                AppState::new_game(rules, style, use_color, observer)
            }
        }
    };

    app.show_confetti = show_confetti;
    app.show_check_banner = show_check_banner;
    app.captured_sort = captured_sort;
    run(app)
}

fn build_banqi_rules(
    preset: Option<&PresetArg>,
    house: Option<&str>,
    seed: Option<u64>,
) -> Result<RuleSet> {
    let mut flags = HouseRules::empty();
    if let Some(p) = preset {
        flags |= match p {
            PresetArg::Purist => chess_core::rules::PRESET_PURIST,
            PresetArg::Taiwan => chess_core::rules::PRESET_TAIWAN,
            PresetArg::Aggressive => chess_core::rules::PRESET_AGGRESSIVE,
        };
    }
    if let Some(s) = house {
        for tok in s.split(',') {
            flags |= match tok.trim() {
                "chain" => HouseRules::CHAIN_CAPTURE,
                "dark" | "dark-chain" | "dark-capture" => HouseRules::DARK_CAPTURE,
                "dark-trade" | "trade" => HouseRules::DARK_CAPTURE_TRADE,
                "rush" | "chariot-rush" => HouseRules::CHARIOT_RUSH,
                "horse" | "horse-diagonal" | "diag" => HouseRules::HORSE_DIAGONAL,
                "cannon-fast" | "fast-cannon" => HouseRules::CANNON_FAST_MOVE,
                other => return Err(anyhow!("unknown house rule: {other}")),
            };
        }
    }
    let flags = chess_core::rules::house::normalize(flags);
    Ok(match seed {
        Some(s) => RuleSet::banqi_with_seed(flags, s),
        None => RuleSet::banqi(flags),
    })
}

fn run(mut app: AppState) -> Result<()> {
    let mut terminal = setup_terminal()?;
    let result = run_loop(&mut terminal, &mut app);
    teardown_terminal(&mut terminal).ok();
    result
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, crossterm::event::EnableMouseCapture)
        .context("enter alt screen")?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend).context("init terminal")?;
    Ok(terminal)
}

fn teardown_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen, crossterm::event::DisableMouseCapture)
        .ok();
    terminal.show_cursor().ok();
    Ok(())
}

fn run_loop(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut AppState) -> Result<()> {
    loop {
        // Drain ws events first so the next draw shows server-pushed state.
        // No-op outside Net mode.
        app.tick_net();

        terminal.draw(|frame| ui::draw(frame, app))?;
        if app.should_quit {
            return Ok(());
        }

        // Shorter poll in Net / Lobby modes keeps server pushes feeling
        // snappy (we redraw at most every poll interval). 200ms in
        // single-process modes is fine — no async source to drain. While
        // a confetti burst is active (or pending), drop to ~20fps so the
        // particles animate smoothly.
        let animating = app.confetti_anim.is_some() || app.confetti_pending;
        let poll_ms = if animating {
            50
        } else if matches!(app.screen, app::Screen::Net(_) | app::Screen::Lobby(_)) {
            60
        } else {
            200
        };
        if event::poll(Duration::from_millis(poll_ms))? {
            let mode = app.input_mode();
            let quit_confirm = app.quit_confirm_open;
            let action = match event::read()? {
                Event::Key(k) if k.kind == event::KeyEventKind::Press => {
                    from_key(k, mode, quit_confirm)
                }
                Event::Mouse(m) => from_mouse(m),
                Event::Resize(_, _) => Action::None,
                _ => Action::None,
            };
            app.dispatch(action);
        }
    }
}
