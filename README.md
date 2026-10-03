# jev-chess

A UCI chess engine, written in Rust, that lets [Jev](https://typesafe.ai)
(TypeSafe AI's "System One" decision model) pick its moves.

## How it works

1. The engine tracks the game from UCI `position` commands using
   [`shakmaty`](https://crates.io/crates/shakmaty), which also generates all
   legal moves.
2. On `go`, it sends Jev a single `choice` question: the `state` describes the
   position (FEN, ASCII board, move history) and each option is a legal move
   keyed by its UCI notation (e.g. `e2e4`) with a short description
   (`e4: pawn from e2 to e4, gives check`, ...).
3. The option with the highest probability is returned as `bestmove`.

If there is only one legal move, Jev is not called. If the API call fails or no
API key is configured, the first legal move is played and the error is reported
as a UCI `info string`.

## Configuration

Each setting can be given as a command-line flag or an environment variable
(flags take precedence):

| Flag        | Variable      | Default                                 | Description                     |
| ----------- | ------------- | --------------------------------------- | ------------------------------- |
| `--api-key` | `JEV_API_KEY` | (required)                              | Sent as a Bearer token          |
| `--api-url` | `JEV_API_URL` | `https://api.typesafe.ai/v1/systemone`  | Decision endpoint               |
| `--model`   | `JEV_MODEL`   | `jev-latest`                            | Jev model to use                |
| `--log-file`| `JEV_CHESS_LOG` | (unset)                               | Append UCI traffic and Jev's top-ranked moves to this file |
| `--prompt`  | `JEV_PROMPT`  | built-in `prompts/baseline.toml`        | Prompt template file (see below) |

Additional flags:

- `--random` plays uniformly random legal moves without calling Jev (a
  reference opponent for tournaments).
- `--show-prompt [FEN|startpos [moves ...]]` prints the rendered state, instructions and
  options for a position and exits; handy when writing prompt variants.

Flags are convenient for GUIs such as BanksiaGUI that launch the engine with
fixed arguments, e.g. `jev-chess --api-key sk-...`. Note that
command-line arguments are visible to other users via `ps`. If the GUI passes
all arguments as one string, it is split on whitespace. Invalid arguments are
reported on stderr but never stop the engine from speaking UCI.

The same settings are also exposed as UCI options `ApiKey`, `ApiUrl` and
`Model`, configurable from the GUI's engine options dialog. An empty option
value falls back to the flag/environment setting.

## Usage

```sh
cargo build --release
./target/release/jev-chess --api-key ...
```

Register `target/release/jev-chess` as a UCI engine in any chess GUI
(Cute Chess, Arena, BanksiaGUI, ...), or talk to it directly:

```
uci
position startpos moves e2e4
go
```

Before `bestmove` the engine emits `info depth 1 score cp 0 pv <move>` (a
neutral score; Jev doesn't evaluate positions) so that match runners such as
fastchess accept it.

## Prompt templates

The text sent to Jev comes from a TOML template. Copy
[`prompts/baseline.toml`](prompts/baseline.toml) to start a variant; it
documents the available placeholders:

- `state`, `instructions`: `{side}`, `{opponent}`, `{fen}`, `{board}`,
  `{moves}`, `{history}`, `{check}`
- `option` (one per legal move):
  - naming the move: `{uci}`, `{san}`, `{san_plain}` (no `x`), `{piece}`,
    `{from}`, `{to}`
  - move facts: `{capture}`, `{promotion}`, `{castling}`, `{gives_check}`, or
    all four as `{details}`
  - static one-ply analysis: `{safety}` (can the moved piece be captured?),
    `{hanging}` (other pieces left exposed), `{attacks}` (enemy pieces the moved
    piece attacks), `{material}` (balance after the move)

Move facts and analysis are empty or start with `, `, so they can be chained.
Example variants in `prompts/`:

| File | Options |
| --- | --- |
| `baseline.toml` | SAN, squares, captures/checks (the default) |
| `bare.toml` | move name and squares only, no consequences |
| `uci-only.toml` | just the UCI move |
| `safety-only.toml` | bare plus `{safety}` and `{hanging}` warnings |
| `annotated.toml` | baseline plus all analysis placeholders |
| `safety-capture.toml` | safety-only plus `{capture}` |
| `safety-check.toml` | safety-only plus `{gives_check}` |
| `safety-material.toml` | safety-only plus `{material}` |

Unknown placeholders are rejected when the template is loaded. Use
`jev-chess --prompt my.toml --show-prompt` to preview the result.

## Tournaments

`tournament.py` runs a round-robin between prompt variants using
[fastchess](https://github.com/Disservin/fastchess). The script downloads
fastchess into `tools/` on first use and builds the engine.

```sh
export JEV_API_KEY=...
./tournament.py prompts/baseline.toml prompts/my-variant.toml --random --rounds 5
```

Each prompt file becomes one engine, named after the file stem. `--random`
adds the random-move engine as a reference. Each round is a pair of games
from a random opening in `openings.epd`, with colours swapped.

Main options: `--rounds` (default 10), `--concurrency` (default 4), `--tc`
(default `300+5`), `--maxmoves` (default 150, then adjudicated as a draw),
`--seed`, `--model`, `--api-url` and `--out`. See `./tournament.py --help`.

Results go to `results/<timestamp>/`:

- `summary.txt`: the Elo table
- `games.pgn`
- `fastchess.log`
- one log per engine, with Jev's ranked moves
- copies of the prompts used

Every Jev engine makes one API call per move, so a 10-round match between two
prompts costs about 20 games × ~40 calls per side.

## Development

```sh
cargo test
cargo clippy
```
