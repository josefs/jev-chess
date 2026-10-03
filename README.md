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
| `--model`   | `JEV_MODEL`   | (unset)                                 | Optional `model` field, e.g. `jev-latest` for gateways that need it |
| `--log-file`| `JEV_CHESS_LOG` | (unset)                               | Append UCI traffic and Jev's top-ranked moves to this file |

Flags are convenient for GUIs such as BanksiaGUI that launch the engine with
fixed arguments, e.g. `jev-chess --api-key sk-... --model jev-latest`. Note that
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

## Development

```sh
cargo test
cargo clippy
```
