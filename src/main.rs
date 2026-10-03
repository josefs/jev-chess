//! jev-chess: a UCI chess engine that delegates move selection to Jev.

mod game;
mod jev;

use std::{
    env,
    fs::{File, OpenOptions},
    io::{self, BufRead, Write},
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

use clap::{Parser, error::ErrorKind};
use game::Game;
use jev::JevClient;

const NAME: &str = concat!("jev-chess ", env!("CARGO_PKG_VERSION"));

/// A UCI chess engine that lets Jev pick its moves.
///
/// Each flag falls back to the corresponding environment variable. The same
/// settings are also exposed as UCI options (ApiKey, ApiUrl, Model).
#[derive(Parser, Clone)]
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

    /// Append a log of all UCI traffic and Jev decisions to this file.
    #[arg(long, env = "JEV_CHESS_LOG")]
    log_file: Option<PathBuf>,
}

static LOG: OnceLock<Mutex<File>> = OnceLock::new();

fn log(msg: &str) {
    if let Some(file) = LOG.get()
        && let Ok(mut f) = file.lock()
    {
        let _ = writeln!(f, "{msg}");
        let _ = f.flush();
    }
}

fn warn(msg: &str) {
    eprintln!("warning: {msg}");
    log(&format!("!! warning: {msg}"));
}

/// Parses command-line arguments without ever exiting on bad input, since a
/// GUI probing the engine needs it to answer `uci` regardless.
fn parse_args() -> Args {
    // Some GUIs pass all arguments as a single string; split it up.
    let argv: Vec<String> = env::args()
        .enumerate()
        .flat_map(|(i, a)| {
            if i == 0 {
                vec![a]
            } else {
                a.split_whitespace()
                    .map(|t| t.trim_matches(|c| c == '"' || c == '\'').to_string())
                    .collect()
            }
        })
        .collect();

    match Args::try_parse_from(&argv) {
        Ok(args) => args,
        Err(e) if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) => {
            e.exit()
        }
        Err(e) => {
            warn(&format!("ignoring invalid arguments: {e}"));
            Args::try_parse_from(&argv[..1]).unwrap_or_else(|_| Args {
                api_key: None,
                api_url: jev::DEFAULT_URL.to_string(),
                model: None,
                log_file: None,
            })
        }
    }
}

/// Settings that can be overridden at runtime via UCI `setoption`.
/// An empty value falls back to the command-line/environment setting.
#[derive(Default)]
struct UciOptions {
    api_key: String,
    api_url: String,
    model: String,
}

impl UciOptions {
    /// Applies `setoption name <id> value <x>`; returns whether anything changed.
    fn set(&mut self, args: &str) -> bool {
        let Some(rest) = args.trim().strip_prefix("name") else {
            return false;
        };
        let (name, value) = match rest.split_once(" value") {
            Some((n, v)) => (n.trim(), v.trim()),
            None => (rest.trim(), ""),
        };
        let value = if value == "<empty>" { "" } else { value };
        let slot = match name.to_ascii_lowercase().as_str() {
            "apikey" => &mut self.api_key,
            "apiurl" => &mut self.api_url,
            "model" => &mut self.model,
            _ => return false,
        };
        *slot = value.to_string();
        true
    }
}

fn non_empty(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_string())
}

fn build_client(args: &Args, opts: &UciOptions) -> Option<JevClient> {
    let Some(key) = non_empty(&opts.api_key).or_else(|| args.api_key.clone()) else {
        warn("no API key (--api-key / JEV_API_KEY / ApiKey option); playing the first legal move");
        return None;
    };
    let url = non_empty(&opts.api_url).unwrap_or_else(|| args.api_url.clone());
    let model = non_empty(&opts.model).or_else(|| args.model.clone());
    JevClient::new(url, key, model)
        .inspect_err(|e| warn(&format!("{e:#}; playing the first legal move")))
        .ok()
}

fn main() {
    let args = parse_args();
    if let Some(path) = &args.log_file {
        match OpenOptions::new().create(true).append(true).open(path) {
            Ok(f) => {
                let _ = LOG.set(Mutex::new(f));
                log(&format!("== {NAME} started"));
            }
            Err(e) => warn(&format!("cannot open log file {}: {e}", path.display())),
        }
    }
    let mut opts = UciOptions::default();
    // Built lazily so that `setoption` commands sent after startup take effect.
    let mut client: Option<Option<JevClient>> = None;

    let stdin = io::stdin();
    let mut out = io::stdout();
    let mut game = Game::default();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line
            .to_ascii_lowercase()
            .starts_with("setoption name apikey")
        {
            log("<< setoption name ApiKey value <redacted>");
        } else {
            log(&format!("<< {line}"));
        }
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));

        match cmd {
            "uci" => {
                send(&mut out, &format!("id name {NAME}"));
                send(&mut out, "id author josefs");
                send(&mut out, "option name ApiKey type string default <empty>");
                send(&mut out, "option name ApiUrl type string default <empty>");
                send(&mut out, "option name Model type string default <empty>");
                send(&mut out, "uciok");
            }
            "isready" => send(&mut out, "readyok"),
            "setoption" => {
                if opts.set(rest) {
                    client = None;
                }
            }
            "ucinewgame" => game = Game::default(),
            "position" => match Game::from_uci_position(rest) {
                Ok(g) => game = g,
                Err(e) => send(&mut out, &format!("info string bad position: {e:#}")),
            },
            "go" => {
                let c = client.get_or_insert_with(|| build_client(&args, &opts));
                let mv = choose_move(&game, c.as_ref(), &mut out);
                send(&mut out, &format!("bestmove {mv}"));
            }
            "quit" => break,
            // `stop`, `ponderhit`, `debug`, `register`: nothing to do.
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
            let mut ranked: Vec<_> = decision.probabilities.iter().collect();
            ranked.sort_by(|a, b| b.1.total_cmp(a.1));
            let top: Vec<_> = ranked
                .iter()
                .take(5)
                .map(|(m, p)| format!("{m}={p:.3}"))
                .collect();
            log(&format!("   jev top moves: {}", top.join(" ")));
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
    log(&format!(">> {msg}"));
    let _ = writeln!(out, "{msg}");
    let _ = out.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setoption_parsing() {
        let mut o = UciOptions::default();
        assert!(o.set("name ApiKey value sk-1"));
        assert!(o.set("name Model value jev latest"));
        assert!(o.set("name ApiUrl value <empty>"));
        assert!(!o.set("name Hash value 16"));
        assert_eq!(o.api_key, "sk-1");
        assert_eq!(o.model, "jev latest");
        assert_eq!(o.api_url, "");
    }
}
