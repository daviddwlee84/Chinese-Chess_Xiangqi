//! `chess-net-server` — single-room ws host for chess-core.
//!
//! Examples:
//!   chess-net-server xiangqi --port 7878
//!   chess-net-server xiangqi --port 7878 --strict
//!   chess-net-server banqi --port 7878 --preset taiwan --seed 42

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use chess_core::rules::{HouseRules, RuleSet};
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(name = "chess-net-server", about = "ws server for chess-core (single room MVP)")]
struct Cli {
    /// Listen port (loopback only).
    #[arg(long, default_value_t = 7878)]
    port: u16,

    /// Override bind address. Defaults to 127.0.0.1:<port>.
    #[arg(long)]
    addr: Option<SocketAddr>,

    /// Serve a static directory (e.g. `clients/chess-web/dist`) at `/`.
    /// SPA fallback — unknown paths return `index.html`. Falls back to
    /// `CHESS_NET_STATIC_DIR` env var if not provided. When set, `GET /`
    /// serves index.html; ws clients should use `--connect ws://host/ws`.
    #[arg(long)]
    static_dir: Option<PathBuf>,

    /// Per-room cap on `?role=spectator` connections. The (cap+1)th
    /// spectator gets `Error{"room watch capacity reached"}`. Defaults to
    /// 16; falls back to `CHESS_NET_MAX_SPECTATORS` env if not provided.
    #[arg(long)]
    max_spectators: Option<usize>,

    #[command(subcommand)]
    variant: VariantCmd,
}

#[derive(Subcommand, Debug)]
enum VariantCmd {
    /// Xiangqi (9×10). Casual rules by default; pass `--strict` for the
    /// standard self-check filter.
    Xiangqi {
        #[arg(long)]
        strict: bool,
    },
    /// Banqi (4×8 face-down).
    Banqi {
        #[arg(long, value_enum)]
        preset: Option<PresetArg>,
        /// Comma-separated house rules: chain,dark,rush,horse-diagonal,cannon-fast.
        #[arg(long)]
        house: Option<String>,
        #[arg(long)]
        seed: Option<u64>,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum PresetArg {
    Purist,
    Taiwan,
    Aggressive,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let addr = cli
        .addr
        .unwrap_or_else(|| SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), cli.port));

    let rules = match cli.variant {
        VariantCmd::Xiangqi { strict } => {
            if strict {
                RuleSet::xiangqi()
            } else {
                RuleSet::xiangqi_casual()
            }
        }
        VariantCmd::Banqi { preset, house, seed } => {
            build_banqi_rules(preset.as_ref(), house.as_deref(), seed)?
        }
    };

    let static_dir =
        cli.static_dir.or_else(|| std::env::var_os("CHESS_NET_STATIC_DIR").map(PathBuf::from));
    let max_spectators = cli
        .max_spectators
        .or_else(|| std::env::var("CHESS_NET_MAX_SPECTATORS").ok().and_then(|s| s.parse().ok()));

    let mut opts = chess_net::ServeOpts::new(rules).with_static_dir(static_dir);
    if let Some(cap) = max_spectators {
        opts = opts.with_max_spectators(cap);
    }
    chess_net::run_with(addr, opts).await
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
