//! jev-chess: a UCI chess engine that delegates move selection to Jev.

mod game;
mod jev;
mod prompt;

use std::{
    env,
    fs::{File, OpenOptions},
    hash::{BuildHasher, Hasher, RandomState},
    io::{self, BufRead, Write},
    path::PathBuf,
    process::ExitCode,
    sync::{Mutex, OnceLock},
};

use clap::{Parser, error::ErrorKind};
use game::Game;
use jev::JevClient;
use prompt::Prompt;

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

    /// Jev model to use.
    #[arg(long, env = "JEV_MODEL", default_value = jev::DEFAULT_MODEL)]
    model: String,

    /// Append a log of all UCI traffic and Jev decisions to this file.
    #[arg(long, env = "JEV_CHESS_LOG")]
    log_file: Option<PathBuf>,

    /// Prompt template file (TOML); see prompts/baseline.toml for the format.
    #[arg(long, env = "JEV_PROMPT")]
    prompt: Option<PathBuf>,

    /// Sample Jev's move from its probabilities raised to 1/T instead of always
    /// taking the most likely move. 0 = always the top move, 1 = sample in
    /// proportion to Jev's probabilities.
    #[arg(long, env = "JEV_TEMPERATURE", default_value_t = 0.0, value_parser = parse_temperature)]
    temperature: f64,

    /// Ignore Jev and play uniformly random legal moves (a baseline opponent).
    #[arg(long)]
    random: bool,

    /// Print the Jev request for a position (UCI `position` syntax) and exit.
    #[arg(long, value_name = "POSITION", num_args = 0..=1, default_missing_value = "startpos")]
    show_prompt: Option<String>,
}

static LOG: OnceLock<Mutex<File>> = OnceLock::new();

fn log(msg: &str) {
    if let Some(file) = LOG.get()
        && let Ok(mut f) = file.lock()
    {
        let _ = f.write_all(format!("[{}] {msg}\n", std::process::id()).as_bytes());
    }
}

fn warn(msg: &str) {
    eprintln!("warning: {msg}");
    log(&format!("!! warning: {msg}"));
}

fn parse_temperature(s: &str) -> Result<f64, String> {
    match s.parse::<f64>() {
        Ok(t) if t >= 0.0 && t.is_finite() => Ok(t),
        _ => Err("expected a non-negative number".to_string()),
    }
}

/// Parses command-line arguments without ever exiting on bad input, since a
/// GUI probing the engine needs it to answer `uci` regardless.
fn parse_args() -> Args {
    let mut argv: Vec<String> = env::args().collect();
    // Some GUIs pass all arguments as a single string; split it up.
    if argv.len() == 2 && argv[1].starts_with("--") && argv[1].contains(char::is_whitespace) {
        let joined = argv.pop().unwrap();
        argv.extend(
            joined
                .split_whitespace()
                .map(|t| t.trim_matches(|c| c == '"' || c == '\'').to_string()),
        );
    }

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
                model: jev::DEFAULT_MODEL.to_string(),
                log_file: None,
                prompt: None,
                temperature: 0.0,
                random: false,
                show_prompt: None,
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
    let model = non_empty(&opts.model).unwrap_or_else(|| args.model.clone());
    JevClient::new(url, key, model)
        .inspect_err(|e| warn(&format!("{e:#}; playing the first legal move")))
        .ok()
}

fn load_prompt(args: &Args) -> anyhow::Result<Prompt> {
    match &args.prompt {
        Some(path) => Prompt::load(path),
        None => Ok(Prompt::builtin()),
    }
}

/// Prints the request that would be sent to Jev for `position`.
fn show_prompt(args: &Args, position: &str) -> ExitCode {
    let result = load_prompt(args).and_then(|prompt| {
        let position = position.trim();
        let game = if position.starts_with("startpos") || position.starts_with("fen") {
            Game::from_uci_position(position)?
        } else {
            Game::from_uci_position(&format!("fen {position}"))?
        };
        let vars = game.state_vars();
        println!("=== state\n{}", prompt.state(&vars));
        println!("=== instructions\n{}", prompt.instructions(&vars));
        println!("=== options");
        for (uci, desc) in game.options(&game.legal_moves(), &prompt) {
            println!("{uci}: {desc}");
        }
        Ok(())
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let args = parse_args();
    if let Some(position) = &args.show_prompt {
        return show_prompt(&args, position);
    }
    let prompt = load_prompt(&args).unwrap_or_else(|e| {
        warn(&format!("{e:#}; using the built-in prompt"));
        Prompt::builtin()
    });
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
    let mut rng = RandomState::new().build_hasher();

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
                let mv = if args.random {
                    random_move(&game, &mut rng, &mut out)
                } else {
                    let c = client.get_or_insert_with(|| build_client(&args, &opts));
                    choose_move(
                        &game,
                        c.as_ref(),
                        &prompt,
                        args.temperature,
                        &mut rng,
                        &mut out,
                    )
                };
                // Match runners such as fastchess expect a scored info line; Jev gives
                // no evaluation, so report a neutral score.
                send(&mut out, &format!("info depth 1 score cp 0 pv {mv}"));
                send(&mut out, &format!("bestmove {mv}"));
            }
            "quit" => break,
            // `stop`, `ponderhit`, `debug`, `register`: nothing to do.
            _ => {}
        }
    }
    ExitCode::SUCCESS
}

fn random_move(game: &Game, rng: &mut impl Hasher, out: &mut impl Write) -> String {
    let moves = game.legal_moves();
    if moves.is_empty() {
        send(out, "info string no legal moves");
        return "0000".to_string();
    }
    let mv = Game::to_uci(&moves[(random_unit(rng) * moves.len() as f64) as usize]);
    send(out, &format!("info string random move {mv}"));
    mv
}

/// A pseudo-random number in [0, 1).
fn random_unit(rng: &mut impl Hasher) -> f64 {
    rng.write_u8(0);
    (rng.finish() >> 11) as f64 / (1u64 << 53) as f64
}

/// Samples a key with weight `p^(1/temperature)`. Returns `None` if all weights
/// are zero or the temperature is zero.
fn sample<'a>(
    probabilities: impl IntoIterator<Item = (&'a String, f64)>,
    temperature: f64,
    unit: f64,
) -> Option<&'a String> {
    if temperature <= 0.0 {
        return None;
    }
    let weighted: Vec<_> = probabilities
        .into_iter()
        .map(|(k, p)| (k, p.max(0.0).powf(1.0 / temperature)))
        .filter(|(_, w)| w.is_finite() && *w > 0.0)
        .collect();
    let total: f64 = weighted.iter().map(|(_, w)| w).sum();
    if total <= 0.0 {
        return None;
    }
    let mut target = unit * total;
    for (k, w) in &weighted {
        if target < *w {
            return Some(k);
        }
        target -= w;
    }
    weighted.last().map(|(k, _)| *k)
}

fn choose_move(
    game: &Game,
    client: Option<&JevClient>,
    prompt: &Prompt,
    temperature: f64,
    rng: &mut impl Hasher,
    out: &mut impl Write,
) -> String {
    let moves = game.legal_moves();
    let Some(first) = moves.first() else {
        send(out, "info string no legal moves");
        return "0000".to_string();
    };
    let fallback = Game::to_uci(first);
    if moves.len() == 1 {
        send(out, &format!("info string only legal move {fallback}"));
        return fallback;
    }
    let Some(client) = client else {
        send(
            out,
            &format!("info string no Jev client; playing {fallback}"),
        );
        return fallback;
    };

    let options = game.options(&moves, prompt);
    let vars = game.state_vars();
    match client.choose(&prompt.state(&vars), &prompt.instructions(&vars), &options) {
        Ok(decision) => {
            let mut ranked: Vec<_> = decision.probabilities.iter().collect();
            ranked.sort_by(|a, b| b.1.total_cmp(a.1));
            let top: Vec<_> = ranked
                .iter()
                .take(5)
                .map(|(m, p)| format!("{m}={p:.3}"))
                .collect();
            log(&format!("   jev top moves: {}", top.join(" ")));
            let valid = decision
                .probabilities
                .iter()
                .filter(|(k, _)| options.contains_key(*k))
                .map(|(k, p)| (k, *p));
            let (choice, how) = match sample(valid, temperature, random_unit(rng)) {
                Some(k) => (k.clone(), "sampled"),
                None => (decision.choice.clone(), "chose"),
            };
            let p = decision.probabilities.get(&choice).copied().unwrap_or(0.0);
            send(out, &format!("info string jev {how} {choice} (p={p:.3})"));
            choice
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
    fn sampling() {
        let (a, b, c) = ("a".to_string(), "b".to_string(), "c".to_string());
        let probs = || [(&a, 0.5), (&b, 0.5), (&c, 0.0)];
        assert_eq!(sample(probs(), 0.0, 0.9), None);
        assert_eq!(sample(probs(), 1.0, 0.1), Some(&a));
        assert_eq!(sample(probs(), 1.0, 0.9), Some(&b));
        assert_eq!(sample([(&c, 0.0)], 1.0, 0.5), None);
        // Low temperature sharpens towards the most likely move.
        assert_eq!(sample([(&a, 0.4), (&b, 0.6)], 0.05, 0.01), Some(&b));
        assert!(parse_temperature("-1").is_err());
        assert_eq!(parse_temperature("0.5"), Ok(0.5));
    }

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
