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
mod glyph;
mod input;
mod orient;
mod ui;

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

use crate::app::AppState;
use crate::glyph::Style;
use crate::input::{from_key, from_mouse, Action};

#[derive(Parser, Debug)]
#[command(name = "chess-tui", about = "ratatui frontend for chess-core")]
struct Cli {
    /// Render style.
    #[arg(long, value_enum, default_value_t = StyleArg::Cjk)]
    style: StyleArg,

    /// Disable ANSI color (keeps the chosen glyph set).
    #[arg(long)]
    no_color: bool,

    /// Render from this side's perspective (debug; default RED).
    #[arg(long = "as", value_enum, default_value_t = SideArg::Red)]
    observer: SideArg,

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

    let app = match &cli.cmd {
        None => AppState::new_picker(style, use_color, observer),
        Some(Cmd::Xiangqi { strict }) => {
            let rules = if *strict { RuleSet::xiangqi() } else { RuleSet::xiangqi_casual() };
            AppState::new_game(rules, style, use_color, observer)
        }
        Some(Cmd::Banqi { preset, house, seed }) => {
            let rules = build_banqi_rules(preset.as_ref(), house.as_deref(), *seed)?;
            AppState::new_game(rules, style, use_color, observer)
        }
    };

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
                "dark" | "dark-chain" => HouseRules::DARK_CHAIN,
                "rush" | "chariot-rush" => HouseRules::CHARIOT_RUSH,
                "horse-diagonal" | "diag" => HouseRules::HORSE_DIAGONAL,
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
        terminal.draw(|frame| ui::draw(frame, app))?;
        if app.should_quit {
            return Ok(());
        }
        if event::poll(Duration::from_millis(200))? {
            let in_picker = matches!(app.screen, app::Screen::Picker(_));
            let quit_confirm = app.quit_confirm_open;
            let action = match event::read()? {
                Event::Key(k) if k.kind == event::KeyEventKind::Press => {
                    from_key(k, in_picker, quit_confirm)
                }
                Event::Mouse(m) => from_mouse(m),
                Event::Resize(_, _) => Action::None,
                _ => Action::None,
            };
            app.dispatch(action);
        }
    }
}
