//! jev-chess: a UCI chess engine that delegates move selection to Jev.

mod game;
mod jev;

use std::io::{self, BufRead, Write};

use game::Game;
use jev::JevClient;

use clap::Parser;

const NAME: &str = concat!("jev-chess ", env!("CARGO_PKG_VERSION"));

/// A UCI chess engine that lets Jev pick its moves.
///
/// Each flag falls back to the corresponding environment variable.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// Jev API key, sent as a Bearer token.
    #[arg(long, env = "JEV_API_KEY", hide_env_values = true)]
    api_key: Option<String>,

    /// Jev decision endpoint.
    #[arg(long, env = "JEV_API_URL", default_value = jev::DEFAULT_URL)]
    api_url: String,

    /// Optional `model` field, e.g. `jev-latest` for gateways that need it.
    #[arg(long, env = "JEV_MODEL")]
    model: Option<String>,
}

fn main() {
    let args = Args::parse();
    let client = match args.api_key {
        None => {
            eprintln!(
                "warning: no API key (--api-key / JEV_API_KEY); playing the first legal move"
            );
            None
        }
        Some(key) => match JevClient::new(args.api_url, key, args.model) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("warning: {e:#}; playing the first legal move");
                None
            }
        },
    };

    let stdin = io::stdin();
    let mut out = io::stdout();
    let mut game = Game::default();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        let (cmd, args) = line.split_once(' ').unwrap_or((line, ""));

        match cmd {
            "uci" => {
                send(&mut out, &format!("id name {NAME}"));
                send(&mut out, "id author josefs");
                send(&mut out, "uciok");
            }
            "isready" => send(&mut out, "readyok"),
            "ucinewgame" => game = Game::default(),
            "position" => match Game::from_uci_position(args) {
                Ok(g) => game = g,
                Err(e) => send(&mut out, &format!("info string bad position: {e:#}")),
            },
            "go" => {
                let mv = choose_move(&game, client.as_ref(), &mut out);
                send(&mut out, &format!("bestmove {mv}"));
            }
            "quit" => break,
            // `stop`, `setoption`, `ponderhit`, `debug`, `register`: nothing to do.
            _ => {}
        }
    }
}

fn choose_move(game: &Game, client: Option<&JevClient>, out: &mut impl Write) -> String {
    let moves = game.legal_moves();
    let Some(first) = moves.first() else {
        return "0000".to_string();
    };
    let fallback = Game::to_uci(first);
    if moves.len() == 1 {
        return fallback;
    }
    let Some(client) = client else {
        return fallback;
    };

    let options = game.options(&moves);
    match client.choose(&game.describe_state(), &game.instructions(), &options) {
        Ok(decision) => {
            let p = decision
                .probabilities
                .get(&decision.choice)
                .copied()
                .unwrap_or(0.0);
            send(
                out,
                &format!("info string jev chose {} (p={p:.3})", decision.choice),
            );
            decision.choice
        }
        Err(e) => {
            send(
                out,
                &format!("info string jev error: {e:#}; playing {fallback}"),
            );
            fallback
        }
    }
}

fn send(out: &mut impl Write, msg: &str) {
    let _ = writeln!(out, "{msg}");
    let _ = out.flush();
}
