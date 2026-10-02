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

| Variable      | Default                                 | Description                     |
| ------------- | --------------------------------------- | ------------------------------- |
| `JEV_API_KEY` | (required)                              | Sent as a Bearer token          |
| `JEV_API_URL` | `https://api.typesafe.ai/v1/systemone`  | Decision endpoint               |
| `JEV_MODEL`   | (unset)                                 | Optional `model` field, e.g. `jev-latest` for gateways that need it |

## Usage

```sh
cargo build --release
export JEV_API_KEY=...
./target/release/jev-chess
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
